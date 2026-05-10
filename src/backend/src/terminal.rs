use std::{
    io::{Read, Write},
    path::Path,
    pin::Pin,
    sync::{Arc, Mutex},
    thread,
};

use futures_core::Stream;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
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
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
}

impl TerminalSession {
    fn spawn() -> Result<(Self, ReceiverStream<Result<OpenTerminalResponse, Status>>), Status> {
        Self::spawn_with_cwd(None)
    }

    fn spawn_with_cwd(
        cwd: Option<&Path>,
    ) -> Result<(Self, ReceiverStream<Result<OpenTerminalResponse, Status>>), Status> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: DEFAULT_ROWS,
                cols: DEFAULT_COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| Status::internal(error.to_string()))?;
        let shell = default_shell();
        let mut command = CommandBuilder::new(&shell);
        // 给 PTY 设置必要的终端能力环境;不设的话 top/vim/htop 等全屏程序
        // 在 xterm.js 里渲染会出错,bash 也会因为 TERM 缺失而退化到 dumb 模式
        command.env("TERM", "xterm-256color");
        if std::env::var("LANG").is_err() {
            command.env("LANG", "en_US.UTF-8");
        }
        // 透传一些 systemd unit 通常会清空但 shell 启动需要的变量
        for key in ["HOME", "USER", "LOGNAME", "PATH", "LANG", "LC_ALL", "SHELL"] {
            if let Ok(value) = std::env::var(key) {
                command.env(key, value);
            }
        }
        // bash 用 -l 走登录 shell,会读 /etc/profile + ~/.bash_profile,补全 / PATH 都齐
        if shell.ends_with("bash") {
            command.arg("-l");
        }
        if let Some(cwd) = cwd {
            command.cwd(cwd);
        }
        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| Status::internal(error.to_string()))?;
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| Status::internal(error.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| Status::internal(error.to_string()))?;
        let master = Arc::new(Mutex::new(pair.master));
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

            let _ = child.wait();
            let _ = sender.blocking_send(Ok(OpenTerminalResponse {
                status: Some(ok_response("terminal exited")),
                data: Vec::new(),
                exited: true,
            }));
        });

        Ok((
            Self {
                writer: Arc::new(Mutex::new(writer)),
                master,
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

        master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| Status::internal(error.to_string()))
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
}
