//! 服务托管抽象:把"装 unit / enable / disable / reload / 查状态"这几个操作
//! 从调用方(appstore.rs、site_service.rs、ssl.rs、security.rs、site.rs)里摘出来,
//! 统一走这一层,而不是各自 `Command::new("systemctl")`。
//!
//! 现在只实现 systemd 这一种后端 —— OpenRC 留给后续里程碑(需要先有可移植的
//! socket activation 监督者替代节俭模式,再补 OpenRC 的 `supervise-daemon` 渲染),
//! 这次只是把分散的调用收口,不改变任何现有行为。
//!
//! `InitSystem` 的探测用 `/run/systemd/system` 目录是否存在(即 systemd 自己
//! `sd_booted()` 的判断方式),而不是"有没有 systemctl 命令"——很多容器环境里
//! systemctl 命令在,但 PID 1 根本不是 systemd。

use std::{env, path::PathBuf, sync::OnceLock};

use tonic::Status;

use crate::appstore::sanitize_status_text;
use crate::trf;

fn io_status(error: impl std::fmt::Display) -> Status {
    Status::internal(sanitize_status_text(error.to_string()))
}

/// 本机的初始化系统,进程启动后探测一次、全程不变。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitSystem {
    Systemd,
    /// 预留,后续里程碑实现。
    OpenRc,
    /// 两者都不是(比如 install.sh 的 `--daemon` 模式)。
    None,
}

/// `RUSTPANEL_INIT_SYSTEM` 覆盖探测结果(值:`systemd`/`openrc`/`none`),给测试和
/// 容器沙箱用——CI 的自托管 runner 自己就是真 systemd 主机,不覆盖的话测不出
/// 非 systemd 分支的行为。
fn detect() -> InitSystem {
    match env::var("RUSTPANEL_INIT_SYSTEM").ok().as_deref() {
        Some("systemd") => return InitSystem::Systemd,
        Some("openrc") => return InitSystem::OpenRc,
        Some("none") => return InitSystem::None,
        _ => {}
    }
    if std::path::Path::new("/run/systemd/system").is_dir() {
        InitSystem::Systemd
    } else if std::path::Path::new("/sbin/openrc").exists()
        || std::path::Path::new("/sbin/rc-service").exists()
    {
        InitSystem::OpenRc
    } else {
        InitSystem::None
    }
}

/// 不缓存的探测,给测试用——`current()` 的 `OnceLock` 在同一个测试进程里只会
/// 探测一次,没法在不同测试之间切换;生产代码应该用 `current()`。
#[cfg(test)]
fn detect_uncached() -> InitSystem {
    detect()
}

pub fn current() -> InitSystem {
    static INIT: OnceLock<InitSystem> = OnceLock::new();
    *INIT.get_or_init(detect)
}

fn unsupported(op: &str) -> Status {
    Status::unavailable(sanitize_status_text(trf!(
        "{}: 当前主机既不是 systemd 也不是已支持的 OpenRC 环境",
        "{}: this host is neither systemd nor a supported OpenRC environment",
        op
    )))
}

/// 常驻服务重启策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartPolicy {
    Always,
    OnFailure,
}

impl RestartPolicy {
    fn as_systemd_str(self) -> &'static str {
        match self {
            RestartPolicy::Always => "always",
            RestartPolicy::OnFailure => "on-failure",
        }
    }
}

/// 一个常驻服务的描述,跟具体 init 系统无关——调用方只填这个,渲染成什么格式
/// 由 `render_systemd_unit`(以后还有 OpenRC 的渲染函数)决定。
/// 字段按需添加:现在只覆盖 `cli.rs::systemd_service()` 需要的部分,deploy/
/// systemd-units.sh 那套更完整的生成逻辑(节俭模式 socket 依赖等)留给后续
/// 把它也迁过来的里程碑再扩展这个结构体。
pub struct ServiceSpec<'a> {
    pub description: &'a str,
    pub exec: &'a std::path::Path,
    pub args: &'a [&'a str],
    pub env: &'a [(&'a str, &'a str)],
    pub workdir: Option<&'a std::path::Path>,
    pub restart: RestartPolicy,
    pub restart_sec: u32,
    pub timeout_stop_sec: u32,
}

