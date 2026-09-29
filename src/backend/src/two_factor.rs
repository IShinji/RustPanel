//! 面板两步验证(TOTP)的密钥存储与绑定流程。
//!
//! 此前 2FA 只能靠手动设 `RUSTPANEL_TOTP_SECRET` 环境变量,面板里却有一个
//! 「强制两步验证」开关 —— 没配密钥时打开它,所有人都登不进面板。现在:
//! - 在面板里生成密钥 → 扫码 → 输入验证码确认后才生效(未确认的只是 pending,10 分钟过期);
//! - 关闭同样要当前验证码;
//! - 环境变量里的密钥仍优先(source = "env"),面板里不能关。
//!
//! 密钥是登录凭据,文件按机密落盘(0600,见 `statefile::write_secret_atomic`)。

use std::path::PathBuf;

use data_encoding::BASE32_NOPAD;
use serde::{Deserialize, Serialize};

const SECRET_BYTES: usize = 20;
const PENDING_TTL_SECONDS: u64 = 10 * 60;
const ISSUER: &str = "RustPanel";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct StoredTwoFactor {
    /// 已确认生效的密钥(base32);空 = 未启用
    #[serde(default)]
    secret: String,
    /// 待确认的新密钥(base32)
    #[serde(default)]
    pending: String,
    #[serde(default)]
    pending_created_at: u64,
}

fn store_path() -> PathBuf {
    crate::paths::state_root("security", "RUSTPANEL_SECURITY_ROOT").join("two-factor.json")
}

async fn load() -> StoredTwoFactor {
    match tokio::fs::read(store_path()).await {
        Ok(content) => serde_json::from_slice(&content).unwrap_or_default(),
        Err(_) => StoredTwoFactor::default(),
    }
}

async fn save(state: &StoredTwoFactor) -> std::io::Result<()> {
    let path = store_path();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let content = serde_json::to_vec_pretty(state).map_err(std::io::Error::other)?;
    crate::statefile::write_secret_atomic(&path, content).await
}

fn env_secret() -> Option<String> {
    std::env::var("RUSTPANEL_TOTP_SECRET")
        .or_else(|_| std::env::var("RUSTPANEL_2FA_SECRET"))
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// 当前生效的 TOTP 密钥来源:"env" / "panel" / None。
pub async fn source() -> Option<&'static str> {
    if env_secret().is_some() {
        Some("env")
    } else if !load().await.secret.is_empty() {
        Some("panel")
    } else {
        None
    }
}

/// 当前生效的 TOTP 密钥(原始字节);env 优先,其次面板里绑定的。
pub async fn active_secret() -> Option<Vec<u8>> {
    let encoded = match env_secret() {
        Some(secret) => secret,
        None => load().await.secret,
    };
    decode(&encoded)
}

fn decode(encoded: &str) -> Option<Vec<u8>> {
    let normalized: String = encoded
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '=')
        .map(|ch| ch.to_ascii_uppercase())
        .collect();
    if normalized.is_empty() {
        return None;
    }
    BASE32_NOPAD.decode(normalized.as_bytes()).ok()
}

fn generate_secret() -> Vec<u8> {
    // uuid v4 走 getrandom(系统 CSPRNG);两个拼起来取 20 字节
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes.truncate(SECRET_BYTES);
    bytes
}

fn otpauth_uri(secret_b32: &str, account: &str) -> String {
    let label = format!("{ISSUER}:{account}");
    format!(
        "otpauth://totp/{}?secret={secret_b32}&issuer={ISSUER}&algorithm=SHA1&digits=6&period=30",
        percent_encode(&label)
    )
}

fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b':' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn qr_svg(data: &str) -> Option<String> {
    let code = qrcode::QrCode::new(data.as_bytes()).ok()?;
    Some(
        code.render::<qrcode::render::svg::Color<'_>>()
            .min_dimensions(200, 200)
            .quiet_zone(true)
            .build(),
    )
}

