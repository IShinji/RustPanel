use std::{
    io::{Read, Write},
    path::Path,
    pin::Pin,
    sync::{Arc, Mutex},
    thread,
};

use futures_core::Stream;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response as GrpcResponse, Status, Streaming};
use tracing::warn;

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        open_terminal_request::Payload, terminal_service_server::TerminalService,
        OpenTerminalRequest, OpenTerminalResponse, ResizeTerminalRequest, ResizeTerminalResponse,
        TerminalResize,
    },
};

const DEFAULT_COLS: u16 = 120;
const DEFAULT_ROWS: u16 = 30;
const TERMINAL_CHANNEL_SIZE: usize = 128;

#[derive(Clone, Debug, Default)]
pub struct TerminalServiceImpl;

#[tonic::async_trait]
impl TerminalService for TerminalServiceImpl {
    type OpenTerminalStream =
        Pin<Box<dyn Stream<Item = Result<OpenTerminalResponse, Status>> + Send>>;

    async fn open_terminal(
        &self,
        request: Request<Streaming<OpenTerminalRequest>>,
    ) -> Result<GrpcResponse<Self::OpenTerminalStream>, Status> {
        let (session, output_stream) = TerminalSession::spawn()?;
        let mut input_stream = request.into_inner();

        tokio::spawn(async move {
            loop {
                match input_stream.message().await {
                    Ok(Some(message)) => {
                        if let Err(error) = session.handle_input(message) {
                            warn!(%error, "failed to handle terminal input");
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        warn!(%error, "terminal input stream failed");
                        break;
                    }
                }
            }
        });

        Ok(GrpcResponse::new(Box::pin(output_stream)))
    }

    async fn resize_terminal(
        &self,
        _request: Request<ResizeTerminalRequest>,
    ) -> Result<GrpcResponse<ResizeTerminalResponse>, Status> {
        Ok(GrpcResponse::new(ResizeTerminalResponse {
            status: Some(ok_response(
                "terminal resize is applied on the active bidirectional stream",
            )),
        }))
    }
}

pub struct TerminalSession {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Arc<Mutex<Box<dyn PtyResize + Send>>>,
}

/// PTY 后端抽象:完整构建走 portable-pty,micro 构建(无 `pty` feature)走内置 openpty。
trait PtyResize {
    fn resize(&self, rows: u16, cols: u16) -> Result<(), String>;
}

struct PtyProcess {
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn PtyResize + Send>,
    /// 阻塞等待 shell 退出(在读线程里调用)。
    wait: Box<dyn FnOnce() + Send>,
}

struct ShellCommand {
    program: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    cwd: Option<std::path::PathBuf>,
}

impl TerminalSession {
    fn spawn() -> Result<(Self, ReceiverStream<Result<OpenTerminalResponse, Status>>), Status> {
        Self::spawn_with_cwd(None)
    }

    fn spawn_with_cwd(
        cwd: Option<&Path>,
    ) -> Result<(Self, ReceiverStream<Result<OpenTerminalResponse, Status>>), Status> {
        let shell = default_shell();
        // 给 PTY 设置必要的终端能力环境;不设的话 top/vim/htop 等全屏程序
        // 在 xterm.js 里渲染会出错,bash 也会因为 TERM 缺失而退化到 dumb 模式
        let mut env = vec![("TERM".to_owned(), "xterm-256color".to_owned())];
        if std::env::var("LANG").is_err() {
            env.push(("LANG".to_owned(), "en_US.UTF-8".to_owned()));
        }
        // 透传一些 systemd unit 通常会清空但 shell 启动需要的变量
        for key in ["HOME", "USER", "LOGNAME", "PATH", "LANG", "LC_ALL", "SHELL"] {
            if let Ok(value) = std::env::var(key) {
                env.push((key.to_owned(), value));
            }
        }
        // bash 用 -l 走登录 shell,会读 /etc/profile + ~/.bash_profile,补全 / PATH 都齐
        let args = if shell.ends_with("bash") {
            vec!["-l".to_owned()]
        } else {
            Vec::new()
        };
        let PtyProcess {
            mut reader,
            writer,
            master,
            wait,
        } = pty_backend::spawn(
            ShellCommand {
                program: shell,
                args,
                env,
                cwd: cwd.map(Path::to_path_buf),
            },
            DEFAULT_ROWS,
            DEFAULT_COLS,
        )?;
        let (sender, receiver) = mpsc::channel(TERMINAL_CHANNEL_SIZE);

        thread::spawn(move || {
            let mut buffer = [0_u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(size) => {
                        if sender
                            .blocking_send(Ok(OpenTerminalResponse {
                                status: Some(ok_response("ok")),
                                data: buffer[..size].to_vec(),
                                exited: false,
                            }))
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = sender.blocking_send(Err(Status::internal(error.to_string())));
                        break;
                    }
                }
            }

            wait();
            let _ = sender.blocking_send(Ok(OpenTerminalResponse {
                status: Some(ok_response("terminal exited")),
                data: Vec::new(),
                exited: true,
            }));
        });

