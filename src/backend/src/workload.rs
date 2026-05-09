use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, System};
use tokio::sync::Mutex;
use tonic::{Request, Response as GrpcResponse, Status};
use uuid::Uuid;

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        workload_service_server::WorkloadService, DeleteWorkloadRequest, DeleteWorkloadResponse,
        GetWorkloadLogRequest, GetWorkloadLogResponse, ListWorkloadsRequest, ListWorkloadsResponse,
        StartWorkloadRequest, StartWorkloadResponse, StopWorkloadRequest, StopWorkloadResponse,
        UpsertWorkloadRequest, UpsertWorkloadResponse, WorkloadItem, WorkloadState,
    },
};

const DEFAULT_WORKLOAD_ROOT: &str = "/tmp/rustpanel/workloads";
const DEFAULT_LOG_LIMIT_BYTES: u64 = 5 * 1024 * 1024;
const DEFAULT_MEMORY_LIMIT_MB: u64 = 32;
const DEFAULT_RESTART_LIMIT: u32 = 3;

#[derive(Clone)]
pub struct WorkloadServiceImpl {
    store: WorkloadStore,
    processes: ProcessRegistry,
}

impl WorkloadServiceImpl {
    pub fn new() -> Self {
        Self {
            store: WorkloadStore::from_env(),
            processes: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Default for WorkloadServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl WorkloadService for WorkloadServiceImpl {
    async fn list_workloads(
        &self,
        _request: Request<ListWorkloadsRequest>,
    ) -> Result<GrpcResponse<ListWorkloadsResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_WORKLOADS)?;
        let mut workloads = self.store.load().await?;
        overlay_runtime_state(&mut workloads, &self.processes).await;

        Ok(GrpcResponse::new(ListWorkloadsResponse {
            status: Some(ok_response("ok")),
            workloads,
        }))
    }

    async fn upsert_workload(
        &self,
        request: Request<UpsertWorkloadRequest>,
    ) -> Result<GrpcResponse<UpsertWorkloadResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_WORKLOADS)?;
        let mut workload = request
            .into_inner()
            .workload
            .ok_or_else(|| Status::invalid_argument("workload is required"))?;
        normalize_workload(&mut workload)?;
        let mut workloads = self.store.load().await?;
        workloads.retain(|stored| stored.id != workload.id);
        workloads.push(workload.clone());
        self.store.save(&workloads).await?;

        Ok(GrpcResponse::new(UpsertWorkloadResponse {
            status: Some(ok_response("workload saved")),
            workload: Some(workload),
        }))
    }

    async fn start_workload(
        &self,
        request: Request<StartWorkloadRequest>,
    ) -> Result<GrpcResponse<StartWorkloadResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_WORKLOADS)?;
        let id = request.into_inner().id;
        let workload = self.store.load_one(&id).await?;
        let workload =
            start_supervisor(self.store.clone(), self.processes.clone(), workload).await?;

        Ok(GrpcResponse::new(StartWorkloadResponse {
            status: Some(ok_response("workload started")),
            workload: Some(workload),
        }))
    }

    async fn stop_workload(
        &self,
        request: Request<StopWorkloadRequest>,
    ) -> Result<GrpcResponse<StopWorkloadResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_WORKLOADS)?;
        let id = request.into_inner().id;
        let pid = {
            let mut processes = self.processes.lock().await;
            let Some(entry) = processes.get_mut(&id) else {
                let workload = self
                    .store
                    .update_state(&id, WorkloadState::Stopped, 0, "stopped")
                    .await?;
                return Ok(GrpcResponse::new(StopWorkloadResponse {
                    status: Some(ok_response("workload already stopped")),
                    workload: Some(workload),
                }));
            };
            entry.stopping = true;
            entry.pid
        };
        terminate_pid(pid).await;
        let workload = self
            .store
            .update_state(&id, WorkloadState::Stopped, 0, "stopped")
            .await?;

        Ok(GrpcResponse::new(StopWorkloadResponse {
            status: Some(ok_response("workload stopped")),
            workload: Some(workload),
        }))
    }

    async fn delete_workload(
        &self,
        request: Request<DeleteWorkloadRequest>,
    ) -> Result<GrpcResponse<DeleteWorkloadResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_WORKLOADS)?;
        let id = request.into_inner().id;
        if let Some(pid) = self
            .processes
            .lock()
            .await
            .remove(&id)
            .map(|entry| entry.pid)
        {
            terminate_pid(pid).await;
        }
        self.store.delete(&id).await?;

        Ok(GrpcResponse::new(DeleteWorkloadResponse {
            status: Some(ok_response("workload deleted")),
        }))
    }

    async fn get_workload_log(
        &self,
        request: Request<GetWorkloadLogRequest>,
    ) -> Result<GrpcResponse<GetWorkloadLogResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_WORKLOADS)?;
        let request = request.into_inner();
        let workload = self.store.load_one(&request.id).await?;
        let max_bytes = request.max_bytes.clamp(1, 256 * 1024);
        let content = read_tail(&PathBuf::from(workload.log_path), max_bytes).await?;

        Ok(GrpcResponse::new(GetWorkloadLogResponse {
            status: Some(ok_response("ok")),
            content,
        }))
    }
}

