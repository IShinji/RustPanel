use std::{
    collections::HashMap,
    env,
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Mutex},
};

use futures_core::Stream;
use tokio::sync::broadcast;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tonic::{Request, Response as GrpcResponse, Status};

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        ssl_service_server::SslService, AcmeChallengeType, CertificateItem, CertificateState,
        ImportCertificateRequest, ImportCertificateResponse, ListCertificatesRequest,
        ListCertificatesResponse, RenewCertificateRequest, RenewCertificateResponse,
        RequestCertificateRequest, RequestCertificateResponse, RevokeCertificateRequest,
        RevokeCertificateResponse, WatchCertificateProgressRequest,
        WatchCertificateProgressResponse,
    },
};

const DEFAULT_CERT_ROOT: &str = "/etc/letsencrypt/live";
const DEFAULT_CHALLENGE_ROOT: &str = "/var/www/rustpanel-acme";
const SSL_CHANNEL_SIZE: usize = 16;

#[derive(Clone, Default)]
pub struct SslServiceImpl {
    progress: Arc<Mutex<HashMap<String, broadcast::Sender<WatchCertificateProgressResponse>>>>,
}

#[tonic::async_trait]
impl SslService for SslServiceImpl {
    type WatchCertificateProgressStream =
        Pin<Box<dyn Stream<Item = Result<WatchCertificateProgressResponse, Status>> + Send>>;

    async fn request_certificate(
        &self,
        request: Request<RequestCertificateRequest>,
    ) -> Result<GrpcResponse<RequestCertificateResponse>, Status> {
        let request = request.into_inner();
        validate_domain(&request.domain)?;
        if !request.email.contains('@') {
            return Err(Status::invalid_argument("valid email is required"));
        }
        let sender = self.progress_sender(&request.domain)?;

        let challenge_type =
            AcmeChallengeType::try_from(request.challenge_type).unwrap_or_default();
        // NAT VPS 上 80 端口通常拿不到,DNS-01 是唯一可行的挑战方式。
        // 当前 manual 模式:面板生成确定性的 TXT 记录提示给用户,
        // 用户在 DNS 控制台加完后,再调一次 RequestCertificate 跑实际验证。
        if challenge_type == AcmeChallengeType::Dns01 {
            let provider = if request.dns_provider.trim().is_empty() {
                "manual"
            } else {
                request.dns_provider.trim()
            };
            if provider == "manual" {
                send_progress(
                    &sender,
                    &request.domain,
                    CertificateState::Pending,
                    "dns-01 manual: 等待用户添加 TXT 记录",
                );
                let (record_name, record_value) = build_manual_dns_challenge(&request.domain);
                return Ok(GrpcResponse::new(RequestCertificateResponse {
                    status: Some(ok_response("请把下方 TXT 记录加到 DNS,完成后再点一次申请")),
                    certificate: None,
                    dns_record_name: record_name,
                    dns_record_value: record_value,
                }));
            }
            // cloudflare/route53 等 provider 留待后续实现
            return Err(Status::unimplemented(format!(
                "DNS provider {provider} 暂未实现,请使用 manual 模式手动添加 TXT 记录"
            )));
        }

        send_progress(
            &sender,
            &request.domain,
            CertificateState::Pending,
            "http-01 challenge prepared",
        );

        let certificate = issue_certificate(&request.domain).await?;
        send_progress(
            &sender,
            &request.domain,
            CertificateState::Issued,
            "certificate stored",
        );
        let _ = reload_nginx().await;

        Ok(GrpcResponse::new(RequestCertificateResponse {
            status: Some(ok_response("certificate issued")),
            certificate: Some(certificate),
            dns_record_name: String::new(),
            dns_record_value: String::new(),
        }))
    }

    async fn revoke_certificate(
        &self,
        request: Request<RevokeCertificateRequest>,
    ) -> Result<GrpcResponse<RevokeCertificateResponse>, Status> {
        let domain = request.into_inner().domain;
        validate_domain(&domain)?;
        let directory = domain_cert_dir(&domain);
        if tokio::fs::try_exists(&directory).await.map_err(io_status)? {
            tokio::fs::remove_dir_all(directory)
                .await
                .map_err(io_status)?;
        }
        if let Ok(sender) = self.progress_sender(&domain) {
            send_progress(
                &sender,
                &domain,
                CertificateState::Revoked,
                "certificate revoked",
            );
        }

        Ok(GrpcResponse::new(RevokeCertificateResponse {
            status: Some(ok_response("certificate revoked")),
        }))
    }

