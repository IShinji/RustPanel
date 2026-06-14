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
    time::{SystemTime, UNIX_EPOCH},
};

use instant_acme::{
    Account, AccountCredentials, AuthorizationStatus, ChallengeType, Identifier, LetsEncrypt,
    NewAccount, NewOrder, OrderStatus, RetryPolicy,
};
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
    created_at_seconds: u64,
    // 0.7 时代有 cert_keypair_pem 字段(start 时生成 keypair,resume 时复用);
    // 0.8 起 Order::finalize() 自己生成 keypair,字段不再需要。从 0.7 升来的
    // 旧 pending 文件 serde(default) 兜底,新字段没有就当 None。
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
    /// false → LE staging,true → LE production。
    /// 默认 false 给开发 / 第一次试管道用,确认面板能跑通后再 UI toggle 到
    /// true 拿真证书。
    #[serde(default)]
    pub production: bool,
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

/// LE directory URL,优先看 AcmeSettings.production(UI toggle)。
/// 兜底:env var RUSTPANEL_ACME_PRODUCTION=1(老部署 / 单测 / 升级前的
/// 二进制兼容路径)。读 settings 失败时 fallback 到 env,settings 文件
/// 不存在时也走 env,完全保留旧行为。
async fn directory_url() -> &'static str {
    let production = match read_settings().await {
        Ok(s) => s.production,
        Err(_) => false,
    } || env::var("RUSTPANEL_ACME_PRODUCTION")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if production {
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
    // tmp + rename 保证原子,避免崩溃/并发写出半截 JSON 后 read_pending
    // 反序列化失败,把该域名永久卡在错误态。
    let tmp = path.with_extension("json.tmp");
    tokio::fs::write(&tmp, content).await?;
    tokio::fs::rename(&tmp, &path).await?;
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
    let (account, credentials) = Account::builder()?
        .create(&new_account, directory_url().await.to_owned(), None)
        .await?;

    let identifier = Identifier::Dns(domain.to_owned());
    let mut order = account
        .new_order(&NewOrder::new(std::slice::from_ref(&identifier)))
        .await?;
    let order_url = order.url().to_owned();

    // 抓 DNS-01 challenge URL + value;authorizations 是个 streaming
    // iterator,要在一个限定 scope 里跑完借用才能释放 &mut order。
    let (challenge_url, dns_value) = {
        let mut authzs = order.authorizations();
        let Some(authz_result) = authzs.next().await else {
            return Err(AcmeError::NoDns01Challenge(domain.to_owned()));
        };
        let mut authz = authz_result?;
        let challenge = authz
            .challenge(ChallengeType::Dns01)
            .ok_or_else(|| AcmeError::NoDns01Challenge(domain.to_owned()))?;
        // ChallengeHandle.url 是字段不是方法,直接读
        let url = challenge.url.to_owned();
        let value = challenge.key_authorization().dns_value();
        (url, value)
    };

    let credentials_json = serde_json::to_string(&credentials)?;
    let pending = PendingOrder {
        domain: domain.to_owned(),
        email: email.to_owned(),
        account_credentials: credentials_json,
        order_url,
        challenge_url,
        dns_record_value: dns_value.clone(),
        created_at_seconds: now_seconds(),
    };
    write_pending(&pending).await?;

    Ok(RequestOutcome::Challenge(DnsChallengeNeeded {
        record_name: format!("_acme-challenge.{domain}"),
        record_value: dns_value,
    }))
}