        Ok((
            Self {
                writer: Arc::new(Mutex::new(writer)),
                master: Arc::new(Mutex::new(master)),
            },
            ReceiverStream::new(receiver),
        ))
    }

    fn handle_input(&self, input: OpenTerminalRequest) -> Result<(), Status> {
        match input.payload {
            Some(Payload::Data(data)) => self.write_data(&data),
            Some(Payload::Resize(resize)) => self.resize(resize),
            None => Ok(()),
        }
    }

    pub fn write_data(&self, data: &[u8]) -> Result<(), Status> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| Status::internal("terminal writer lock poisoned"))?;
        writer
            .write_all(data)
            .map_err(|error| Status::internal(error.to_string()))?;
        writer
            .flush()
            .map_err(|error| Status::internal(error.to_string()))
    }

    pub fn resize(&self, resize: TerminalResize) -> Result<(), Status> {
        let cols = normalize_size(resize.cols, DEFAULT_COLS);
        let rows = normalize_size(resize.rows, DEFAULT_ROWS);
        let master = self
            .master
            .lock()
            .map_err(|_| Status::internal("terminal master lock poisoned"))?;

        master.resize(rows, cols).map_err(Status::internal)
    }
}

#[cfg(feature = "pty")]
mod pty_backend {
    use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
    use tonic::Status;

    use super::{PtyProcess, PtyResize, ShellCommand};

    impl PtyResize for Box<dyn MasterPty + Send> {
        fn resize(&self, rows: u16, cols: u16) -> Result<(), String> {
            MasterPty::resize(
                self.as_ref(),
                PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
            .map_err(|error| error.to_string())
        }
    }

    pub(super) fn spawn(shell: ShellCommand, rows: u16, cols: u16) -> Result<PtyProcess, Status> {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(internal)?;
        let mut command = CommandBuilder::new(&shell.program);
        command.args(&shell.args);
        for (key, value) in &shell.env {
            command.env(key, value);
        }
        if let Some(cwd) = &shell.cwd {
            command.cwd(cwd);
        }
        let mut child = pair.slave.spawn_command(command).map_err(internal)?;
        drop(pair.slave);
        let reader = pair.master.try_clone_reader().map_err(internal)?;
        let writer = pair.master.take_writer().map_err(internal)?;
        Ok(PtyProcess {
            reader,
            writer,
            master: Box::new(pair.master),
            wait: Box::new(move || {
                let _ = child.wait();
            }),
        })
    }

    fn internal(error: impl std::fmt::Display) -> Status {
        Status::internal(error.to_string())
    }
}

/// 内置 openpty:micro 构建不带 portable-pty(及其 serial/nix/filedescriptor 依赖),
/// 用 libc 直接开 PTY、setsid + TIOCSCTTY 让 shell 以它为控制终端。
#[cfg(all(unix, not(feature = "pty")))]
mod pty_backend {
    use std::{
        fs::File,
        os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        os::unix::process::CommandExt,
        process::{Command, Stdio},
    };

    use tonic::Status;

    use super::{PtyProcess, PtyResize, ShellCommand};

    struct NativeMaster(File);

    fn winsize(rows: u16, cols: u16) -> libc::winsize {
        libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }
    }

    impl PtyResize for NativeMaster {
        fn resize(&self, rows: u16, cols: u16) -> Result<(), String> {
            let size = winsize(rows, cols);
            // SAFETY: TIOCSWINSZ 只读取传入的 winsize,fd 由 self 持有。
            let rc = unsafe { libc::ioctl(self.0.as_raw_fd(), libc::TIOCSWINSZ as _, &size) };
            if rc == -1 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            Ok(())
        }
    }

    fn set_cloexec(fd: RawFd) -> std::io::Result<()> {
        // SAFETY: 只改 fd 标志位。
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    fn openpty(rows: u16, cols: u16) -> std::io::Result<(OwnedFd, OwnedFd)> {
        let mut master: libc::c_int = -1;
        let mut slave: libc::c_int = -1;
        let mut size = winsize(rows, cols);
        // SAFETY: 出参指针都指向本栈帧的有效变量;name/termios 传空表示用默认值。
        let rc = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                // winp 在 Linux 是 *const、macOS 是 *mut;裸指针两边都能隐式转换
                std::ptr::addr_of_mut!(size),
            )
        };
        if rc == -1 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: openpty 成功时两个 fd 都是新打开、归本函数所有的。
        let (master, slave) =
            unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
        // 不打 CLOEXEC 的话 shell 及其子进程都会继承 master,会话关不掉。
        set_cloexec(master.as_raw_fd())?;
        set_cloexec(slave.as_raw_fd())?;
        Ok((master, slave))
    }

    pub(super) fn spawn(shell: ShellCommand, rows: u16, cols: u16) -> Result<PtyProcess, Status> {
        let io = |error: std::io::Error| Status::internal(error.to_string());
        let (master, slave) = openpty(rows, cols).map_err(io)?;
        let slave = File::from(slave);

        let mut command = Command::new(&shell.program);
        command
            .args(&shell.args)
            .envs(shell.env.iter().map(|(key, value)| (key, value)))
            .stdin(Stdio::from(slave.try_clone().map_err(io)?))
            .stdout(Stdio::from(slave.try_clone().map_err(io)?))
            .stderr(Stdio::from(slave));
        if let Some(cwd) = &shell.cwd {
            command.current_dir(cwd);
        }
        // SAFETY: pre_exec 在 fork 后的子进程里运行,只调用 async-signal-safe 的
        // setsid / ioctl。stdin 此时已 dup2 为 PTY slave。
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command.spawn().map_err(io)?;
        drop(command);

        let master = File::from(master);
        let reader = master.try_clone().map_err(io)?;
        let writer = master.try_clone().map_err(io)?;
        Ok(PtyProcess {
            reader: Box::new(reader),
            writer: Box::new(writer),
            master: Box::new(NativeMaster(master)),
            wait: Box::new(move || {
                let _ = child.wait();
            }),
        })
    }
}