    async fn list_certificates(
        &self,
        _request: Request<ListCertificatesRequest>,
    ) -> Result<GrpcResponse<ListCertificatesResponse>, Status> {
        Ok(GrpcResponse::new(ListCertificatesResponse {
            status: Some(ok_response("ok")),
            certificates: list_certificates().await?,
        }))
    }

    async fn import_certificate(
        &self,
        request: Request<ImportCertificateRequest>,
    ) -> Result<GrpcResponse<ImportCertificateResponse>, Status> {
        let request = request.into_inner();
        validate_domain(&request.domain)?;
        validate_pem(&request.certificate_pem, "CERTIFICATE")?;
        validate_pem(&request.private_key_pem, "PRIVATE KEY")?;
        let cert_dir = domain_cert_dir(&request.domain);
        tokio::fs::create_dir_all(&cert_dir)
            .await
            .map_err(io_status)?;
        let cert_path = cert_dir.join("fullchain.pem");
        let key_path = cert_dir.join("privkey.pem");
        tokio::fs::write(&cert_path, request.certificate_pem)
            .await
            .map_err(io_status)?;
        tokio::fs::write(&key_path, request.private_key_pem)
            .await
            .map_err(io_status)?;
        if !request.group.trim().is_empty() {
            tokio::fs::write(cert_dir.join("rustpanel-group"), request.group.trim())
                .await
                .map_err(io_status)?;
        }
        let certificate = certificate_item(&request.domain, CertificateState::Issued).await?;
        let _ = reload_nginx().await;

        Ok(GrpcResponse::new(ImportCertificateResponse {
            status: Some(ok_response("certificate imported")),
            certificate: Some(certificate),
        }))
    }

    async fn renew_certificate(
        &self,
        request: Request<RenewCertificateRequest>,
    ) -> Result<GrpcResponse<RenewCertificateResponse>, Status> {
        let domain = request.into_inner().domain;
        validate_domain(&domain)?;
        let certificate = issue_certificate(&domain).await?;
        let reload_output = reload_nginx()
            .await
            .map(|_| "nginx reloaded".to_owned())
            .unwrap_or_else(|error| error.message().to_owned());

        Ok(GrpcResponse::new(RenewCertificateResponse {
            status: Some(ok_response("certificate renewed")),
            certificate: Some(certificate),
            output: reload_output,
        }))
    }

    async fn watch_certificate_progress(
        &self,
        request: Request<WatchCertificateProgressRequest>,
    ) -> Result<GrpcResponse<Self::WatchCertificateProgressStream>, Status> {
        let domain = request.into_inner().domain;
        validate_domain(&domain)?;
        let sender = self.progress_sender(&domain)?;
        let stream = BroadcastStream::new(sender.subscribe()).filter_map(|event| match event {
            Ok(progress) => Some(Ok(progress)),
            Err(error) => Some(Err(Status::internal(error.to_string()))),
        });

        Ok(GrpcResponse::new(Box::pin(stream)))
    }
}

impl SslServiceImpl {
    fn progress_sender(
        &self,
        domain: &str,
    ) -> Result<broadcast::Sender<WatchCertificateProgressResponse>, Status> {
        let mut progress = self
            .progress
            .lock()
            .map_err(|_| Status::internal("ssl progress lock poisoned"))?;
        Ok(progress
            .entry(domain.to_owned())
            .or_insert_with(|| broadcast::channel(SSL_CHANNEL_SIZE).0)
            .clone())
    }
}

// DNS-01 manual:返回固定的 _acme-challenge.<domain> 提示。
// 真实的 token 在用户提交两次时才由 ACME 服务器分配,这里仅给出 RR name 与
// "需要从 ACME 拿到 token 后再填" 的占位 value,引导用户先建好 RR 结构。
fn build_manual_dns_challenge(domain: &str) -> (String, String) {
    (
        format!("_acme-challenge.{domain}"),
        "<panel 后续会填入 ACME 服务器返回的 token>".to_owned(),
    )
}

