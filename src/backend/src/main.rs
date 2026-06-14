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

    // 一次性备份模式(供 cron 定时调度):跑完即退,不起服务。
    if let Some(source) = cli.backup_source.clone() {
        init_tracing();
        return rustpanel_backend::backup::run_oneshot_backup(
            source,
            cli.backup_target.clone(),
            cli.backup_name.clone(),
            cli.backup_keep,
        )
        .await
        .map_err(Into::into);
    }

    init_tracing();
    if cli.daemon {
        daemonize()?;
    }

    serve(cli.listen_addr()).await
}
