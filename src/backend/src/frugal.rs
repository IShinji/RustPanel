//! 节俭模式:systemd socket activation + 空闲自动退出。
//!
//! 低配主机上面板大部分时间没人用,却常驻几十 MB。节俭模式把监听 socket 交给
//! systemd(`rustpanel-backend.socket`),面板空闲 N 分钟后正常退出(exit 0),
//! 下一个连接到来时 systemd 再把它拉起来。只有**确实从 systemd 拿到 socket**
//! 时才允许空闲退出 —— 否则退出等于面板失联。
//!
//! 「忙」的判定:在途请求(含 SSE / gRPC 流式响应,直到响应体被丢弃)、WS 终端、
//! 站点部署任务、受托管的 workload / proxy 子进程(它们在面板的 cgroup 里,
//! 面板退出会被 systemd 一起杀掉)、待触发的回滚计时器。任何一个存在都不退出。

use std::{
    env,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        OnceLock,
    },
    time::{Duration, Instant},
};

/// micro 档未显式配置时的默认空闲退出分钟数。
pub const DEFAULT_MICRO_IDLE_EXIT_MINUTES: u64 = 10;

static BUSY: AtomicUsize = AtomicUsize::new(0);
static LAST_ACTIVITY_MS: AtomicU64 = AtomicU64::new(0);

fn epoch() -> &'static Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now)
}

fn now_ms() -> u64 {
    u64::try_from(epoch().elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// 记一次活动,空闲计时从现在重新开始。
pub fn touch() {
    LAST_ACTIVITY_MS.store(now_ms(), Ordering::Relaxed);
}

/// 持有期间面板视为「忙」,不会空闲退出。创建与释放都会刷新活动时间。
#[must_use = "BusyGuard 一旦被丢弃就不再阻止空闲退出"]
#[derive(Debug)]
pub struct BusyGuard(());

impl BusyGuard {
    pub fn new() -> Self {
        BUSY.fetch_add(1, Ordering::SeqCst);
        touch();
        Self(())
    }
}

impl Default for BusyGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        BUSY.fetch_sub(1, Ordering::SeqCst);
        touch();
    }
}

fn idle_for() -> Duration {
    Duration::from_millis(now_ms().saturating_sub(LAST_ACTIVITY_MS.load(Ordering::Relaxed)))
}

fn should_exit(busy: usize, idle: Duration, limit: Duration) -> bool {
    busy == 0 && idle >= limit
}

/// 空闲退出阈值;None = 不做空闲退出。
pub fn idle_exit_after(socket_activated: bool) -> Option<Duration> {
    idle_exit_policy(
        env::var("RUSTPANEL_IDLE_EXIT_MINUTES").ok().as_deref(),
        crate::runtime::is_micro_profile(),
        socket_activated,
    )
}

fn idle_exit_policy(value: Option<&str>, micro: bool, socket_activated: bool) -> Option<Duration> {
    // 没有 systemd socket 兜底时退出 = 面板失联(Restart=always 还会立刻拉起,白折腾)。
    if !socket_activated {
        return None;
    }
    let minutes = match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => value.parse::<u64>().ok()?,
        None if micro => DEFAULT_MICRO_IDLE_EXIT_MINUTES,
        None => return None,
    };
    (minutes > 0).then(|| Duration::from_secs(minutes.saturating_mul(60)))
}

/// 空闲达到 `limit` 且没有任何 BusyGuard 时返回;用作 graceful shutdown 信号。
pub async fn wait_until_idle(limit: Duration) {
    touch();
    let tick = (limit / 4).clamp(Duration::from_secs(5), Duration::from_secs(60));
    loop {
        tokio::time::sleep(tick).await;
        if should_exit(BUSY.load(Ordering::SeqCst), idle_for(), limit) {
            tracing::info!(
                idle_seconds = idle_for().as_secs(),
                "idle limit reached, exiting until systemd socket activation wakes us"
            );
            return;
        }
    }
}

