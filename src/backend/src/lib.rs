#![allow(clippy::result_large_err)]

use std::{convert::Infallible, env, net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    body::{Body, Bytes},
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        Multipart, Path as AxumPath, Query, Request, State,
    },
    http::{
        header::{AUTHORIZATION, CONTENT_TYPE},
        HeaderValue, Response as HttpResponse, StatusCode,
    },
    middleware::{from_fn_with_state, Next},
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
pub mod proxy;
pub mod runtime;
pub mod security;
pub mod site;
pub mod ssl;
pub mod terminal;
pub mod workload;

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
    proxy_service_server::ProxyServiceServer,
    security_service_server::SecurityServiceServer,
    site_service_server::SiteServiceServer,
    ssl_service_server::SslServiceServer,
    system_service_server::{SystemService, SystemServiceServer},
    terminal_service_server::TerminalServiceServer,
    workload_service_server::WorkloadServiceServer,
    GetSystemInfoRequest, GetSystemInfoResponse, HealthCheckRequest, HealthCheckResponse,
    HealthStatus, ListRuntimeModulesRequest, ListRuntimeModulesResponse, Response, SystemStatus,
    TerminalResize,
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
        // sysinfo 已经在 monitor 里使用,这里直接读静态信息;
        // env::var("HOSTNAME") 在 systemd 启动的进程里通常未设置,会显示 unknown,
        // kernel_version 之前硬编码为 unknown,所以页面才出现"unknown · linux · unknown · x86_64"。
        let hostname = sysinfo::System::host_name()
            .or_else(|| env::var("HOSTNAME").ok())
            .unwrap_or_else(|| "unknown".to_owned());
        let operating_system = sysinfo::System::name()
            .map(|name| {
                if let Some(version) = sysinfo::System::os_version() {
                    format!("{name} {version}")
                } else {
                    name
                }
            })
            .unwrap_or_else(|| env::consts::OS.to_owned());
        let kernel_version = sysinfo::System::kernel_version()
            .or_else(sysinfo::System::long_os_version)
            .unwrap_or_else(|| "unknown".to_owned());

        Ok(GrpcResponse::new(GetSystemInfoResponse {
            status: Some(ok_response("ok")),
            hostname,
            operating_system,
            kernel_version,
            architecture: env::consts::ARCH.to_owned(),
        }))
    }

    async fn list_runtime_modules(
        &self,
        _request: GrpcRequest<ListRuntimeModulesRequest>,
    ) -> Result<GrpcResponse<ListRuntimeModulesResponse>, Status> {
        let modules = runtime::from_env();

        Ok(GrpcResponse::new(ListRuntimeModulesResponse {
            status: Some(ok_response("ok")),
            modules: modules.statuses().into_iter().map(Into::into).collect(),
            profile: modules.profile().to_owned(),
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
    let authority = auth::JwtAuthority::from_env()?;
    let auth_service = auth::AuthServiceImpl::from_env(authority.clone())?;
    let listener = TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;
    info!(%local_addr, "rustpanel backend listening");

    axum::serve(
        listener,
        Shared::new(multiplex_service_with_auth(auth_service, authority)),
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
    let authority = auth::JwtAuthority::from_env().expect("valid JWT authority");

    http_router_with_state(
        HttpState {
            files: file_service.manager(),
            monitor: monitor_service.collector(),
            security: security::SecurityConfig::from_env(),
        },
        authority,
    )
}

// 拆分公共路由(健康检查、面板静态资源、用户站点)和需要鉴权的 /api/* 路由,
// 公共路由必须能让前端在登录前加载,/api/* 一律走 token 校验。
fn http_router_with_state(state: HttpState, authority: auth::JwtAuthority) -> Router {
    let api_routes = Router::new()
        .route("/api/monitor/status", get(http_monitor_status))
        .route("/api/monitor/watch", get(http_monitor_watch))
        .route("/api/fs/upload", post(http_file_upload))
        .route("/api/fs/upload/chunk", post(http_file_upload_chunk))
        .route("/api/fs/download", get(http_file_download))
        .route("/api/terminal/ws", get(http_terminal_ws))
        .layer(from_fn_with_state(
            Arc::new(authority),
            require_http_auth_middleware,
        ));

    Router::new()
        .route("/healthz", get(http_health_check))
        .route("/sites/*path", get(http_builtin_static_site))
        .merge(api_routes)
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
    let authority = auth::JwtAuthority::from_env().expect("valid JWT authority");
    let auth_service =
        auth::AuthServiceImpl::from_env(authority.clone()).expect("valid auth service");

    multiplex_service_with_auth(auth_service, authority)
}

fn multiplex_service_with_auth(
    auth_service: auth::AuthServiceImpl,
    authority: auth::JwtAuthority,
) -> impl tower::Service<
    Request,
    Response = HttpResponse<Body>,
    Error = Infallible,
    Future: Send + 'static,
> + Clone {
    let monitor_service = monitor::MonitorServiceImpl::new();
    let file_service = files::FileSystemServiceImpl::new();
    let http = http_router_with_state(
        HttpState {
            files: file_service.manager(),
            monitor: monitor_service.collector(),
            security: security::SecurityConfig::from_env(),
        },
        authority.clone(),
    );
    // AuthService 自身必须公开,login 是无 token 状态下唯一能调用的 RPC;其余 15 个服务一律拦截。
    let auth_interceptor = auth::AuthInterceptor::new(authority);
    let grpc = Server::builder()
        .accept_http1(true)
        .layer(tonic_web::GrpcWebLayer::new())
        .add_service(AuthServiceServer::new(auth_service))
        .add_service(AuditServiceServer::with_interceptor(
            audit::AuditServiceImpl,
            auth_interceptor.clone(),
        ))
        .add_service(ClusterServiceServer::with_interceptor(
            cluster::ClusterServiceImpl,
            auth_interceptor.clone(),
        ))
        .add_service(SystemServiceServer::with_interceptor(
            SystemServiceImpl,
            auth_interceptor.clone(),
        ))
        .add_service(MonitorServiceServer::with_interceptor(
            monitor_service,
            auth_interceptor.clone(),
        ))
        .add_service(SecurityServiceServer::with_interceptor(
            security::SecurityServiceImpl::new(),
            auth_interceptor.clone(),
        ))
        .add_service(TerminalServiceServer::with_interceptor(
            terminal::TerminalServiceImpl,
            auth_interceptor.clone(),
        ))
        .add_service(FileSystemServiceServer::with_interceptor(
            file_service,
            auth_interceptor.clone(),
        ))
        .add_service(DockerServiceServer::with_interceptor(
            docker::DockerServiceImpl,
            auth_interceptor.clone(),
        ))
        .add_service(AppStoreServiceServer::with_interceptor(
            appstore::AppStoreServiceImpl,
            auth_interceptor.clone(),
        ))
        .add_service(SiteServiceServer::with_interceptor(
            site::SiteServiceImpl::new(),
            auth_interceptor.clone(),
        ))
        .add_service(SslServiceServer::with_interceptor(
            ssl::SslServiceImpl::default(),
            auth_interceptor.clone(),
        ))
        .add_service(DatabaseServiceServer::with_interceptor(
            database::DatabaseServiceImpl,
            auth_interceptor.clone(),
        ))
        .add_service(CronServiceServer::with_interceptor(
            cron::CronServiceImpl::new(),
            auth_interceptor.clone(),
        ))
        .add_service(WorkloadServiceServer::with_interceptor(
            workload::WorkloadServiceImpl::new(),
            auth_interceptor.clone(),
        ))
        .add_service(ProxyServiceServer::with_interceptor(
            proxy::ProxyServiceImpl::new(),
            auth_interceptor,
        ))
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

// 校验 /api/* 请求附带的 token。WebSocket / SSE 浏览器无法设置 Authorization 头,
// 所以也接受 ?token=<jwt> query 参数,二者命中其一即可。
async fn require_http_auth_middleware(
    State(authority): State<Arc<auth::JwtAuthority>>,
    request: Request,
    next: Next,
) -> Result<AxumResponse, StatusCode> {
    let token = extract_http_token(&request).ok_or(StatusCode::UNAUTHORIZED)?;
    authority
        .validate(&token)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    Ok(next.run(request).await)
}

fn extract_http_token(request: &Request) -> Option<String> {
    if let Some(value) = request.headers().get(AUTHORIZATION) {
        if let Ok(text) = value.to_str() {
            if let Some(token) = text.strip_prefix("Bearer ") {
                let trimmed = token.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_owned());
                }
            }
        }
    }
    let query = request.uri().query()?;
    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix("token=") {
            let decoded = urlencoding_decode(value);
            if !decoded.is_empty() {
                return Some(decoded);
            }
        }
    }
    None
}

// 简易 URL 解码,只处理 %XX,不引新依赖。query 里的 token 通常是 base64url JWT,
// 不会出现需要复杂解码的字符,所以这个最小实现够用。
fn urlencoding_decode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        let byte = bytes[idx];
        if byte == b'%' && idx + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_value(bytes[idx + 1]), hex_value(bytes[idx + 2])) {
                out.push((hi * 16 + lo) as char);
                idx += 3;
                continue;
            }
        }
        if byte == b'+' {
            out.push(' ');
        } else {
            out.push(byte as char);
        }
        idx += 1;
    }
    out
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
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

