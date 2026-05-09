use std::{convert::Infallible, env, net::SocketAddr};

use axum::{
    body::Body,
    extract::Request,
    http::{header::CONTENT_TYPE, HeaderValue, Response as HttpResponse, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use http_body_util::BodyExt;
use serde::Serialize;
use tokio::net::TcpListener;
use tonic::{transport::Server, Request as GrpcRequest, Response as GrpcResponse, Status};
use tower::{make::Shared, service_fn, ServiceExt};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub mod auth;

pub mod proto {
    pub mod rustpanel {
        pub mod v1 {
            tonic::include_proto!("rustpanel.v1");
        }
    }
}

use proto::rustpanel::v1::{
    system_service_server::{SystemService, SystemServiceServer},
    GetSystemInfoRequest, GetSystemInfoResponse, HealthCheckRequest, HealthCheckResponse,
    HealthStatus, Response,
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
    let listener = TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;
    info!(%local_addr, "rustpanel backend listening");

    axum::serve(listener, Shared::new(multiplex_service())).await?;

    Ok(())
}

pub fn http_router() -> Router {
    Router::new()
        .route("/healthz", get(http_health_check))
        .fallback(http_fallback)
}

fn multiplex_service() -> impl tower::Service<
    Request,
    Response = HttpResponse<Body>,
    Error = Infallible,
    Future: Send + 'static,
> + Clone {
    let grpc = Server::builder()
        .add_service(SystemServiceServer::new(SystemServiceImpl))
        .into_service();
    let http = http_router();

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

fn ok_response(message: impl Into<String>) -> Response {
    Response {
        code: 0,
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
    value
        .to_str()
        .is_ok_and(|content_type| content_type.starts_with("application/grpc"))
}

async fn http_health_check() -> impl IntoResponse {
    Json(HttpStatus {
        status: "ok",
        service: "rustpanel-backend",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn http_fallback() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(HttpStatus {
            status: "not_found",
            service: "rustpanel-backend",
            version: env!("CARGO_PKG_VERSION"),
        }),
    )
}

fn internal_error_response(message: String) -> HttpResponse<Body> {
    let body = Body::from(format!("internal service error: {message}"));

    HttpResponse::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(body)
        .expect("static internal error response must be valid")
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
