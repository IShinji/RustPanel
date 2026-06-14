use std::{env, path::PathBuf, sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use tonic::{Request, Response as GrpcResponse, Status};
use uuid::Uuid;

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        notification_service_server::NotificationService, DeleteNotificationChannelRequest,
        DeleteNotificationChannelResponse, GetNotificationSettingsRequest,
        GetNotificationSettingsResponse, ListNotificationChannelsRequest,
        ListNotificationChannelsResponse, ListNotificationHistoryRequest,
        ListNotificationHistoryResponse, NotificationChannel, NotificationChannelKind,
        NotificationEventKind, NotificationRecord, NotificationRule, NotificationSettings,
        TestNotificationChannelRequest, TestNotificationChannelResponse,
        UpdateNotificationSettingsRequest, UpdateNotificationSettingsResponse,
        UpsertNotificationChannelRequest, UpsertNotificationChannelResponse,
    },
};

const DEFAULT_NOTIFICATION_ROOT: &str = "/tmp/rustpanel/notification";
const HISTORY_MAX: usize = 200;
const HTTP_TIMEOUT_SECONDS: u64 = 10;
// 返回给前端时密钥脱敏成这个占位;upsert 时原样回传表示"保持不变"。
const SECRET_REDACTED: &str = "__rustpanel_secret_kept__";

#[derive(Clone)]
pub struct NotificationServiceImpl {
    store: NotificationStore,
}

impl NotificationServiceImpl {
    pub fn new() -> Self {
        Self {
            store: NotificationStore::from_env(),
        }
    }
}

impl Default for NotificationServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl NotificationService for NotificationServiceImpl {
    async fn list_notification_channels(
        &self,
        _request: Request<ListNotificationChannelsRequest>,
    ) -> Result<GrpcResponse<ListNotificationChannelsResponse>, Status> {
        let state = self.store.load().await?;
        Ok(GrpcResponse::new(ListNotificationChannelsResponse {
            status: Some(ok_response("ok")),
            channels: state
                .channels
                .into_iter()
                .map(|channel| redact(channel.into_proto()))
                .collect(),
        }))
    }

    async fn upsert_notification_channel(
        &self,
        request: Request<UpsertNotificationChannelRequest>,
    ) -> Result<GrpcResponse<UpsertNotificationChannelResponse>, Status> {
        let incoming = request
            .into_inner()
            .channel
            .ok_or_else(|| Status::invalid_argument("notification channel is required"))?;
        validate_channel(&incoming)?;

        let _guard = self.store.write_lock.lock().await;
        let mut state = self.store.load().await?;
        let now = current_timestamp();
        let existing = state.channels.iter().find(|c| c.id == incoming.id).cloned();

        let mut stored = StoredChannel::from_proto(incoming);
        if stored.id.trim().is_empty() {
            stored.id = Uuid::new_v4().to_string();
            stored.created_at_seconds = now;
        } else if let Some(old) = &existing {
            stored.created_at_seconds = old.created_at_seconds;
            // 前端留空 / 回传脱敏占位 → 保留原密钥,避免编辑时把密钥清掉。
            if stored.secret.trim().is_empty() || stored.secret == SECRET_REDACTED {
                stored.secret = old.secret.clone();
            }
        } else {
            stored.created_at_seconds = now;
        }
        stored.updated_at_seconds = now;

        state.channels.retain(|c| c.id != stored.id);
        state.channels.push(stored.clone());
        self.store.save(&state).await?;

        Ok(GrpcResponse::new(UpsertNotificationChannelResponse {
            status: Some(ok_response("notification channel saved")),
            channel: Some(redact(stored.into_proto())),
        }))
    }

    async fn delete_notification_channel(
        &self,
        request: Request<DeleteNotificationChannelRequest>,
    ) -> Result<GrpcResponse<DeleteNotificationChannelResponse>, Status> {
        let id = request.into_inner().id;
        let _guard = self.store.write_lock.lock().await;
        let mut state = self.store.load().await?;
        let before = state.channels.len();
        state.channels.retain(|c| c.id != id);
        if state.channels.len() == before {
            return Err(Status::not_found("notification channel not found"));
        }
        self.store.save(&state).await?;
        Ok(GrpcResponse::new(DeleteNotificationChannelResponse {
            status: Some(ok_response("notification channel deleted")),
        }))
    }

