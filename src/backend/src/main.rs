use clap::Parser;
use rustpanel_backend::{
    auth::JwtAuthority,
    cli::{daemonize, Cli},
    init_tracing, serve,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cli = Cli::parse();
    if cli.setup {
        println!("{}", cli.systemd_service());
        return Ok(());
    }

    init_tracing();
    if cli.daemon {
        daemonize()?;
    }

    let _jwt_authority = JwtAuthority::from_env()?;
    serve(cli.listen_addr()).await
}