pub struct SetupChallenge {
    pub secret: String,
    pub otpauth_uri: String,
    pub qr_svg: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TwoFactorError {
    #[error("两步验证由环境变量 RUSTPANEL_TOTP_SECRET 管理,请改环境变量")]
    ManagedByEnv,
    #[error("没有待确认的绑定,或已超过 10 分钟,请重新开始绑定")]
    NoPendingSetup,
    #[error("验证码不正确")]
    InvalidCode,
    #[error("两步验证未启用")]
    NotEnabled,
    #[error("保存两步验证状态失败: {0}")]
    Io(#[from] std::io::Error),
}

/// 生成新的待确认密钥;已启用的密钥在确认前保持不变。
pub async fn begin_setup(account: &str) -> Result<SetupChallenge, TwoFactorError> {
    if env_secret().is_some() {
        return Err(TwoFactorError::ManagedByEnv);
    }
    let secret = BASE32_NOPAD.encode(&generate_secret());
    let mut state = load().await;
    state.pending = secret.clone();
    state.pending_created_at = now_seconds();
    save(&state).await?;
    let uri = otpauth_uri(&secret, account);
    Ok(SetupChallenge {
        qr_svg: qr_svg(&uri).unwrap_or_default(),
        otpauth_uri: uri,
        secret,
    })
}

/// 用待确认密钥校验验证码,通过则生效。
pub async fn confirm_setup(code: &str) -> Result<(), TwoFactorError> {
    if env_secret().is_some() {
        return Err(TwoFactorError::ManagedByEnv);
    }
    let mut state = load().await;
    if state.pending.is_empty()
        || now_seconds().saturating_sub(state.pending_created_at) > PENDING_TTL_SECONDS
    {
        return Err(TwoFactorError::NoPendingSetup);
    }
    let secret = decode(&state.pending).ok_or(TwoFactorError::NoPendingSetup)?;
    if !crate::auth::verify_totp(&secret, code.trim(), now_seconds()) {
        return Err(TwoFactorError::InvalidCode);
    }
    state.secret = std::mem::take(&mut state.pending);
    state.pending_created_at = 0;
    save(&state).await?;
    Ok(())
}

/// 用当前生效的密钥校验验证码,通过则关闭两步验证。
pub async fn disable(code: &str) -> Result<(), TwoFactorError> {
    if env_secret().is_some() {
        return Err(TwoFactorError::ManagedByEnv);
    }
    let mut state = load().await;
    let secret = decode(&state.secret).ok_or(TwoFactorError::NotEnabled)?;
    if !crate::auth::verify_totp(&secret, code.trim(), now_seconds()) {
        return Err(TwoFactorError::InvalidCode);
    }
    state.secret.clear();
    state.pending.clear();
    state.pending_created_at = 0;
    save(&state).await?;
    Ok(())
}

fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_secret_round_trips_through_base32() {
        let secret = generate_secret();
        assert_eq!(secret.len(), SECRET_BYTES);
        let encoded = BASE32_NOPAD.encode(&secret);
        assert_eq!(decode(&encoded), Some(secret.clone()));
        // 用户手抄时常见的小写 / 空格 / 补位也能认
        let messy = format!(" {} ==", encoded.to_ascii_lowercase());
        assert_eq!(decode(&messy), Some(secret));
        assert_eq!(decode(""), None);
    }

    #[test]
    fn otpauth_uri_escapes_label() {
        let uri = otpauth_uri("ABC", "admin@my host");
        assert!(uri.starts_with("otpauth://totp/RustPanel:admin%40my%20host?secret=ABC"));
        assert!(uri.contains("issuer=RustPanel"));
    }

    #[test]
    fn qr_svg_renders() {
        let svg = qr_svg("otpauth://totp/RustPanel:admin?secret=ABC").expect("svg");
        assert!(svg.contains("<svg"));
    }
}