/// `ServiceSpec` → systemd `.service` unit 文本。
pub fn render_systemd_unit(spec: &ServiceSpec) -> String {
    let mut unit = String::new();
    unit.push_str("[Unit]\n");
    unit.push_str(&format!("Description={}\n", spec.description));
    unit.push_str("After=network-online.target\n");
    unit.push_str("Wants=network-online.target\n");
    unit.push('\n');
    unit.push_str("[Service]\n");
    unit.push_str("Type=simple\n");
    if let Some(workdir) = spec.workdir {
        unit.push_str(&format!("WorkingDirectory={}\n", workdir.display()));
    }
    let args = if spec.args.is_empty() {
        String::new()
    } else {
        format!(" {}", spec.args.join(" "))
    };
    unit.push_str(&format!("ExecStart={}{args}\n", spec.exec.display()));
    for (key, value) in spec.env {
        unit.push_str(&format!("Environment={key}={value}\n"));
    }
    unit.push_str(&format!("Restart={}\n", spec.restart.as_systemd_str()));
    unit.push_str(&format!("RestartSec={}\n", spec.restart_sec));
    unit.push_str(&format!("TimeoutStopSec={}\n", spec.timeout_stop_sec));
    unit.push_str("NoNewPrivileges=true\n");
    unit.push('\n');
    unit.push_str("[Install]\n");
    unit.push_str("WantedBy=multi-user.target\n");
    unit
}

/// unit 文件写入目录,`RUSTPANEL_SYSTEMD_DIR` env 可覆盖(测试 / 容器沙箱用)。
/// 迁自 appstore.rs::systemd_unit_dir,行为不变。
pub fn unit_dir() -> PathBuf {
    env::var("RUSTPANEL_SYSTEMD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/etc/systemd/system"))
}