type ProcessRegistry = Arc<Mutex<HashMap<String, ProcessEntry>>>;

#[derive(Clone, Debug)]
struct ProcessEntry {
    pid: i32,
    stopping: bool,
}

async fn start_supervisor(
    store: WorkloadStore,
    processes: ProcessRegistry,
    mut workload: WorkloadItem,
) -> Result<WorkloadItem, Status> {
    normalize_workload(&mut workload)?;
    if processes.lock().await.contains_key(&workload.id) {
        return Err(Status::already_exists("workload is already running"));
    }

    let (child, pid) = spawn_child(&store, &workload).await?;
    processes.lock().await.insert(
        workload.id.clone(),
        ProcessEntry {
            pid,
            stopping: false,
        },
    );
    workload.state = WorkloadState::Running.into();
    workload.pid = pid;
    workload.updated_at_seconds = current_timestamp();
    store.replace(workload.clone()).await?;

    tokio::spawn(supervise_workload(
        store,
        processes,
        workload.clone(),
        child,
    ));

    Ok(workload)
}

async fn supervise_workload(
    store: WorkloadStore,
    processes: ProcessRegistry,
    workload: WorkloadItem,
    mut child: tokio::process::Child,
) {
    let mut attempts = 0_u32;
    let mut workload = workload;

    loop {
        let pid = workload.pid;
        let monitor = tokio::spawn(monitor_memory(
            processes.clone(),
            workload.id.clone(),
            pid,
            workload.memory_limit_mb,
        ));
        let status = child.wait().await;
        monitor.abort();

        let stopping = {
            let mut processes = processes.lock().await;
            let stopping = processes
                .get(&workload.id)
                .map(|entry| entry.stopping)
                .unwrap_or(false);
            processes.remove(&workload.id);
            stopping
        };

        let failed = status
            .as_ref()
            .map(|status| !status.success())
            .unwrap_or(true);
        let should_restart = !stopping && workload.autostart && attempts < workload.restart_limit;
        if !should_restart {
            let state = if failed && !stopping {
                WorkloadState::Failed
            } else {
                WorkloadState::Stopped
            };
            let message = match status {
                Ok(status) => format!("exited with {status}"),
                Err(error) => error.to_string(),
            };
            let _ = store.update_state(&workload.id, state, 0, &message).await;
            break;
        }

        attempts += 1;
        tokio::time::sleep(Duration::from_secs(2)).await;
        match spawn_child(&store, &workload).await {
            Ok((next_child, next_pid)) => {
                workload.pid = next_pid;
                workload.state = WorkloadState::Running.into();
                workload.last_message = format!("restarted attempt {attempts}");
                let _ = store.replace(workload.clone()).await;
                processes.lock().await.insert(
                    workload.id.clone(),
                    ProcessEntry {
                        pid: next_pid,
                        stopping: false,
                    },
                );
                child = next_child;
            }
            Err(error) => {
                let _ = store
                    .update_state(&workload.id, WorkloadState::Failed, 0, error.message())
                    .await;
                break;
            }
        }
    }
}

async fn monitor_memory(processes: ProcessRegistry, id: String, pid: i32, memory_limit_mb: u64) {
    if memory_limit_mb == 0 {
        return;
    }
    let limit = memory_limit_mb.saturating_mul(1024 * 1024);
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let should_stop = processes
            .lock()
            .await
            .get(&id)
            .is_some_and(|entry| entry.pid == pid && !entry.stopping);
        if !should_stop {
            return;
        }
        if process_memory_bytes(pid) > limit {
            terminate_pid(pid).await;
            return;
        }
    }
}

