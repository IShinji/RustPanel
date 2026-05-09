use clap::Parser;
use rustpanel_backend::{
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

    serve(cli.listen_addr()).await
}
