use rustpanel_backend::{auth::JwtAuthority, default_addr, init_tracing, serve};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    init_tracing();
    let _jwt_authority = JwtAuthority::from_env()?;
    serve(default_addr()).await
}
