#![allow(clippy::result_large_err)]

use std::{convert::Infallible, env, net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    body::{Body, Bytes},
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        Multipart, Query, Request, State,
    },
    http::{header::CONTENT_TYPE, HeaderValue, Response as HttpResponse, StatusCode},
    response::sse::{Event, KeepAlive},
    response::{IntoResponse, Response as AxumResponse},
    routing::{get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio_stream::wrappers::IntervalStream;
use tokio_util::io::ReaderStream;
use tonic::{transport::Server, Request as GrpcRequest, Response as GrpcResponse, Status};
use tower::{make::Shared, service_fn, ServiceExt};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub mod appstore;
pub mod audit;
pub mod auth;
pub mod cli;
pub mod cluster;
pub mod cron;
pub mod database;
pub mod docker;
pub mod files;
pub mod monitor;
pub mod security;
pub mod site;
pub mod ssl;
pub mod terminal;

mod assets;
pub mod proto {
    pub mod rustpanel {
        pub mod v1 {
            tonic::include_proto!("rustpanel.v1");
        }
    }
}

use proto::rustpanel::v1::{
    app_store_service_server::AppStoreServiceServer,
    audit_service_server::AuditServiceServer,
    auth_service_server::AuthServiceServer,
    cluster_service_server::ClusterServiceServer,
    cron_service_server::CronServiceServer,
    database_service_server::DatabaseServiceServer,
    docker_service_server::DockerServiceServer,
    file_system_service_server::FileSystemServiceServer,
    monitor_service_server::MonitorServiceServer,
    security_service_server::SecurityServiceServer,
    site_service_server::SiteServiceServer,
    ssl_service_server::SslServiceServer,
    system_service_server::{SystemService, SystemServiceServer},
    terminal_service_server::TerminalServiceServer,
    GetSystemInfoRequest, GetSystemInfoResponse, HealthCheckRequest, HealthCheckResponse,
    HealthStatus, Response, SystemStatus, TerminalResize,
};

#[derive(Clone, Debug, Default)]
pub struct SystemServiceImpl;

#[tonic::async_trait]
impl SystemService for SystemServiceImpl {
    async fn health_check(
        &self,
        _request: GrpcRequest<HealthCheckRequest>,
    ) -> Result<GrpcResponse<HealthCheckResponse>, Status> {
        Ok(GrpcResponse::new(HealthCheckResponse {
            status: Some(ok_response("ok")),
            health: HealthStatus::Serving.into(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        }))
    }

    async fn get_system_info(
        &self,
        _request: GrpcRequest<GetSystemInfoRequest>,
    ) -> Result<GrpcResponse<GetSystemInfoResponse>, Status> {
        Ok(GrpcResponse::new(GetSystemInfoResponse {
            status: Some(ok_response("ok")),
            hostname: env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_owned()),
            operating_system: env::consts::OS.to_owned(),
            kernel_version: "unknown".to_owned(),
            architecture: env::consts::ARCH.to_owned(),
        }))
    }
}

#[derive(Debug, Serialize)]
struct HttpStatus {
    status: &'static str,
    service: &'static str,
    version: &'static str,
}

pub fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().compact())
        .init();
}

pub fn default_addr() -> SocketAddr {
    env::var("RUSTPANEL_BACKEND_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8080".to_owned())
        .parse()
        .expect("RUSTPANEL_BACKEND_ADDR must be a valid socket address")
}

pub async fn serve(addr: SocketAddr) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let auth_service = auth::AuthServiceImpl::from_env(auth::JwtAuthority::from_env()?)?;
    let listener = TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;
    info!(%local_addr, "rustpanel backend listening");

    axum::serve(
        listener,
        Shared::new(multiplex_service_with_auth(auth_service)),
    )
    .await?;

    Ok(())
}

#[derive(Clone)]
struct HttpState {
    files: files::FileManager,
    monitor: Arc<monitor::SystemCollector>,
    security: security::SecurityConfig,
}

