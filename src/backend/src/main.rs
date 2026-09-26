use clap::Parser;
use rustpanel_backend::{
    cli::{daemonize, Cli},
    init_tracing, serve_with_listener,
};

type BoxError = Box<dyn std::error::Error + Send + Sync>;

fn main() -> Result<(), BoxError> {
    // 显式选 ring 作为进程级 CryptoProvider:依赖里若同时出现 ring 与 aws-lc-rs,
    // rustls 自动选择会 panic(ACME / 出站 HTTPS 一发起就崩)。已安装过则忽略。
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cli = Cli::parse();
    // fork 必须发生在 tokio 起线程之前:fork 后子进程里只剩调用线程,
    // 之前在 runtime 内 daemonize 会让 worker 线程全部"消失"。
    let serving = cli.is_serving();
    if serving && cli.daemon {
        daemonize()?;
    }
    // systemd socket activation:同样必须在起线程之前(要改 env)。
    let activated = if serving {
        rustpanel_backend::frugal::take_activated_listener()?
    } else {
        None
    };

    // micro 档默认只开 2 个 worker(TOKIO_WORKER_THREADS 可覆盖,tokio 自己读);
    // 其它档位交给 tokio 按核数决定。
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder.enable_all();
    if let Some(workers) = rustpanel_backend::runtime::tokio_worker_threads() {
        builder.worker_threads(workers);
    }
    builder.build()?.block_on(run(cli, activated))
}

async fn run(cli: Cli, activated: Option<std::net::TcpListener>) -> Result<(), BoxError> {
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

    // 系统 cron 回调:跑一次计划任务后退出。
    if let Some(task_id) = cli.run_cron_task.as_deref() {
        init_tracing();
        return rustpanel_backend::cron::run_oneshot_task(task_id)
            .await
            .map_err(Into::into);
    }

    // systemd timer 回调:续签临期证书后退出。
    if cli.renew_certs {
        init_tracing();
        return rustpanel_backend::ssl::renew_due_certificates()
            .await
            .map_err(Into::into);
    }

    init_tracing();
    serve_with_listener(cli.listen_addr(), activated).await
}