async fn http_builtin_static_site(
    AxumPath(path): AxumPath<String>,
) -> Result<AxumResponse, HttpError> {
    let Some(file_path) = site::builtin_site_file(&path)
        .await
        .map_err(HttpError::from_status)?
    else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let file = tokio::fs::File::open(&file_path)
        .await
        .map_err(HttpError::internal)?;
    let content_type = mime_guess::from_path(&file_path)
        .first_or_octet_stream()
        .to_string();
    let body = Body::from_stream(ReaderStream::new(file));
    let response = HttpResponse::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header("cache-control", "public, max-age=60")
        .body(body)
        .map_err(HttpError::internal)?;

    Ok(response)
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
    async fn multiplexed_server_accepts_grpc_health_check_with_bearer_token() {
        // 构造一个有效 token,验证 SystemService.HealthCheck 在挂上 AuthInterceptor 后仍能正常访问
        let authority = auth::JwtAuthority::from_env().expect("authority");
        let issued = authority.issue("admin").expect("issue token");
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
        let mut request = GrpcRequest::new(HealthCheckRequest {});
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {}", issued.token)
                .parse()
                .expect("metadata"),
        );
        let response = client
            .health_check(request)
            .await
            .expect("health check")
            .into_inner();

        assert_eq!(response.health, HealthStatus::Serving as i32);

        server.abort();
    }

    #[tokio::test]
    async fn multiplexed_server_rejects_grpc_without_token() {
        // 同一个 SystemService.HealthCheck 不带 token 必须返回 Unauthenticated,确认拦截器生效
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
        let status = client
            .health_check(HealthCheckRequest {})
            .await
            .expect_err("must require auth");

        assert_eq!(status.code(), tonic::Code::Unauthenticated);

        server.abort();
    }

    #[tokio::test]
    async fn http_api_routes_require_bearer_token() {
        // 通过 multiplex_service 入口测,确认 /api/* 没 token 返回 401,带合法 token 通过中间件
        let authority = auth::JwtAuthority::from_env().expect("authority");
        let issued = authority.issue("admin").expect("issue");
        let service = multiplex_service();

        let unauth_response = service
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/monitor/status")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(unauth_response.status(), StatusCode::UNAUTHORIZED);

        let auth_response = service
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/monitor/status")
                    .header("authorization", format!("Bearer {}", issued.token))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        // 路由通过中间件,处理函数返回 200 / OK 即可证明鉴权放行;实际 body 由 monitor 模块决定
        assert_ne!(auth_response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn http_healthz_remains_public() {
        // /healthz 必须保持公开,外部探针不需要 token
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

    #[test]
    fn extract_http_token_supports_header_and_query() {
        let header_request = HttpRequest::builder()
            .uri("/api/monitor/status")
            .header("authorization", "Bearer abc.def.ghi")
            .body(Body::empty())
            .expect("request");
        assert_eq!(
            extract_http_token(&header_request).as_deref(),
            Some("abc.def.ghi")
        );

        let query_request = HttpRequest::builder()
            .uri("/api/terminal/ws?cwd=/&token=qrs.tuv.wxy")
            .body(Body::empty())
            .expect("request");
        assert_eq!(
            extract_http_token(&query_request).as_deref(),
            Some("qrs.tuv.wxy")
        );

        let none_request = HttpRequest::builder()
            .uri("/api/monitor/status")
            .body(Body::empty())
            .expect("request");
        assert!(extract_http_token(&none_request).is_none());
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