async fn run_systemctl(args: &[&str]) -> Result<(), Status> {
    let output = tokio::process::Command::new("systemctl")
        .args(args)
        .output()
        .await
        .map_err(io_status)?;
    if !output.status.success() {
        return Err(Status::unavailable(sanitize_status_text(format!(
            "systemctl {} 失败: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        ))));
    }
    Ok(())
}

/// 把已经渲染好的 unit 文本原子写到 `unit_dir()/<filename>`。只负责落盘,
/// 不做 daemon-reload / enable——调用方按需自己组合(装新应用时通常紧接着
/// `daemon_reload` + `enable_now`;模板 unit 像 `sws@.service` 写一次不用再碰)。
pub async fn write_unit(filename: &str, content: &str) -> Result<PathBuf, Status> {
    let dir = unit_dir();
    tokio::fs::create_dir_all(&dir).await.map_err(io_status)?;
    let path = dir.join(filename);
    crate::statefile::write_atomic(&path, content)
        .await
        .map_err(io_status)?;
    Ok(path)
}

pub async fn daemon_reload() -> Result<(), Status> {
    match current() {
        InitSystem::Systemd => run_systemctl(&["daemon-reload"]).await,
        _ => Ok(()), // OpenRC/None 没有等价概念,no-op
    }
}

pub async fn enable_now(unit: &str) -> Result<(), Status> {
    match current() {
        InitSystem::Systemd => run_systemctl(&["enable", "--now", unit]).await,
        _ => Err(unsupported("enable_now")),
    }
}

/// stop + disable。容错场景(卸载时 unit 可能已经不存在)请用
/// `let _ = service_manager::disable_now(...).await;` 忽略返回值。
pub async fn disable_now(unit: &str) -> Result<(), Status> {
    match current() {
        InitSystem::Systemd => run_systemctl(&["disable", "--now", unit]).await,
        _ => Err(unsupported("disable_now")),
    }
}

pub async fn restart(unit: &str) -> Result<(), Status> {
    match current() {
        InitSystem::Systemd => run_systemctl(&["restart", unit]).await,
        _ => Err(unsupported("restart")),
    }
}

pub async fn reload(unit: &str) -> Result<(), Status> {
    match current() {
        InitSystem::Systemd => run_systemctl(&["reload", unit]).await,
        _ => Err(unsupported("reload")),
    }
}

/// unit 自己定义「reload」是什么(没配 `ExecReload` 的话 systemd 会自动退化成
/// restart),不需要像 `reload`/`can_reload` 那样先查一遍能力。
pub async fn reload_or_restart(unit: &str) -> Result<(), Status> {
    match current() {
        InitSystem::Systemd => run_systemctl(&["reload-or-restart", unit]).await,
        _ => Err(unsupported("reload_or_restart")),
    }
}

/// 低层:直接跑一条 systemctl 命令拿原始 `Output`,给需要自定义报错文案的调用方
/// (比如 site_service.rs 要保留自己的双语错误格式)。大多数场景请优先用上面
/// 这些已经包好成功/失败判断的高层函数。
pub async fn run_raw(args: &[&str]) -> Result<std::process::Output, Status> {
    match current() {
        InitSystem::Systemd => tokio::process::Command::new("systemctl")
            .args(args)
            .output()
            .await
            .map_err(io_status),
        _ => Err(unsupported("run_raw")),
    }
}

/// `logs()`:systemd 上读 journal;OpenRC 没有 journal,后续里程碑改成尾读
/// `ServiceSpec.log_file`。
pub async fn logs_tail(unit: &str, lines: u32) -> Result<String, Status> {
    match current() {
        InitSystem::Systemd => {
            let output = tokio::process::Command::new("journalctl")
                .args(["-u", unit, "--no-pager", "-o", "short-iso", "-n"])
                .arg(lines.to_string())
                .output()
                .await
                .map_err(io_status)?;
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        }
        _ => Err(unsupported("logs_tail")),
    }
}

/// 依次尝试几条候选 shell 命令(各自独立 `sh -c` 执行),第一条成功就返回;
/// 全部失败则把最后一条的 stderr 包成 `Status::internal`(不做百分号转义,
/// 和迁移前 security.rs 里这段的既有行为一致)。用于 sshd 这类服务名在不同
/// 发行版下可能不一样、systemctl 之外还能退到 sysvinit `service` 的场景——
/// 等价于 shell 里的 `A || B || C`,只是用 Rust 控制流代替一行长 `sh -c`。
pub async fn try_commands(commands: &[&str]) -> Result<(), Status> {
    let mut last_stderr = String::new();
    for command in commands {
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .output()
            .await
            .map_err(io_status)?;
        if output.status.success() {
            return Ok(());
        }
        last_stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    }
    Err(Status::internal(last_stderr))
}

/// 查状态是只读操作,任何失败(包括非 systemd 主机)都当"没在跑",不往上抛错——
/// 和改之前 appstore.rs 里裸 `Command::new("systemctl").status()` 的既有语义一致。
pub async fn is_active(unit: &str) -> bool {
    if current() != InitSystem::Systemd {
        return false;
    }
    tokio::process::Command::new("systemctl")
        .args(["is-active", "--quiet", unit])
        .status()
        .await
        .map(|status| status.success())
        .unwrap_or(false)
}

/// rpxy 这类没配 `ExecReload` 的 unit,硬 reload 只会报一句无意义的
/// "Job type reload is not applicable"。调用方应该先查这个,`false` 就别调
/// `reload()`。同样是只读查询,失败当 `false`。
pub async fn can_reload(unit: &str) -> bool {
    if current() != InitSystem::Systemd {
        return false;
    }
    let Ok(output) = tokio::process::Command::new("systemctl")
        .args(["show", "-p", "CanReload", "--value", unit])
        .output()
        .await
    else {
        return false;
    };
    String::from_utf8_lossy(&output.stdout).trim() == "yes"
}

#[cfg(test)]
mod tests {
    use super::*;

    // 一个测试函数里顺序断言三种取值,不拆成多个 #[test] ——
    // cargo test 默认并行跑测试线程,拆开会在同一个进程里互相覆盖这个 env var。
    #[test]
    fn detect_reads_override_env_var() {
        env::set_var("RUSTPANEL_INIT_SYSTEM", "systemd");
        assert_eq!(detect_uncached(), InitSystem::Systemd);
        env::set_var("RUSTPANEL_INIT_SYSTEM", "openrc");
        assert_eq!(detect_uncached(), InitSystem::OpenRc);
        env::set_var("RUSTPANEL_INIT_SYSTEM", "none");
        assert_eq!(detect_uncached(), InitSystem::None);
        env::remove_var("RUSTPANEL_INIT_SYSTEM");
    }

    #[test]
    fn current_is_cached_across_calls() {
        assert_eq!(current(), current());
    }
}