pub fn http_router() -> Router {
    let monitor_service = monitor::MonitorServiceImpl::new();
    let file_service = files::FileSystemServiceImpl::new();

    http_router_with_state(HttpState {
        files: file_service.manager(),
        monitor: monitor_service.collector(),
        security: security::SecurityConfig::from_env(),
    })
}

fn http_router_with_state(state: HttpState) -> Router {
    Router::new()
        .route("/healthz", get(http_health_check))
        .route("/api/monitor/status", get(http_monitor_status))
        .route("/api/monitor/watch", get(http_monitor_watch))
        .route("/api/fs/upload", post(http_file_upload))
        .route("/api/fs/upload/chunk", post(http_file_upload_chunk))
        .route("/api/fs/download", get(http_file_download))
        .route("/api/terminal/ws", get(http_terminal_ws))
        .fallback(static_fallback)
        .with_state(state)
}

#[cfg(test)]
fn multiplex_service() -> impl tower::Service<
    Request,
    Response = HttpResponse<Body>,
    Error = Infallible,
    Future: Send + 'static,
> + Clone {
    let auth_service = auth::AuthServiceImpl::from_env(
        auth::JwtAuthority::from_env().expect("valid JWT authority"),
    )
    .expect("valid auth service");

    multiplex_service_with_auth(auth_service)
}

fn multiplex_service_with_auth(
    auth_service: auth::AuthServiceImpl,
) -> impl tower::Service<
    Request,
    Response = HttpResponse<Body>,
    Error = Infallible,
    Future: Send + 'static,
> + Clone {
    let monitor_service = monitor::MonitorServiceImpl::new();
    let file_service = files::FileSystemServiceImpl::new();
    let http = http_router_with_state(HttpState {
        files: file_service.manager(),
        monitor: monitor_service.collector(),
        security: security::SecurityConfig::from_env(),
    });
    let grpc = Server::builder()
        .accept_http1(true)
        .layer(tonic_web::GrpcWebLayer::new())
        .add_service(AuthServiceServer::new(auth_service))
        .add_service(AuditServiceServer::new(audit::AuditServiceImpl))
        .add_service(ClusterServiceServer::new(cluster::ClusterServiceImpl))
        .add_service(SystemServiceServer::new(SystemServiceImpl))
        .add_service(MonitorServiceServer::new(monitor_service))
        .add_service(SecurityServiceServer::new(
            security::SecurityServiceImpl::new(),
        ))
        .add_service(TerminalServiceServer::new(terminal::TerminalServiceImpl))
        .add_service(FileSystemServiceServer::new(file_service))
        .add_service(DockerServiceServer::new(docker::DockerServiceImpl))
        .add_service(AppStoreServiceServer::new(appstore::AppStoreServiceImpl))
        .add_service(SiteServiceServer::new(site::SiteServiceImpl::new()))
        .add_service(SslServiceServer::new(ssl::SslServiceImpl::default()))
        .add_service(DatabaseServiceServer::new(database::DatabaseServiceImpl))
        .add_service(CronServiceServer::new(cron::CronServiceImpl::new()))
        .into_service();

    service_fn(move |request: Request| {
        let grpc = grpc.clone();
        let http = http.clone();

        async move {
            if is_grpc_request(&request) {
                let request = request.map(|body| {
                    body.map_err(|error| Status::internal(error.to_string()))
                        .boxed_unsync()
                });
                let response = grpc
                    .oneshot(request)
                    .await
                    .map(|response| response.map(Body::new))
                    .unwrap_or_else(|error| internal_error_response(error.to_string()));

                Ok(response)
            } else {
                let response = http
                    .oneshot(request)
                    .await
                    .unwrap_or_else(|error| match error {});

                Ok(response)
            }
        }
    })
}

pub(crate) fn ok_response(message: impl Into<String>) -> Response {
    Response {
        code: 0,
        message: message.into(),
        data: None,
    }
}

