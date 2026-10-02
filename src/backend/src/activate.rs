//! 可移植的 socket activation 监督者:取代 systemd 专属的 `.socket` unit,
//! 让"空闲挂起、来连接再拉起"这个节俭模式的核心机制不再绑死 systemd——
//! OpenRC(用 `supervise-daemon` 包这个子命令本身)和真没有 init 系统的场景
//! 都能用上。
//!
//! 设计:
//! - 监督者自己 bind 监听 socket,`poll()` 等可读就 fork;**不在这里
//!   `accept()`**——连接留在内核 backlog 里,由子进程自己 accept,行为上
//!   对应 systemd `Accept=no` 的语义。
//! - 子进程把 socket fd 复制到 fd 3(处理好 fd 本来就是 3 的边界情况——
//!   `dup2(3, 3)` 是空操作,不会像正常 dup2 那样清掉 `FD_CLOEXEC`,必须显式
//!   补一刀 `fcntl`),设 `RUSTPANEL_ACTIVATED_FD=3`,`exec` 回同一个二进制
//!   (不带 `--activate`,走正常常驻服务路径;`frugal::take_activated_listener`
//!   认这个环境变量)。
//! - 监督者阻塞 `waitpid` 等子进程退出,之后回到 `poll()` 循环。
//! - 崩溃风暴防护:子进程活不满 [`FAST_FAILURE_THRESHOLD`] 就退出算一次"快速
//!   失败",连续失败达到阈值后指数退避,不然子进程一起来就崩、又有连接在排队时
//!   `poll()` 会一直可读,监督者会不停 fork,单核小鸡直接被打满。
//! - 转发 `SIGTERM`/`SIGINT` 给当前子进程、等它退出再自己退出——OpenRC
//!   `supervise-daemon stop` 靠这个收尾,不然子进程会变成孤儿继续跑。
//!
//! 只有一个线程(没有 tokio runtime),`fork()` 在这里是安全的:fork 安全性的
//! 风险主要来自多线程父进程里别的线程在 fork 时持有 malloc/mutex 锁导致子进程
//! 里死锁,单线程程序不存在这个问题。

use std::{
    net::{SocketAddr, TcpListener},
    os::{
        fd::{AsRawFd, RawFd},
        unix::process::CommandExt,
    },
    path::Path,
    process::Command,
    sync::atomic::{AtomicBool, AtomicI32, Ordering},
    time::{Duration, Instant},
};

/// 子进程活不满这么久就算"快速失败"。
const FAST_FAILURE_THRESHOLD: Duration = Duration::from_secs(2);
/// 连续快速失败达到这个次数才开始退避,偶发的单次崩溃不用等。
const BACKOFF_AFTER_FAILURES: u32 = 3;
/// 单次退避上限,避免指数增长没有尽头。
const MAX_BACKOFF: Duration = Duration::from_secs(30);

static CURRENT_CHILD_PID: AtomicI32 = AtomicI32::new(0);
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_shutdown_signal(_sig: libc::c_int) {
    // 信号处理函数里只能调用 async-signal-safe 的操作:原子读写和 kill(2)
    // 都在安全列表里,别的(比如打日志、分配内存)都不行。
    SHUTTING_DOWN.store(true, Ordering::SeqCst);
    let pid = CURRENT_CHILD_PID.load(Ordering::SeqCst);
    if pid > 0 {
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
}

fn install_signal_handlers() {
    unsafe {
        libc::signal(
            libc::SIGTERM,
            handle_shutdown_signal as *const () as libc::sighandler_t,
        );
        libc::signal(
            libc::SIGINT,
            handle_shutdown_signal as *const () as libc::sighandler_t,
        );
    }
}

/// `poll()` 等监听 socket 可读;每秒醒一次重新检查 `SHUTTING_DOWN`,不然信号
/// 处理函数设了标记,主循环还在 `poll()` 里一直睡到下个连接才会发现。
/// 返回 `true` 表示有连接在排队,`false` 表示该退出了。
fn wait_for_connection_or_shutdown(fd: RawFd) -> bool {
    loop {
        if SHUTTING_DOWN.load(Ordering::SeqCst) {
            return false;
        }
        let mut fds = [libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        }];
        // SAFETY: fds 指向一个活着的、长度为 1 的栈上数组。
        let rc = unsafe { libc::poll(fds.as_mut_ptr(), 1, 1000) };
        if rc < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue; // 大概率是我们自己的信号处理函数打断的,回去重新检查标记
            }
            eprintln!("activate: poll 失败,1 秒后重试: {err}");
            std::thread::sleep(Duration::from_secs(1));
            continue;
        }
        if rc > 0 && fds[0].revents & libc::POLLIN != 0 {
            return true;
        }
        // rc == 0:超时,回去重新检查 SHUTTING_DOWN
    }
}