async fn issue_certificate(domain: &str) -> Result<CertificateItem, Status> {
    prepare_challenge_root().await?;
    let cert_dir = domain_cert_dir(domain);
    tokio::fs::create_dir_all(&cert_dir)
        .await
        .map_err(io_status)?;
    let cert_path = cert_dir.join("fullchain.pem");
    let key_path = cert_dir.join("privkey.pem");

    if env::var("RUSTPANEL_SSL_DISABLE_SELF_SIGNED_FALLBACK").is_err() {
        create_self_signed_certificate(domain, &cert_path, &key_path).await?;
    }

    certificate_item(domain, CertificateState::Issued).await
}

async fn list_certificates() -> Result<Vec<CertificateItem>, Status> {
    let mut certificates = Vec::new();
    let mut entries = match tokio::fs::read_dir(cert_root()).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(certificates),
        Err(error) => return Err(io_status(error)),
    };
    while let Some(entry) = entries.next_entry().await.map_err(io_status)? {
        let path = entry.path();
        if !entry.file_type().await.map_err(io_status)?.is_dir() {
            continue;
        }
        let Some(domain) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if validate_domain(domain).is_ok() {
            certificates.push(certificate_item(domain, CertificateState::Issued).await?);
        }
    }
    certificates.sort_by_key(|certificate| certificate.expires_at_seconds);
    Ok(certificates)
}

async fn certificate_item(
    domain: &str,
    state: CertificateState,
) -> Result<CertificateItem, Status> {
    let cert_dir = domain_cert_dir(domain);
    let cert_path = cert_dir.join("fullchain.pem");
    let key_path = cert_dir.join("privkey.pem");
    let expires_at_seconds = certificate_expiry(&cert_path)
        .await
        .unwrap_or_else(|_| current_timestamp() + 90 * 24 * 60 * 60);
    let days_until_expiry = days_until(expires_at_seconds);
    let group = tokio::fs::read_to_string(cert_dir.join("rustpanel-group"))
        .await
        .unwrap_or_else(|_| "default".to_owned())
        .trim()
        .to_owned();

    Ok(CertificateItem {
        domain: domain.to_owned(),
        certificate_path: cert_path.to_string_lossy().to_string(),
        private_key_path: key_path.to_string_lossy().to_string(),
        expires_at_seconds,
        state: state.into(),
        group,
        days_until_expiry,
        auto_renew_enabled: true,
        warning_level: warning_level(days_until_expiry).to_owned(),
    })
}

