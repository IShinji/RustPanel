use std::{net::SocketAddr, path::PathBuf};

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "rustpanel-backend")]
#[command(about = "RustPanel backend daemon")]
pub struct Cli {
    #[arg(long, env = "RUSTPANEL_BACKEND_ADDR", default_value = "127.0.0.1:8080")]
    pub addr: SocketAddr,
    #[arg(long)]
    pub port: Option<u16>,
    #[arg(long)]
    pub daemon: bool,
    #[arg(long)]
    pub setup: bool,
    #[arg(
        long,
        env = "RUSTPANEL_BACKEND_BIN",
        default_value = "/usr/local/bin/rustpanel-backend"
    )]
    pub bin: PathBuf,
    // 一次性备份模式(供 cron 定时调度):设了 --backup-source 就跑一次备份后退出,
    // 不启动服务。--backup-target 留空=仅本地;--backup-keep N=同源只保留最新 N 份。
    #[arg(long)]
    pub backup_source: Option<String>,
    #[arg(long, default_value = "")]
    pub backup_target: String,
    #[arg(long, default_value = "")]
    pub backup_name: String,
    #[arg(long, default_value_t = 0)]
    pub backup_keep: u32,
    // restic 增量备份模式(供 cron 定时调度):设了 --restic-source 就跑一次 restic
    // 备份后退出。--restic-repo 为 restic 仓库(本地路径或 sftp:/s3:/rest: 后端);
    // 密码经 RESTIC_PASSWORD / RESTIC_PASSWORD_FILE env 传入;--restic-keep N=forget
    // 后保留最新 N 个快照(多来源共库时配 --restic-tag 隔离)。
    #[arg(long)]
    pub restic_source: Option<String>,
    #[arg(long, default_value = "")]
    pub restic_repo: String,
    #[arg(long, default_value_t = 0)]
    pub restic_keep: u32,
    #[arg(long, default_value = "")]
    pub restic_tag: String,
}

impl Cli {
    pub fn listen_addr(&self) -> SocketAddr {
        let mut addr = self.addr;
        if let Some(port) = self.port {
            addr.set_port(port);
        }

        addr
    }

    pub fn systemd_service(&self) -> String {
        format!(
            "[Unit]\n\
Description=RustPanel backend service\n\
After=network-online.target\n\
Wants=network-online.target\n\n\
[Service]\n\
Type=simple\n\
ExecStart={} --port {}\n\
Environment=RUSTPANEL_ENV=production\n\
Environment=RUSTPANEL_JWT_SECRET=replace-with-at-least-32-random-bytes\n\
Restart=always\n\
RestartSec=3\n\
NoNewPrivileges=true\n\n\
[Install]\n\
WantedBy=multi-user.target\n",
            self.bin.display(),
            self.listen_addr().port()
        )
    }
}

#[cfg(unix)]
pub fn daemonize() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    daemonize::Daemonize::new().start()?;
    Ok(())
}

#[cfg(not(unix))]
pub fn daemonize() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    Err("daemon mode is only supported on Unix platforms".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_overrides_addr_port() {
        let cli = Cli::try_parse_from([
            "rustpanel-backend",
            "--addr",
            "127.0.0.1:8080",
            "--port",
            "18080",
        ])
        .expect("cli");

        assert_eq!(cli.listen_addr().to_string(), "127.0.0.1:18080");
    }

    #[test]
    fn setup_generates_systemd_service_with_selected_port() {
        let cli =
            Cli::try_parse_from(["rustpanel-backend", "--setup", "--port", "18080"]).expect("cli");
        let service = cli.systemd_service();

        assert!(service.contains("ExecStart=/usr/local/bin/rustpanel-backend --port 18080"));
        assert!(service.contains("RUSTPANEL_JWT_SECRET=replace-with-at-least-32-random-bytes"));
    }
}
