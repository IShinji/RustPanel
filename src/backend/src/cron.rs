use std::{env, path::PathBuf, sync::Arc, time::Duration};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;
use tokio_cron_scheduler::JobScheduler;
use tonic::{Request, Response as GrpcResponse, Status};
use uuid::Uuid;

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        cron_service_server::CronService, CreateCronTaskRequest, CreateCronTaskResponse, CronRun,
        CronRunState, CronTask, CronTaskState, GetCronTaskLogRequest, GetCronTaskLogResponse,
        ListCronTasksRequest, ListCronTasksResponse, RunCronTaskRequest, RunCronTaskResponse,
        UpdateCronTaskStateRequest, UpdateCronTaskStateResponse,
    },
};

const DEFAULT_CRON_ROOT: &str = "/tmp/rustpanel/cron";
const DEFAULT_TIMEOUT_SECONDS: u64 = 300;

#[derive(Clone)]
pub struct CronServiceImpl {
    store: CronStore,
    scheduler: Arc<OnceCell<JobScheduler>>,
}

impl CronServiceImpl {
    pub fn new() -> Self {
        Self {
            store: CronStore::from_env(),
            scheduler: Arc::new(OnceCell::new()),
        }
    }

    async fn ensure_scheduler(&self) -> Result<(), Status> {
        self.scheduler
            .get_or_try_init(|| async {
                let scheduler = JobScheduler::new().await.map_err(io_status)?;
                scheduler.start().await.map_err(io_status)?;
                Ok::<JobScheduler, Status>(scheduler)
            })
            .await
            .map(|_| ())
    }
}

impl Default for CronServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl CronService for CronServiceImpl {
    async fn list_cron_tasks(
        &self,
        _request: Request<ListCronTasksRequest>,
    ) -> Result<GrpcResponse<ListCronTasksResponse>, Status> {
        self.ensure_scheduler().await?;
        let tasks = self.store.load().await?;

        Ok(GrpcResponse::new(ListCronTasksResponse {
            status: Some(ok_response("ok")),
            tasks: tasks.into_iter().map(StoredCronTask::into_proto).collect(),
        }))
    }

    async fn create_cron_task(
        &self,
        request: Request<CreateCronTaskRequest>,
    ) -> Result<GrpcResponse<CreateCronTaskResponse>, Status> {
        self.ensure_scheduler().await?;
        let mut task = request
            .into_inner()
            .task
            .ok_or_else(|| Status::invalid_argument("task is required"))?;
        validate_task(&task)?;
        if task.id.trim().is_empty() {
            task.id = Uuid::new_v4().to_string();
        }
        if task.timeout_seconds == 0 {
            task.timeout_seconds = DEFAULT_TIMEOUT_SECONDS;
        }
        if task.state == CronTaskState::Unspecified as i32 {
            task.state = CronTaskState::Enabled.into();
        }
        let mut tasks = self.store.load().await?;
        tasks.retain(|stored| stored.id != task.id);
        tasks.push(StoredCronTask::from_proto(task.clone()));
        self.store.save(&tasks).await?;

        Ok(GrpcResponse::new(CreateCronTaskResponse {
            status: Some(ok_response("cron task saved")),
            task: Some(task),
        }))
    }

    async fn update_cron_task_state(
        &self,
        request: Request<UpdateCronTaskStateRequest>,
    ) -> Result<GrpcResponse<UpdateCronTaskStateResponse>, Status> {
        let request = request.into_inner();
        let mut tasks = self.store.load().await?;
        let mut found = false;
        for task in &mut tasks {
            if task.id == request.task_id {
                task.state = request.state;
                found = true;
            }
        }
        if !found {
            return Err(Status::not_found("cron task not found"));
        }
        self.store.save(&tasks).await?;

        Ok(GrpcResponse::new(UpdateCronTaskStateResponse {
            status: Some(ok_response("cron task state updated")),
        }))
    }

    async fn run_cron_task(
        &self,
        request: Request<RunCronTaskRequest>,
    ) -> Result<GrpcResponse<RunCronTaskResponse>, Status> {
        let task_id = request.into_inner().task_id;
        let task = self
            .store
            .load()
            .await?
            .into_iter()
            .find(|task| task.id == task_id)
            .ok_or_else(|| Status::not_found("cron task not found"))?;
        let run = run_task(&self.store, &task).await?;

        Ok(GrpcResponse::new(RunCronTaskResponse {
            status: Some(ok_response("cron task executed")),
            run: Some(run),
        }))
    }

    async fn get_cron_task_log(
        &self,
        request: Request<GetCronTaskLogRequest>,
    ) -> Result<GrpcResponse<GetCronTaskLogResponse>, Status> {
        let task_id = request.into_inner().task_id;
        let log_path = self.store.log_dir().join(format!("{task_id}.log"));
        let content = tokio::fs::read_to_string(log_path)
            .await
            .unwrap_or_default();

        Ok(GrpcResponse::new(GetCronTaskLogResponse {
            status: Some(ok_response("ok")),
            content,
        }))
    }
}

