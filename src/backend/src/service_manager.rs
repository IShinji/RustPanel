//! 服务托管抽象:把"装 unit / enable / disable / reload / 查状态"这几个操作
//! 从调用方(appstore.rs、site_service.rs、ssl.rs、security.rs、site.rs)里摘出来,
//! 统一走这一层,而不是各自 `Command::new("systemctl")`。
//!
//! 现在实现 systemd 和 OpenRC 两种后端。OpenRC 这边只用通用知识写出结构正确的
//! `openrc-run` 脚本和 `rc-service`/`rc-update` 调用,**没有在真实 OpenRC
//! 主机上跑过**——真实行为验证留给后续里程碑在真 Alpine LXC 容器里的端到端测试,
//! 这里的单测只覆盖纯函数部分(脚本渲染、路径计算),不是"已验证在生产可用"的
//! 保证。
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

/// 日志目录,`RUSTPANEL_OPENRC_LOG_DIR` env 可覆盖(测试 / 容器沙箱用)。
/// OpenRC 没有 journal,`supervise-daemon` 的 `output_log`/`error_log` 都指到
/// 这个目录下 `<name>.log`。
pub fn openrc_log_dir() -> PathBuf {
    env::var("RUSTPANEL_OPENRC_LOG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/log/rustpanel"))
}

pub fn openrc_log_path(name: &str) -> PathBuf {
    openrc_log_dir().join(format!("{name}.log"))
}

/// `ServiceSpec` → OpenRC `openrc-run` 脚本文本(`supervisor=supervise-daemon`,
/// 不是老式直接管 pidfile 的写法)。`name` 用来算日志文件路径,不写进脚本内容
/// 本身(脚本靠 `$RC_SVCNAME` 拿自己的名字,同一份内容可以给不同文件名的
/// symlink/copy 复用,跟 systemd 模板 unit 的 `%i` 是类似的考虑)。
pub fn render_openrc_script(spec: &ServiceSpec, name: &str) -> String {
    let mut script = String::new();
    script.push_str("#!/sbin/openrc-run\n\n");
    script.push_str(&format!("description=\"{}\"\n", spec.description));
    script.push_str(&format!("command=\"{}\"\n", spec.exec.display()));
    if !spec.args.is_empty() {
        script.push_str(&format!("command_args=\"{}\"\n", spec.args.join(" ")));
    }
    if let Some(workdir) = spec.workdir {
        script.push_str(&format!("directory=\"{}\"\n", workdir.display()));
    }
    script.push_str("supervisor=\"supervise-daemon\"\n");
    script.push_str("pidfile=\"/run/${RC_SVCNAME}.pid\"\n");
    // supervise-daemon 自己管respawn,不区分 always/on-failure——这里只用
    // RestartPolicy 控制有没有 respawn_max(on-failure 更保守,避免一直重启
    // 一个本来就该一次性退出的东西;always 不设上限)。
    if spec.restart == RestartPolicy::OnFailure {
        script.push_str("respawn_max=5\n");
    }
    script.push_str(&format!("respawn_delay={}\n", spec.restart_sec));
    let log_path = openrc_log_path(name);
    script.push_str(&format!("output_log=\"{}\"\n", log_path.display()));
    script.push_str(&format!("error_log=\"{}\"\n", log_path.display()));
    for (key, value) in spec.env {
        script.push_str(&format!("export {key}=\"{value}\"\n"));
    }
    script.push('\n');
    script.push_str("depend() {\n\tneed net\n\tafter net-online\n}\n");
    script
}

/// unit 文件写入目录,按当前 InitSystem 决定:systemd 写 `.service` 文本到
/// `/etc/systemd/system`(`RUSTPANEL_SYSTEMD_DIR` 覆盖);OpenRC 写可执行脚本到
/// `/etc/init.d`(`RUSTPANEL_OPENRC_INIT_DIR` 覆盖)。
pub fn unit_dir() -> PathBuf {
    match current() {
        InitSystem::OpenRc => env::var("RUSTPANEL_OPENRC_INIT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/etc/init.d")),
        _ => env::var("RUSTPANEL_SYSTEMD_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/etc/systemd/system")),
    }
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