/// 取 systemd socket activation 传进来的监听 socket(sd_listen_fds 协议:
/// LISTEN_PID == 自己、LISTEN_FDS >= 1,fd 从 3 开始)。
///
/// **必须在 tokio 起线程之前调用**:这里会清掉 LISTEN_* 环境变量(免得终端
/// shell / workload 子进程误以为自己被激活),多线程下改 env 不安全。
#[cfg(unix)]
pub fn take_activated_listener() -> std::io::Result<Option<std::net::TcpListener>> {
    use std::os::fd::{FromRawFd, RawFd};

    const SD_LISTEN_FDS_START: RawFd = 3;

    let for_us = env::var("LISTEN_PID")
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        == Some(std::process::id());
    let fds = env::var("LISTEN_FDS")
        .ok()
        .and_then(|value| value.trim().parse::<RawFd>().ok())
        .unwrap_or(0);
    env::remove_var("LISTEN_PID");
    env::remove_var("LISTEN_FDS");
    env::remove_var("LISTEN_FDNAMES");
    if !for_us || fds < 1 {
        return Ok(None);
    }

    for fd in SD_LISTEN_FDS_START..SD_LISTEN_FDS_START.saturating_add(fds) {
        // systemd 传进来的 fd 没有 CLOEXEC;不打上的话每个终端 shell /
        // workload 都会继承一份监听 socket。
        // SAFETY: fcntl 只改 fd 标志位,fd 无效时返回 -1,不涉及内存。
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
            return Err(std::io::Error::last_os_error());
        }
    }
    // 只用第一个 fd(单个 ListenStream);其余 fd 保持 CLOEXEC 由进程退出回收。
    // SAFETY: 协议保证 fd 3 是 systemd 交给本进程、且只交给本进程的监听 socket,
    // 此后由 TcpListener 独占所有权。
    let listener = unsafe { std::net::TcpListener::from_raw_fd(SD_LISTEN_FDS_START) };
    // 不是 TCP socket(配置错成 ListenDatagram 等)时这里就报错,别静默跑。
    listener.local_addr()?;
    listener.set_nonblocking(true)?;
    Ok(Some(listener))
}

#[cfg(not(unix))]
pub fn take_activated_listener() -> std::io::Result<Option<std::net::TcpListener>> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_exit_requires_socket_activation() {
        assert_eq!(idle_exit_policy(Some("5"), true, false), None);
        assert_eq!(idle_exit_policy(None, true, false), None);
    }

    #[test]
    fn micro_defaults_to_ten_minutes_and_env_overrides() {
        assert_eq!(
            idle_exit_policy(None, true, true),
            Some(Duration::from_secs(600))
        );
        assert_eq!(idle_exit_policy(None, false, true), None);
        assert_eq!(
            idle_exit_policy(Some("3"), false, true),
            Some(Duration::from_secs(180))
        );
        assert_eq!(idle_exit_policy(Some("0"), true, true), None);
        assert_eq!(idle_exit_policy(Some("abc"), true, true), None);
        assert_eq!(
            idle_exit_policy(Some(" "), true, true),
            Some(Duration::from_secs(600))
        );
    }

    #[test]
    fn busy_work_blocks_exit_even_when_idle_long_enough() {
        let limit = Duration::from_secs(60);
        assert!(!should_exit(1, Duration::from_secs(3600), limit));
        assert!(!should_exit(0, Duration::from_secs(59), limit));
        assert!(should_exit(0, Duration::from_secs(60), limit));
    }

    #[test]
    fn busy_guard_refreshes_activity() {
        // BUSY 是进程全局,其它并发测试也会增减,这里只断言不依赖计数的部分。
        let guard = BusyGuard::new();
        assert!(BUSY.load(Ordering::SeqCst) >= 1);
        drop(guard);
        assert!(idle_for() < Duration::from_secs(5));
    }
}