/// fork + 子进程里 dup2/exec,父进程里 waitpid 等它退出,返回子进程存活时长。
fn spawn_and_wait(listener_fd: RawFd, exec: &Path) -> std::io::Result<Duration> {
    let start = Instant::now();
    // SAFETY: 单线程程序里 fork 是安全的——没有别的线程可能在 fork 瞬间持着
    // malloc/mutex 锁,子进程不会因此死锁。
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if pid == 0 {
        // ===== 子进程 =====
        if listener_fd != 3 {
            // SAFETY: dup2 只操作 fd 表,参数都是本函数持有的有效 fd。
            if unsafe { libc::dup2(listener_fd, 3) } == -1 {
                std::process::exit(126);
            }
        }
        // 不管上面是不是真的拷贝了一份(listener_fd 本来就是 3 时 dup2(3,3)
        // 是空操作、不会清 FD_CLOEXEC),都显式清一次,否则 exec 后 fd 3 直接丢失。
        // SAFETY: 3 在上面要么被 dup2 验证过有效,要么本来就是 listener 自己的 fd。
        if unsafe { libc::fcntl(3, libc::F_SETFD, 0) } == -1 {
            std::process::exit(126);
        }
        std::env::set_var("RUSTPANEL_ACTIVATED_FD", "3");
        // Command::exec() 替换当前进程镜像,成功就不会返回。
        let err = Command::new(exec).exec();
        eprintln!("activate: exec {} 失败: {err}", exec.display());
        std::process::exit(127);
    }

    // ===== 父进程(监督者)=====
    CURRENT_CHILD_PID.store(pid, Ordering::SeqCst);
    let mut status: libc::c_int = 0;
    loop {
        // SAFETY: pid 是刚 fork 出来的合法子进程 id,status 指向本函数的栈变量。
        let r = unsafe { libc::waitpid(pid, &mut status, 0) };
        if r == -1 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            CURRENT_CHILD_PID.store(0, Ordering::SeqCst);
            return Err(err);
        }
        break;
    }
    CURRENT_CHILD_PID.store(0, Ordering::SeqCst);
    Ok(start.elapsed())
}

fn backoff_duration(consecutive_fast_failures: u32) -> Duration {
    let exponent = consecutive_fast_failures.saturating_sub(BACKOFF_AFTER_FAILURES);
    let secs = 2u64.saturating_pow(exponent.min(10));
    Duration::from_secs(secs).min(MAX_BACKOFF)
}

/// 监督者主循环。调用前不能起 tokio runtime(fork 要求单线程)。
pub fn run(listen_addr: SocketAddr, exec: &Path) -> std::io::Result<()> {
    install_signal_handlers();
    let listener = TcpListener::bind(listen_addr)?;
    let fd = listener.as_raw_fd();
    eprintln!(
        "activate: 监督 {listen_addr},等待连接后拉起 {}",
        exec.display()
    );

    let mut consecutive_fast_failures: u32 = 0;

    loop {
        if !wait_for_connection_or_shutdown(fd) {
            return Ok(());
        }

        if consecutive_fast_failures >= BACKOFF_AFTER_FAILURES {
            let backoff = backoff_duration(consecutive_fast_failures);
            eprintln!(
                "activate: 连续 {consecutive_fast_failures} 次快速失败,退避 {backoff:?} 再试"
            );
            std::thread::sleep(backoff);
            if SHUTTING_DOWN.load(Ordering::SeqCst) {
                return Ok(());
            }
        }

        match spawn_and_wait(fd, exec) {
            Ok(alive) => {
                if alive < FAST_FAILURE_THRESHOLD {
                    consecutive_fast_failures += 1;
                } else {
                    consecutive_fast_failures = 0;
                }
            }
            Err(err) => {
                eprintln!("activate: fork/wait 失败: {err}");
                consecutive_fast_failures += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_is_minimal_at_or_below_threshold() {
        assert_eq!(backoff_duration(0), Duration::from_secs(1));
        assert_eq!(
            backoff_duration(BACKOFF_AFTER_FAILURES),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn backoff_grows_exponentially_then_caps() {
        assert_eq!(
            backoff_duration(BACKOFF_AFTER_FAILURES + 1),
            Duration::from_secs(2)
        );
        assert_eq!(
            backoff_duration(BACKOFF_AFTER_FAILURES + 2),
            Duration::from_secs(4)
        );
        assert_eq!(backoff_duration(BACKOFF_AFTER_FAILURES + 20), MAX_BACKOFF);
    }

    // fork 的真实端到端行为(真的 fork+exec+dup2+waitpid)留给手动 / 集成测试——
    // 单测环境里 fork 一个 cargo test 进程本身风险很大(继承一堆测试 harness
    // 状态),这里只测没有副作用的纯函数部分。
}
