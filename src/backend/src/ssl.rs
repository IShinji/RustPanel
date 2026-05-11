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
        ssl_service_server::SslService, AcmeChallengeType, AcmeSettings as ProtoAcmeSettings,
        CertificateItem, CertificateState, GetAcmeSettingsRequest, GetAcmeSettingsResponse,
        ImportCertificateRequest, ImportCertificateResponse, ListCertificatesRequest,
        ListCertificatesResponse, RenewCertificateRequest, RenewCertificateResponse,
        RequestCertificateRequest, RequestCertificateResponse, RevokeCertificateRequest,
        RevokeCertificateResponse, UpdateAcmeSettingsRequest, UpdateAcmeSettingsResponse,
        WatchCertificateProgressRequest, WatchCertificateProgressResponse,
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
        let mut request = request.into_inner();
        validate_domain(&request.domain)?;
        // 调用方没传 email 时,回落到面板级 AcmeSettings.contact_email。
        // 设计意图:UI 不应该每次申请都问邮箱,装面板时一次性配好就行。
        if request.email.trim().is_empty() {
            let settings = crate::acme::read_settings()
                .await
                .map_err(|e| Status::internal(format!("read acme settings: {e}")))?;
            if settings.contact_email.trim().is_empty() {
                return Err(Status::failed_precondition(
                    "未设置 ACME 联系邮箱;请在面板里填一次真实邮箱(Settings → ACME)",
                ));
            }
            request.email = settings.contact_email;
        }
        if !request.email.contains('@') {
            return Err(Status::invalid_argument("valid email is required"));
        }
        if crate::acme::is_forbidden_email_domain(&request.email) {
            // LE 服务端必拒,提前拦下,避免一次冤枉的网络往返 + 把
            // 真实错误吞在 instant-acme 的不太友好的 wrap 里。
            return Err(Status::invalid_argument(
                "联系邮箱不能是 example.com / example.org / example.net 域(Let's Encrypt 已禁用)",
            ));
        }
        let sender = self.progress_sender(&request.domain)?;

        let challenge_type =
            AcmeChallengeType::try_from(request.challenge_type).unwrap_or_default();
        // 安全默认:UNSPECIFIED 一律按 DNS-01 处理。
        // 原因:此前 challenge_type 不指定时会 fall through 到 HTTP-01 路径,
        //   而 HTTP-01 在 NAT VPS 上根本拿不到 80 端口 → 退到 issue_certificate
        //   的**自签证书回退** → UI 显示"一键成功",但浏览器红色不信任,
        //   等于骗用户。这里把"未指定"等价于 DNS-01,避免误导。
        // 真要走 HTTP-01 必须显式传 AcmeChallengeType::Http01。
        let effective_challenge = if challenge_type == AcmeChallengeType::Unspecified {
            AcmeChallengeType::Dns01
        } else {
            challenge_type
        };
        // NAT VPS 上 80 端口通常拿不到,DNS-01 是唯一可行的挑战方式。
        // P8-04-5:走真实 instant-acme 状态机,第一次返回真 TXT,第二次完成
        // 验证拿到证书。manual 模式默认走 staging,生产需要 RUSTPANEL_ACME_PRODUCTION=1。
        if effective_challenge == AcmeChallengeType::Dns01 {
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
                    "dns-01: 调用 ACME 服务器创建 order",
                );
                let outcome = crate::acme::request_or_resume_dns01(&request.domain, &request.email)
                    .await
                    .map_err(|e| Status::internal(format!("acme error: {e}")))?;
                match outcome {
                    crate::acme::RequestOutcome::Challenge(c) => {
                        send_progress(
                            &sender,
                            &request.domain,
                            CertificateState::Pending,
                            "等待用户添加 TXT 记录后再调用一次申请继续",
                        );
                        return Ok(GrpcResponse::new(RequestCertificateResponse {
                            status: Some(ok_response(
                                "请把下方 TXT 记录加到 DNS,生效后再点一次申请",
                            )),
                            certificate: None,
                            dns_record_name: c.record_name,
                            dns_record_value: c.record_value,
                        }));
                    }
                    crate::acme::RequestOutcome::Issued(cert) => {
                        let cert_dir = domain_cert_dir(&request.domain);
                        let (cert_path, key_path) =
                            crate::acme::install_certificate(&request.domain, &cert, &cert_root())
                                .await
                                .map_err(|e| Status::internal(format!("acme install: {e}")))?;
                        let _ = cert_dir; // 兼容旧路径
                        let _ = (cert_path, key_path);
                        clear_bootstrap_marker(&request.domain).await;
                        send_progress(
                            &sender,
                            &request.domain,
                            CertificateState::Issued,
                            "certificate stored via ACME",
                        );
                        let _ = reload_nginx().await;
                        let item =
                            certificate_item(&request.domain, CertificateState::Issued).await?;
                        return Ok(GrpcResponse::new(RequestCertificateResponse {
                            status: Some(ok_response("certificate issued")),
                            certificate: Some(item),
                            dns_record_name: String::new(),
                            dns_record_value: String::new(),
                        }));
                    }
                }
            }
            // cloudflare/route53 等 provider 留待后续实现
            return Err(Status::unimplemented(format!(
                "DNS provider {provider} 暂未实现,请使用 manual 模式手动添加 TXT 记录"
            )));
        }

        // 显式 HTTP-01:走真实 instant-acme 状态机,面板自动把 token 写
        // 到 webroot 里。前提是 nginx 已经在 80 端口监听该域名 + webroot
        // /.well-known/acme-challenge/ 可达。NAT VPS 80 端口不暴露的情况
        // 下,validation 会卡在 pending → 24 次 5 秒轮询后报 Timeout,
        // 用户该转用 DNS-01。
        prepare_challenge_root().await?;
        send_progress(
            &sender,
            &request.domain,
            CertificateState::Pending,
            "http-01: ACME 创建 order,等待服务器拉取 challenge 文件",
        );
        let webroot = challenge_root();
        let cert = crate::acme::request_http01_blocking(&request.domain, &request.email, &webroot)
            .await
            .map_err(|e| Status::internal(format!("acme http-01: {e}")))?;
        let (cert_path, key_path) =
            crate::acme::install_certificate(&request.domain, &cert, &cert_root())
                .await
                .map_err(|e| Status::internal(format!("acme install: {e}")))?;
        let _ = (cert_path, key_path);
        clear_bootstrap_marker(&request.domain).await;
        send_progress(
            &sender,
            &request.domain,
            CertificateState::Issued,
            "certificate stored via ACME (http-01)",
        );
        let _ = reload_nginx().await;
        let item = certificate_item(&request.domain, CertificateState::Issued).await?;
        Ok(GrpcResponse::new(RequestCertificateResponse {
            status: Some(ok_response("certificate issued via http-01")),
            certificate: Some(item),
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
        clear_bootstrap_marker(&request.domain).await;
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
        // 之前直接调 issue_certificate 写自签覆盖,等于把用户的 Let's Encrypt
        // 真证书踩成自签 —— **不是续签**。改成走真实 DNS-01 ACME:第一次
        // 返回 TXT(用 output 字段透出),第二次完成。
        // email 从面板 AcmeSettings 里取(GetAcmeSettings/UpdateAcmeSettings),
        // 之前用 `admin@<domain>` 是占位 → 用户域名要是 example.com 还会被
        // LE forbiddenDomains 拒。
        let settings = crate::acme::read_settings()
            .await
            .map_err(|e| Status::internal(format!("read acme settings: {e}")))?;
        let email = settings.contact_email.trim().to_owned();
        if email.is_empty() {
            return Err(Status::failed_precondition(
                "未设置 ACME 联系邮箱;请去面板设置里填一次真实邮箱再续签",
            ));
        }
        let outcome = crate::acme::request_or_resume_dns01(&domain, &email)
            .await
            .map_err(|e| Status::internal(format!("acme renew: {e}")))?;
        match outcome {
            crate::acme::RequestOutcome::Challenge(c) => {
                Ok(GrpcResponse::new(RenewCertificateResponse {
                    status: Some(ok_response("请把下方 TXT 加到 DNS 后再点一次续签完成签发")),
                    certificate: None,
                    output: format!("TXT 名称: {}\nTXT 值:  {}", c.record_name, c.record_value),
                }))
            }
            crate::acme::RequestOutcome::Issued(cert) => {
                crate::acme::install_certificate(&domain, &cert, &cert_root())
                    .await
                    .map_err(|e| Status::internal(format!("acme install: {e}")))?;
                clear_bootstrap_marker(&domain).await;
                let item = certificate_item(&domain, CertificateState::Issued).await?;
                let reload_output = reload_nginx()
                    .await
                    .map(|_| "nginx reloaded".to_owned())
                    .unwrap_or_else(|error| error.message().to_owned());
                Ok(GrpcResponse::new(RenewCertificateResponse {
                    status: Some(ok_response("certificate renewed via DNS-01")),
                    certificate: Some(item),
                    output: reload_output,
                }))
            }
        }
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

    async fn get_acme_settings(
        &self,
        _request: Request<GetAcmeSettingsRequest>,
    ) -> Result<GrpcResponse<GetAcmeSettingsResponse>, Status> {
        let settings = crate::acme::read_settings()
            .await
            .map_err(|e| Status::internal(format!("read acme settings: {e}")))?;
        Ok(GrpcResponse::new(GetAcmeSettingsResponse {
            status: Some(ok_response("ok")),
            settings: Some(ProtoAcmeSettings {
                contact_email: settings.contact_email,
            }),
        }))
    }

    async fn update_acme_settings(
        &self,
        request: Request<UpdateAcmeSettingsRequest>,
    ) -> Result<GrpcResponse<UpdateAcmeSettingsResponse>, Status> {
        let proto = request
            .into_inner()
            .settings
            .ok_or_else(|| Status::invalid_argument("settings is required"))?;
        let trimmed = proto.contact_email.trim().to_owned();
        // 空字符串等同清除;非空则做合法性校验。
        if !trimmed.is_empty() {
            if !trimmed.contains('@') {
                return Err(Status::invalid_argument("contact_email 不是有效邮箱(缺 @)"));
            }
            if crate::acme::is_forbidden_email_domain(&trimmed) {
                return Err(Status::invalid_argument(
                    "contact_email 不能是 example.com / example.org / example.net 域",
                ));
            }
        }
        let to_save = crate::acme::AcmeSettings {
            contact_email: trimmed,
        };
        crate::acme::write_settings(&to_save)
            .await
            .map_err(|e| Status::internal(format!("write acme settings: {e}")))?;
        Ok(GrpcResponse::new(UpdateAcmeSettingsResponse {
            status: Some(ok_response("saved")),
            settings: Some(ProtoAcmeSettings {
                contact_email: to_save.contact_email,
            }),
        }))
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
    // bootstrap marker 存在 → 这是占位自签证书,真 ACME 还没签下来。
    // 不能让 UI 显示"已签发剩余 30 天",会误导用户以为面板真签了。
    // 改成 state=PENDING + warning="self-signed-bootstrap",UI 负责把
    // 它渲染成"待签发 (placeholder)"而不是绿色已就绪。
    let is_bootstrap = tokio::fs::try_exists(&bootstrap_marker_path(domain))
        .await
        .unwrap_or(false);
    let effective_state = if is_bootstrap {
        CertificateState::Pending
    } else {
        state
    };
    let warning = if is_bootstrap {
        "self-signed-bootstrap".to_owned()
    } else {
        warning_level(days_until_expiry).to_owned()
    };

    Ok(CertificateItem {
        domain: domain.to_owned(),
        certificate_path: cert_path.to_string_lossy().to_string(),
        private_key_path: key_path.to_string_lossy().to_string(),
        expires_at_seconds,
        state: effective_state.into(),
        group,
        days_until_expiry,
        auto_renew_enabled: true,
        warning_level: warning,
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
    // 之前走 `openssl req -x509 ...` CLI,在没装 openssl 的最小镜像上
    // 直接 NotFound(os error 2)。换成纯 Rust 的 rcgen(已经在
    // Cargo.toml 里,acme.rs 也用它做 CSR),零外部二进制依赖。
    let mut params = rcgen::CertificateParams::new(vec![domain.to_owned()])
        .map_err(|e| Status::internal(format!("rcgen params: {e}")))?;
    params.distinguished_name = rcgen::DistinguishedName::new();
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, domain);
    // rcgen 0.13 默认把 not_after 塞到 4096-01-01,这玩意一进 UI 就显示
    //   "剩余 755921 天" —— 用户看了完全摸不着头脑、还以为面板瞎签了。
    // bootstrap 自签证书本就是临时占位(给 nginx -t 通过),设 30 天就够;
    // 真 LE 签发会原地覆盖,不会出现"30 天后自签真的过期"的情况;
    // 真出现了也是真实情况(用户没完成 ACME 流程),UI 早就该报警。
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::hours(1);
    params.not_after = now + time::Duration::days(30);
    let keypair =
        rcgen::KeyPair::generate().map_err(|e| Status::internal(format!("rcgen key: {e}")))?;
    let cert = params
        .self_signed(&keypair)
        .map_err(|e| Status::internal(format!("rcgen sign: {e}")))?;
    if let Some(parent) = cert_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(io_status)?;
    }
    tokio::fs::write(cert_path, cert.pem())
        .await
        .map_err(io_status)?;
    tokio::fs::write(key_path, keypair.serialize_pem())
        .await
        .map_err(io_status)?;
    Ok(())
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

/// 给其他模块(rpxy / leaf / vSMTP 等)用的"按域名取 fullchain + privkey
/// 路径"统一入口。证书是否实际存在由调用方再 metadata 验证 ——
/// 这里只是路径合约,不做 IO。
pub(crate) fn acme_cert_paths(domain: &str) -> (PathBuf, PathBuf) {
    let dir = domain_cert_dir(domain);
    (dir.join("fullchain.pem"), dir.join("privkey.pem"))
}

/// 创建站点时启用 SSL,但真证书还没签的"bootstrap snakeoil"机制:
/// 在 acme_cert_paths 对应的位置写一份**自签证书**,让 nginx -t 不致
/// 因为找不到 ssl_certificate 文件而拒绝启动。后续 ACME(DNS-01 / HTTP-01)
/// 签发成功会原地覆盖,nginx reload 后浏览器就拿到真证书。
/// 已存在时直接返回,不覆盖已签的真证书。
pub(crate) async fn bootstrap_self_signed_if_missing(domain: &str) -> Result<(), Status> {
    let (cert_path, key_path) = acme_cert_paths(domain);
    if tokio::fs::try_exists(&cert_path).await.unwrap_or(false) {
        return Ok(());
    }
    if let Some(parent) = cert_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(io_status)?;
    }
    create_self_signed_certificate(domain, &cert_path, &key_path).await?;
    // 留个 sidecar 标记 → list_certificates 知道这只是占位、不是真 LE
    // 证书,UI 能据此显示"待签发"而不是"已签发剩余 30 天"。
    // 真 ACME 签完 install_certificate 时会清掉这个 marker(下面的
    // clear_bootstrap_marker)。
    let marker = bootstrap_marker_path(domain);
    let _ = tokio::fs::write(&marker, b"self-signed placeholder\n").await;
    Ok(())
}

fn bootstrap_marker_path(domain: &str) -> PathBuf {
    domain_cert_dir(domain).join("rustpanel-bootstrap")
}

/// 真 ACME 签发完成 / 用户导入证书后调用,清掉 bootstrap 标记。
/// 找不到文件(从来没 bootstrap 过)也没事。
pub(crate) async fn clear_bootstrap_marker(domain: &str) {
    let _ = tokio::fs::remove_file(bootstrap_marker_path(domain)).await;
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
    fn acme_cert_paths_returns_per_domain_fullchain_and_privkey() {
        let (cert, key) = acme_cert_paths("example.com");
        assert!(cert.ends_with("example.com/fullchain.pem"));
        assert!(key.ends_with("example.com/privkey.pem"));
        // cert 与 key 必须在同一目录下,后续模块(rpxy / leaf)可以
        // 把目录作为单位 watch
        assert_eq!(cert.parent(), key.parent());
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