async fn spawn_child(
    store: &WorkloadStore,
    workload: &WorkloadItem,
) -> Result<(tokio::process::Child, i32), Status> {
    tokio::fs::create_dir_all(store.log_dir())
        .await
        .map_err(io_status)?;
    trim_log(&PathBuf::from(&workload.log_path), workload.log_limit_bytes).await?;
    let stdout = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&workload.log_path)
        .map_err(io_status)?;
    let stderr = stdout.try_clone().map_err(io_status)?;
    let mut command = tokio::process::Command::new(default_shell());
    command.arg("-lc").arg(&workload.command);
    if !workload.cwd.trim().is_empty() {
        command.current_dir(&workload.cwd);
    }
    for entry in &workload.env {
        if let Some((key, value)) = entry.split_once('=') {
            command.env(key.trim(), value);
        }
    }
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(io_status)?;
    let pid = child
        .id()
        .and_then(|pid| i32::try_from(pid).ok())
        .ok_or_else(|| Status::internal("spawned workload has no pid"))?;

    Ok((child, pid))
}

#[derive(Clone, Debug)]
struct WorkloadStore {
    root: Arc<PathBuf>,
}

impl WorkloadStore {
    fn from_env() -> Self {
        let root = env::var("RUSTPANEL_WORKLOAD_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_WORKLOAD_ROOT));
        Self {
            root: Arc::new(root),
        }
    }

    async fn load(&self) -> Result<Vec<WorkloadItem>, Status> {
        match tokio::fs::read_to_string(self.workloads_path()).await {
            Ok(content) => serde_json::from_str::<Vec<StoredWorkload>>(&content)
                .map_err(io_status)
                .map(|items| items.into_iter().map(StoredWorkload::into_proto).collect()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(io_status(error)),
        }
    }

    async fn load_one(&self, id: &str) -> Result<WorkloadItem, Status> {
        self.load()
            .await?
            .into_iter()
            .find(|workload| workload.id == id)
            .ok_or_else(|| Status::not_found("workload not found"))
    }

    async fn save(&self, workloads: &[WorkloadItem]) -> Result<(), Status> {
        tokio::fs::create_dir_all(self.root.as_ref())
            .await
            .map_err(io_status)?;
        let stored = workloads
            .iter()
            .cloned()
            .map(StoredWorkload::from_proto)
            .collect::<Vec<_>>();
        let content = serde_json::to_string_pretty(&stored).map_err(io_status)?;
        tokio::fs::write(self.workloads_path(), content)
            .await
            .map_err(io_status)
    }

    async fn replace(&self, workload: WorkloadItem) -> Result<(), Status> {
        let mut workloads = self.load().await?;
        workloads.retain(|stored| stored.id != workload.id);
        workloads.push(workload);
        self.save(&workloads).await
    }

    async fn update_state(
        &self,
        id: &str,
        state: WorkloadState,
        pid: i32,
        message: &str,
    ) -> Result<WorkloadItem, Status> {
        let mut workloads = self.load().await?;
        let Some(workload) = workloads.iter_mut().find(|workload| workload.id == id) else {
            return Err(Status::not_found("workload not found"));
        };
        workload.state = state.into();
        workload.pid = pid;
        workload.last_message = message.to_owned();
        workload.updated_at_seconds = current_timestamp();
        let updated = workload.clone();
        self.save(&workloads).await?;
        Ok(updated)
    }

    async fn delete(&self, id: &str) -> Result<(), Status> {
        let mut workloads = self.load().await?;
        let before = workloads.len();
        workloads.retain(|workload| workload.id != id);
        if workloads.len() == before {
            return Err(Status::not_found("workload not found"));
        }
        self.save(&workloads).await
    }

    fn workloads_path(&self) -> PathBuf {
        self.root.join("workloads.json")
    }

    fn log_dir(&self) -> PathBuf {
        self.root.join("logs")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredWorkload {
    id: String,
    name: String,
    command: String,
    cwd: String,
    env: Vec<String>,
    autostart: bool,
    memory_limit_mb: u64,
    log_limit_bytes: u64,
    restart_limit: u32,
    schedule_cron: String,
    state: i32,
    pid: i32,
    log_path: String,
    last_message: String,
    updated_at_seconds: u64,
}

impl StoredWorkload {
    fn from_proto(workload: WorkloadItem) -> Self {
        Self {
            id: workload.id,
            name: workload.name,
            command: workload.command,
            cwd: workload.cwd,
            env: workload.env,
            autostart: workload.autostart,
            memory_limit_mb: workload.memory_limit_mb,
            log_limit_bytes: workload.log_limit_bytes,
            restart_limit: workload.restart_limit,
            schedule_cron: workload.schedule_cron,
            state: workload.state,
            pid: workload.pid,
            log_path: workload.log_path,
            last_message: workload.last_message,
            updated_at_seconds: workload.updated_at_seconds,
        }
    }

    fn into_proto(self) -> WorkloadItem {
        WorkloadItem {
            id: self.id,
            name: self.name,
            command: self.command,
            cwd: self.cwd,
            env: self.env,
            autostart: self.autostart,
            memory_limit_mb: self.memory_limit_mb,
            log_limit_bytes: self.log_limit_bytes,
            restart_limit: self.restart_limit,
            schedule_cron: self.schedule_cron,
            state: self.state,
            pid: self.pid,
            log_path: self.log_path,
            last_message: self.last_message,
            updated_at_seconds: self.updated_at_seconds,
        }
    }
}

fn normalize_workload(workload: &mut WorkloadItem) -> Result<(), Status> {
    if workload.name.trim().is_empty() {
        return Err(Status::invalid_argument("workload name is required"));
    }
    if workload.command.trim().is_empty() {
        return Err(Status::invalid_argument("workload command is required"));
    }
    if workload.id.trim().is_empty() {
        workload.id = Uuid::new_v4().to_string();
    }
    if workload.cwd.trim().is_empty() {
        workload.cwd = ".".to_owned();
    }
    if workload.memory_limit_mb == 0 {
        workload.memory_limit_mb = DEFAULT_MEMORY_LIMIT_MB;
    }
    if workload.log_limit_bytes == 0 {
        workload.log_limit_bytes = DEFAULT_LOG_LIMIT_BYTES;
    }
    if workload.restart_limit == 0 {
        workload.restart_limit = DEFAULT_RESTART_LIMIT;
    }
    if workload.state == WorkloadState::Unspecified as i32 {
        workload.state = WorkloadState::Stopped.into();
    }
    if workload.log_path.trim().is_empty() {
        workload.log_path = WorkloadStore::from_env()
            .root
            .join("logs")
            .join(format!("{}.log", workload.id))
            .to_string_lossy()
            .to_string();
    }
    workload.updated_at_seconds = current_timestamp();
    Ok(())
}

async fn overlay_runtime_state(workloads: &mut [WorkloadItem], processes: &ProcessRegistry) {
    let processes = processes.lock().await;
    for workload in workloads {
        if let Some(entry) = processes.get(&workload.id) {
            workload.pid = entry.pid;
            workload.state = WorkloadState::Running.into();
        } else if workload.state == WorkloadState::Running as i32 {
            workload.state = WorkloadState::Stopped.into();
            workload.pid = 0;
        }
    }
}

async fn terminate_pid(pid: i32) {
    let _ = tokio::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .output()
        .await;
}

fn process_memory_bytes(pid: i32) -> u64 {
    let Ok(pid) = u32::try_from(pid) else {
        return 0;
    };
    let mut system = System::new();
    system.refresh_process(Pid::from_u32(pid));
    system
        .process(Pid::from_u32(pid))
        .map(|process| process.memory())
        .unwrap_or_default()
}

async fn read_tail(path: &Path, max_bytes: u64) -> Result<String, Status> {
    let bytes = tokio::fs::read(path).await.unwrap_or_default();
    let start = bytes.len().saturating_sub(max_bytes as usize);
    Ok(String::from_utf8_lossy(&bytes[start..]).to_string())
}

async fn trim_log(path: &Path, max_bytes: u64) -> Result<(), Status> {
    let Ok(metadata) = tokio::fs::metadata(path).await else {
        return Ok(());
    };
    if metadata.len() <= max_bytes {
        return Ok(());
    }
    let content = read_tail(path, max_bytes).await?;
    tokio::fs::write(path, content).await.map_err(io_status)
}

fn default_shell() -> String {
    env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned())
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn io_status(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_sets_micro_defaults() {
        let mut workload = WorkloadItem {
            name: "crawler".to_owned(),
            command: "echo ok".to_owned(),
            ..Default::default()
        };

        normalize_workload(&mut workload).expect("normalized");

        assert_eq!(workload.memory_limit_mb, DEFAULT_MEMORY_LIMIT_MB);
        assert_eq!(workload.log_limit_bytes, DEFAULT_LOG_LIMIT_BYTES);
        assert_eq!(workload.restart_limit, DEFAULT_RESTART_LIMIT);
        assert!(!workload.id.is_empty());
    }
}