    async fn test_notification_channel(
        &self,
        request: Request<TestNotificationChannelRequest>,
    ) -> Result<GrpcResponse<TestNotificationChannelResponse>, Status> {
        let id = request.into_inner().id;
        let _guard = self.store.write_lock.lock().await;
        let mut state = self.store.load().await?;
        let channel = state
            .channels
            .iter()
            .find(|c| c.id == id)
            .cloned()
            .ok_or_else(|| Status::not_found("notification channel not found"))?;

        let title = "RustPanel 测试通知";
        let body = "这是一条来自 RustPanel 的测试消息,收到说明渠道配置正确。";
        // 测试无视 enabled,直接发这一个渠道。
        let (delivered, failed) = dispatch(std::slice::from_ref(&channel), title, body, true).await;
        let record = StoredRecord {
            id: Uuid::new_v4().to_string(),
            event: NotificationEventKind::Test as i32,
            title: title.to_owned(),
            body: body.to_owned(),
            occurred_at_seconds: current_timestamp(),
            delivered_channels: delivered.clone(),
            failed_channels: failed.clone(),
        };
        state.history.push(record.clone());
        truncate_history(&mut state.history);
        self.store.save(&state).await?;

        let status = if failed.is_empty() {
            ok_response("test notification delivered")
        } else {
            crate::error_response(1, format!("delivery failed for: {}", failed.join(", ")))
        };
        Ok(GrpcResponse::new(TestNotificationChannelResponse {
            status: Some(status),
            record: Some(record.into_proto()),
        }))
    }

    async fn get_notification_settings(
        &self,
        _request: Request<GetNotificationSettingsRequest>,
    ) -> Result<GrpcResponse<GetNotificationSettingsResponse>, Status> {
        let state = self.store.load().await?;
        let rules = if state.settings.rules.is_empty() {
            default_rules()
        } else {
            state.settings.rules
        };
        Ok(GrpcResponse::new(GetNotificationSettingsResponse {
            status: Some(ok_response("ok")),
            settings: Some(NotificationSettings {
                rules: rules.into_iter().map(StoredRule::into_proto).collect(),
            }),
        }))
    }

    async fn update_notification_settings(
        &self,
        request: Request<UpdateNotificationSettingsRequest>,
    ) -> Result<GrpcResponse<UpdateNotificationSettingsResponse>, Status> {
        let settings = request
            .into_inner()
            .settings
            .ok_or_else(|| Status::invalid_argument("notification settings are required"))?;

        let _guard = self.store.write_lock.lock().await;
        let mut state = self.store.load().await?;
        state.settings = StoredSettings::from_proto(settings);
        self.store.save(&state).await?;
        Ok(GrpcResponse::new(UpdateNotificationSettingsResponse {
            status: Some(ok_response("notification settings updated")),
            settings: Some(state.settings.into_proto()),
        }))
    }

    async fn list_notification_history(
        &self,
        request: Request<ListNotificationHistoryRequest>,
    ) -> Result<GrpcResponse<ListNotificationHistoryResponse>, Status> {
        let limit = request.into_inner().limit.clamp(1, 500) as usize;
        let mut history = self.store.load().await?.history;
        history.sort_by_key(|record| std::cmp::Reverse(record.occurred_at_seconds));
        history.truncate(limit);
        Ok(GrpcResponse::new(ListNotificationHistoryResponse {
            status: Some(ok_response("ok")),
            records: history.into_iter().map(StoredRecord::into_proto).collect(),
        }))
    }
}