#[cfg(all(not(unix), not(feature = "pty")))]
mod pty_backend {
    use tonic::Status;

    use super::{PtyProcess, ShellCommand};

    pub(super) fn spawn(
        _shell: ShellCommand,
        _rows: u16,
        _cols: u16,
    ) -> Result<PtyProcess, Status> {
        Err(Status::unimplemented(
            "this build has no PTY backend; rebuild with the `pty` feature",
        ))
    }
}

pub fn spawn_web_terminal() -> Result<(TerminalSession, ReceiverStream<Vec<u8>>), Status> {
    spawn_web_terminal_with_cwd(None)
}

pub fn spawn_web_terminal_with_cwd(
    cwd: Option<&Path>,
) -> Result<(TerminalSession, ReceiverStream<Vec<u8>>), Status> {
    let (session, output) = TerminalSession::spawn_with_cwd(cwd)?;
    let (sender, receiver) = mpsc::channel(TERMINAL_CHANNEL_SIZE);

    tokio::spawn(async move {
        let mut output = output;
        while let Some(message) = tokio_stream::StreamExt::next(&mut output).await {
            match message {
                Ok(message) if !message.data.is_empty() => {
                    if sender.send(message.data).await.is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    let _ = sender
                        .send(format!("terminal error: {error}\r\n").into_bytes())
                        .await;
                    break;
                }
            }
        }
    });

    Ok((session, ReceiverStream::new(receiver)))
}

fn default_shell() -> String {
    // 优先使用 SHELL 环境变量;systemd 服务下 SHELL 通常未设置,
    // 退而求其次按可用性挑选 bash > zsh > sh,避免落到没有 tab 补全的 dash
    if let Ok(shell) = std::env::var("SHELL") {
        if !shell.trim().is_empty() {
            return shell;
        }
    }
    if cfg!(target_os = "windows") {
        return "powershell.exe".to_owned();
    }
    for candidate in ["/bin/bash", "/usr/bin/bash", "/bin/zsh", "/usr/bin/zsh"] {
        if std::path::Path::new(candidate).exists() {
            return candidate.to_owned();
        }
    }
    "/bin/sh".to_owned()
}

fn normalize_size(value: u32, default_value: u16) -> u16 {
    u16::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .unwrap_or(default_value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_size_uses_default_for_zero() {
        assert_eq!(normalize_size(0, 80), 80);
        assert_eq!(normalize_size(120, 80), 120);
    }

    #[cfg(unix)]
    #[test]
    fn pty_backend_gives_shell_a_sized_controlling_terminal() {
        let PtyProcess {
            mut reader,
            writer,
            master,
            wait,
        } = pty_backend::spawn(
            ShellCommand {
                program: "/bin/sh".to_owned(),
                args: vec![
                    "-c".to_owned(),
                    "stty size; tty >/dev/null && echo has-ctty; sleep 1; stty size".to_owned(),
                ],
                env: Vec::new(),
                cwd: None,
            },
            24,
            80,
        )
        .expect("spawn pty shell");
        drop(writer);

        let mut output = Vec::new();
        let mut buffer = [0_u8; 1024];
        let mut resized = false;
        // shell 退出后 Linux 上读 master 会返回 EIO 而不是 0,两种都当结束。
        while let Ok(size) = reader.read(&mut buffer) {
            if size == 0 {
                break;
            }
            output.extend_from_slice(&buffer[..size]);
            if !resized && String::from_utf8_lossy(&output).contains("has-ctty") {
                master.resize(40, 120).expect("resize");
                resized = true;
            }
        }
        wait();
        let output = String::from_utf8_lossy(&output);
        assert!(output.contains("24 80"), "initial size: {output:?}");
        assert!(output.contains("40 120"), "resized: {output:?}");
        assert!(output.contains("has-ctty"), "tty output: {output:?}");
    }
}