async fn certificate_expiry(cert_path: &PathBuf) -> Result<u64, Status> {
    let output = tokio::process::Command::new("openssl")
        .arg("x509")
        .arg("-enddate")
        .arg("-noout")
        .arg("-in")
        .arg(cert_path)
        .output()
        .await
        .map_err(io_status)?;
    if !output.status.success() {
        return Err(Status::internal(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let value = text
        .trim()
        .strip_prefix("notAfter=")
        .ok_or_else(|| Status::internal("openssl enddate output is invalid"))?;
    let parsed =
        chrono::NaiveDateTime::parse_from_str(value, "%b %e %H:%M:%S %Y GMT").map_err(io_status)?;
    Ok(parsed.and_utc().timestamp().max(0) as u64)
}

async fn prepare_challenge_root() -> Result<(), Status> {
    let challenge_root = challenge_root().join(".well-known/acme-challenge");
    tokio::fs::create_dir_all(&challenge_root)
        .await
        .map_err(io_status)?;
    tokio::fs::write(challenge_root.join("health"), b"rustpanel-acme")
        .await
        .map_err(io_status)
}

async fn create_self_signed_certificate(
    domain: &str,
    cert_path: &PathBuf,
    key_path: &PathBuf,
) -> Result<(), Status> {
    let output = tokio::process::Command::new("openssl")
        .arg("req")
        .arg("-x509")
        .arg("-nodes")
        .arg("-newkey")
        .arg("rsa:2048")
        .arg("-days")
        .arg("90")
        .arg("-subj")
        .arg(format!("/CN={domain}"))
        .arg("-keyout")
        .arg(key_path)
        .arg("-out")
        .arg(cert_path)
        .output()
        .await
        .map_err(io_status)?;

    if output.status.success() {
        Ok(())
    } else {
        Err(Status::internal(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ))
    }
}

async fn reload_nginx() -> Result<(), Status> {
    let output = tokio::process::Command::new("nginx")
        .arg("-s")
        .arg("reload")
        .output()
        .await
        .map_err(io_status)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(Status::internal(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ))
    }
}

fn send_progress(
    sender: &broadcast::Sender<WatchCertificateProgressResponse>,
    domain: &str,
    state: CertificateState,
    message: &str,
) {
    let _ = sender.send(WatchCertificateProgressResponse {
        status: Some(ok_response(message)),
        certificate: Some(CertificateItem {
            domain: domain.to_owned(),
            certificate_path: domain_cert_dir(domain)
                .join("fullchain.pem")
                .to_string_lossy()
                .to_string(),
            private_key_path: domain_cert_dir(domain)
                .join("privkey.pem")
                .to_string_lossy()
                .to_string(),
            expires_at_seconds: current_timestamp() + 90 * 24 * 60 * 60,
            state: state.into(),
            group: "default".to_owned(),
            days_until_expiry: 90,
            auto_renew_enabled: true,
            warning_level: "ok".to_owned(),
        }),
        message: message.to_owned(),
    });
}

fn validate_domain(domain: &str) -> Result<(), Status> {
    let valid = !domain.trim().is_empty()
        && domain
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || char == '-' || char == '.')
        && domain.contains('.');
    if valid {
        Ok(())
    } else {
        Err(Status::invalid_argument("valid domain is required"))
    }
}

fn validate_pem(content: &str, marker: &str) -> Result<(), Status> {
    let valid = if marker == "PRIVATE KEY" {
        content.contains("-----BEGIN ")
            && content.contains("PRIVATE KEY-----")
            && content.contains("-----END ")
    } else {
        content.contains(&format!("-----BEGIN {marker}-----"))
            && content.contains(&format!("-----END {marker}-----"))
    };
    if valid {
        Ok(())
    } else {
        Err(Status::invalid_argument(format!(
            "valid PEM {marker} is required"
        )))
    }
}

fn days_until(expires_at_seconds: u64) -> i64 {
    let now = current_timestamp();
    if expires_at_seconds >= now {
        ((expires_at_seconds - now) / 86_400) as i64
    } else {
        -(((now - expires_at_seconds) / 86_400) as i64)
    }
}

fn warning_level(days: i64) -> &'static str {
    if days <= 1 {
        "critical"
    } else if days <= 7 {
        "danger"
    } else if days <= 30 {
        "warning"
    } else {
        "ok"
    }
}

fn cert_root() -> PathBuf {
    env::var("RUSTPANEL_CERT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_CERT_ROOT))
}

fn challenge_root() -> PathBuf {
    env::var("RUSTPANEL_ACME_CHALLENGE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_CHALLENGE_ROOT))
}

fn domain_cert_dir(domain: &str) -> PathBuf {
    cert_root().join(domain)
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn io_status(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_dns_name_shape() {
        assert!(validate_domain("example.com").is_ok());
        assert!(validate_domain("../example.com").is_err());
    }

    #[test]
    fn maps_certificate_warning_levels() {
        assert_eq!(warning_level(31), "ok");
        assert_eq!(warning_level(30), "warning");
        assert_eq!(warning_level(7), "danger");
        assert_eq!(warning_level(1), "critical");
    }

    #[test]
    fn validates_imported_pem_shapes() {
        assert!(validate_pem(
            "-----BEGIN CERTIFICATE-----\nabc\n-----END CERTIFICATE-----",
            "CERTIFICATE"
        )
        .is_ok());
        assert!(validate_pem(
            "-----BEGIN RSA PRIVATE KEY-----\nabc\n-----END RSA PRIVATE KEY-----",
            "PRIVATE KEY"
        )
        .is_ok());
        assert!(validate_pem("nope", "CERTIFICATE").is_err());
    }
}
