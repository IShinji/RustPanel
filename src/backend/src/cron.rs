use std::{
    env,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use chrono::Utc;
use serde::{Deserialize, Serialize};
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

/// 定时调度交给系统 cron:任务存 tasks.json,启用的任务写成一份 cron.d 文件
/// (`RUSTPANEL_SYSTEM_CRONTAB`,如 /etc/cron.d/rustpanel),每行回调
/// `rustpanel-backend --run-cron-task <id>` 一次性执行。面板进程不再常驻调度器,
/// 节俭模式下面板空闲退出也不影响计划任务。未配置该 env 时只存不调度。
#[derive(Clone)]
pub struct CronServiceImpl {
    store: CronStore,
}

impl CronServiceImpl {
    pub fn new() -> Self {
        Self {
            store: CronStore::from_env(),
        }
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
        sync_system_crontab(&self.store, &tasks).await?;

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
        sync_system_crontab(&self.store, &tasks).await?;

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

/// `--run-cron-task <id>`:系统 cron 回调入口,跑一次任务、写日志后退出。
/// 任务已删除或被停用时静默跳过(crontab 同步前的窗口期)。
pub async fn run_oneshot_task(task_id: &str) -> Result<(), Status> {
    let store = CronStore::from_env();
    let Some(task) = store
        .load()
        .await?
        .into_iter()
        .find(|task| task.id == task_id)
    else {
        tracing::warn!(task_id, "cron task not found, skipping");
        return Ok(());
    };
    if task.state != CronTaskState::Enabled as i32 {
        return Ok(());
    }
    let run = run_task(&store, &task).await?;
    if run.state == CronRunState::Succeeded as i32 {
        Ok(())
    } else {
        Err(Status::internal(format!(
            "cron task {} finished with exit code {}",
            task.name, run.exit_code
        )))
    }
}

/// 启动时调用:按已存任务重写一次系统 crontab(未配置 RUSTPANEL_SYSTEM_CRONTAB 时 no-op)。
pub async fn sync_system_crontab_from_store() -> Result<(), Status> {
    let store = CronStore::from_env();
    let tasks = store.load().await?;
    sync_system_crontab(&store, &tasks).await
}

fn system_crontab_path() -> Option<PathBuf> {
    env::var("RUSTPANEL_SYSTEM_CRONTAB")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

async fn sync_system_crontab(store: &CronStore, tasks: &[StoredCronTask]) -> Result<(), Status> {
    let Some(path) = system_crontab_path() else {
        return Ok(());
    };
    let exe = current_exe_path()?;
    let env_file = env::var("RUSTPANEL_ENV_FILE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from);
    let content = render_crontab(tasks, &exe, store.root.as_ref(), env_file.as_deref());
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(io_status)?;
    }
    // cron.d 要求 root 所有、非组/其他可写;write_atomic 按 umask(root 下 0644)。
    crate::statefile::write_atomic(&path, content)
        .await
        .map_err(io_status)
}

/// 升级时二进制被原地替换后,Linux 上 /proc/self/exe 会带 " (deleted)" 后缀;
/// 写进 crontab 的必须是磁盘上的真实路径。
fn current_exe_path() -> Result<PathBuf, Status> {
    let exe = env::current_exe().map_err(io_status)?;
    let text = exe.to_string_lossy();
    Ok(match text.strip_suffix(" (deleted)") {
        Some(stripped) => PathBuf::from(stripped),
        None => exe,
    })
}

fn render_crontab(
    tasks: &[StoredCronTask],
    exe: &Path,
    cron_root: &Path,
    env_file: Option<&Path>,
) -> String {
    let mut out = String::from(
        "# 由 RustPanel 生成,面板里改计划任务会覆盖本文件,请勿手改。\n\
         SHELL=/bin/sh\n\
         PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\n",
    );
    for task in tasks {
        if task.state != CronTaskState::Enabled as i32 {
            continue;
        }
        let Some(schedule) = system_cron_schedule(&task.cron_expression) else {
            tracing::warn!(
                task = %task.name,
                expression = %task.cron_expression,
                "cron expression cannot be expressed in system cron, skipping"
            );
            continue;
        };
        // 命令本身不进 crontab(免去 % 转义与注入面),只回调 task id。
        let mut command = String::new();
        if let Some(env_file) = env_file {
            command.push_str(&format!(
                "set -a; . {}; set +a; ",
                shell_quote(&env_file.to_string_lossy())
            ));
        }
        command.push_str(&format!(
            "RUSTPANEL_CRON_ROOT={} {} --run-cron-task {} >/dev/null 2>&1",
            shell_quote(&cron_root.to_string_lossy()),
            shell_quote(&exe.to_string_lossy()),
            shell_quote(&task.id),
        ));
        out.push_str(&format!(
            "# {}\n{schedule} root {}\n",
            task.name.replace(['\n', '\r'], " "),
            command.replace('%', "\\%")
        ));
    }
    out
}

/// 面板用 6 段(秒 分 时 日 月 周)表达式,系统 cron 是 5 段。秒位只接受 0
/// (系统 cron 最细到分钟);5 段原样;@daily 等宏原样。其余一律拒绝。
fn system_cron_schedule(expression: &str) -> Option<String> {
    let expression = expression.trim();
    if let Some(name) = expression.strip_prefix('@') {
        return matches!(
            name,
            "reboot" | "yearly" | "annually" | "monthly" | "weekly" | "daily" | "hourly"
        )
        .then(|| expression.to_owned());
    }
    let fields: Vec<&str> = expression.split_whitespace().collect();
    let fields = match fields.len() {
        5 => fields,
        6 if fields[0] == "0" => fields[1..].to_vec(),
        _ => return None,
    };
    // 只放行 cron 语法字符,顺手杜绝换行 / 命令注入。
    let valid = fields.iter().all(|field| {
        field
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '*' | '/' | ',' | '-'))
    });
    valid.then(|| fields.join(" "))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn validate_task(task: &CronTask) -> Result<(), Status> {
    if task.name.trim().is_empty() {
        return Err(Status::invalid_argument("task name is required"));
    }
    if task.cron_expression.trim().is_empty() {
        return Err(Status::invalid_argument("cron expression is required"));
    }
    if system_cron_schedule(&task.cron_expression).is_none() {
        return Err(Status::invalid_argument(
            "cron 表达式需为 5 段、秒位为 0 的 6 段(秒 分 时 日 月 周)或 @daily 等宏",
        ));
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
        // 之前是裸 write:崩溃/并发会留半截 JSON,让 load 永久 500。
        crate::statefile::write_atomic(&self.task_path(), content)
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

    fn sample_task() -> CronTask {
        CronTask {
            id: String::new(),
            name: "backup".to_owned(),
            cron_expression: "0 0 * * * *".to_owned(),
            command: "echo ok".to_owned(),
            state: CronTaskState::Enabled.into(),
            timeout_seconds: 30,
            next_run_at: String::new(),
        }
    }

    #[test]
    fn validate_rejects_blank_name_and_expression() {
        // 只有空白字符也算空:否则会存下一个永远跑不起来的任务。
        let mut blank_name = sample_task();
        blank_name.name = "   ".to_owned();
        assert_eq!(
            validate_task(&blank_name).expect_err("name").code(),
            tonic::Code::InvalidArgument
        );

        let mut blank_expression = sample_task();
        blank_expression.cron_expression = "  ".to_owned();
        assert_eq!(
            validate_task(&blank_expression)
                .expect_err("expression")
                .code(),
            tonic::Code::InvalidArgument
        );

        let mut blank_command = sample_task();
        blank_command.command = "\t".to_owned();
        assert_eq!(
            validate_task(&blank_command).expect_err("command").code(),
            tonic::Code::InvalidArgument
        );
    }

    #[tokio::test]
    async fn store_round_trips_tasks_atomically() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = CronStore {
            root: Arc::new(dir.path().to_path_buf()),
        };

        assert!(store.load().await.expect("empty load").is_empty());

        let task = StoredCronTask::from_proto(sample_task());
        store.save(std::slice::from_ref(&task)).await.expect("save");

        let loaded = store.load().await.expect("load");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "backup");
        // 原子写:落盘后不该留下临时文件。
        assert!(!dir.path().join("tasks.json.tmp").exists());
    }

    #[test]
    fn converts_panel_expressions_to_system_cron() {
        assert_eq!(
            system_cron_schedule("0 0 2 * * *").as_deref(),
            Some("0 2 * * *")
        );
        assert_eq!(
            system_cron_schedule("0 */15 * * * *").as_deref(),
            Some("*/15 * * * *")
        );
        assert_eq!(
            system_cron_schedule(" 30 4 * * MON-FRI ").as_deref(),
            Some("30 4 * * MON-FRI")
        );
        assert_eq!(system_cron_schedule("@daily").as_deref(), Some("@daily"));
        // 秒位非 0、quartz 的 ?、7 段、未知宏、注入字符都拒绝
        assert_eq!(system_cron_schedule("*/10 * * * * *"), None);
        assert_eq!(system_cron_schedule("0 0 12 ? * *"), None);
        assert_eq!(system_cron_schedule("0 0 0 1 1 * 2027"), None);
        assert_eq!(system_cron_schedule("@every"), None);
        assert_eq!(system_cron_schedule("* * * * *;rm"), None);
    }

    #[test]
    fn crontab_calls_back_enabled_tasks_only() {
        let mut enabled = StoredCronTask::from_proto(sample_task());
        enabled.id = "task-1".to_owned();
        enabled.cron_expression = "0 0 3 * * *".to_owned();
        let mut paused = enabled.clone();
        paused.id = "task-2".to_owned();
        paused.state = CronTaskState::Paused.into();
        let mut invalid = enabled.clone();
        invalid.id = "task-3".to_owned();
        invalid.cron_expression = "*/5 * * * * *".to_owned();

        let rendered = render_crontab(
            &[enabled, paused, invalid],
            Path::new("/opt/rp/bin/rustpanel-backend"),
            Path::new("/data/cron"),
            Some(Path::new("/opt/rp/.env")),
        );

        assert!(rendered.contains(
            "0 3 * * * root set -a; . '/opt/rp/.env'; set +a; RUSTPANEL_CRON_ROOT='/data/cron' \
             '/opt/rp/bin/rustpanel-backend' --run-cron-task 'task-1' >/dev/null 2>&1"
        ));
        assert!(!rendered.contains("task-2"));
        assert!(!rendered.contains("task-3"));
    }

    #[test]
    fn crontab_escapes_percent_and_quotes() {
        let mut task = StoredCronTask::from_proto(sample_task());
        task.id = "id".to_owned();
        let rendered = render_crontab(&[task], Path::new("/it's/100%/bin"), Path::new("/c"), None);
        assert!(rendered.contains("'/it'\\''s/100\\%/bin'"));
    }
}