pub(crate) fn error_response(code: i32, message: impl Into<String>) -> Response {
    Response {
        code,
        message: message.into(),
        data: None,
    }
}

fn is_grpc_request(request: &Request) -> bool {
    request
        .headers()
        .get(CONTENT_TYPE)
        .is_some_and(is_grpc_content_type)
}

fn is_grpc_content_type(value: &HeaderValue) -> bool {
    value.to_str().is_ok_and(|content_type| {
        content_type.starts_with("application/grpc")
            || content_type.starts_with("application/grpc-web")
    })
}

async fn http_health_check() -> impl IntoResponse {
    Json(HttpStatus {
        status: "ok",
        service: "rustpanel-backend",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn http_monitor_status(
    State(state): State<HttpState>,
) -> Result<Json<MonitorSnapshot>, HttpError> {
    state
        .monitor
        .snapshot()
        .map(MonitorSnapshot::from)
        .map(Json)
        .map_err(HttpError::from_status)
}

async fn http_monitor_watch(
    State(state): State<HttpState>,
) -> axum::response::Sse<impl futures_core::Stream<Item = Result<Event, Infallible>> + Send + 'static>
{
    let stream =
        IntervalStream::new(tokio::time::interval(Duration::from_secs(1))).map(move |_| {
            let snapshot = state
                .monitor
                .snapshot()
                .map(MonitorSnapshot::from)
                .ok()
                .and_then(|snapshot| serde_json::to_string(&snapshot).ok())
                .unwrap_or_else(|| "{}".to_owned());

            Ok(Event::default().event("system_status").data(snapshot))
        });

    axum::response::Sse::new(stream).keep_alive(KeepAlive::default())
}

#[derive(Debug, Deserialize)]
struct FilePathQuery {
    path: String,
}

async fn http_file_upload(
    State(state): State<HttpState>,
    Query(query): Query<FilePathQuery>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, HttpError> {
    let mut saved = Vec::new();
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(HttpError::bad_request)?
    {
        let target_path = if let Some(file_name) = field.file_name().map(sanitize_upload_name) {
            let base = query.path.trim_end_matches('/');
            state
                .files
                .resolve_for_write(&format!("{base}/{file_name}"))
                .map_err(HttpError::from_status)?
        } else {
            state
                .files
                .resolve_for_write(&query.path)
                .map_err(HttpError::from_status)?
        };
        let mut file = tokio::fs::File::create(&target_path)
            .await
            .map_err(HttpError::internal)?;
        while let Some(chunk) = field.chunk().await.map_err(HttpError::bad_request)? {
            file.write_all(&chunk).await.map_err(HttpError::internal)?;
        }
        saved.push(state.files.public_path(&target_path));
    }

    Ok(Json(UploadResponse { saved }))
}

#[derive(Debug, Deserialize)]
struct ChunkUploadQuery {
    path: String,
    upload_id: String,
    chunk_index: u32,
    total_chunks: u32,
    file_name: String,
}

async fn http_file_upload_chunk(
    State(state): State<HttpState>,
    Query(query): Query<ChunkUploadQuery>,
    body: Bytes,
) -> Result<impl IntoResponse, HttpError> {
    if query.total_chunks == 0 || query.chunk_index >= query.total_chunks {
        return Err(HttpError::bad_request("invalid chunk index"));
    }
    let chunk_root = std::env::temp_dir()
        .join("rustpanel-upload-chunks")
        .join(sanitize_upload_name(&query.upload_id));
    tokio::fs::create_dir_all(&chunk_root)
        .await
        .map_err(HttpError::internal)?;
    tokio::fs::write(chunk_root.join(query.chunk_index.to_string()), body)
        .await
        .map_err(HttpError::internal)?;

    if query.chunk_index + 1 < query.total_chunks {
        return Ok(Json(UploadResponse { saved: Vec::new() }));
    }

    let target_path = state
        .files
        .resolve_for_write(&format!(
            "{}/{}",
            query.path.trim_end_matches('/'),
            sanitize_upload_name(&query.file_name)
        ))
        .map_err(HttpError::from_status)?;
    let mut target = tokio::fs::File::create(&target_path)
        .await
        .map_err(HttpError::internal)?;
    for index in 0..query.total_chunks {
        let chunk = tokio::fs::read(chunk_root.join(index.to_string()))
            .await
            .map_err(HttpError::internal)?;
        target
            .write_all(&chunk)
            .await
            .map_err(HttpError::internal)?;
    }
    let _ = tokio::fs::remove_dir_all(chunk_root).await;

    Ok(Json(UploadResponse {
        saved: vec![state.files.public_path(&target_path)],
    }))
}

async fn http_file_download(
    State(state): State<HttpState>,
    Query(query): Query<FilePathQuery>,
) -> Result<impl IntoResponse, HttpError> {
    let path = state
        .files
        .resolve_existing(&query.path)
        .map_err(HttpError::from_status)?;
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(HttpError::internal)?;
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "download".to_owned());
    let body = Body::from_stream(ReaderStream::new(file));
    let response = HttpResponse::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "application/octet-stream")
        .header(
            "content-disposition",
            format!("attachment; filename=\"{}\"", file_name.replace('"', "")),
        )
        .body(body)
        .map_err(HttpError::internal)?;

    Ok(response)
}

#[derive(Debug, Deserialize)]
struct TerminalQuery {
    cwd: Option<String>,
}

async fn http_terminal_ws(
    State(state): State<HttpState>,
    Query(query): Query<TerminalQuery>,
    upgrade: WebSocketUpgrade,
) -> impl IntoResponse {
    let cwd = query
        .cwd
        .as_deref()
        .and_then(|path| state.files.resolve_existing(path).ok());
    upgrade.on_upgrade(move |socket| handle_terminal_socket(socket, cwd))
}

async fn handle_terminal_socket(socket: WebSocket, cwd: Option<std::path::PathBuf>) {
    let Ok((session, mut output)) = terminal::spawn_web_terminal_with_cwd(cwd.as_deref()) else {
        return;
    };
    let (mut sender, mut receiver) = socket.split();

    loop {
        tokio::select! {
            incoming = receiver.next() => {
                match incoming {
                    Some(Ok(WsMessage::Text(text))) => {
                        if let Ok(resize) = serde_json::from_str::<TerminalResizeMessage>(&text) {
                            if resize.r#type == "resize" {
                                let _ = session.resize(TerminalResize {
                                    cols: resize.cols,
                                    rows: resize.rows,
                                });
                                continue;
                            }
                        }
                        if session.write_data(text.as_bytes()).is_err() {
                            break;
                        }
                    }
                    Some(Ok(WsMessage::Binary(data))) => {
                        if session.write_data(&data).is_err() {
                            break;
                        }
                    }
                    Some(Ok(WsMessage::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
            outgoing = output.next() => {
                match outgoing {
                    Some(data) => {
                        if sender.send(WsMessage::Binary(data)).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }
}

async fn static_fallback(State(state): State<HttpState>, request: Request) -> AxumResponse {
    let path = request.uri().path();
    let access_path = state.security.panel_access_path().await;
    let asset_path = if access_path == "/" || path.starts_with("/assets/") {
        Some(path)
    } else if path == access_path {
        Some("/")
    } else if let Some(stripped) = path.strip_prefix(&format!("{access_path}/")) {
        Some(if stripped.is_empty() { "/" } else { stripped })
    } else {
        None
    };

    asset_path
        .map(|path| assets::static_response(path).into_response())
        .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
}

fn internal_error_response(message: String) -> HttpResponse<Body> {
    let body = Body::from(format!("internal service error: {message}"));

    HttpResponse::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(body)
        .expect("static internal error response must be valid")
}

#[derive(Debug, Serialize)]
struct UploadResponse {
    saved: Vec<String>,
}

#[derive(Debug, Serialize)]
struct MonitorSnapshot {
    timestamp_seconds: u64,
    cpu_usage_percent: f32,
    cpu_cores: usize,
    memory_total_bytes: u64,
    memory_used_bytes: u64,
    memory_available_bytes: u64,
    load_one_minute: f64,
    load_five_minutes: f64,
    load_fifteen_minutes: f64,
    network_received_bytes: u64,
    network_transmitted_bytes: u64,
    disk_total_space_bytes: u64,
    disk_available_space_bytes: u64,
    uptime_seconds: u64,
}

impl From<SystemStatus> for MonitorSnapshot {
    fn from(status: SystemStatus) -> Self {
        let memory = status.memory.unwrap_or_default();
        let load = status.load_average.unwrap_or_default();
        Self {
            timestamp_seconds: status.timestamp_seconds,
            cpu_usage_percent: status.cpu_usage_percent,
            cpu_cores: status.cpu_cores.len(),
            memory_total_bytes: memory.total_bytes,
            memory_used_bytes: memory.used_bytes,
            memory_available_bytes: memory.available_bytes,
            load_one_minute: load.one_minute,
            load_five_minutes: load.five_minutes,
            load_fifteen_minutes: load.fifteen_minutes,
            network_received_bytes: status
                .networks
                .iter()
                .map(|network| network.received_bytes)
                .sum(),
            network_transmitted_bytes: status
                .networks
                .iter()
                .map(|network| network.transmitted_bytes)
                .sum(),
            disk_total_space_bytes: status.disks.iter().map(|disk| disk.total_space_bytes).sum(),
            disk_available_space_bytes: status
                .disks
                .iter()
                .map(|disk| disk.available_space_bytes)
                .sum(),
            uptime_seconds: status.uptime_seconds,
        }
    }
}

#[derive(Debug, Deserialize)]
struct TerminalResizeMessage {
    r#type: String,
    cols: u32,
    rows: u32,
}

#[derive(Debug)]
struct HttpError {
    status: StatusCode,
    message: String,
}

impl HttpError {
    fn bad_request(error: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: error.to_string(),
        }
    }

    fn internal(error: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        }
    }

    fn from_status(status: Status) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: status.message().to_owned(),
        }
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> AxumResponse {
        (self.status, self.message).into_response()
    }
}

fn sanitize_upload_name(name: &str) -> String {
    name.chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() || matches!(char, '.' | '-' | '_') {
                char
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request as HttpRequest;
    use proto::rustpanel::v1::system_service_client::SystemServiceClient;
    use tower::ServiceExt;

    #[tokio::test]
    async fn system_health_check_returns_serving_status() {
        let service = SystemServiceImpl;
        let response = service
            .health_check(GrpcRequest::new(HealthCheckRequest {}))
            .await
            .expect("health check")
            .into_inner();

        assert_eq!(response.status.expect("status").code, 0);
        assert_eq!(response.health, HealthStatus::Serving as i32);
    }

    #[tokio::test]
    async fn http_health_check_returns_ok() {
        let response = http_router()
            .oneshot(
                HttpRequest::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn multiplexed_server_accepts_grpc_health_check() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, Shared::new(multiplex_service()))
                .await
                .expect("server");
        });

        let mut client = SystemServiceClient::connect(format!("http://{addr}"))
            .await
            .expect("connect");
        let response = client
            .health_check(HealthCheckRequest {})
            .await
            .expect("health check")
            .into_inner();

        assert_eq!(response.health, HealthStatus::Serving as i32);

        server.abort();
    }

    #[test]
    fn grpc_detection_uses_content_type() {
        let grpc_request = HttpRequest::builder()
            .header(CONTENT_TYPE, "application/grpc+proto")
            .body(Body::empty())
            .expect("request");
        let http_request = HttpRequest::builder()
            .header(CONTENT_TYPE, "application/json")
            .body(Body::empty())
            .expect("request");

        assert!(is_grpc_request(&grpc_request));
        assert!(!is_grpc_request(&http_request));
    }
}