async fn run_command(program: &str, args: &[&str]) -> Result<(), Status> {
    let output = tokio::process::Command::new(program)
        .args(args)
        .output()
        .await
        .map_err(io_status)?;
    if !output.status.success() {
        return Err(Status::unavailable(sanitize_status_text(format!(
            "{program} {} 失败: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        ))));
    }
    Ok(())
}

/// 把已经渲染好的 unit 文本原子写到 `unit_dir()/<filename>`。只负责落盘,
/// 不做 daemon-reload / enable——调用方按需自己组合(装新应用时通常紧接着
/// `daemon_reload` + `enable_now`;模板 unit 像 `sws@.service` 写一次不用再碰)。
/// OpenRC 的 init 脚本必须可执行,这里按当前 InitSystem 补 `+x`。
pub async fn write_unit(filename: &str, content: &str) -> Result<PathBuf, Status> {
    let dir = unit_dir();
    tokio::fs::create_dir_all(&dir).await.map_err(io_status)?;
    let path = dir.join(filename);
    crate::statefile::write_atomic(&path, content)
        .await
        .map_err(io_status)?;
    if current() == InitSystem::OpenRc {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = tokio::fs::metadata(&path)
                .await
                .map_err(io_status)?
                .permissions();
            perms.set_mode(0o755);
            tokio::fs::set_permissions(&path, perms)
                .await
                .map_err(io_status)?;
        }
    }
    Ok(path)
}

pub async fn daemon_reload() -> Result<(), Status> {
    match current() {
        InitSystem::Systemd => run_systemctl(&["daemon-reload"]).await,
        // OpenRC 脚本每次调用都是现读现跑,没有"重新加载 unit 定义"这个概念。
        _ => Ok(()),
    }
}

pub async fn enable_now(unit: &str) -> Result<(), Status> {
    match current() {
        InitSystem::Systemd => run_systemctl(&["enable", "--now", unit]).await,
        InitSystem::OpenRc => {
            run_command("rc-update", &["add", unit, "default"]).await?;
            run_command("rc-service", &[unit, "start"]).await
        }
        InitSystem::None => Err(unsupported("enable_now")),
    }
}

/// stop + disable。容错场景(卸载时 unit 可能已经不存在)请用
/// `let _ = service_manager::disable_now(...).await;` 忽略返回值。
pub async fn disable_now(unit: &str) -> Result<(), Status> {
    match current() {
        InitSystem::Systemd => run_systemctl(&["disable", "--now", unit]).await,
        InitSystem::OpenRc => {
            // stop 失败(比如本来就没跑)不该挡住 rc-update del,两步都尽量做。
            let stop = run_command("rc-service", &[unit, "stop"]).await;
            let del = run_command("rc-update", &["del", unit, "default"]).await;
            stop.and(del)
        }
        InitSystem::None => Err(unsupported("disable_now")),
    }
}

pub async fn restart(unit: &str) -> Result<(), Status> {
    match current() {
        InitSystem::Systemd => run_systemctl(&["restart", unit]).await,
        InitSystem::OpenRc => run_command("rc-service", &[unit, "restart"]).await,
        InitSystem::None => Err(unsupported("restart")),
    }
}

pub async fn reload(unit: &str) -> Result<(), Status> {
    match current() {
        InitSystem::Systemd => run_systemctl(&["reload", unit]).await,
        InitSystem::OpenRc => run_command("rc-service", &[unit, "reload"]).await,
        InitSystem::None => Err(unsupported("reload")),
    }
}

/// unit 自己定义「reload」是什么(没配 `ExecReload` 的话 systemd 会自动退化成
/// restart),不需要像 `reload`/`can_reload` 那样先查一遍能力。OpenRC 没有原生的
/// "reload 不行就 restart" 动词,这里显式做两步尝试。
pub async fn reload_or_restart(unit: &str) -> Result<(), Status> {
    match current() {
        InitSystem::Systemd => run_systemctl(&["reload-or-restart", unit]).await,
        InitSystem::OpenRc => {
            if reload(unit).await.is_ok() {
                Ok(())
            } else {
                restart(unit).await
            }
        }
        InitSystem::None => Err(unsupported("reload_or_restart")),
    }
}

/// 低层:直接跑一条 systemctl 命令拿原始 `Output`,给需要自定义报错文案的调用方
/// (比如 site_service.rs 要保留自己的双语错误格式)。只有 systemd 实现了这个——
/// OpenRC 调用方请用上面已经包好的高层函数。大多数场景请优先用那些。
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

/// `logs()`:systemd 上读 journal;OpenRC 没有 journal,尾读
/// `openrc_log_path(unit)` 指向的文件(`render_openrc_script` 渲染时就是写到
/// 这个固定路径,两边约定一致)。
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
        InitSystem::OpenRc => {
            let path = openrc_log_path(unit);
            tail_file(&path, lines).await
        }
        InitSystem::None => Err(unsupported("logs_tail")),
    }
}

/// 简单文件尾读:只从尾部 seek 读最多 `MAX_TAIL_BYTES`,大日志也不整文件进内存。
/// site_service.rs 的 `file_tail`/`read_file_tail` 是同样的思路但可见性和返回
/// 形状不一样(多了个"被截断"标记,这边暂时不需要),这里独立实现一份。
const MAX_TAIL_BYTES: u64 = 64 * 1024;

async fn tail_file(path: &std::path::Path, lines: u32) -> Result<String, Status> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || -> Result<String, Status> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(&path).map_err(|error| {
            Status::not_found(sanitize_status_text(format!(
                "打不开日志 {}: {error}",
                path.display()
            )))
        })?;
        let len = file.metadata().map_err(io_status)?.len();
        let start = len.saturating_sub(MAX_TAIL_BYTES);
        file.seek(SeekFrom::Start(start)).map_err(io_status)?;
        let mut buffer = Vec::new();
        file.take(MAX_TAIL_BYTES)
            .read_to_end(&mut buffer)
            .map_err(io_status)?;
        let text = String::from_utf8_lossy(&buffer);
        let text = if start > 0 {
            text.split_once('\n').map_or("", |(_, rest)| rest)
        } else {
            &text
        };
        let all: Vec<&str> = text.lines().collect();
        let keep = usize::try_from(lines).unwrap_or(usize::MAX).min(all.len());
        Ok(all[all.len() - keep..].join("\n"))
    })
    .await
    .map_err(|error| Status::internal(sanitize_status_text(error.to_string())))?
}

