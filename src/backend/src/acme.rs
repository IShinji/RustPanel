// P8-04-5:真实 ACME DNS-01 状态机。
//
// 设计意图:RequestCertificate 在 NAT VPS 上必须走 DNS-01,因为 80 端口拿不到。
// 流程必须分两次调用:第一次返回需要写入的 TXT(真实 ACME token),用户加 DNS,
// 第二次同 domain 调用 → 我们恢复 Order → 通知 ACME 验证 → finalize 拿证书。
//
// 持久化关键状态(account credentials + order URL + challenge URL + dns_value)
// 到 $RUSTPANEL_ACME_ROOT/pending/<domain>.json,直到完成或失败。
//
// 默认走 Let's Encrypt **staging** 目录,避免开发/测试触碰真实速率限制。
// 生产环境需要显式 RUSTPANEL_ACME_PRODUCTION=1 切到 production。

use std::{
    env,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use instant_acme::{
    Account, AccountCredentials, AuthorizationStatus, ChallengeType, Identifier, LetsEncrypt,
    NewAccount, NewOrder, OrderStatus,
};
use rcgen::{CertificateParams, DistinguishedName, KeyPair};
use serde::{Deserialize, Serialize};

const DEFAULT_ACME_ROOT: &str = "/var/lib/rustpanel/acme";

#[derive(Debug, thiserror::Error)]
pub enum AcmeError {
    #[error("instant-acme: {0}")]
    Acme(#[from] instant_acme::Error),
    #[error("rcgen: {0}")]
    Rcgen(#[from] rcgen::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("authorization {0:?}")]
    Authorization(AuthorizationStatus),
    #[error("order {0:?}")]
    Order(OrderStatus),
    #[error("no DNS-01 challenge for {0}")]
    NoDns01Challenge(String),
    #[error("no HTTP-01 challenge for {0}")]
    NoHttp01Challenge(String),
    #[error("authorization timeout for {0}")]
    Timeout(String),
}

/// 调用方拿到的"第一次"结果:面板需要把 TXT 写到这里。
#[derive(Debug, Clone)]
pub struct DnsChallengeNeeded {
    /// 完整的 RR name,例如 "_acme-challenge.example.com"
    pub record_name: String,
    /// 真实 ACME 计算出来的 TXT 值
    pub record_value: String,
}

/// 调用方拿到的"第二次"结果:证书已签发。
#[derive(Debug, Clone)]
pub struct IssuedCertificate {
    /// 完整链 PEM(包含中间证书)
    pub certificate_pem: String,
    /// 私钥 PEM
    pub private_key_pem: String,
}

#[derive(Debug, Clone)]
pub enum RequestOutcome {
    Challenge(DnsChallengeNeeded),
    Issued(IssuedCertificate),
}

#[derive(Serialize, Deserialize)]
struct PendingOrder {
    domain: String,
    email: String,
    /// instant-acme AccountCredentials 序列化后的字符串(由 acme crate 提供 Serialize)
    account_credentials: String,
    order_url: String,
    challenge_url: String,
    dns_record_value: String,
    /// 第一次提交时已经为这次 issuance 生成的 cert keypair PEM。复用以保证 CSR 的
    /// 私钥就是后续要写到 privkey.pem 的那把。
    cert_keypair_pem: String,
    created_at_seconds: u64,
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn acme_root() -> PathBuf {
    env::var("RUSTPANEL_ACME_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_ACME_ROOT))
}

fn pending_path(domain: &str) -> PathBuf {
    acme_root().join("pending").join(format!("{domain}.json"))
}

fn settings_path() -> PathBuf {
    acme_root().join("settings.json")
}

/// 面板级 ACME 偏好。目前只有 contact_email,后续可以扩(staging vs
/// production override、默认 DNS provider 等)。**与浏览器无关**:换个
/// 浏览器 / 清缓存都不该丢。
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct AcmeSettings {
    #[serde(default)]
    pub contact_email: String,
}

/// Let's Encrypt 自 2024 起把这三个 RFC 2606 保留域加入 forbiddenDomains,
/// 任何 contact 落在它们上面都会 `invalidContact` 拒了。前端 + 后端都拦,
/// 防止"占位符邮箱被持久化"再次发生。
pub fn is_forbidden_email_domain(email: &str) -> bool {
    let lower = email.trim().to_ascii_lowercase();
    ["@example.com", "@example.org", "@example.net", "@example."]
        .iter()
        .any(|forbidden| lower.ends_with(forbidden) || lower.contains(forbidden))
}

pub async fn read_settings() -> Result<AcmeSettings, AcmeError> {
    match tokio::fs::read_to_string(settings_path()).await {
        Ok(content) => Ok(serde_json::from_str(&content)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(AcmeSettings::default()),
        Err(error) => Err(error.into()),
    }
}

pub async fn write_settings(settings: &AcmeSettings) -> Result<(), AcmeError> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let content = serde_json::to_string_pretty(settings)?;
    // tmp + rename 保证原子,避免半写状态。
    let tmp = path.with_extension("json.tmp");
    tokio::fs::write(&tmp, content).await?;
    tokio::fs::rename(&tmp, &path).await?;
    Ok(())
}

fn directory_url() -> &'static str {
    if env::var("RUSTPANEL_ACME_PRODUCTION")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        LetsEncrypt::Production.url()
    } else {
        LetsEncrypt::Staging.url()
    }
}

async fn read_pending(domain: &str) -> Result<Option<PendingOrder>, AcmeError> {
    match tokio::fs::read_to_string(pending_path(domain)).await {
        Ok(content) => Ok(Some(serde_json::from_str(&content)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

async fn write_pending(order: &PendingOrder) -> Result<(), AcmeError> {
    let path = pending_path(&order.domain);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let content = serde_json::to_string_pretty(order)?;
    tokio::fs::write(path, content).await?;
    Ok(())
}

async fn delete_pending(domain: &str) -> Result<(), AcmeError> {
    let path = pending_path(domain);
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// 主入口:第一次调用返回 TXT 需求,第二次调用(同 domain)继续走完。
pub async fn request_or_resume_dns01(
    domain: &str,
    email: &str,
) -> Result<RequestOutcome, AcmeError> {
    // pending 读不出来不算致命:可能是上次 instant-acme 版本的 schema
    // 落到磁盘后我们升级了,字段对不上(典型报错 "missing field token");
    // 也可能是 order URL 已经过期(LE 一次性的)。这种情况直接丢掉脏
    // 状态,当成"从未发起过"重新走 start_order,而不是把错误抛给用户
    // 让他不知道怎么继续。
    match read_pending(domain).await {
        Ok(Some(state)) => match finalize_order(state).await {
            Ok(outcome) => Ok(outcome),
            // instant-acme 内部 (de)serialize 失败 / 存的 order URL 过期
            // → 也按"脏状态"处理,清掉重来。Authorization/Order 业务态
            // 的错(Invalid / Expired / 用户没加 TXT 等)保留,不能默默
            // 抹掉,得让调用方看到真实原因。
            Err(AcmeError::Json(_) | AcmeError::Acme(_)) => {
                let _ = delete_pending(domain).await;
                start_order(domain, email).await
            }
            Err(other) => Err(other),
        },
        Ok(None) => start_order(domain, email).await,
        Err(_) => {
            let _ = delete_pending(domain).await;
            start_order(domain, email).await
        }
    }
}

async fn start_order(domain: &str, email: &str) -> Result<RequestOutcome, AcmeError> {
    let new_account = NewAccount {
        contact: &[&format!("mailto:{email}")],
        terms_of_service_agreed: true,
        only_return_existing: false,
    };
    let (account, credentials) = Account::create(&new_account, directory_url(), None).await?;

    let identifier = Identifier::Dns(domain.to_owned());
    let mut order = account
        .new_order(&NewOrder {
            identifiers: &[identifier],
        })
        .await?;
    let order_url = order.url().to_owned();

    let authorizations = order.authorizations().await?;
    let auth = authorizations
        .first()
        .ok_or_else(|| AcmeError::NoDns01Challenge(domain.to_owned()))?;
    let challenge = auth
        .challenges
        .iter()
        .find(|c| c.r#type == ChallengeType::Dns01)
        .ok_or_else(|| AcmeError::NoDns01Challenge(domain.to_owned()))?;

    let key_authorization = order.key_authorization(challenge);
    let dns_value = key_authorization.dns_value();
    let record_name = format!("_acme-challenge.{domain}");

    // 提前生成证书 keypair,后续 finalize 时复用同一份私钥
    let mut cert_params = CertificateParams::new(vec![domain.to_owned()])?;
    cert_params.distinguished_name = DistinguishedName::new();
    let cert_keypair = KeyPair::generate()?;
    let cert_keypair_pem = cert_keypair.serialize_pem();

    let credentials_json = serde_json::to_string(&credentials)?;

    let pending = PendingOrder {
        domain: domain.to_owned(),
        email: email.to_owned(),
        account_credentials: credentials_json,
        order_url,
        challenge_url: challenge.url.clone(),
        dns_record_value: dns_value.clone(),
        cert_keypair_pem,
        created_at_seconds: now_seconds(),
    };
    write_pending(&pending).await?;

    Ok(RequestOutcome::Challenge(DnsChallengeNeeded {
        record_name,
        record_value: dns_value,
    }))
}

async fn finalize_order(state: PendingOrder) -> Result<RequestOutcome, AcmeError> {
    let credentials: AccountCredentials = serde_json::from_str(&state.account_credentials)?;
    let account = Account::from_credentials(credentials).await?;

    // 用保存的 order URL 恢复 Order
    let mut order = account.order(state.order_url.clone()).await?;

    // 通知 ACME server 已经摆好挑战,触发它来验 DNS
    order.set_challenge_ready(&state.challenge_url).await?;

    // 轮询 authorization 状态:每 5 秒一次,最多 24 次(2 分钟)
    let mut tries = 0u32;
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let authorizations = order.authorizations().await?;
        let status = authorizations
            .first()
            .map(|a| a.status)
            .unwrap_or(AuthorizationStatus::Pending);
        match status {
            AuthorizationStatus::Valid => break,
            AuthorizationStatus::Invalid
            | AuthorizationStatus::Revoked
            | AuthorizationStatus::Expired => {
                return Err(AcmeError::Authorization(status));
            }
            _ => {
                tries += 1;
                if tries >= 24 {
                    return Err(AcmeError::Timeout(state.domain));
                }
            }
        }
    }

    // 用预先保存的 keypair 生成 CSR
    let mut params = CertificateParams::new(vec![state.domain.clone()])?;
    params.distinguished_name = DistinguishedName::new();
    let keypair = KeyPair::from_pem(&state.cert_keypair_pem)?;
    let csr = params.serialize_request(&keypair)?;

    order.finalize(csr.der()).await?;

    // 等 finalize 完成 → certificate 可下载
    let mut tries = 0u32;
    let cert_chain_pem = loop {
        tokio::time::sleep(Duration::from_secs(3)).await;
        if let Some(chain) = order.certificate().await? {
            break chain;
        }
        let status = order.state().status;
        if matches!(status, OrderStatus::Invalid) {
            return Err(AcmeError::Order(status));
        }
        tries += 1;
        if tries >= 20 {
            return Err(AcmeError::Timeout(state.domain.clone()));
        }
    };

    delete_pending(&state.domain).await?;

    Ok(RequestOutcome::Issued(IssuedCertificate {
        certificate_pem: cert_chain_pem,
        private_key_pem: state.cert_keypair_pem,
    }))
}

/// HTTP-01 单次阻塞签发 —— 与 DNS-01 不同,HTTP-01 不需要两步交互:
/// 面板自己把 token 写到 webroot/.well-known/acme-challenge/<token>,
/// 同步等 ACME 服务器拉取 + 验证 + finalize 拿证书。前提是:
/// 1. nginx 已经在 80 端口监听该域名(create_site 已经写了 vhost 配置)
/// 2. nginx vhost 的 location ^~ /.well-known/acme-challenge/ 指向 webroot
/// 3. 公网到 VPS 的 80 端口可达 —— NAT VPS 默认不满足,需用 DNS-01
pub async fn request_http01_blocking(
    domain: &str,
    email: &str,
    webroot: &Path,
) -> Result<IssuedCertificate, AcmeError> {
    let new_account = NewAccount {
        contact: &[&format!("mailto:{email}")],
        terms_of_service_agreed: true,
        only_return_existing: false,
    };
    let (account, _credentials) = Account::create(&new_account, directory_url(), None).await?;

    let identifier = Identifier::Dns(domain.to_owned());
    let mut order = account
        .new_order(&NewOrder {
            identifiers: &[identifier],
        })
        .await?;

    let authorizations = order.authorizations().await?;
    let auth = authorizations
        .first()
        .ok_or_else(|| AcmeError::NoHttp01Challenge(domain.to_owned()))?;
    let challenge = auth
        .challenges
        .iter()
        .find(|c| c.r#type == ChallengeType::Http01)
        .ok_or_else(|| AcmeError::NoHttp01Challenge(domain.to_owned()))?;

    let key_authorization = order.key_authorization(challenge);
    let token = challenge.token.clone();
    let body = key_authorization.as_str().to_owned();

    // 写 challenge 文件到 webroot/.well-known/acme-challenge/<token>
    let acme_dir = webroot.join(".well-known/acme-challenge");
    tokio::fs::create_dir_all(&acme_dir).await?;
    let challenge_path = acme_dir.join(&token);
    tokio::fs::write(&challenge_path, body.as_bytes()).await?;

    // 实际的 ACME 状态机部分用闭包包起来,即使中途出错也把 challenge 文件清掉
    let result = async {
        order.set_challenge_ready(&challenge.url).await?;

        // 轮询 authorization:每 5 秒一次,最多 24 次(2 分钟)
        let mut tries = 0u32;
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            let authorizations = order.authorizations().await?;
            let status = authorizations
                .first()
                .map(|a| a.status)
                .unwrap_or(AuthorizationStatus::Pending);
            match status {
                AuthorizationStatus::Valid => break,
                AuthorizationStatus::Invalid
                | AuthorizationStatus::Revoked
                | AuthorizationStatus::Expired => {
                    return Err(AcmeError::Authorization(status));
                }
                _ => {
                    tries += 1;
                    if tries >= 24 {
                        return Err(AcmeError::Timeout(domain.to_owned()));
                    }
                }
            }
        }

        // CSR
        let mut params = CertificateParams::new(vec![domain.to_owned()])?;
        params.distinguished_name = DistinguishedName::new();
        let cert_keypair = KeyPair::generate()?;
        let cert_keypair_pem = cert_keypair.serialize_pem();
        let csr = params.serialize_request(&cert_keypair)?;
        order.finalize(csr.der()).await?;

        // 拿证书链
        let mut tries = 0u32;
        let cert_chain_pem = loop {
            tokio::time::sleep(Duration::from_secs(3)).await;
            if let Some(chain) = order.certificate().await? {
                break chain;
            }
            let status = order.state().status;
            if matches!(status, OrderStatus::Invalid) {
                return Err(AcmeError::Order(status));
            }
            tries += 1;
            if tries >= 20 {
                return Err(AcmeError::Timeout(domain.to_owned()));
            }
        };

        Ok(IssuedCertificate {
            certificate_pem: cert_chain_pem,
            private_key_pem: cert_keypair_pem,
        })
    }
    .await;

    // 不论成功还是失败,都尝试删 challenge 文件;失败也只是文件残留,不致命
    let _ = tokio::fs::remove_file(&challenge_path).await;

    result
}

/// 把签发好的证书 PEM 写到标准 letsencrypt 路径,供 ssl.rs 后续 reload nginx 读。
pub async fn install_certificate(
    domain: &str,
    cert: &IssuedCertificate,
    cert_root: &Path,
) -> Result<(PathBuf, PathBuf), AcmeError> {
    let dir = cert_root.join(domain);
    tokio::fs::create_dir_all(&dir).await?;
    let cert_path = dir.join("fullchain.pem");
    let key_path = dir.join("privkey.pem");
    tokio::fs::write(&cert_path, &cert.certificate_pem).await?;
    tokio::fs::write(&key_path, &cert.private_key_pem).await?;
    Ok((cert_path, key_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    // 两个 env-driven 检查合到一个测试里串行跑,避免与其它平行测试共享
    // RUSTPANEL_ACME_PRODUCTION 这个 process-global env var。
    #[test]
    fn directory_url_respects_production_env() {
        env::remove_var("RUSTPANEL_ACME_PRODUCTION");
        let staging = directory_url();
        assert!(
            staging.contains("staging"),
            "expected staging url, got {staging}"
        );

        env::set_var("RUSTPANEL_ACME_PRODUCTION", "1");
        let production = directory_url();
        assert!(
            !production.contains("staging"),
            "expected production url, got {production}"
        );
        env::remove_var("RUSTPANEL_ACME_PRODUCTION");
    }

    #[tokio::test]
    async fn pending_round_trip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        env::set_var("RUSTPANEL_ACME_ROOT", tmp.path());

        let order = PendingOrder {
            domain: "example.com".into(),
            email: "admin@example.com".into(),
            account_credentials: "{\"k\":\"v\"}".into(),
            order_url: "https://acme/order/1".into(),
            challenge_url: "https://acme/chall/1".into(),
            dns_record_value: "abc.def".into(),
            cert_keypair_pem: "-----BEGIN PRIVATE KEY-----\nXXX\n-----END PRIVATE KEY-----\n"
                .into(),
            created_at_seconds: 0,
        };
        write_pending(&order).await.unwrap();

        let loaded = read_pending("example.com").await.unwrap().unwrap();
        assert_eq!(loaded.order_url, order.order_url);
        assert_eq!(loaded.dns_record_value, order.dns_record_value);

        delete_pending("example.com").await.unwrap();
        let after = read_pending("example.com").await.unwrap();
        assert!(after.is_none());

        env::remove_var("RUSTPANEL_ACME_ROOT");
    }
}
