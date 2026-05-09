use std::{
    env,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tonic::{Request, Response as GrpcResponse, Status};
use uuid::Uuid;

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        audit_service_server::AuditService, AnalyzeAuditEventsRequest, AnalyzeAuditEventsResponse,
        AuditEvent, ClearAuditEventsRequest, ClearAuditEventsResponse, ListAuditEventsRequest,
        ListAuditEventsResponse, RecordAuditEventRequest, RecordAuditEventResponse,
    },
};

const DEFAULT_AUDIT_ROOT: &str = "/tmp/rustpanel/audit";
const DEFAULT_AUDIT_LIMIT: usize = 200;

#[derive(Clone, Debug, Default)]
pub struct AuditServiceImpl;

#[tonic::async_trait]
impl AuditService for AuditServiceImpl {
    async fn record_audit_event(
        &self,
        request: Request<RecordAuditEventRequest>,
    ) -> Result<GrpcResponse<RecordAuditEventResponse>, Status> {
        let mut event = request
            .into_inner()
            .event
            .ok_or_else(|| Status::invalid_argument("audit event is required"))?;
        normalize_event(&mut event);
        append_event(&event).await?;

        Ok(GrpcResponse::new(RecordAuditEventResponse {
            status: Some(ok_response("audit event recorded")),
            event: Some(event),
        }))
    }

    async fn list_audit_events(
        &self,
        request: Request<ListAuditEventsRequest>,
    ) -> Result<GrpcResponse<ListAuditEventsResponse>, Status> {
        let request = request.into_inner();
        let events = filter_events(
            read_events().await?,
            &request.module,
            &request.query,
            request.limit,
        );

        Ok(GrpcResponse::new(ListAuditEventsResponse {
            status: Some(ok_response("ok")),
            events,
        }))
    }

    async fn clear_audit_events(
        &self,
        _request: Request<ClearAuditEventsRequest>,
    ) -> Result<GrpcResponse<ClearAuditEventsResponse>, Status> {
        match tokio::fs::remove_file(audit_path()).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_status(error)),
        }

        Ok(GrpcResponse::new(ClearAuditEventsResponse {
            status: Some(ok_response("audit events cleared")),
        }))
    }

    async fn analyze_audit_events(
        &self,
        request: Request<AnalyzeAuditEventsRequest>,
    ) -> Result<GrpcResponse<AnalyzeAuditEventsResponse>, Status> {
        let request = request.into_inner();
        let events = filter_events(read_events().await?, "", &request.query, request.limit);
        let provider = analysis_provider();
        let risk_findings = risk_findings(&events);
        let summary = render_analysis_summary(events.len(), &risk_findings, &provider);

        Ok(GrpcResponse::new(AnalyzeAuditEventsResponse {
            status: Some(ok_response("audit events analyzed")),
            summary,
            risk_findings,
            provider,
        }))
    }
}

pub async fn append_audit_event(
    module: &str,
    action: &str,
    description: impl Into<String>,
    source_ip: impl Into<String>,
) -> Result<(), Status> {
    let event = AuditEvent {
        id: Uuid::new_v4().to_string(),
        user: "system".to_owned(),
        module: module.to_owned(),
        action: action.to_owned(),
        description: description.into(),
        source_ip: source_ip.into(),
        level: infer_level(action),
        timestamp_seconds: current_timestamp(),
    };
    append_event(&event).await
}

async fn append_event(event: &AuditEvent) -> Result<(), Status> {
    tokio::fs::create_dir_all(audit_root())
        .await
        .map_err(io_status)?;
    let stored = StoredAuditEvent::from_proto(event.clone());
    let mut line = serde_json::to_string(&stored).map_err(io_status)?;
    line.push('\n');
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(audit_path())
        .await
        .map_err(io_status)?;
    file.write_all(line.as_bytes()).await.map_err(io_status)
}

async fn read_events() -> Result<Vec<AuditEvent>, Status> {
    let content = match tokio::fs::read_to_string(audit_path()).await {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(io_status(error)),
    };
    let mut events = content
        .lines()
        .filter_map(|line| serde_json::from_str::<StoredAuditEvent>(line).ok())
        .map(StoredAuditEvent::into_proto)
        .collect::<Vec<_>>();
    events.sort_by_key(|event| std::cmp::Reverse(event.timestamp_seconds));
    Ok(events)
}

fn filter_events(
    events: Vec<AuditEvent>,
    module: &str,
    query: &str,
    limit: u32,
) -> Vec<AuditEvent> {
    let module = module.trim();
    let query = query.trim().to_ascii_lowercase();
    let limit = if limit == 0 {
        DEFAULT_AUDIT_LIMIT
    } else {
        limit as usize
    };

    events
        .into_iter()
        .filter(|event| module.is_empty() || event.module == module)
        .filter(|event| {
            query.is_empty()
                || event.action.to_ascii_lowercase().contains(&query)
                || event.description.to_ascii_lowercase().contains(&query)
                || event.source_ip.to_ascii_lowercase().contains(&query)
        })
        .take(limit)
        .collect()
}