/// 供其它模块(如 SSH 自动封禁)best-effort 调用的事件分发入口。永不 panic /
/// 永不向调用方传播错误;建议调用方用 `tokio::spawn` 触发,避免阻塞主流程。
/// 仅当对应事件规则启用且存在已启用渠道时才真正发送,并落历史。
pub(crate) async fn notify_event(event: NotificationEventKind, title: &str, body: &str) {
    let store = NotificationStore::from_env();
    let _guard = store.write_lock.lock().await;
    let mut state = match store.load().await {
        Ok(state) => state,
        Err(error) => {
            tracing::warn!(%error, "notification: load state failed");
            return;
        }
    };
    if !event_enabled(&state.settings, event) {
        return;
    }
    if !state.channels.iter().any(|channel| channel.enabled) {
        return;
    }
    let (delivered, failed) = dispatch(&state.channels, title, body, false).await;
    state.history.push(StoredRecord {
        id: Uuid::new_v4().to_string(),
        event: event as i32,
        title: title.to_owned(),
        body: body.to_owned(),
        occurred_at_seconds: current_timestamp(),
        delivered_channels: delivered,
        failed_channels: failed,
    });
    truncate_history(&mut state.history);
    if let Err(error) = store.save(&state).await {
        tracing::warn!(%error, "notification: save history failed");
    }
}

/// 向渠道列表分发。`include_disabled=true` 时无视 enabled(用于测试单个渠道)。
async fn dispatch(
    channels: &[StoredChannel],
    title: &str,
    body: &str,
    include_disabled: bool,
) -> (Vec<String>, Vec<String>) {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECONDS))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(%error, "notification: build http client failed");
            let failed = channels
                .iter()
                .filter(|c| include_disabled || c.enabled)
                .map(|c| c.name.clone())
                .collect();
            return (Vec::new(), failed);
        }
    };
    let mut delivered = Vec::new();
    let mut failed = Vec::new();
    for channel in channels.iter().filter(|c| include_disabled || c.enabled) {
        match send_to_channel(&client, channel, title, body).await {
            Ok(()) => delivered.push(channel.name.clone()),
            Err(error) => {
                tracing::warn!(channel = %channel.name, %error, "notification delivery failed");
                failed.push(channel.name.clone());
            }
        }
    }
    (delivered, failed)
}

async fn send_to_channel(
    client: &reqwest::Client,
    channel: &StoredChannel,
    title: &str,
    body: &str,
) -> Result<(), String> {
    let kind = NotificationChannelKind::try_from(channel.kind)
        .unwrap_or(NotificationChannelKind::Unspecified);
    let target = channel.target.trim();
    if target.is_empty() {
        return Err("channel target is empty".to_owned());
    }
    let secret = channel.secret.trim();
    let text = format!("{title}\n{body}");

    let request = match kind {
        NotificationChannelKind::Webhook => {
            let mut builder = client.post(target).json(&serde_json::json!({
                "title": title,
                "body": body,
                "text": text,
            }));
            if !secret.is_empty() {
                builder = builder.bearer_auth(secret);
            }
            builder
        }
        NotificationChannelKind::Telegram => {
            if secret.is_empty() {
                return Err("telegram bot token is empty".to_owned());
            }
            let url = format!("https://api.telegram.org/bot{secret}/sendMessage");
            client
                .post(url)
                .json(&serde_json::json!({ "chat_id": target, "text": text }))
        }
        NotificationChannelKind::Dingtalk | NotificationChannelKind::Wecom => client
            .post(target)
            .json(&serde_json::json!({ "msgtype": "text", "text": { "content": text } })),
        NotificationChannelKind::Bark => client
            .post(target)
            .form(&[("title", title), ("body", body)]),
        NotificationChannelKind::Unspecified => {
            return Err("unknown channel kind".to_owned());
        }
    };

    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status();
    if status.is_success() {
        Ok(())
    } else {
        let detail: String = response
            .text()
            .await
            .unwrap_or_default()
            .chars()
            .take(200)
            .collect();
        Err(format!("HTTP {status}: {detail}"))
    }
}

