// Phase F:30 秒自动回滚护栏。
//
// 调用方语义:
//   1. 高风险动作执行前:rollback::schedule(...) → 拿到 action_id + 30s 倒计时
//   2. 真正执行高风险动作(改 SSH/iptables/面板端口)
//   3. 前端展示倒计时,用户能登录则点"保留",调 ConfirmRollback
//   4. 用户没点 → tokio 后台任务到期执行 revert_command

use std::{
    collections::HashMap,
    env,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tokio::{
    sync::{Mutex, Notify},
    time::Duration,
};
use tonic::{Request as GrpcRequest, Response as GrpcResponse, Status};
use uuid::Uuid;

use crate::proto::rustpanel::v1::{
    rollback_service_server::RollbackService, ConfirmRollbackRequest, ConfirmRollbackResponse,
    ListPendingRollbacksRequest, ListPendingRollbacksResponse, PendingRollbackAction, Response,
    ScheduleRollbackRequest, ScheduleRollbackResponse,
};

const DEFAULT_ROLLBACK_ROOT: &str = "/tmp/rustpanel/rollback";
const DEFAULT_ROLLBACK_SECONDS: u32 = 30;
const MAX_ROLLBACK_SECONDS: u32 = 600;

fn ok_response(message: &str) -> Response {
    Response {
        code: 0,
        message: message.to_owned(),
        data: None,
    }
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn io_status<E: std::fmt::Display>(error: E) -> Status {
    Status::internal(format!("io error: {error}"))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredAction {
    action_id: String,
    title: String,
    description: String,
    revert_command: String,
    snapshot_json: String,
    scheduled_at_seconds: u64,
    expires_at_seconds: u64,
}

impl From<StoredAction> for PendingRollbackAction {
    fn from(value: StoredAction) -> Self {
        PendingRollbackAction {
            action_id: value.action_id,
            title: value.title,
            description: value.description,
            revert_command: value.revert_command,
            snapshot_json: value.snapshot_json,
            scheduled_at_seconds: value.scheduled_at_seconds,
            expires_at_seconds: value.expires_at_seconds,
        }
    }
}

#[derive(Clone)]
pub struct RollbackServiceImpl {
    inner: Arc<RollbackState>,
}

struct RollbackState {
    root: PathBuf,
    actions: Mutex<HashMap<String, StoredAction>>,
    cancellers: Mutex<HashMap<String, Arc<Notify>>>,
}

impl RollbackServiceImpl {
    pub fn new() -> Self {
        let root = env::var("RUSTPANEL_ROLLBACK_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_ROLLBACK_ROOT));
        Self {
            inner: Arc::new(RollbackState {
                root,
                actions: Mutex::new(HashMap::new()),
                cancellers: Mutex::new(HashMap::new()),
            }),
        }
    }

    fn actions_path(&self) -> PathBuf {
        self.inner.root.join("actions.json")
    }

    async fn save(&self, actions: &HashMap<String, StoredAction>) -> Result<(), Status> {
        tokio::fs::create_dir_all(&self.inner.root)
            .await
            .map_err(io_status)?;
        let list: Vec<&StoredAction> = actions.values().collect();
        let content = serde_json::to_string_pretty(&list).map_err(io_status)?;
        tokio::fs::write(self.actions_path(), content)
            .await
            .map_err(io_status)
    }

    fn spawn_revert_task(&self, action_id: String, after: Duration) -> Arc<Notify> {
        let notify = Arc::new(Notify::new());
        let cancel_clone = notify.clone();
        let inner = self.inner.clone();
        let id = action_id.clone();

        tokio::spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep(after) => {
                    // 到期触发回滚
                    let mut actions = inner.actions.lock().await;
                    let action = match actions.remove(&id) {
                        Some(a) => a,
                        None => return,
                    };
                    drop(actions);
                    // 执行 revert_command(空字符串则跳过 shell 执行,仅删除记录)
                    if !action.revert_command.trim().is_empty() {
                        let result = tokio::process::Command::new("sh")
                            .arg("-c")
                            .arg(&action.revert_command)
                            .output()
                            .await;
                        match result {
                            Ok(output) => {
                                if !output.status.success() {
                                    tracing::warn!(
                                        action_id = %action.action_id,
                                        title = %action.title,
                                        stderr = %String::from_utf8_lossy(&output.stderr),
                                        "rollback revert command exited non-zero"
                                    );
                                }
                            }
                            Err(error) => {
                                tracing::error!(
                                    action_id = %action.action_id,
                                    %error,
                                    "rollback revert command failed to spawn"
                                );
                            }
                        }
                    }
                    // 持久化:把已触发的动作从 actions.json 里移掉
                    let actions_snapshot = inner.actions.lock().await.clone();
                    let root = inner.root.clone();
                    let _ = save_actions_to_disk(&root, &actions_snapshot).await;
                    let mut cancellers = inner.cancellers.lock().await;
                    cancellers.remove(&id);
                    tracing::info!(
                        action_id = %action.action_id,
                        title = %action.title,
                        "auto-rollback executed"
                    );
                }
                _ = cancel_clone.notified() => {
                    // 用户确认保留,定时任务安静退出
                }
            }
        });

        notify
    }
}

impl Default for RollbackServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

async fn save_actions_to_disk(
    root: &std::path::Path,
    actions: &HashMap<String, StoredAction>,
) -> Result<(), std::io::Error> {
    tokio::fs::create_dir_all(root).await?;
    let list: Vec<&StoredAction> = actions.values().collect();
    let content = serde_json::to_string_pretty(&list).unwrap_or_else(|_| "[]".to_owned());
    tokio::fs::write(root.join("actions.json"), content).await
}

#[tonic::async_trait]
impl RollbackService for RollbackServiceImpl {
    async fn schedule_rollback(
        &self,
        request: GrpcRequest<ScheduleRollbackRequest>,
    ) -> Result<GrpcResponse<ScheduleRollbackResponse>, Status> {
        let req = request.into_inner();
        if req.title.trim().is_empty() {
            return Err(Status::invalid_argument("title is required"));
        }
        let after_seconds = if req.rollback_after_seconds == 0 {
            DEFAULT_ROLLBACK_SECONDS
        } else {
            req.rollback_after_seconds.min(MAX_ROLLBACK_SECONDS)
        };
        let now = now_seconds();
        let action_id = Uuid::new_v4().simple().to_string();
        let stored = StoredAction {
            action_id: action_id.clone(),
            title: req.title,
            description: req.description,
            revert_command: req.revert_command,
            snapshot_json: req.snapshot_json,
            scheduled_at_seconds: now,
            expires_at_seconds: now + after_seconds as u64,
        };

        {
            let mut actions = self.inner.actions.lock().await;
            actions.insert(action_id.clone(), stored.clone());
            self.save(&actions).await?;
        }

        let canceller =
            self.spawn_revert_task(action_id.clone(), Duration::from_secs(after_seconds as u64));
        {
            let mut cancellers = self.inner.cancellers.lock().await;
            cancellers.insert(action_id.clone(), canceller);
        }

        Ok(GrpcResponse::new(ScheduleRollbackResponse {
            status: Some(ok_response("rollback scheduled")),
            action: Some(stored.into()),
        }))
    }

    async fn confirm_rollback(
        &self,
        request: GrpcRequest<ConfirmRollbackRequest>,
    ) -> Result<GrpcResponse<ConfirmRollbackResponse>, Status> {
        let req = request.into_inner();
        let id = req.action_id;
        // 取消 timer
        let canceller = {
            let mut cancellers = self.inner.cancellers.lock().await;
            cancellers.remove(&id)
        };
        let removed = {
            let mut actions = self.inner.actions.lock().await;
            let removed = actions.remove(&id).is_some();
            if removed {
                self.save(&actions).await?;
            }
            removed
        };
        if let Some(canceller) = canceller {
            canceller.notify_one();
        }
        if !removed {
            return Err(Status::not_found("action_id not found"));
        }
        Ok(GrpcResponse::new(ConfirmRollbackResponse {
            status: Some(ok_response("kept; rollback canceled")),
        }))
    }

    async fn list_pending_rollbacks(
        &self,
        _request: GrpcRequest<ListPendingRollbacksRequest>,
    ) -> Result<GrpcResponse<ListPendingRollbacksResponse>, Status> {
        let actions = self.inner.actions.lock().await;
        let mut list: Vec<PendingRollbackAction> =
            actions.values().cloned().map(Into::into).collect();
        list.sort_by_key(|a| a.expires_at_seconds);
        Ok(GrpcResponse::new(ListPendingRollbacksResponse {
            status: Some(ok_response("ok")),
            actions: list,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn schedule_then_confirm_cancels_revert() {
        let tmp = tempfile::tempdir().expect("tempdir");
        env::set_var("RUSTPANEL_ROLLBACK_ROOT", tmp.path());
        let svc = RollbackServiceImpl::new();

        let resp = svc
            .schedule_rollback(GrpcRequest::new(ScheduleRollbackRequest {
                title: "test".into(),
                description: "".into(),
                revert_command: "echo never".into(),
                snapshot_json: "{}".into(),
                rollback_after_seconds: 5,
            }))
            .await
            .unwrap()
            .into_inner();
        let id = resp.action.unwrap().action_id;

        // 立即确认
        svc.confirm_rollback(GrpcRequest::new(ConfirmRollbackRequest {
            action_id: id.clone(),
        }))
        .await
        .unwrap();

        // 列表里不再有它
        let pending = svc
            .list_pending_rollbacks(GrpcRequest::new(ListPendingRollbacksRequest {}))
            .await
            .unwrap()
            .into_inner();
        assert!(pending.actions.is_empty());

        env::remove_var("RUSTPANEL_ROLLBACK_ROOT");
    }

    #[tokio::test]
    async fn schedule_auto_reverts_after_timeout() {
        let tmp = tempfile::tempdir().expect("tempdir");
        env::set_var("RUSTPANEL_ROLLBACK_ROOT", tmp.path());
        let svc = RollbackServiceImpl::new();

        // revert_command 留空避免在测试沙箱里跑 shell
        svc.schedule_rollback(GrpcRequest::new(ScheduleRollbackRequest {
            title: "auto-revert-test".into(),
            description: "".into(),
            revert_command: "".into(),
            snapshot_json: "{}".into(),
            rollback_after_seconds: 1,
        }))
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_millis(1500)).await;

        let pending = svc
            .list_pending_rollbacks(GrpcRequest::new(ListPendingRollbacksRequest {}))
            .await
            .unwrap()
            .into_inner();
        assert!(pending.actions.is_empty(), "auto revert should clear list");

        env::remove_var("RUSTPANEL_ROLLBACK_ROOT");
    }
}