fn normalize_event(event: &mut AuditEvent) {
    if event.id.trim().is_empty() {
        event.id = Uuid::new_v4().to_string();
    }
    if event.user.trim().is_empty() {
        event.user = "system".to_owned();
    }
    if event.source_ip.trim().is_empty() {
        event.source_ip = "local".to_owned();
    }
    if event.level.trim().is_empty() {
        event.level = infer_level(&event.action);
    }
    if event.timestamp_seconds == 0 {
        event.timestamp_seconds = current_timestamp();
    }
}

fn infer_level(action: &str) -> String {
    let action = action.to_ascii_lowercase();
    if action.contains("failed") || action.contains("delete") || action.contains("ban") {
        "warning".to_owned()
    } else if action.contains("clear") || action.contains("remove") {
        "danger".to_owned()
    } else {
        "info".to_owned()
    }
}

fn risk_findings(events: &[AuditEvent]) -> Vec<String> {
    let failed_logins = events
        .iter()
        .filter(|event| event.module == "auth" && event.action.contains("failed"))
        .count();
    let destructive = events
        .iter()
        .filter(|event| {
            event.action.contains("delete")
                || event.action.contains("remove")
                || event.action.contains("clear")
        })
        .count();
    let security_changes = events
        .iter()
        .filter(|event| event.module == "security")
        .count();
    let mut findings = Vec::new();

    if failed_logins >= 3 {
        findings.push(format!(
            "{failed_logins} 次登录失败，建议检查来源 IP 并启用 2FA。"
        ));
    }
    if destructive > 0 {
        findings.push(format!(
            "{destructive} 次删除/清理类操作，建议复核操作人和备份状态。"
        ));
    }
    if security_changes > 0 {
        findings.push(format!(
            "{security_changes} 次安全配置变更，建议确认变更窗口和审批记录。"
        ));
    }
    if findings.is_empty() {
        findings.push("未发现明显高风险模式。".to_owned());
    }

    findings
}

fn render_analysis_summary(event_count: usize, findings: &[String], provider: &str) -> String {
    format!(
        "分析事件 {event_count} 条，使用 {provider}。结论：{}",
        findings.first().cloned().unwrap_or_default()
    )
}

fn analysis_provider() -> String {
    env::var("RUSTPANEL_AI_LOG_ANALYSIS_PROVIDER").unwrap_or_else(|_| "local-heuristic".to_owned())
}

fn audit_root() -> PathBuf {
    env::var("RUSTPANEL_AUDIT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_AUDIT_ROOT))
}

fn audit_path() -> PathBuf {
    audit_root().join("audit.jsonl")
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

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredAuditEvent {
    id: String,
    user: String,
    module: String,
    action: String,
    description: String,
    source_ip: String,
    level: String,
    timestamp_seconds: u64,
}

impl StoredAuditEvent {
    fn from_proto(event: AuditEvent) -> Self {
        Self {
            id: event.id,
            user: event.user,
            module: event.module,
            action: event.action,
            description: event.description,
            source_ip: event.source_ip,
            level: event.level,
            timestamp_seconds: event.timestamp_seconds,
        }
    }

    fn into_proto(self) -> AuditEvent {
        AuditEvent {
            id: self.id,
            user: self.user,
            module: self.module,
            action: self.action,
            description: self.description,
            source_ip: self.source_ip,
            level: self.level,
            timestamp_seconds: self.timestamp_seconds,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_events_by_module_and_query() {
        let events = vec![AuditEvent {
            id: "1".to_owned(),
            user: "system".to_owned(),
            module: "auth".to_owned(),
            action: "login_failed".to_owned(),
            description: "invalid password".to_owned(),
            source_ip: "127.0.0.1".to_owned(),
            level: "warning".to_owned(),
            timestamp_seconds: 1,
        }];

        let filtered = filter_events(events, "auth", "invalid", 10);

        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn risk_analysis_flags_repeated_login_failures() {
        let events = (0..3)
            .map(|index| AuditEvent {
                id: index.to_string(),
                user: "system".to_owned(),
                module: "auth".to_owned(),
                action: "login_failed".to_owned(),
                description: "bad credentials".to_owned(),
                source_ip: "127.0.0.1".to_owned(),
                level: "warning".to_owned(),
                timestamp_seconds: index,
            })
            .collect::<Vec<_>>();

        let findings = risk_findings(&events);

        assert!(findings[0].contains("3"));
    }
}