fn event_enabled(settings: &StoredSettings, event: NotificationEventKind) -> bool {
    let target = event as i32;
    match settings.rules.iter().find(|rule| rule.event == target) {
        Some(rule) => rule.enabled,
        // 无显式规则:默认开(让安全告警开箱即用),具体默认见 default_rules。
        None => default_rules()
            .iter()
            .find(|rule| rule.event == target)
            .map(|rule| rule.enabled)
            .unwrap_or(true),
    }
}

fn truncate_history(history: &mut Vec<StoredRecord>) {
    if history.len() > HISTORY_MAX {
        let excess = history.len() - HISTORY_MAX;
        history.drain(0..excess);
    }
}

fn default_rules() -> Vec<StoredRule> {
    vec![
        StoredRule {
            event: NotificationEventKind::CertExpiry as i32,
            enabled: true,
            threshold: 14,
        },
        StoredRule {
            event: NotificationEventKind::HighLoad as i32,
            enabled: true,
            threshold: 90,
        },
        StoredRule {
            event: NotificationEventKind::DiskFull as i32,
            enabled: true,
            threshold: 90,
        },
        StoredRule {
            event: NotificationEventKind::SshAutoBan as i32,
            enabled: true,
            threshold: 0,
        },
        StoredRule {
            // 登录失败较吵,默认关,运维可手动开。
            event: NotificationEventKind::LoginFailed as i32,
            enabled: false,
            threshold: 5,
        },
    ]
}

fn validate_channel(channel: &NotificationChannel) -> Result<(), Status> {
    if channel.name.trim().is_empty() {
        return Err(Status::invalid_argument("channel name is required"));
    }
    let kind = NotificationChannelKind::try_from(channel.kind)
        .unwrap_or(NotificationChannelKind::Unspecified);
    if kind == NotificationChannelKind::Unspecified {
        return Err(Status::invalid_argument("channel kind is required"));
    }
    if channel.target.trim().is_empty() {
        return Err(Status::invalid_argument("channel target is required"));
    }
    Ok(())
}

fn redact(mut channel: NotificationChannel) -> NotificationChannel {
    if !channel.secret.is_empty() {
        channel.secret = SECRET_REDACTED.to_owned();
    }
    channel
}

#[derive(Clone, Debug)]
struct NotificationStore {
    root: Arc<PathBuf>,
    // 串行化 load→改→save,防并发写丢更新(渠道列表 / 历史)。
    write_lock: Arc<tokio::sync::Mutex<()>>,
}