/// 面板自身的日志体积上限:busybox 没有 logrotate,OpenRC 的 `output_log`/
/// `error_log` 会无限增长,128MB 磁盘上这是最容易把机器撑爆的方式。超过
/// `max_bytes` 就只保留最后一半,原子替换(不是追加模式下的截断,避免正在
/// 写入的进程拿到一个长度对不上的 fd)。
pub async fn cap_log_file(path: &std::path::Path, max_bytes: u64) -> Result<(), Status> {
    let metadata = match tokio::fs::metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_status(error)),
    };
    if metadata.len() <= max_bytes {
        return Ok(());
    }
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || -> Result<(), Status> {
        use std::io::{Read, Seek, SeekFrom, Write};
        let keep = max_bytes / 2;
        let mut file = std::fs::File::open(&path).map_err(io_status)?;
        let len = file.metadata().map_err(io_status)?.len();
        let start = len.saturating_sub(keep);
        file.seek(SeekFrom::Start(start)).map_err(io_status)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).map_err(io_status)?;
        drop(file);
        let tmp = path.with_extension("log.rotating");
        let mut tmp_file = std::fs::File::create(&tmp).map_err(io_status)?;
        tmp_file.write_all(&buffer).map_err(io_status)?;
        tmp_file.sync_all().map_err(io_status)?;
        std::fs::rename(&tmp, &path).map_err(io_status)?;
        Ok(())
    })
    .await
    .map_err(|error| Status::internal(sanitize_status_text(error.to_string())))?
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

/// 查状态是只读操作,任何失败(包括 `InitSystem::None`)都当"没在跑",不往上
/// 抛错——和改之前 appstore.rs 里裸 `Command::new("systemctl").status()` 的
/// 既有语义一致。
pub async fn is_active(unit: &str) -> bool {
    match current() {
        InitSystem::Systemd => tokio::process::Command::new("systemctl")
            .args(["is-active", "--quiet", unit])
            .status()
            .await
            .map(|status| status.success())
            .unwrap_or(false),
        InitSystem::OpenRc => tokio::process::Command::new("rc-service")
            .args([unit, "status"])
            .status()
            .await
            .map(|status| status.success())
            .unwrap_or(false),
        InitSystem::None => false,
    }
}