async fn run_task(store: &CronStore, task: &StoredCronTask) -> Result<CronRun, Status> {
    let started_at = current_timestamp();
    let run_id = Uuid::new_v4().to_string();
    tokio::fs::create_dir_all(store.log_dir())
        .await
        .map_err(io_status)?;
    let log_path = store.log_dir().join(format!("{}.log", task.id));
    let command = task.command.clone();
    let timeout = Duration::from_secs(task.timeout_seconds.max(1));
    let shell = default_shell();
    let output = tokio::time::timeout(
        timeout,
        tokio::process::Command::new(shell)
            .arg("-lc")
            .arg(command)
            .output(),
    )
    .await;
    let finished_at = current_timestamp();

    let (state, exit_code, log_content) = match output {
        Ok(Ok(output)) => {
            let exit_code = output.status.code().unwrap_or_default();
            let state = if output.status.success() {
                CronRunState::Succeeded
            } else {
                CronRunState::Failed
            };
            let mut content = String::from_utf8_lossy(&output.stdout).to_string();
            content.push_str(&String::from_utf8_lossy(&output.stderr));
            (state, exit_code, content)
        }
        Ok(Err(error)) => (CronRunState::Failed, -1, error.to_string()),
        Err(_) => (
            CronRunState::TimedOut,
            -1,
            format!("task timed out after {} seconds", task.timeout_seconds),
        ),
    };

    tokio::fs::write(&log_path, log_content)
        .await
        .map_err(io_status)?;

    Ok(CronRun {
        id: run_id,
        task_id: task.id.clone(),
        state: state.into(),
        exit_code,
        log_path: log_path.to_string_lossy().to_string(),
        started_at_seconds: started_at,
        finished_at_seconds: finished_at,
    })
}

fn validate_task(task: &CronTask) -> Result<(), Status> {
    if task.name.trim().is_empty() {
        return Err(Status::invalid_argument("task name is required"));
    }
    if task.cron_expression.trim().is_empty() {
        return Err(Status::invalid_argument("cron expression is required"));
    }
    if task.command.trim().is_empty() {
        return Err(Status::invalid_argument("command is required"));
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct CronStore {
    root: Arc<PathBuf>,
}

impl CronStore {
    fn from_env() -> Self {
        let root = env::var("RUSTPANEL_CRON_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_CRON_ROOT));

        Self {
            root: Arc::new(root),
        }
    }

    async fn load(&self) -> Result<Vec<StoredCronTask>, Status> {
        match tokio::fs::read_to_string(self.task_path()).await {
            Ok(content) => serde_json::from_str(&content).map_err(io_status),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(io_status(error)),
        }
    }

    async fn save(&self, tasks: &[StoredCronTask]) -> Result<(), Status> {
        tokio::fs::create_dir_all(self.root.as_ref())
            .await
            .map_err(io_status)?;
        let content = serde_json::to_string_pretty(tasks).map_err(io_status)?;
        tokio::fs::write(self.task_path(), content)
            .await
            .map_err(io_status)
    }

    fn task_path(&self) -> PathBuf {
        self.root.join("tasks.json")
    }

    fn log_dir(&self) -> PathBuf {
        self.root
            .join("logs")
            .join(Utc::now().format("%Y-%m-%d").to_string())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StoredCronTask {
    id: String,
    name: String,
    cron_expression: String,
    command: String,
    state: i32,
    timeout_seconds: u64,
}

impl StoredCronTask {
    fn from_proto(task: CronTask) -> Self {
        Self {
            id: task.id,
            name: task.name,
            cron_expression: task.cron_expression,
            command: task.command,
            state: task.state,
            timeout_seconds: task.timeout_seconds,
        }
    }

    fn into_proto(self) -> CronTask {
        CronTask {
            id: self.id,
            name: self.name,
            cron_expression: self.cron_expression,
            command: self.command,
            state: self.state,
            timeout_seconds: self.timeout_seconds,
            next_run_at: String::new(),
        }
    }
}

fn default_shell() -> &'static str {
    if cfg!(target_os = "windows") {
        "powershell.exe"
    } else {
        "/bin/sh"
    }
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
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
    fn validates_required_task_fields() {
        let mut task = CronTask {
            id: String::new(),
            name: "backup".to_owned(),
            cron_expression: "0 0 * * * *".to_owned(),
            command: "echo ok".to_owned(),
            state: CronTaskState::Enabled.into(),
            timeout_seconds: 30,
            next_run_at: String::new(),
        };
        assert!(validate_task(&task).is_ok());

        task.command.clear();
        assert!(validate_task(&task).is_err());
    }
}