impl NotificationStore {
    fn from_env() -> Self {
        let root = env::var("RUSTPANEL_NOTIFICATION_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_NOTIFICATION_ROOT));
        Self {
            root: Arc::new(root),
            write_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    async fn load(&self) -> Result<StoredState, Status> {
        match tokio::fs::read_to_string(self.state_path()).await {
            Ok(content) => serde_json::from_str(&content).map_err(io_status),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(StoredState::default())
            }
            Err(error) => Err(io_status(error)),
        }
    }

    async fn save(&self, state: &StoredState) -> Result<(), Status> {
        tokio::fs::create_dir_all(self.root.as_ref())
            .await
            .map_err(io_status)?;
        let content = serde_json::to_string_pretty(state).map_err(io_status)?;
        // tmp + rename:避免半截 JSON 让 load 永久 500。
        let path = self.state_path();
        let tmp = path.with_extension("json.tmp");
        tokio::fs::write(&tmp, content).await.map_err(io_status)?;
        tokio::fs::rename(&tmp, &path).await.map_err(io_status)
    }

    fn state_path(&self) -> PathBuf {
        self.root.join("state.json")
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct StoredState {
    #[serde(default)]
    channels: Vec<StoredChannel>,
    #[serde(default)]
    settings: StoredSettings,
    #[serde(default)]
    history: Vec<StoredRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredChannel {
    id: String,
    name: String,
    kind: i32,
    target: String,
    secret: String,
    enabled: bool,
    created_at_seconds: u64,
    updated_at_seconds: u64,
}

impl StoredChannel {
    fn from_proto(channel: NotificationChannel) -> Self {
        Self {
            id: channel.id,
            name: channel.name,
            kind: channel.kind,
            target: channel.target,
            secret: channel.secret,
            enabled: channel.enabled,
            created_at_seconds: channel.created_at_seconds,
            updated_at_seconds: channel.updated_at_seconds,
        }
    }

    fn into_proto(self) -> NotificationChannel {
        NotificationChannel {
            id: self.id,
            name: self.name,
            kind: self.kind,
            target: self.target,
            secret: self.secret,
            enabled: self.enabled,
            created_at_seconds: self.created_at_seconds,
            updated_at_seconds: self.updated_at_seconds,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct StoredSettings {
    #[serde(default)]
    rules: Vec<StoredRule>,
}

impl StoredSettings {
    fn from_proto(settings: NotificationSettings) -> Self {
        Self {
            rules: settings
                .rules
                .into_iter()
                .map(StoredRule::from_proto)
                .collect(),
        }
    }

    fn into_proto(self) -> NotificationSettings {
        NotificationSettings {
            rules: self.rules.into_iter().map(StoredRule::into_proto).collect(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredRule {
    event: i32,
    enabled: bool,
    threshold: u32,
}

impl StoredRule {
    fn from_proto(rule: NotificationRule) -> Self {
        Self {
            event: rule.event,
            enabled: rule.enabled,
            threshold: rule.threshold,
        }
    }

    fn into_proto(self) -> NotificationRule {
        NotificationRule {
            event: self.event,
            enabled: self.enabled,
            threshold: self.threshold,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredRecord {
    id: String,
    event: i32,
    title: String,
    body: String,
    occurred_at_seconds: u64,
    #[serde(default)]
    delivered_channels: Vec<String>,
    #[serde(default)]
    failed_channels: Vec<String>,
}

impl StoredRecord {
    fn into_proto(self) -> NotificationRecord {
        NotificationRecord {
            id: self.id,
            event: self.event,
            title: self.title,
            body: self.body,
            occurred_at_seconds: self.occurred_at_seconds,
            delivered_channels: self.delivered_channels,
            failed_channels: self.failed_channels,
        }
    }
}

fn io_status(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_hides_nonempty_secret() {
        let channel = NotificationChannel {
            secret: "super-secret".to_owned(),
            ..Default::default()
        };
        assert_eq!(redact(channel).secret, SECRET_REDACTED);
        let empty = NotificationChannel::default();
        assert_eq!(redact(empty).secret, "");
    }

    #[test]
    fn validate_channel_requires_kind_and_target() {
        let mut channel = NotificationChannel {
            name: "ops".to_owned(),
            kind: NotificationChannelKind::Webhook as i32,
            target: "https://example.com/hook".to_owned(),
            ..Default::default()
        };
        assert!(validate_channel(&channel).is_ok());

        channel.kind = NotificationChannelKind::Unspecified as i32;
        assert!(validate_channel(&channel).is_err());

        channel.kind = NotificationChannelKind::Webhook as i32;
        channel.target = "  ".to_owned();
        assert!(validate_channel(&channel).is_err());
    }

    #[test]
    fn event_enabled_falls_back_to_defaults() {
        let empty = StoredSettings::default();
        // SSH 自动封禁默认开;登录失败默认关。
        assert!(event_enabled(&empty, NotificationEventKind::SshAutoBan));
        assert!(!event_enabled(&empty, NotificationEventKind::LoginFailed));
    }

    #[test]
    fn truncate_history_keeps_tail() {
        let mut history: Vec<StoredRecord> = (0..(HISTORY_MAX + 5))
            .map(|index| StoredRecord {
                id: index.to_string(),
                event: 0,
                title: String::new(),
                body: String::new(),
                occurred_at_seconds: index as u64,
                delivered_channels: Vec::new(),
                failed_channels: Vec::new(),
            })
            .collect();
        truncate_history(&mut history);
        assert_eq!(history.len(), HISTORY_MAX);
        // 最旧的被丢弃,保留尾部。
        assert_eq!(history.first().map(|r| r.id.as_str()), Some("5"));
    }
}