async fn finalize_order(state: PendingOrder) -> Result<RequestOutcome, AcmeError> {
    let credentials: AccountCredentials = serde_json::from_str(&state.account_credentials)?;
    let account = Account::builder()?.from_credentials(credentials).await?;

    let mut order = account.order(state.order_url.clone()).await?;

    // 通知 ACME server 已经摆好 TXT,让它来验。在 0.8 里 set_ready 挂在
    // ChallengeHandle 上,只能通过迭代 authorizations 拿到 —— scope 里拿,
    // 出 scope 释放 &mut order 才能继续 poll_ready / finalize。
    //
    // 如果 authz 已经 valid(用户在 panel 上没看到我们存的 TXT 已 propagate
    // 之前就再点了一次),authz.challenge() 仍能返回 ChallengeHandle,
    // set_ready 在 valid 状态下是 idempotent no-op,ACME server 不会再校验。
    {
        let mut authzs = order.authorizations();
        let mut handled = false;
        while let Some(result) = authzs.next().await {
            let mut authz = result?;
            // 业务态 short-circuit:authz 已经 invalid / expired / revoked,
            // 这条 order 没救,留给上层去 delete_pending + 重发新 order。
            match authz.status {
                AuthorizationStatus::Invalid
                | AuthorizationStatus::Revoked
                | AuthorizationStatus::Expired => {
                    return Err(AcmeError::Authorization(authz.status));
                }
                AuthorizationStatus::Valid => {
                    handled = true;
                    continue;
                }
                _ => {}
            }
            if let Some(mut chal) = authz.challenge(ChallengeType::Dns01) {
                chal.set_ready().await?;
                handled = true;
            }
        }
        if !handled {
            return Err(AcmeError::NoDns01Challenge(state.domain.clone()));
        }
    }

    // 等 order 状态进 Ready(LE 验完 TXT)。RetryPolicy 默认指数退避,
    // 内部 timeout 满了会返 Error::Timeout。
    let status = order.poll_ready(&RetryPolicy::default()).await?;
    if status != OrderStatus::Ready {
        return Err(AcmeError::Order(status));
    }

    // finalize 在 0.8 自己生成 keypair 并返回 PEM(rcgen feature 默认开)。
    // 我们之前手动 CSR + 自己存 keypair_pem 那一套不再需要。
    let private_key_pem = order.finalize().await?;
    let cert_chain_pem = order.poll_certificate(&RetryPolicy::default()).await?;

    delete_pending(&state.domain).await?;

    Ok(RequestOutcome::Issued(IssuedCertificate {
        certificate_pem: cert_chain_pem,
        private_key_pem,
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
    let (account, _credentials) = Account::builder()?
        .create(&new_account, directory_url().await.to_owned(), None)
        .await?;

    let identifier = Identifier::Dns(domain.to_owned());
    let mut order = account
        .new_order(&NewOrder::new(std::slice::from_ref(&identifier)))
        .await?;

    // 拿 challenge token + key_authorization body,提前写到 webroot,
    // 释放 &mut order 再 set_ready / poll_ready / finalize。
    let (token, body) = {
        let mut authzs = order.authorizations();
        let Some(authz_result) = authzs.next().await else {
            return Err(AcmeError::NoHttp01Challenge(domain.to_owned()));
        };
        let mut authz = authz_result?;
        let challenge = authz
            .challenge(ChallengeType::Http01)
            .ok_or_else(|| AcmeError::NoHttp01Challenge(domain.to_owned()))?;
        // HTTP-01 token 是 challenge URL 最后一段?用 key_authorization
        // 拿到的 body 含 token.<base64>,但 ACME 要求文件名是 token 本身。
        // 0.8 ChallengeHandle 没有直接暴露 token,但 KeyAuthorization::as_str
        // 返回 "token.<thumbprint>",我们 split 第一段即 token。
        let key_auth = challenge.key_authorization();
        let body_str = key_auth.as_str().to_owned();
        let tok = body_str.split('.').next().unwrap_or(&body_str).to_owned();
        (tok, body_str)
    };

    let acme_dir = webroot.join(".well-known/acme-challenge");
    tokio::fs::create_dir_all(&acme_dir).await?;
    let challenge_path = acme_dir.join(&token);
    tokio::fs::write(&challenge_path, body.as_bytes()).await?;

    let result = async {
        // 通知 LE:文件已就位。0.8 set_ready 在 ChallengeHandle 上,
        // 又得通过 authorizations() 拿一次。
        {
            let mut authzs = order.authorizations();
            let Some(authz_result) = authzs.next().await else {
                return Err(AcmeError::NoHttp01Challenge(domain.to_owned()));
            };
            let mut authz = authz_result?;
            let mut chal = authz
                .challenge(ChallengeType::Http01)
                .ok_or_else(|| AcmeError::NoHttp01Challenge(domain.to_owned()))?;
            chal.set_ready().await?;
        }

        let status = order.poll_ready(&RetryPolicy::default()).await?;
        if status != OrderStatus::Ready {
            return Err(AcmeError::Order(status));
        }
        let private_key_pem = order.finalize().await?;
        let cert_chain_pem = order.poll_certificate(&RetryPolicy::default()).await?;
        Ok(IssuedCertificate {
            certificate_pem: cert_chain_pem,
            private_key_pem,
        })
    }
    .await;

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
    write_private_key(&key_path, cert.private_key_pem.as_bytes()).await?;
    Ok((cert_path, key_path))
}

/// 写私钥文件并在 Unix 上收紧到 0600。私钥绝不能世界可读 —— root 默认
/// umask 下裸 `fs::write` 会落成 0644,本机任意用户/容器都能读走 TLS
/// 私钥。先写后 chmod 的极小窗口对本威胁模型可接受(目标是不留 0644,
/// 而非防 TOCTOU)。ssl 模块写私钥也复用此助手。
pub(crate) async fn write_private_key(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    tokio::fs::write(path, contents).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // RUSTPANEL_ACME_ROOT / RUSTPANEL_ACME_PRODUCTION 是 process-global env
    // vars。cargo test 默认并发,多个 acme 测试同时改 env 会互相清掉对方
    // 的 tempdir 路径 → 一个测试读不到另一个写的文件。用 tokio 的 Mutex
    // 而不是 std 的 —— clippy await_holding_lock 不让跨 await 持 std 锁;
    // tokio::sync::Mutex 设计就是干这个的。
    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn directory_url_respects_production_env() {
        let _guard = ENV_LOCK.lock().await;
        let tmp = tempfile::tempdir().expect("tempdir");
        env::set_var("RUSTPANEL_ACME_ROOT", tmp.path());
        env::remove_var("RUSTPANEL_ACME_PRODUCTION");
        let staging = directory_url().await;
        assert!(
            staging.contains("staging"),
            "expected staging url, got {staging}"
        );

        env::set_var("RUSTPANEL_ACME_PRODUCTION", "1");
        let production = directory_url().await;
        assert!(
            !production.contains("staging"),
            "expected production url, got {production}"
        );
        env::remove_var("RUSTPANEL_ACME_PRODUCTION");
        env::remove_var("RUSTPANEL_ACME_ROOT");
    }

    #[tokio::test]
    async fn directory_url_settings_override_takes_precedence() {
        let _guard = ENV_LOCK.lock().await;
        let tmp = tempfile::tempdir().expect("tempdir");
        env::set_var("RUSTPANEL_ACME_ROOT", tmp.path());
        env::remove_var("RUSTPANEL_ACME_PRODUCTION");

        write_settings(&AcmeSettings {
            contact_email: "x@example.org.real".into(),
            production: true,
        })
        .await
        .unwrap();
        let url = directory_url().await;
        assert!(
            !url.contains("staging"),
            "settings.production=true 应该走 prod,got {url}"
        );

        env::remove_var("RUSTPANEL_ACME_ROOT");
    }

    #[tokio::test]
    async fn pending_round_trip() {
        let _guard = ENV_LOCK.lock().await;
        let tmp = tempfile::tempdir().expect("tempdir");
        env::set_var("RUSTPANEL_ACME_ROOT", tmp.path());

        let order = PendingOrder {
            domain: "example.com".into(),
            email: "admin@example.com".into(),
            account_credentials: "{\"k\":\"v\"}".into(),
            order_url: "https://acme/order/1".into(),
            challenge_url: "https://acme/chall/1".into(),
            dns_record_value: "abc.def".into(),
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
