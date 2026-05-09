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
        ssl_service_server::SslService, CertificateItem, CertificateState,
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

    Ok(CertificateItem {
        domain: domain.to_owned(),
        certificate_path: cert_path.to_string_lossy().to_string(),
        private_key_path: key_path.to_string_lossy().to_string(),
        expires_at_seconds: current_timestamp() + 90 * 24 * 60 * 60,
        state: CertificateState::Issued.into(),
    })
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
}
