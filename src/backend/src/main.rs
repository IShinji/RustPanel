use rustpanel_backend::{default_addr, init_tracing, serve};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    init_tracing();
    serve(default_addr()).await
}
