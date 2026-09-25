use clap::Parser;
use rustpanel_backend::{
    cli::{daemonize, Cli},
    init_tracing, serve,
};

type BoxError = Box<dyn std::error::Error + Send + Sync>;

fn main() -> Result<(), BoxError> {
    let cli = Cli::parse();
    // fork 必须发生在 tokio 起线程之前:fork 后子进程里只剩调用线程,
    // 之前在 runtime 内 daemonize 会让 worker 线程全部"消失"。
    let serving = !cli.setup && cli.backup_source.is_none() && cli.restic_source.is_none();
    if serving && cli.daemon {
        daemonize()?;
    }

    // micro 档默认只开 2 个 worker(TOKIO_WORKER_THREADS 可覆盖,tokio 自己读);
    // 其它档位交给 tokio 按核数决定。
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder.enable_all();
    if let Some(workers) = rustpanel_backend::runtime::tokio_worker_threads() {
        builder.worker_threads(workers);
    }
    builder.build()?.block_on(run(cli))
}

async fn run(cli: Cli) -> Result<(), BoxError> {
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

    // restic 增量备份模式(供 cron 定时调度):跑完即退,不起服务。
    if let Some(source) = cli.restic_source.clone() {
        init_tracing();
        return rustpanel_backend::backup::run_restic_backup(
            source,
            cli.restic_repo.clone(),
            cli.restic_keep,
            cli.restic_tag.clone(),
        )
        .await
        .map_err(Into::into);
    }

    init_tracing();
    serve(cli.listen_addr()).await
}