/// rpxy 这类没配 `ExecReload` 的 unit,硬 reload 只会报一句无意义的
/// "Job type reload is not applicable"。调用方应该先查这个,`false` 就别调
/// `reload()`。同样是只读查询,失败当 `false`。
/// OpenRC 没有等价的能力查询,保守返回 `false`(调用方因此会跳过 reload,
/// 不会误发一个脚本没实现的动作;真要 reload-or-restart 语义请用
/// [`reload_or_restart`],它在 OpenRC 下会自己先试 reload 再退回 restart)。
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

    #[test]
    fn render_systemd_unit_matches_expected_shape() {
        let port = "18080".to_owned();
        let exec = std::path::PathBuf::from("/usr/local/bin/rustpanel-backend");
        let unit = render_systemd_unit(&ServiceSpec {
            description: "RustPanel backend service",
            exec: &exec,
            args: &["--port", &port],
            env: &[("MALLOC_ARENA_MAX", "2")],
            workdir: None,
            restart: RestartPolicy::Always,
            restart_sec: 3,
            timeout_stop_sec: 15,
        });
        assert!(unit.contains("ExecStart=/usr/local/bin/rustpanel-backend --port 18080\n"));
        assert!(unit.contains("Environment=MALLOC_ARENA_MAX=2\n"));
        assert!(unit.contains("Restart=always\n"));
        assert!(!unit.contains("WorkingDirectory"));
    }

    #[test]
    fn render_openrc_script_contains_supervise_daemon_directives() {
        let exec = std::path::PathBuf::from("/usr/local/bin/rustpanel-backend");
        let workdir = std::path::PathBuf::from("/www/wwwroot/rustpanel");
        env::set_var("RUSTPANEL_OPENRC_LOG_DIR", "/var/log/rustpanel-test");
        let script = render_openrc_script(
            &ServiceSpec {
                description: "RustPanel backend service",
                exec: &exec,
                args: &[],
                env: &[("MALLOC_ARENA_MAX", "2")],
                workdir: Some(&workdir),
                restart: RestartPolicy::OnFailure,
                restart_sec: 3,
                timeout_stop_sec: 15,
            },
            "rustpanel-backend",
        );
        env::remove_var("RUSTPANEL_OPENRC_LOG_DIR");

        assert!(script.starts_with("#!/sbin/openrc-run\n"));
        assert!(script.contains("command=\"/usr/local/bin/rustpanel-backend\"\n"));
        assert!(script.contains("supervisor=\"supervise-daemon\"\n"));
        assert!(script.contains("directory=\"/www/wwwroot/rustpanel\"\n"));
        assert!(script.contains("respawn_max=5\n")); // OnFailure 才设上限
        assert!(script.contains("respawn_delay=3\n"));
        assert!(script.contains("output_log=\"/var/log/rustpanel-test/rustpanel-backend.log\"\n"));
        assert!(script.contains("export MALLOC_ARENA_MAX=\"2\"\n"));
        assert!(script.contains("depend() {"));
    }

    #[test]
    fn render_openrc_script_omits_respawn_max_for_always() {
        let exec = std::path::PathBuf::from("/usr/local/bin/rustpanel-backend");
        let script = render_openrc_script(
            &ServiceSpec {
                description: "x",
                exec: &exec,
                args: &[],
                env: &[],
                workdir: None,
                restart: RestartPolicy::Always,
                restart_sec: 3,
                timeout_stop_sec: 15,
            },
            "x",
        );
        assert!(!script.contains("respawn_max"));
    }

    #[tokio::test]
    async fn cap_log_file_is_noop_under_threshold() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let path = dir.path().join("small.log");
        tokio::fs::write(&path, b"hello").await.expect("write");
        cap_log_file(&path, 1024).await.expect("cap");
        let content = tokio::fs::read_to_string(&path).await.expect("read");
        assert_eq!(content, "hello");
    }

    #[tokio::test]
    async fn cap_log_file_truncates_and_keeps_tail_above_threshold() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let path = dir.path().join("big.log");
        let content = "0123456789\n".repeat(2000); // ~22000 bytes
        tokio::fs::write(&path, &content).await.expect("write");
        cap_log_file(&path, 1000).await.expect("cap");
        let after = tokio::fs::read_to_string(&path).await.expect("read");
        // keep = max_bytes / 2,原样截取最后这么多字节,不做按行对齐
        assert_eq!(after.len(), 500);
        assert!(content.ends_with(&after));
    }

    #[tokio::test]
    async fn tail_file_reports_missing_file_as_not_found() {
        let err = tail_file(std::path::Path::new("/definitely/missing"), 10)
            .await
            .expect_err("should error");
        assert_eq!(err.code(), tonic::Code::NotFound);
    }
}
