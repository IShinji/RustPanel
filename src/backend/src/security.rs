use std::{
    env,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tonic::{Request, Response as GrpcResponse, Status};
use uuid::Uuid;

use crate::{
    audit, ok_response,
    proto::rustpanel::v1::{
        security_service_server::SecurityService, DeleteFirewallRuleRequest,
        DeleteFirewallRuleResponse, DeleteWafRuleRequest, DeleteWafRuleResponse,
        ExportFirewallRulesRequest, ExportFirewallRulesResponse, FirewallAction, FirewallBackend,
        FirewallDirection, FirewallProtocol, FirewallRule, GenerateSshKeyRequest,
        GenerateSshKeyResponse, GetSshSettingsRequest, GetSshSettingsResponse,
        GetWafSettingsRequest, GetWafSettingsResponse, ImportFirewallRulesRequest,
        ImportFirewallRulesResponse, ListFirewallRulesRequest, ListFirewallRulesResponse,
        ListSshLoginEventsRequest, ListSshLoginEventsResponse, ListWafAttackEventsRequest,
        ListWafAttackEventsResponse, RecordSshLoginEventRequest, RecordSshLoginEventResponse,
        SecurityOptions, SetFirewallRuleEnabledRequest, SetFirewallRuleEnabledResponse,
        SshKeyAlgorithm, SshKeyItem, SshLoginEvent, SshSettings, UpdateSecurityOptionsRequest,
        UpdateSecurityOptionsResponse, UpdateSshSettingsRequest, UpdateSshSettingsResponse,
        UpdateWafSettingsRequest, UpdateWafSettingsResponse, UpsertFirewallRuleRequest,
        UpsertFirewallRuleResponse, UpsertWafRuleRequest, UpsertWafRuleResponse, WafAttackEvent,
        WafRule, WafRuleKind, WafSettings,
    },
};

const DEFAULT_SECURITY_ROOT: &str = "/tmp/rustpanel/security";
const APPLY_ENV: &str = "RUSTPANEL_SECURITY_APPLY";
const DEFAULT_SCAN_BURST: u32 = 20;
const DEFAULT_SCAN_WINDOW_SECONDS: u32 = 60;
const DEFAULT_PANEL_ACCESS_PATH: &str = "/";
const DEFAULT_WAF_REQUESTS_PER_MINUTE: u32 = 120;
const DEFAULT_WAF_BURST: u32 = 30;
const DEFAULT_WAF_BLOCK_SECONDS: u32 = 600;
const DEFAULT_SSH_PORT: u32 = 22;
const DEFAULT_SSH_FAILED_LIMIT: u32 = 5;
const DEFAULT_SSH_FAILED_WINDOW_SECONDS: u32 = 600;

#[derive(Clone)]
pub struct SecurityServiceImpl {
    store: SecurityStore,
    rollback: Option<crate::rollback::RollbackServiceImpl>,
}

impl SecurityServiceImpl {
    pub fn new() -> Self {
        Self {
            store: SecurityStore::from_env(),
            rollback: None,
        }
    }

    /// Phase F P8-06-3:把 RollbackService 注入,改面板端口 / SSH 端口 /
    /// 防火墙规则前会先 schedule rollback,30 秒内没确认就自动恢复。
    pub fn with_rollback(mut self, rollback: crate::rollback::RollbackServiceImpl) -> Self {
        self.rollback = Some(rollback);
        self
    }

    #[cfg(test)]
    fn with_store(store: SecurityStore) -> Self {
        Self {
            store,
            rollback: None,
        }
    }
}

impl Default for SecurityServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct SecurityConfig {
    store: SecurityStore,
}

impl SecurityConfig {
    pub fn from_env() -> Self {
        Self {
            store: SecurityStore::from_env(),
        }
    }

    pub async fn panel_access_path(&self) -> String {
        self.store
            .load()
            .await
            .map(|state| state.options.panel_access_path)
            .unwrap_or_else(|_| DEFAULT_PANEL_ACCESS_PATH.to_owned())
    }

    pub async fn two_factor_required(&self) -> bool {
        self.store
            .load()
            .await
            .map(|state| state.options.two_factor_required)
            .unwrap_or(false)
    }
}

#[tonic::async_trait]
impl SecurityService for SecurityServiceImpl {
    async fn list_firewall_rules(
        &self,
        _request: Request<ListFirewallRulesRequest>,
    ) -> Result<GrpcResponse<ListFirewallRulesResponse>, Status> {
        let state = self.store.load().await?;

        Ok(GrpcResponse::new(ListFirewallRulesResponse {
            status: Some(ok_response("ok")),
            rules: state
                .rules
                .into_iter()
                .map(StoredFirewallRule::into_proto)
                .collect(),
            options: Some(state.options.into_proto()),
        }))
    }

    async fn upsert_firewall_rule(
        &self,
        request: Request<UpsertFirewallRuleRequest>,
    ) -> Result<GrpcResponse<UpsertFirewallRuleResponse>, Status> {
        let mut rule = request
            .into_inner()
            .rule
            .ok_or_else(|| Status::invalid_argument("firewall rule is required"))?;
        validate_rule(&rule)?;

        let _guard = self.store.write_guard().await;
        let mut state = self.store.load().await?;
        let now = current_timestamp();
        let old_rule = state
            .rules
            .iter()
            .find(|stored| stored.id == rule.id)
            .cloned();
        if rule.id.trim().is_empty() {
            rule.id = Uuid::new_v4().to_string();
            rule.created_at_seconds = now;
        } else if let Some(existing) = &old_rule {
            rule.created_at_seconds = existing.created_at_seconds;
        } else {
            rule.created_at_seconds = now;
        }
        rule.updated_at_seconds = now;

        let stored_rule = StoredFirewallRule::from_proto(rule.clone());
        apply_rule_change(old_rule.as_ref(), Some(&stored_rule), &mut state.options).await?;
        state.rules.retain(|stored| stored.id != rule.id);
        state.rules.push(stored_rule);
        self.store.save(&state).await?;

        Ok(GrpcResponse::new(UpsertFirewallRuleResponse {
            status: Some(ok_response("firewall rule saved")),
            rule: Some(rule),
        }))
    }

    async fn delete_firewall_rule(
        &self,
        request: Request<DeleteFirewallRuleRequest>,
    ) -> Result<GrpcResponse<DeleteFirewallRuleResponse>, Status> {
        let id = request.into_inner().id;
        if id.trim().is_empty() {
            return Err(Status::invalid_argument("firewall rule id is required"));
        }
        let _guard = self.store.write_guard().await;
        let mut state = self.store.load().await?;
        let old_rule = state
            .rules
            .iter()
            .find(|stored| stored.id == id)
            .cloned()
            .ok_or_else(|| Status::not_found("firewall rule not found"))?;

        apply_rule_change(Some(&old_rule), None, &mut state.options).await?;
        state.rules.retain(|stored| stored.id != id);
        self.store.save(&state).await?;

        Ok(GrpcResponse::new(DeleteFirewallRuleResponse {
            status: Some(ok_response("firewall rule deleted")),
        }))
    }

    async fn set_firewall_rule_enabled(
        &self,
        request: Request<SetFirewallRuleEnabledRequest>,
    ) -> Result<GrpcResponse<SetFirewallRuleEnabledResponse>, Status> {
        let request = request.into_inner();
        let _guard = self.store.write_guard().await;
        let mut state = self.store.load().await?;
        let index = state
            .rules
            .iter()
            .position(|stored| stored.id == request.id)
            .ok_or_else(|| Status::not_found("firewall rule not found"))?;
        let old_rule = state.rules[index].clone();
        let mut new_rule = old_rule.clone();
        new_rule.enabled = request.enabled;
        new_rule.updated_at_seconds = current_timestamp();

        apply_rule_change(Some(&old_rule), Some(&new_rule), &mut state.options).await?;
        state.rules[index] = new_rule.clone();
        self.store.save(&state).await?;

        Ok(GrpcResponse::new(SetFirewallRuleEnabledResponse {
            status: Some(ok_response("firewall rule state updated")),
            rule: Some(new_rule.into_proto()),
        }))
    }

    async fn update_security_options(
        &self,
        request: Request<UpdateSecurityOptionsRequest>,
    ) -> Result<GrpcResponse<UpdateSecurityOptionsResponse>, Status> {
        let options = request
            .into_inner()
            .options
            .ok_or_else(|| Status::invalid_argument("security options are required"))?;
        validate_options(&options)?;

        let _guard = self.store.write_guard().await;
        let mut state = self.store.load().await?;
        let old_options = state.options.clone();
        let new_options = StoredSecurityOptions::from_proto(options);

        // P8-06-3:面板访问入口/监听地址/SSH-2FA 这些"动一刀就可能锁外面"的字段
        // 改了 → 走 30 秒回滚护栏。snapshot_json 存旧值,后台 watchdog 任务在
        // 倒计时到期且仍未被 confirm 时把旧值还原。
        let risky_changed = old_options.panel_listen_addr != new_options.panel_listen_addr
            || old_options.panel_access_path != new_options.panel_access_path
            || old_options.two_factor_required != new_options.two_factor_required;

        if risky_changed {
            if let Some(rollback) = self.rollback.as_ref() {
                arm_rollback_watchdog(
                    rollback.clone(),
                    self.store.clone(),
                    &old_options,
                    &new_options,
                )
                .await;
            }
        }

        state.options = new_options;
        apply_options(&mut state.options).await?;
        self.store.save(&state).await?;
        let _ = audit::append_audit_event(
            "security",
            "update_options",
            "updated security options",
            "grpc",
        )
        .await;

        Ok(GrpcResponse::new(UpdateSecurityOptionsResponse {
            status: Some(ok_response("security options updated")),
            options: Some(state.options.into_proto()),
        }))
    }

    async fn export_firewall_rules(
        &self,
        _request: Request<ExportFirewallRulesRequest>,
    ) -> Result<GrpcResponse<ExportFirewallRulesResponse>, Status> {
        let state = self.store.load().await?;
        let backup_json = serde_json::to_string_pretty(&state).map_err(io_status)?;

        Ok(GrpcResponse::new(ExportFirewallRulesResponse {
            status: Some(ok_response("firewall backup exported")),
            backup_json,
        }))
    }

    async fn import_firewall_rules(
        &self,
        request: Request<ImportFirewallRulesRequest>,
    ) -> Result<GrpcResponse<ImportFirewallRulesResponse>, Status> {
        let request = request.into_inner();
        let imported: StoredSecurityState =
            serde_json::from_str(&request.backup_json).map_err(io_status)?;
        let _guard = self.store.write_guard().await;
        let mut current = self.store.load().await?;
        // 旧规则快照:apply 前拿到,apply_imported_state 用它把旧规则从内核
        // 删掉,避免 import 后"删了但内核还留着"。
        let old_rules = current.rules.clone();
        let mut imported = imported.with_defaults();
        for rule in &imported.rules {
            validate_rule(&rule.clone().into_proto())?;
        }
        validate_options(&imported.options.clone().into_proto())?;

        if request.replace_existing {
            current = imported;
        } else {
            current.rules.append(&mut imported.rules);
            current.options = imported.options;
        }
        let mut options = current.options.clone();
        apply_imported_state(&old_rules, &current, &mut options).await?;
        current.options = options;
        self.store.save(&current).await?;

        Ok(GrpcResponse::new(ImportFirewallRulesResponse {
            status: Some(ok_response("firewall backup imported")),
            rules: current
                .rules
                .into_iter()
                .map(StoredFirewallRule::into_proto)
                .collect(),
            options: Some(current.options.into_proto()),
        }))
    }

    async fn get_waf_settings(
        &self,
        _request: Request<GetWafSettingsRequest>,
    ) -> Result<GrpcResponse<GetWafSettingsResponse>, Status> {
        let state = self.store.load().await?;

        Ok(GrpcResponse::new(GetWafSettingsResponse {
            status: Some(ok_response("ok")),
            settings: Some(state.waf_settings.into_proto()),
            rules: state
                .waf_rules
                .into_iter()
                .map(StoredWafRule::into_proto)
                .collect(),
        }))
    }

    async fn update_waf_settings(
        &self,
        request: Request<UpdateWafSettingsRequest>,
    ) -> Result<GrpcResponse<UpdateWafSettingsResponse>, Status> {
        let settings = request
            .into_inner()
            .settings
            .ok_or_else(|| Status::invalid_argument("waf settings are required"))?;
        validate_waf_settings(&settings)?;

        let _guard = self.store.write_guard().await;
        let mut state = self.store.load().await?;
        state.waf_settings = StoredWafSettings::from_proto(settings);
        apply_waf_config(&mut state.waf_settings, &state.waf_rules).await?;
        self.store.save(&state).await?;
        let _ = audit::append_audit_event("security", "update_waf", "updated WAF settings", "grpc")
            .await;

        Ok(GrpcResponse::new(UpdateWafSettingsResponse {
            status: Some(ok_response("waf settings updated")),
            settings: Some(state.waf_settings.into_proto()),
        }))
    }

    async fn upsert_waf_rule(
        &self,
        request: Request<UpsertWafRuleRequest>,
    ) -> Result<GrpcResponse<UpsertWafRuleResponse>, Status> {
        let mut rule = request
            .into_inner()
            .rule
            .ok_or_else(|| Status::invalid_argument("waf rule is required"))?;
        validate_waf_rule(&rule)?;

        let _guard = self.store.write_guard().await;
        let mut state = self.store.load().await?;
        let now = current_timestamp();
        let existing = state
            .waf_rules
            .iter()
            .find(|stored| stored.id == rule.id)
            .cloned();
        if rule.id.trim().is_empty() {
            rule.id = Uuid::new_v4().to_string();
            rule.created_at_seconds = now;
        } else if let Some(existing) = &existing {
            rule.created_at_seconds = existing.created_at_seconds;
        } else {
            rule.created_at_seconds = now;
        }
        rule.updated_at_seconds = now;
        let stored = StoredWafRule::from_proto(rule.clone());
        state.waf_rules.retain(|item| item.id != rule.id);
        state.waf_rules.push(stored);
        let mut waf_settings = state.waf_settings.clone();
        apply_waf_config(&mut waf_settings, &state.waf_rules).await?;
        state.waf_settings = waf_settings;
        self.store.save(&state).await?;

        Ok(GrpcResponse::new(UpsertWafRuleResponse {
            status: Some(ok_response("waf rule saved")),
            rule: Some(rule),
        }))
    }

    async fn delete_waf_rule(
        &self,
        request: Request<DeleteWafRuleRequest>,
    ) -> Result<GrpcResponse<DeleteWafRuleResponse>, Status> {
        let id = request.into_inner().id;
        let _guard = self.store.write_guard().await;
        let mut state = self.store.load().await?;
        let before = state.waf_rules.len();
        state.waf_rules.retain(|rule| rule.id != id);
        if state.waf_rules.len() == before {
            return Err(Status::not_found("waf rule not found"));
        }
        let mut waf_settings = state.waf_settings.clone();
        apply_waf_config(&mut waf_settings, &state.waf_rules).await?;
        state.waf_settings = waf_settings;
        self.store.save(&state).await?;

        Ok(GrpcResponse::new(DeleteWafRuleResponse {
            status: Some(ok_response("waf rule deleted")),
        }))
    }

    async fn list_waf_attack_events(
        &self,
        request: Request<ListWafAttackEventsRequest>,
    ) -> Result<GrpcResponse<ListWafAttackEventsResponse>, Status> {
        let limit = request.into_inner().limit.clamp(1, 500) as usize;
        let mut events = self.store.load().await?.waf_events;
        events.sort_by_key(|event| std::cmp::Reverse(event.occurred_at_seconds));
        events.truncate(limit);

        Ok(GrpcResponse::new(ListWafAttackEventsResponse {
            status: Some(ok_response("ok")),
            events: events
                .into_iter()
                .map(StoredWafAttackEvent::into_proto)
                .collect(),
        }))
    }

    async fn get_ssh_settings(
        &self,
        _request: Request<GetSshSettingsRequest>,
    ) -> Result<GrpcResponse<GetSshSettingsResponse>, Status> {
        let state = self.store.load().await?;

        Ok(GrpcResponse::new(GetSshSettingsResponse {
            status: Some(ok_response("ok")),
            settings: Some(state.ssh_settings.into_proto()),
            keys: state
                .ssh_keys
                .into_iter()
                .map(StoredSshKeyItem::into_proto)
                .collect(),
        }))
    }

    async fn update_ssh_settings(
        &self,
        request: Request<UpdateSshSettingsRequest>,
    ) -> Result<GrpcResponse<UpdateSshSettingsResponse>, Status> {
        let settings = request
            .into_inner()
            .settings
            .ok_or_else(|| Status::invalid_argument("ssh settings are required"))?;
        validate_ssh_settings(&settings)?;

        let _guard = self.store.write_guard().await;
        let mut state = self.store.load().await?;
        state.ssh_settings = StoredSshSettings::from_proto(settings);
        apply_ssh_config(&mut state.ssh_settings).await?;
        self.store.save(&state).await?;
        let _ = audit::append_audit_event("security", "update_ssh", "updated SSH settings", "grpc")
            .await;

        Ok(GrpcResponse::new(UpdateSshSettingsResponse {
            status: Some(ok_response("ssh settings updated")),
            settings: Some(state.ssh_settings.into_proto()),
        }))
    }

    async fn generate_ssh_key(
        &self,
        request: Request<GenerateSshKeyRequest>,
    ) -> Result<GrpcResponse<GenerateSshKeyResponse>, Status> {
        let request = request.into_inner();
        let algorithm = ssh_key_algorithm(request.algorithm)?;
        let key = generate_ssh_key_pair(&request.name, algorithm).await?;
        let _guard = self.store.write_guard().await;
        let mut state = self.store.load().await?;
        state.ssh_keys.retain(|stored| stored.id != key.id);
        state.ssh_keys.push(key.clone());
        self.store.save(&state).await?;

        Ok(GrpcResponse::new(GenerateSshKeyResponse {
            status: Some(ok_response("ssh key generated")),
            key: Some(key.into_proto()),
        }))
    }

    async fn list_ssh_login_events(
        &self,
        request: Request<ListSshLoginEventsRequest>,
    ) -> Result<GrpcResponse<ListSshLoginEventsResponse>, Status> {
        let limit = request.into_inner().limit.clamp(1, 500) as usize;
        let mut events = self.store.load().await?.ssh_events;
        events.sort_by_key(|event| std::cmp::Reverse(event.occurred_at_seconds));
        events.truncate(limit);

        Ok(GrpcResponse::new(ListSshLoginEventsResponse {
            status: Some(ok_response("ok")),
            events: events
                .into_iter()
                .map(StoredSshLoginEvent::into_proto)
                .collect(),
        }))
    }

    async fn record_ssh_login_event(
        &self,
        request: Request<RecordSshLoginEventRequest>,
    ) -> Result<GrpcResponse<RecordSshLoginEventResponse>, Status> {
        let mut event = request
            .into_inner()
            .event
            .ok_or_else(|| Status::invalid_argument("ssh login event is required"))?;
        validate_ssh_login_event(&event)?;

        let _guard = self.store.write_guard().await;
        let mut state = self.store.load().await?;
        if event.id.trim().is_empty() {
            event.id = Uuid::new_v4().to_string();
        }
        if event.occurred_at_seconds == 0 {
            event.occurred_at_seconds = current_timestamp();
        }
        let auto_banned = maybe_auto_ban_ssh_source(&mut state, &event).await?;
        event.auto_banned = auto_banned;
        state
            .ssh_events
            .push(StoredSshLoginEvent::from_proto(event.clone()));
        self.store.save(&state).await?;

        Ok(GrpcResponse::new(RecordSshLoginEventResponse {
            status: Some(ok_response("ssh login event recorded")),
            event: Some(event),
        }))
    }
}

#[derive(Clone, Debug)]
struct SecurityStore {
    root: Arc<PathBuf>,
    // state.json 把 rules/options/waf/ssh 全装在一个文件里,每个 mutator 都是
    // load 整个文件→改→save 整个文件。必须用一把锁串行化所有写者,否则
    // 并发的 WAF 更新会拿旧快照覆盖刚加的防火墙规则(共享文件的丢更新)。
    write_lock: Arc<tokio::sync::Mutex<()>>,
}

impl SecurityStore {
    fn from_env() -> Self {
        let root = env::var("RUSTPANEL_SECURITY_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_SECURITY_ROOT));
        Self::new(root)
    }

    fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Arc::new(root.into()),
            write_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// 取写锁:所有 load→改→save 的 mutator 必须先拿它,串行化对 state.json
    /// 的读改写。读路径(list/get)不需要,原子写保证它们读到完整文件。
    async fn write_guard(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.write_lock.lock().await
    }

    async fn load(&self) -> Result<StoredSecurityState, Status> {
        match tokio::fs::read_to_string(self.state_path()).await {
            Ok(content) => serde_json::from_str::<StoredSecurityState>(&content)
                .map(StoredSecurityState::with_defaults)
                .map_err(io_status),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(StoredSecurityState::default())
            }
            Err(error) => Err(io_status(error)),
        }
    }

    async fn save(&self, state: &StoredSecurityState) -> Result<(), Status> {
        tokio::fs::create_dir_all(self.root.as_ref())
            .await
            .map_err(io_status)?;
        let content = serde_json::to_string_pretty(state).map_err(io_status)?;
        // tmp + rename:崩溃/并发写出半截 JSON 会让 load() 反序列化失败,
        // 之后每个 security RPC 都 500,直到手工修文件。
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
struct StoredSecurityState {
    #[serde(default)]
    rules: Vec<StoredFirewallRule>,
    #[serde(default)]
    options: StoredSecurityOptions,
    #[serde(default)]
    waf_settings: StoredWafSettings,
    #[serde(default)]
    waf_rules: Vec<StoredWafRule>,
    #[serde(default)]
    waf_events: Vec<StoredWafAttackEvent>,
    #[serde(default)]
    ssh_settings: StoredSshSettings,
    #[serde(default)]
    ssh_events: Vec<StoredSshLoginEvent>,
    #[serde(default)]
    ssh_keys: Vec<StoredSshKeyItem>,
}

impl StoredSecurityState {
    fn with_defaults(mut self) -> Self {
        if self.options.scan_burst == 0 {
            self.options.scan_burst = DEFAULT_SCAN_BURST;
        }
        if self.options.scan_window_seconds == 0 {
            self.options.scan_window_seconds = DEFAULT_SCAN_WINDOW_SECONDS;
        }
        self.options.panel_access_path =
            normalize_panel_access_path(&self.options.panel_access_path)
                .unwrap_or_else(|_| DEFAULT_PANEL_ACCESS_PATH.to_owned());
        if self.waf_settings.requests_per_minute == 0 {
            self.waf_settings.requests_per_minute = DEFAULT_WAF_REQUESTS_PER_MINUTE;
        }
        if self.waf_settings.burst == 0 {
            self.waf_settings.burst = DEFAULT_WAF_BURST;
        }
        if self.waf_settings.block_duration_seconds == 0 {
            self.waf_settings.block_duration_seconds = DEFAULT_WAF_BLOCK_SECONDS;
        }
        if self.waf_settings.nginx_config_path.trim().is_empty() {
            self.waf_settings.nginx_config_path = default_waf_config_path();
        }
        if self.waf_settings.challenge_page_path.trim().is_empty() {
            self.waf_settings.challenge_page_path = default_waf_challenge_path();
        }
        if self.waf_rules.is_empty() {
            self.waf_rules = default_waf_rules();
        }
        if self.ssh_settings.port == 0 {
            self.ssh_settings.port = DEFAULT_SSH_PORT;
        }
        if self.ssh_settings.failed_attempt_limit == 0 {
            self.ssh_settings.failed_attempt_limit = DEFAULT_SSH_FAILED_LIMIT;
        }
        if self.ssh_settings.failed_attempt_window_seconds == 0 {
            self.ssh_settings.failed_attempt_window_seconds = DEFAULT_SSH_FAILED_WINDOW_SECONDS;
        }
        if self.ssh_settings.config_path.trim().is_empty() {
            self.ssh_settings.config_path = default_ssh_config_path();
        }
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredFirewallRule {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    protocol: i32,
    #[serde(default)]
    action: i32,
    #[serde(default)]
    direction: i32,
    #[serde(default)]
    port_start: u32,
    #[serde(default)]
    port_end: u32,
    #[serde(default)]
    source: String,
    #[serde(default)]
    destination: String,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    comment: String,
    #[serde(default)]
    created_at_seconds: u64,
    #[serde(default)]
    updated_at_seconds: u64,
}

impl StoredFirewallRule {
    fn from_proto(rule: FirewallRule) -> Self {
        Self {
            id: rule.id,
            name: rule.name,
            protocol: rule.protocol,
            action: rule.action,
            direction: rule.direction,
            port_start: rule.port_start,
            port_end: rule.port_end,
            source: rule.source,
            destination: rule.destination,
            enabled: rule.enabled,
            comment: rule.comment,
            created_at_seconds: rule.created_at_seconds,
            updated_at_seconds: rule.updated_at_seconds,
        }
    }

    fn into_proto(self) -> FirewallRule {
        FirewallRule {
            id: self.id,
            name: self.name,
            protocol: self.protocol,
            action: self.action,
            direction: self.direction,
            port_start: self.port_start,
            port_end: self.port_end,
            source: self.source,
            destination: self.destination,
            enabled: self.enabled,
            comment: self.comment,
            created_at_seconds: self.created_at_seconds,
            updated_at_seconds: self.updated_at_seconds,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredSecurityOptions {
    #[serde(default)]
    disable_ping: bool,
    #[serde(default)]
    scan_protection_enabled: bool,
    #[serde(default)]
    scan_burst: u32,
    #[serde(default)]
    scan_window_seconds: u32,
    #[serde(default)]
    backend_preference: i32,
    #[serde(default)]
    last_apply_message: String,
    #[serde(default)]
    panel_access_path: String,
    #[serde(default)]
    panel_listen_addr: String,
    #[serde(default)]
    two_factor_required: bool,
}

impl Default for StoredSecurityOptions {
    fn default() -> Self {
        Self {
            disable_ping: false,
            scan_protection_enabled: false,
            scan_burst: DEFAULT_SCAN_BURST,
            scan_window_seconds: DEFAULT_SCAN_WINDOW_SECONDS,
            backend_preference: FirewallBackend::Unspecified.into(),
            last_apply_message: "system apply disabled".to_owned(),
            panel_access_path: env::var("RUSTPANEL_PANEL_ACCESS_PATH")
                .ok()
                .and_then(|path| normalize_panel_access_path(&path).ok())
                .unwrap_or_else(|| DEFAULT_PANEL_ACCESS_PATH.to_owned()),
            panel_listen_addr: env::var("RUSTPANEL_BACKEND_ADDR").unwrap_or_default(),
            two_factor_required: totp_secret_configured(),
        }
    }
}

impl StoredSecurityOptions {
    fn from_proto(options: SecurityOptions) -> Self {
        Self {
            disable_ping: options.disable_ping,
            scan_protection_enabled: options.scan_protection_enabled,
            scan_burst: options.scan_burst,
            scan_window_seconds: options.scan_window_seconds,
            backend_preference: options.backend_preference,
            last_apply_message: options.last_apply_message,
            panel_access_path: normalize_panel_access_path(&options.panel_access_path)
                .unwrap_or_else(|_| DEFAULT_PANEL_ACCESS_PATH.to_owned()),
            panel_listen_addr: options.panel_listen_addr,
            two_factor_required: options.two_factor_required,
        }
    }

    fn into_proto(self) -> SecurityOptions {
        SecurityOptions {
            disable_ping: self.disable_ping,
            scan_protection_enabled: self.scan_protection_enabled,
            scan_burst: self.scan_burst,
            scan_window_seconds: self.scan_window_seconds,
            backend_preference: self.backend_preference,
            last_apply_message: self.last_apply_message,
            panel_access_path: self.panel_access_path,
            panel_listen_addr: self.panel_listen_addr,
            two_factor_required: self.two_factor_required,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredWafSettings {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    cc_protection_enabled: bool,
    #[serde(default)]
    captcha_challenge_enabled: bool,
    #[serde(default)]
    requests_per_minute: u32,
    #[serde(default)]
    burst: u32,
    #[serde(default)]
    block_duration_seconds: u32,
    #[serde(default)]
    nginx_config_path: String,
    #[serde(default)]
    challenge_page_path: String,
    #[serde(default)]
    last_apply_message: String,
}

impl Default for StoredWafSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            cc_protection_enabled: true,
            captcha_challenge_enabled: true,
            requests_per_minute: DEFAULT_WAF_REQUESTS_PER_MINUTE,
            burst: DEFAULT_WAF_BURST,
            block_duration_seconds: DEFAULT_WAF_BLOCK_SECONDS,
            nginx_config_path: default_waf_config_path(),
            challenge_page_path: default_waf_challenge_path(),
            last_apply_message: "waf config not written yet".to_owned(),
        }
    }
}

impl StoredWafSettings {
    fn from_proto(settings: WafSettings) -> Self {
        Self {
            enabled: settings.enabled,
            cc_protection_enabled: settings.cc_protection_enabled,
            captcha_challenge_enabled: settings.captcha_challenge_enabled,
            requests_per_minute: settings.requests_per_minute,
            burst: settings.burst,
            block_duration_seconds: settings.block_duration_seconds,
            nginx_config_path: if settings.nginx_config_path.trim().is_empty() {
                default_waf_config_path()
            } else {
                settings.nginx_config_path
            },
            challenge_page_path: if settings.challenge_page_path.trim().is_empty() {
                default_waf_challenge_path()
            } else {
                settings.challenge_page_path
            },
            last_apply_message: settings.last_apply_message,
        }
    }

    fn into_proto(self) -> WafSettings {
        WafSettings {
            enabled: self.enabled,
            cc_protection_enabled: self.cc_protection_enabled,
            captcha_challenge_enabled: self.captcha_challenge_enabled,
            requests_per_minute: self.requests_per_minute,
            burst: self.burst,
            block_duration_seconds: self.block_duration_seconds,
            nginx_config_path: self.nginx_config_path,
            challenge_page_path: self.challenge_page_path,
            last_apply_message: self.last_apply_message,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredWafRule {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    kind: i32,
    #[serde(default)]
    pattern: String,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    scope_domain: String,
    #[serde(default)]
    comment: String,
    #[serde(default)]
    created_at_seconds: u64,
    #[serde(default)]
    updated_at_seconds: u64,
}

impl StoredWafRule {
    fn from_proto(rule: WafRule) -> Self {
        Self {
            id: rule.id,
            name: rule.name,
            kind: rule.kind,
            pattern: rule.pattern,
            enabled: rule.enabled,
            scope_domain: rule.scope_domain,
            comment: rule.comment,
            created_at_seconds: rule.created_at_seconds,
            updated_at_seconds: rule.updated_at_seconds,
        }
    }

    fn into_proto(self) -> WafRule {
        WafRule {
            id: self.id,
            name: self.name,
            kind: self.kind,
            pattern: self.pattern,
            enabled: self.enabled,
            scope_domain: self.scope_domain,
            comment: self.comment,
            created_at_seconds: self.created_at_seconds,
            updated_at_seconds: self.updated_at_seconds,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredWafAttackEvent {
    #[serde(default)]
    id: String,
    #[serde(default)]
    source_ip: String,
    #[serde(default)]
    country_code: String,
    #[serde(default)]
    country_name: String,
    #[serde(default)]
    rule_id: String,
    #[serde(default)]
    rule_name: String,
    #[serde(default)]
    kind: i32,
    #[serde(default)]
    path: String,
    #[serde(default)]
    user_agent: String,
    #[serde(default)]
    occurred_at_seconds: u64,
}

impl StoredWafAttackEvent {
    fn into_proto(self) -> WafAttackEvent {
        WafAttackEvent {
            id: self.id,
            source_ip: self.source_ip,
            country_code: self.country_code,
            country_name: self.country_name,
            rule_id: self.rule_id,
            rule_name: self.rule_name,
            kind: self.kind,
            path: self.path,
            user_agent: self.user_agent,
            occurred_at_seconds: self.occurred_at_seconds,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredSshSettings {
    #[serde(default)]
    service_enabled: bool,
    #[serde(default)]
    port: u32,
    #[serde(default)]
    password_login_disabled: bool,
    #[serde(default)]
    auto_ban_enabled: bool,
    #[serde(default)]
    failed_attempt_limit: u32,
    #[serde(default)]
    failed_attempt_window_seconds: u32,
    #[serde(default)]
    config_path: String,
    #[serde(default)]
    last_apply_message: String,
}

impl Default for StoredSshSettings {
    fn default() -> Self {
        Self {
            service_enabled: true,
            port: DEFAULT_SSH_PORT,
            password_login_disabled: false,
            auto_ban_enabled: true,
            failed_attempt_limit: DEFAULT_SSH_FAILED_LIMIT,
            failed_attempt_window_seconds: DEFAULT_SSH_FAILED_WINDOW_SECONDS,
            config_path: default_ssh_config_path(),
            last_apply_message: "ssh config not written yet".to_owned(),
        }
    }
}

impl StoredSshSettings {
    fn from_proto(settings: SshSettings) -> Self {
        Self {
            service_enabled: settings.service_enabled,
            port: settings.port,
            password_login_disabled: settings.password_login_disabled,
            auto_ban_enabled: settings.auto_ban_enabled,
            failed_attempt_limit: settings.failed_attempt_limit,
            failed_attempt_window_seconds: settings.failed_attempt_window_seconds,
            config_path: if settings.config_path.trim().is_empty() {
                default_ssh_config_path()
            } else {
                settings.config_path
            },
            last_apply_message: settings.last_apply_message,
        }
    }

    fn into_proto(self) -> SshSettings {
        SshSettings {
            service_enabled: self.service_enabled,
            port: self.port,
            password_login_disabled: self.password_login_disabled,
            auto_ban_enabled: self.auto_ban_enabled,
            failed_attempt_limit: self.failed_attempt_limit,
            failed_attempt_window_seconds: self.failed_attempt_window_seconds,
            config_path: self.config_path,
            last_apply_message: self.last_apply_message,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredSshLoginEvent {
    #[serde(default)]
    id: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    source_ip: String,
    #[serde(default)]
    successful: bool,
    #[serde(default)]
    auto_banned: bool,
    #[serde(default)]
    message: String,
    #[serde(default)]
    occurred_at_seconds: u64,
}

impl StoredSshLoginEvent {
    fn from_proto(event: SshLoginEvent) -> Self {
        Self {
            id: event.id,
            username: event.username,
            source_ip: event.source_ip,
            successful: event.successful,
            auto_banned: event.auto_banned,
            message: event.message,
            occurred_at_seconds: event.occurred_at_seconds,
        }
    }

    fn into_proto(self) -> SshLoginEvent {
        SshLoginEvent {
            id: self.id,
            username: self.username,
            source_ip: self.source_ip,
            successful: self.successful,
            auto_banned: self.auto_banned,
            message: self.message,
            occurred_at_seconds: self.occurred_at_seconds,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredSshKeyItem {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    algorithm: i32,
    #[serde(default)]
    public_key: String,
    #[serde(default)]
    private_key_path: String,
    #[serde(default)]
    created_at_seconds: u64,
}

impl StoredSshKeyItem {
    fn into_proto(self) -> SshKeyItem {
        SshKeyItem {
            id: self.id,
            name: self.name,
            algorithm: self.algorithm,
            public_key: self.public_key,
            private_key_path: self.private_key_path,
            created_at_seconds: self.created_at_seconds,
        }
    }
}

async fn apply_rule_change(
    old_rule: Option<&StoredFirewallRule>,
    new_rule: Option<&StoredFirewallRule>,
    options: &mut StoredSecurityOptions,
) -> Result<(), Status> {
    if !should_apply_system_firewall() {
        options.last_apply_message = "saved; system firewall apply disabled".to_owned();
        return Ok(());
    }
    let Some(backend) = detect_backend(backend_preference(options)).await else {
        options.last_apply_message = "saved; no supported firewall backend found".to_owned();
        return Ok(());
    };

    let mut messages = Vec::new();
    let mut deleted_old = false;
    if old_rule.is_some_and(|rule| rule.enabled) {
        let rule = old_rule.expect("checked old rule");
        let commands = build_rule_commands(backend, rule, FirewallOperation::Delete)?;
        run_commands(commands).await?;
        deleted_old = true;
        messages.push(format!("removed old rule via {}", backend_name(backend)));
    }
    if new_rule.is_some_and(|rule| rule.enabled) {
        let rule = new_rule.expect("checked new rule");
        let commands = build_rule_commands(backend, rule, FirewallOperation::Add)?;
        if let Err(error) = run_commands(commands).await {
            // 加新规则失败:把刚删的旧规则尽力加回去。否则"旧规则没了、新规则
            // 也没生效",且 save 不会执行 → 面板仍显示旧规则在,内核却已无,
            // 运维以为某条 deny 还在保护实际已失效。
            if deleted_old {
                if let Some(old) = old_rule {
                    if let Ok(restore) = build_rule_commands(backend, old, FirewallOperation::Add) {
                        run_commands_best_effort(restore).await;
                    }
                }
            }
            return Err(error);
        }
        messages.push(format!("applied rule via {}", backend_name(backend)));
    }
    if messages.is_empty() {
        messages.push("rule stored without active firewall change".to_owned());
    }
    options.last_apply_message = messages.join("; ");
    Ok(())
}

// P8-06-3 watchdog:改完高风险字段后,在 RollbackService 排一个 30s 计时;
// 同时本进程内开一个 tokio 任务,过了倒计时如果发现 action 还在 pending list
// (说明用户没点"保留"),就把保存的旧 options 写回去 + 重新 apply。
async fn arm_rollback_watchdog(
    rollback: crate::rollback::RollbackServiceImpl,
    store: SecurityStore,
    old_options: &StoredSecurityOptions,
    new_options: &StoredSecurityOptions,
) {
    let snapshot = match serde_json::to_string(old_options) {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(?error, "rollback watchdog: serialize old options failed");
            return;
        }
    };
    let title = format!(
        "面板入口安全选项变更:listen={} → {} / path={} → {} / 2FA={} → {}",
        old_options.panel_listen_addr,
        new_options.panel_listen_addr,
        old_options.panel_access_path,
        new_options.panel_access_path,
        old_options.two_factor_required,
        new_options.two_factor_required,
    );
    let request = crate::proto::rustpanel::v1::ScheduleRollbackRequest {
        title,
        description: "30 秒内未点'保留(我能登录)'将自动回滚到旧设置,避免被锁外面。".to_owned(),
        revert_command: String::new(),
        snapshot_json: snapshot.clone(),
        rollback_after_seconds: 30,
    };
    use crate::proto::rustpanel::v1::rollback_service_server::RollbackService;
    let response = match rollback
        .schedule_rollback(tonic::Request::new(request))
        .await
    {
        Ok(resp) => resp.into_inner(),
        Err(error) => {
            tracing::warn!(?error, "rollback watchdog: schedule_rollback failed");
            return;
        }
    };
    let action_id = match response.action {
        Some(a) => a.action_id,
        None => return,
    };

    tokio::spawn(async move {
        use crate::proto::rustpanel::v1::rollback_service_server::RollbackService;
        // 倒计时 30s + 2s 缓冲
        tokio::time::sleep(std::time::Duration::from_secs(32)).await;
        let still_pending = match rollback
            .list_pending_rollbacks(tonic::Request::new(
                crate::proto::rustpanel::v1::ListPendingRollbacksRequest {},
            ))
            .await
        {
            Ok(resp) => resp
                .into_inner()
                .actions
                .iter()
                .any(|a| a.action_id == action_id),
            Err(_) => false,
        };
        if !still_pending {
            // 用户已 confirm,什么都不做
            return;
        }
        // 用户没保留 → 还原
        let restored: StoredSecurityOptions = match serde_json::from_str(&snapshot) {
            Ok(value) => value,
            Err(error) => {
                tracing::error!(?error, "rollback watchdog: deserialize snapshot failed");
                return;
            }
        };
        let _guard = store.write_guard().await;
        let mut state = match store.load().await {
            Ok(value) => value,
            Err(error) => {
                tracing::error!(?error, "rollback watchdog: load state failed");
                return;
            }
        };
        state.options = restored;
        if let Err(error) = apply_options(&mut state.options).await {
            tracing::error!(?error, "rollback watchdog: apply restored options failed");
        }
        if let Err(error) = store.save(&state).await {
            tracing::error!(?error, "rollback watchdog: save restored state failed");
        }
        tracing::warn!(action_id = %action_id, "auto-rollback restored security options");
    });
}

async fn apply_options(options: &mut StoredSecurityOptions) -> Result<(), Status> {
    if !should_apply_system_firewall() {
        options.last_apply_message = "saved; system firewall apply disabled".to_owned();
        return Ok(());
    }
    let mut commands = Vec::new();
    commands.push(FirewallCommand::new(
        "sysctl",
        vec![
            "-w".to_owned(),
            format!(
                "net.ipv4.icmp_echo_ignore_all={}",
                if options.disable_ping { 1 } else { 0 }
            ),
        ],
    ));
    if options.scan_protection_enabled {
        commands.extend(scan_protection_commands(
            options.scan_burst,
            options.scan_window_seconds,
        ));
    }
    run_commands(commands).await?;
    options.last_apply_message = "security options applied".to_owned();
    Ok(())
}

async fn apply_waf_config(
    settings: &mut StoredWafSettings,
    rules: &[StoredWafRule],
) -> Result<(), Status> {
    validate_waf_settings(&settings.clone().into_proto())?;
    for rule in rules {
        validate_waf_rule(&rule.clone().into_proto())?;
    }

    let config_path = PathBuf::from(&settings.nginx_config_path);
    if let Some(parent) = config_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(io_status)?;
    }
    let challenge_path = PathBuf::from(&settings.challenge_page_path);
    if let Some(parent) = challenge_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(io_status)?;
    }

    tokio::fs::write(&challenge_path, waf_challenge_page())
        .await
        .map_err(io_status)?;
    tokio::fs::write(&config_path, render_waf_nginx_config(settings, rules)?)
        .await
        .map_err(io_status)?;

    if settings.enabled && should_apply_system_firewall() {
        let output = Command::new("nginx")
            .arg("-t")
            .output()
            .await
            .map_err(io_status)?;
        if !output.status.success() {
            return Err(Status::internal(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }
        let output = Command::new("nginx")
            .arg("-s")
            .arg("reload")
            .output()
            .await
            .map_err(io_status)?;
        if !output.status.success() {
            return Err(Status::internal(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }
        settings.last_apply_message = "waf config written and nginx reloaded".to_owned();
    } else {
        settings.last_apply_message =
            format!("waf config written to {}", settings.nginx_config_path);
    }

    Ok(())
}

async fn apply_ssh_config(settings: &mut StoredSshSettings) -> Result<(), Status> {
    validate_ssh_settings(&settings.clone().into_proto())?;
    // 防锁死:要禁用密码登录、且会真的 reload sshd 时,先确认系统里至少有
    // 一份非空 authorized_keys。否则密码关了又没 key = 下次必定登不进去。
    // sshd -t 只验语法,验不出"当前管理员还能不能登",所以单靠它不够。
    if settings.password_login_disabled
        && should_apply_system_firewall()
        && !system_has_authorized_key().await
    {
        return Err(Status::failed_precondition(
            "禁用密码登录前,请先为某个账户安装 SSH 公钥(authorized_keys);\
             否则关闭密码登录后将无法再登录。已阻止本次变更以防被锁在门外。",
        ));
    }
    let config_path = PathBuf::from(&settings.config_path);
    if let Some(parent) = config_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(io_status)?;
    }
    tokio::fs::write(&config_path, render_ssh_config(settings))
        .await
        .map_err(io_status)?;

    if should_apply_system_firewall() {
        let output = Command::new("sshd")
            .arg("-t")
            .output()
            .await
            .map_err(io_status)?;
        if !output.status.success() {
            return Err(Status::internal(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }
        let reload = Command::new("sh")
            .arg("-c")
            .arg("systemctl reload sshd || systemctl reload ssh || service sshd reload || service ssh reload")
            .output()
            .await
            .map_err(io_status)?;
        if !reload.status.success() {
            return Err(Status::internal(
                String::from_utf8_lossy(&reload.stderr).to_string(),
            ));
        }
        settings.last_apply_message = "ssh config written and service reloaded".to_owned();
    } else {
        settings.last_apply_message = format!("ssh config written to {}", settings.config_path);
    }

    Ok(())
}

/// 系统里是否存在至少一份非空 authorized_keys。禁用密码登录前的防锁死
/// 检查:只要 root 或任意 /home 用户装了公钥就放行,一个都没有才拒绝。
/// 保守取向 —— 宁可偶尔误挡,也不要把运维锁在门外。
async fn system_has_authorized_key() -> bool {
    let mut candidates = vec![PathBuf::from("/root/.ssh/authorized_keys")];
    if let Ok(mut entries) = tokio::fs::read_dir("/home").await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            candidates.push(entry.path().join(".ssh/authorized_keys"));
        }
    }
    for path in candidates {
        if let Ok(content) = tokio::fs::read_to_string(&path).await {
            if content
                .lines()
                .any(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
            {
                return true;
            }
        }
    }
    false
}

async fn maybe_auto_ban_ssh_source(
    state: &mut StoredSecurityState,
    event: &SshLoginEvent,
) -> Result<bool, Status> {
    if event.successful || !state.ssh_settings.auto_ban_enabled || event.source_ip.trim().is_empty()
    {
        return Ok(false);
    }
    let window_start = event
        .occurred_at_seconds
        .saturating_sub(u64::from(state.ssh_settings.failed_attempt_window_seconds));
    let failures = state
        .ssh_events
        .iter()
        .filter(|stored| {
            !stored.successful
                && stored.source_ip == event.source_ip
                && stored.occurred_at_seconds >= window_start
        })
        .count()
        + 1;
    if failures < state.ssh_settings.failed_attempt_limit as usize {
        return Ok(false);
    }

    let source = event.source_ip.trim().to_owned();
    if state.rules.iter().any(|rule| {
        rule.name.starts_with("SSH 自动封禁")
            && rule.source == source
            && rule.port_start == state.ssh_settings.port
    }) {
        return Ok(true);
    }

    let now = current_timestamp();
    let rule = StoredFirewallRule {
        id: Uuid::new_v4().to_string(),
        name: format!("SSH 自动封禁 {source}"),
        protocol: FirewallProtocol::Tcp.into(),
        action: FirewallAction::Deny.into(),
        direction: FirewallDirection::Inbound.into(),
        port_start: state.ssh_settings.port,
        port_end: state.ssh_settings.port,
        source,
        destination: String::new(),
        enabled: true,
        comment: format!("{} 次失败登录后自动封禁", failures),
        created_at_seconds: now,
        updated_at_seconds: now,
    };
    let mut options = state.options.clone();
    apply_rule_change(None, Some(&rule), &mut options).await?;
    state.options = options;
    state.rules.push(rule);
    Ok(true)
}

async fn generate_ssh_key_pair(
    name: &str,
    algorithm: SshKeyAlgorithm,
) -> Result<StoredSshKeyItem, Status> {
    let safe_name = safe_key_name(name)?;
    let key_id = Uuid::new_v4().to_string();
    let root = PathBuf::from(default_ssh_key_root());
    tokio::fs::create_dir_all(&root).await.map_err(io_status)?;
    let private_key_path = root.join(format!("{safe_name}-{key_id}"));
    let key_type = match algorithm {
        SshKeyAlgorithm::Rsa => "rsa",
        SshKeyAlgorithm::Ed25519 => "ed25519",
        SshKeyAlgorithm::Unspecified => {
            return Err(Status::invalid_argument("ssh key algorithm is required"));
        }
    };
    let mut command = Command::new("ssh-keygen");
    command
        .arg("-t")
        .arg(key_type)
        .arg("-N")
        .arg("")
        .arg("-C")
        .arg(format!("rustpanel-{safe_name}"))
        .arg("-f")
        .arg(&private_key_path);
    if algorithm == SshKeyAlgorithm::Rsa {
        command.arg("-b").arg("4096");
    }
    let output = command.output().await.map_err(io_status)?;
    if !output.status.success() {
        return Err(Status::internal(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }
    let public_key = tokio::fs::read_to_string(private_key_path.with_extension("pub"))
        .await
        .map_err(io_status)?;

    Ok(StoredSshKeyItem {
        id: key_id,
        name: safe_name,
        algorithm: algorithm.into(),
        public_key,
        private_key_path: private_key_path.to_string_lossy().to_string(),
        created_at_seconds: current_timestamp(),
    })
}

async fn apply_imported_state(
    old_rules: &[StoredFirewallRule],
    state: &StoredSecurityState,
    options: &mut StoredSecurityOptions,
) -> Result<(), Status> {
    if !should_apply_system_firewall() {
        options.last_apply_message = "imported; system firewall apply disabled".to_owned();
        return Ok(());
    }
    let Some(backend) = detect_backend(backend_preference(options)).await else {
        options.last_apply_message = "imported; no supported firewall backend found".to_owned();
        return Ok(());
    };

    // 先尽力删掉旧的已应用规则,否则 import(尤其 replace 模式)后旧内核规则
    // 仍生效,UI 显示已删/已改、实际防火墙没变。删不存在的规则会非零退出,
    // 故走 best-effort 忽略失败。
    let mut deletes = Vec::new();
    for rule in old_rules.iter().filter(|rule| rule.enabled) {
        deletes.extend(build_rule_commands(
            backend,
            rule,
            FirewallOperation::Delete,
        )?);
    }
    run_commands_best_effort(deletes).await;

    let mut commands = Vec::new();
    for rule in state.rules.iter().filter(|rule| rule.enabled) {
        commands.extend(build_rule_commands(backend, rule, FirewallOperation::Add)?);
    }
    if options.disable_ping || options.scan_protection_enabled {
        commands.push(FirewallCommand::new(
            "sysctl",
            vec![
                "-w".to_owned(),
                format!(
                    "net.ipv4.icmp_echo_ignore_all={}",
                    if options.disable_ping { 1 } else { 0 }
                ),
            ],
        ));
        if options.scan_protection_enabled {
            commands.extend(scan_protection_commands(
                options.scan_burst,
                options.scan_window_seconds,
            ));
        }
    }
    run_commands(commands).await?;
    options.last_apply_message = format!("imported rules applied via {}", backend_name(backend));
    Ok(())
}

fn render_waf_nginx_config(
    settings: &StoredWafSettings,
    rules: &[StoredWafRule],
) -> Result<String, Status> {
    let mut config = String::new();
    config.push_str("# Generated by RustPanel. Include the zone line in http{} and the remaining lines in server{}.\n");
    if settings.enabled && settings.cc_protection_enabled {
        config.push_str(&format!(
            "limit_req_zone $binary_remote_addr zone=rustpanel_cc:10m rate={}r/m;\n\n",
            settings.requests_per_minute.max(1)
        ));
        config.push_str(&format!(
            "limit_req zone=rustpanel_cc burst={} nodelay;\n",
            settings.burst.max(1)
        ));
        if settings.captcha_challenge_enabled {
            config.push_str("error_page 429 = /rustpanel-waf-challenge.html;\n");
            config.push_str("location = /rustpanel-waf-challenge.html {\n");
            config.push_str("    default_type text/html;\n");
            config.push_str(&format!(
                "    alias {};\n",
                settings.challenge_page_path.replace('\\', "\\\\")
            ));
            config.push_str("}\n");
        }
    }

    if settings.enabled {
        for rule in rules.iter().filter(|rule| rule.enabled) {
            let kind = waf_rule_kind(rule.kind)?;
            let target = if kind == WafRuleKind::Cc {
                "429"
            } else {
                "403"
            };
            config.push_str(&format!(
                "if ($request_uri ~* \"{}\") {{ return {}; }} # rustpanel:{}:{}\n",
                escape_nginx_regex(&rule.pattern),
                target,
                waf_kind_name(kind),
                rule.id
            ));
            if matches!(kind, WafRuleKind::SqlInjection | WafRuleKind::Xss) {
                config.push_str(&format!(
                    "if ($query_string ~* \"{}\") {{ return {}; }} # rustpanel:{}:{}\n",
                    escape_nginx_regex(&rule.pattern),
                    target,
                    waf_kind_name(kind),
                    rule.id
                ));
            }
        }
    }

    Ok(config)
}

fn waf_challenge_page() -> &'static str {
    r#"<!doctype html>
<html lang="zh-CN">
<head><meta charset="utf-8"><title>RustPanel WAF</title></head>
<body>
<main style="font-family:system-ui;padding:32px;max-width:520px;margin:auto">
<h1>访问验证</h1>
<p>请求频率过高，请完成浏览器验证后继续。</p>
<button onclick="document.cookie='rustpanel_waf_pass=1;path=/;max-age=600';location.reload()">继续访问</button>
</main>
</body>
</html>
"#
}

fn render_ssh_config(settings: &StoredSshSettings) -> String {
    let password_auth = if settings.password_login_disabled {
        "no"
    } else {
        "yes"
    };
    let service_note = if settings.service_enabled {
        "service enabled"
    } else {
        "service disabled from panel state"
    };
    format!(
        "# Generated by RustPanel ({service_note})\nPort {}\nPasswordAuthentication {password_auth}\nPubkeyAuthentication yes\nPermitRootLogin prohibit-password\n",
        settings.port
    )
}

fn default_waf_rules() -> Vec<StoredWafRule> {
    let now = current_timestamp();
    [
        (
            "preset-sqli",
            "SQL 注入过滤",
            WafRuleKind::SqlInjection,
            "(union(.*)select|select(.*)from|information_schema|sleep\\(|benchmark\\()",
            "拦截常见 SQL 注入探测",
        ),
        (
            "preset-xss",
            "XSS 过滤",
            WafRuleKind::Xss,
            "(<script|javascript:|onerror=|onload=|document\\.cookie)",
            "拦截常见脚本注入载荷",
        ),
        (
            "preset-scanner",
            "扫描器拦截",
            WafRuleKind::Scanner,
            "(\\.env|wp-config\\.php|/phpmyadmin|/\\.git/|/vendor/phpunit)",
            "拦截恶意扫描路径",
        ),
    ]
    .into_iter()
    .map(|(id, name, kind, pattern, comment)| StoredWafRule {
        id: id.to_owned(),
        name: name.to_owned(),
        kind: kind.into(),
        pattern: pattern.to_owned(),
        enabled: true,
        scope_domain: String::new(),
        comment: comment.to_owned(),
        created_at_seconds: now,
        updated_at_seconds: now,
    })
    .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FirewallOperation {
    Add,
    Delete,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FirewallCommand {
    program: String,
    args: Vec<String>,
}

impl FirewallCommand {
    fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
        }
    }
}

async fn run_commands(commands: Vec<FirewallCommand>) -> Result<(), Status> {
    for command in commands {
        let output = Command::new(&command.program)
            .args(&command.args)
            .output()
            .await
            .map_err(io_status)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(Status::internal(if stderr.is_empty() {
                format!("{} exited with status {}", command.program, output.status)
            } else {
                stderr
            }));
        }
    }
    Ok(())
}

/// 像 run_commands,但忽略单条命令的非零退出 / spawn 失败(只 debug log)。
/// 用于"先删旧规则"这类本就可能不存在的场景:iptables -D / firewalld
/// --remove-rich-rule 删不存在的规则会非零退出,不该让整个操作失败。
async fn run_commands_best_effort(commands: Vec<FirewallCommand>) {
    for command in commands {
        match Command::new(&command.program)
            .args(&command.args)
            .output()
            .await
        {
            Ok(output) if output.status.success() => {}
            Ok(output) => tracing::debug!(
                program = %command.program,
                stderr = %String::from_utf8_lossy(&output.stderr).trim(),
                "best-effort firewall command exited non-zero (ignored)"
            ),
            Err(error) => tracing::debug!(
                program = %command.program,
                %error,
                "best-effort firewall command spawn failed (ignored)"
            ),
        }
    }
}

async fn detect_backend(preference: FirewallBackend) -> Option<FirewallBackend> {
    if preference != FirewallBackend::Unspecified {
        return backend_available(preference).await.then_some(preference);
    }

    for backend in [
        FirewallBackend::Ufw,
        FirewallBackend::Firewalld,
        FirewallBackend::Iptables,
    ] {
        if backend_available(backend).await {
            return Some(backend);
        }
    }
    None
}

async fn backend_available(backend: FirewallBackend) -> bool {
    let command = match backend {
        FirewallBackend::Ufw => "ufw",
        FirewallBackend::Firewalld => "firewall-cmd",
        FirewallBackend::Iptables => "iptables",
        FirewallBackend::Unspecified => return false,
    };
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {command}"))
        .output()
        .await
        .is_ok_and(|output| output.status.success())
}

fn build_rule_commands(
    backend: FirewallBackend,
    rule: &StoredFirewallRule,
    operation: FirewallOperation,
) -> Result<Vec<FirewallCommand>, Status> {
    match backend {
        FirewallBackend::Ufw => build_ufw_rule_commands(rule, operation),
        FirewallBackend::Firewalld => build_firewalld_rule_commands(rule, operation),
        FirewallBackend::Iptables => build_iptables_rule_commands(rule, operation),
        FirewallBackend::Unspecified => {
            Err(Status::failed_precondition("firewall backend is required"))
        }
    }
}

fn build_ufw_rule_commands(
    rule: &StoredFirewallRule,
    operation: FirewallOperation,
) -> Result<Vec<FirewallCommand>, Status> {
    let protocol = firewall_protocol(rule.protocol)?;
    if protocol == FirewallProtocol::Icmp {
        return Err(Status::unimplemented(
            "per-rule ICMP management requires firewalld or iptables",
        ));
    }
    let action = match firewall_action(rule.action)? {
        FirewallAction::Allow => "allow",
        FirewallAction::Deny => "deny",
        FirewallAction::Reject => "reject",
        FirewallAction::Unspecified => return Err(Status::invalid_argument("action is required")),
    };
    let mut args = Vec::new();
    if operation == FirewallOperation::Delete {
        args.push("delete".to_owned());
    }
    args.push(action.to_owned());
    if firewall_direction(rule.direction)? == FirewallDirection::Outbound {
        args.push("out".to_owned());
    }
    args.extend([
        "proto".to_owned(),
        protocol_name(protocol).to_owned(),
        "from".to_owned(),
        firewall_endpoint(&rule.source).to_owned(),
        "to".to_owned(),
        firewall_endpoint(&rule.destination).to_owned(),
        "port".to_owned(),
        port_range(rule),
    ]);
    if !rule.comment.trim().is_empty() && operation == FirewallOperation::Add {
        args.extend(["comment".to_owned(), rule.comment.trim().to_owned()]);
    }

    Ok(vec![FirewallCommand::new("ufw", args)])
}

fn build_firewalld_rule_commands(
    rule: &StoredFirewallRule,
    operation: FirewallOperation,
) -> Result<Vec<FirewallCommand>, Status> {
    let rich_rule = firewalld_rich_rule(rule)?;
    let switch = match operation {
        FirewallOperation::Add => "--add-rich-rule",
        FirewallOperation::Delete => "--remove-rich-rule",
    };

    Ok(vec![
        FirewallCommand::new(
            "firewall-cmd",
            vec!["--permanent".to_owned(), format!("{switch}={rich_rule}")],
        ),
        FirewallCommand::new("firewall-cmd", vec!["--reload".to_owned()]),
    ])
}

fn build_iptables_rule_commands(
    rule: &StoredFirewallRule,
    operation: FirewallOperation,
) -> Result<Vec<FirewallCommand>, Status> {
    let protocol = firewall_protocol(rule.protocol)?;
    let chain = match firewall_direction(rule.direction)? {
        FirewallDirection::Inbound => "INPUT",
        FirewallDirection::Outbound => "OUTPUT",
        FirewallDirection::Unspecified => {
            return Err(Status::invalid_argument("direction is required"));
        }
    };
    let target = match firewall_action(rule.action)? {
        FirewallAction::Allow => "ACCEPT",
        FirewallAction::Deny => "DROP",
        FirewallAction::Reject => "REJECT",
        FirewallAction::Unspecified => return Err(Status::invalid_argument("action is required")),
    };
    let mut args = vec![
        match operation {
            FirewallOperation::Add => "-A",
            FirewallOperation::Delete => "-D",
        }
        .to_owned(),
        chain.to_owned(),
        "-p".to_owned(),
        protocol_name(protocol).to_owned(),
    ];
    if !rule.source.trim().is_empty() {
        args.extend(["-s".to_owned(), rule.source.trim().to_owned()]);
    }
    if !rule.destination.trim().is_empty() {
        args.extend(["-d".to_owned(), rule.destination.trim().to_owned()]);
    }
    if matches!(protocol, FirewallProtocol::Tcp | FirewallProtocol::Udp) {
        args.extend(["--dport".to_owned(), port_range(rule)]);
    }
    args.extend([
        "-m".to_owned(),
        "comment".to_owned(),
        "--comment".to_owned(),
    ]);
    args.push(format!("rustpanel:{}", rule.id));
    args.extend(["-j".to_owned(), target.to_owned()]);

    Ok(vec![FirewallCommand::new(iptables_program(rule), args)])
}

fn firewalld_rich_rule(rule: &StoredFirewallRule) -> Result<String, Status> {
    let protocol = firewall_protocol(rule.protocol)?;
    let action = match firewall_action(rule.action)? {
        FirewallAction::Allow => "accept",
        FirewallAction::Deny => "drop",
        FirewallAction::Reject => "reject",
        FirewallAction::Unspecified => return Err(Status::invalid_argument("action is required")),
    };
    let mut parts = vec![format!("rule family=\"{}\"", ip_family(rule))];
    if !rule.source.trim().is_empty() {
        parts.push(format!("source address=\"{}\"", rule.source.trim()));
    }
    if !rule.destination.trim().is_empty() {
        parts.push(format!(
            "destination address=\"{}\"",
            rule.destination.trim()
        ));
    }
    match protocol {
        FirewallProtocol::Tcp | FirewallProtocol::Udp => {
            parts.push(format!(
                "port port=\"{}\" protocol=\"{}\"",
                firewalld_port(rule),
                protocol_name(protocol)
            ));
        }
        FirewallProtocol::Icmp => parts.push("protocol value=\"icmp\"".to_owned()),
        FirewallProtocol::Unspecified => {
            return Err(Status::invalid_argument("protocol is required"));
        }
    }
    parts.push(action.to_owned());

    Ok(parts.join(" "))
}

fn scan_protection_commands(scan_burst: u32, scan_window_seconds: u32) -> Vec<FirewallCommand> {
    vec![
        FirewallCommand::new(
            "iptables",
            vec![
                "-A".to_owned(),
                "INPUT".to_owned(),
                "-p".to_owned(),
                "tcp".to_owned(),
                "--syn".to_owned(),
                "-m".to_owned(),
                "recent".to_owned(),
                "--name".to_owned(),
                "rustpanel_scan".to_owned(),
                "--set".to_owned(),
            ],
        ),
        FirewallCommand::new(
            "iptables",
            vec![
                "-A".to_owned(),
                "INPUT".to_owned(),
                "-p".to_owned(),
                "tcp".to_owned(),
                "--syn".to_owned(),
                "-m".to_owned(),
                "recent".to_owned(),
                "--name".to_owned(),
                "rustpanel_scan".to_owned(),
                "--update".to_owned(),
                "--seconds".to_owned(),
                scan_window_seconds.max(1).to_string(),
                "--hitcount".to_owned(),
                scan_burst.max(2).to_string(),
                "-j".to_owned(),
                "DROP".to_owned(),
            ],
        ),
    ]
}

fn validate_rule(rule: &FirewallRule) -> Result<(), Status> {
    if rule.name.trim().is_empty() {
        return Err(Status::invalid_argument("firewall rule name is required"));
    }
    let protocol = firewall_protocol(rule.protocol)?;
    let _ = firewall_action(rule.action)?;
    let _ = firewall_direction(rule.direction)?;
    if matches!(protocol, FirewallProtocol::Tcp | FirewallProtocol::Udp) {
        if rule.port_start == 0 || rule.port_start > 65_535 {
            return Err(Status::invalid_argument("port_start must be 1-65535"));
        }
        if rule.port_end != 0 && (rule.port_end < rule.port_start || rule.port_end > 65_535) {
            return Err(Status::invalid_argument(
                "port_end must be empty or greater than port_start",
            ));
        }
    }
    validate_address_filter(&rule.source, "source")?;
    validate_address_filter(&rule.destination, "destination")?;
    Ok(())
}

fn validate_options(options: &SecurityOptions) -> Result<(), Status> {
    let _ = firewall_backend(options.backend_preference)?;
    let _ = normalize_panel_access_path(&options.panel_access_path)?;
    if !options.panel_listen_addr.trim().is_empty() {
        options
            .panel_listen_addr
            .parse::<SocketAddr>()
            .map_err(|_| Status::invalid_argument("panel_listen_addr must be host:port"))?;
    }
    if options.scan_protection_enabled {
        if options.scan_burst < 2 {
            return Err(Status::invalid_argument("scan_burst must be at least 2"));
        }
        if options.scan_window_seconds == 0 {
            return Err(Status::invalid_argument(
                "scan_window_seconds must be greater than 0",
            ));
        }
    }
    Ok(())
}

fn validate_waf_settings(settings: &WafSettings) -> Result<(), Status> {
    if settings.requests_per_minute == 0 {
        return Err(Status::invalid_argument(
            "requests_per_minute must be greater than 0",
        ));
    }
    if settings.burst == 0 {
        return Err(Status::invalid_argument("burst must be greater than 0"));
    }
    if settings.block_duration_seconds == 0 {
        return Err(Status::invalid_argument(
            "block_duration_seconds must be greater than 0",
        ));
    }
    if settings.nginx_config_path.trim().is_empty()
        || settings.challenge_page_path.trim().is_empty()
    {
        return Err(Status::invalid_argument(
            "nginx_config_path and challenge_page_path are required",
        ));
    }
    Ok(())
}

fn validate_waf_rule(rule: &WafRule) -> Result<(), Status> {
    if rule.name.trim().is_empty() {
        return Err(Status::invalid_argument("waf rule name is required"));
    }
    let _ = waf_rule_kind(rule.kind)?;
    if rule.pattern.trim().is_empty() {
        return Err(Status::invalid_argument("waf rule pattern is required"));
    }
    if rule.pattern.contains('"') || rule.pattern.contains('\n') || rule.pattern.contains('\r') {
        return Err(Status::invalid_argument(
            "waf rule pattern must not contain quotes or new lines",
        ));
    }
    Ok(())
}

fn validate_ssh_settings(settings: &SshSettings) -> Result<(), Status> {
    if settings.port == 0 || settings.port > 65_535 {
        return Err(Status::invalid_argument("ssh port must be 1-65535"));
    }
    if settings.failed_attempt_limit < 2 {
        return Err(Status::invalid_argument(
            "failed_attempt_limit must be at least 2",
        ));
    }
    if settings.failed_attempt_window_seconds == 0 {
        return Err(Status::invalid_argument(
            "failed_attempt_window_seconds must be greater than 0",
        ));
    }
    if settings.config_path.trim().is_empty() {
        return Err(Status::invalid_argument("ssh config_path is required"));
    }
    Ok(())
}

fn validate_ssh_login_event(event: &SshLoginEvent) -> Result<(), Status> {
    if event.username.trim().is_empty() {
        return Err(Status::invalid_argument("ssh username is required"));
    }
    validate_address_filter(&event.source_ip, "source_ip")?;
    Ok(())
}

pub fn normalize_panel_access_path(path: &str) -> Result<String, Status> {
    let path = path.trim();
    if path.is_empty() || path == "/" {
        return Ok(DEFAULT_PANEL_ACCESS_PATH.to_owned());
    }
    if !path.starts_with('/') || path.contains(char::is_whitespace) || path.contains("//") {
        return Err(Status::invalid_argument(
            "panel_access_path must start with / and contain no whitespace",
        ));
    }
    Ok(path.trim_end_matches('/').to_owned())
}

fn validate_address_filter(value: &str, label: &str) -> Result<(), Status> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(());
    }
    if let Some((ip, prefix)) = value.split_once('/') {
        let ip = ip
            .parse::<IpAddr>()
            .map_err(|_| Status::invalid_argument(format!("{label} must be an IP or CIDR")))?;
        let prefix = prefix
            .parse::<u8>()
            .map_err(|_| Status::invalid_argument(format!("{label} CIDR prefix is invalid")))?;
        let max_prefix = if ip.is_ipv4() { 32 } else { 128 };
        if prefix <= max_prefix {
            Ok(())
        } else {
            Err(Status::invalid_argument(format!(
                "{label} CIDR prefix must be <= {max_prefix}"
            )))
        }
    } else {
        value
            .parse::<IpAddr>()
            .map(|_| ())
            .map_err(|_| Status::invalid_argument(format!("{label} must be an IP or CIDR")))
    }
}

fn firewall_protocol(value: i32) -> Result<FirewallProtocol, Status> {
    FirewallProtocol::try_from(value)
        .ok()
        .filter(|protocol| *protocol != FirewallProtocol::Unspecified)
        .ok_or_else(|| Status::invalid_argument("protocol is required"))
}

fn firewall_action(value: i32) -> Result<FirewallAction, Status> {
    FirewallAction::try_from(value)
        .ok()
        .filter(|action| *action != FirewallAction::Unspecified)
        .ok_or_else(|| Status::invalid_argument("action is required"))
}

fn firewall_direction(value: i32) -> Result<FirewallDirection, Status> {
    FirewallDirection::try_from(value)
        .ok()
        .filter(|direction| *direction != FirewallDirection::Unspecified)
        .ok_or_else(|| Status::invalid_argument("direction is required"))
}

fn firewall_backend(value: i32) -> Result<FirewallBackend, Status> {
    FirewallBackend::try_from(value)
        .ok()
        .ok_or_else(|| Status::invalid_argument("backend preference is invalid"))
}

fn waf_rule_kind(value: i32) -> Result<WafRuleKind, Status> {
    WafRuleKind::try_from(value)
        .ok()
        .filter(|kind| *kind != WafRuleKind::Unspecified)
        .ok_or_else(|| Status::invalid_argument("waf rule kind is required"))
}

fn ssh_key_algorithm(value: i32) -> Result<SshKeyAlgorithm, Status> {
    SshKeyAlgorithm::try_from(value)
        .ok()
        .filter(|algorithm| *algorithm != SshKeyAlgorithm::Unspecified)
        .ok_or_else(|| Status::invalid_argument("ssh key algorithm is required"))
}

fn backend_preference(options: &StoredSecurityOptions) -> FirewallBackend {
    firewall_backend(options.backend_preference).unwrap_or(FirewallBackend::Unspecified)
}

fn protocol_name(protocol: FirewallProtocol) -> &'static str {
    match protocol {
        FirewallProtocol::Tcp => "tcp",
        FirewallProtocol::Udp => "udp",
        FirewallProtocol::Icmp => "icmp",
        FirewallProtocol::Unspecified => "unspecified",
    }
}

fn backend_name(backend: FirewallBackend) -> &'static str {
    match backend {
        FirewallBackend::Ufw => "ufw",
        FirewallBackend::Firewalld => "firewalld",
        FirewallBackend::Iptables => "iptables",
        FirewallBackend::Unspecified => "unspecified",
    }
}

fn waf_kind_name(kind: WafRuleKind) -> &'static str {
    match kind {
        WafRuleKind::Cc => "cc",
        WafRuleKind::SqlInjection => "sql-injection",
        WafRuleKind::Xss => "xss",
        WafRuleKind::Keyword => "keyword",
        WafRuleKind::Scanner => "scanner",
        WafRuleKind::Unspecified => "unspecified",
    }
}

fn escape_nginx_regex(pattern: &str) -> String {
    pattern.replace('\\', "\\\\")
}

fn firewall_endpoint(value: &str) -> &str {
    let value = value.trim();
    if value.is_empty() {
        "any"
    } else {
        value
    }
}

fn port_range(rule: &StoredFirewallRule) -> String {
    if rule.port_end == 0 || rule.port_end == rule.port_start {
        rule.port_start.to_string()
    } else {
        format!("{}:{}", rule.port_start, rule.port_end)
    }
}

fn firewalld_port(rule: &StoredFirewallRule) -> String {
    if rule.port_end == 0 || rule.port_end == rule.port_start {
        rule.port_start.to_string()
    } else {
        format!("{}-{}", rule.port_start, rule.port_end)
    }
}

fn iptables_program(rule: &StoredFirewallRule) -> &'static str {
    if rule.source.contains(':') || rule.destination.contains(':') {
        "ip6tables"
    } else {
        "iptables"
    }
}

fn ip_family(rule: &StoredFirewallRule) -> &'static str {
    if rule.source.contains(':') || rule.destination.contains(':') {
        "ipv6"
    } else {
        "ipv4"
    }
}

fn should_apply_system_firewall() -> bool {
    env::var(APPLY_ENV).is_ok_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE"))
}

fn default_waf_config_path() -> String {
    env::var("RUSTPANEL_WAF_CONFIG_PATH")
        .unwrap_or_else(|_| format!("{DEFAULT_SECURITY_ROOT}/nginx-waf.conf"))
}

fn default_waf_challenge_path() -> String {
    env::var("RUSTPANEL_WAF_CHALLENGE_PATH")
        .unwrap_or_else(|_| format!("{DEFAULT_SECURITY_ROOT}/waf-challenge.html"))
}

fn default_ssh_config_path() -> String {
    env::var("RUSTPANEL_SSHD_CONFIG_PATH")
        .unwrap_or_else(|_| format!("{DEFAULT_SECURITY_ROOT}/sshd-rustpanel.conf"))
}

fn default_ssh_key_root() -> String {
    env::var("RUSTPANEL_SSH_KEY_ROOT")
        .unwrap_or_else(|_| format!("{DEFAULT_SECURITY_ROOT}/ssh-keys"))
}

fn safe_key_name(name: &str) -> Result<String, Status> {
    let safe = name
        .trim()
        .chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() || char == '-' {
                char.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned();
    if safe.is_empty() {
        Err(Status::invalid_argument("ssh key name is required"))
    } else {
        Ok(safe)
    }
}

fn totp_secret_configured() -> bool {
    env::var("RUSTPANEL_TOTP_SECRET")
        .or_else(|_| env::var("RUSTPANEL_2FA_SECRET"))
        .is_ok_and(|value| !value.trim().is_empty())
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

    fn sample_rule() -> FirewallRule {
        FirewallRule {
            id: "rule-1".to_owned(),
            name: "ssh".to_owned(),
            protocol: FirewallProtocol::Tcp.into(),
            action: FirewallAction::Allow.into(),
            direction: FirewallDirection::Inbound.into(),
            port_start: 22,
            port_end: 22,
            source: "10.0.0.0/24".to_owned(),
            destination: String::new(),
            enabled: true,
            comment: "office network".to_owned(),
            created_at_seconds: 0,
            updated_at_seconds: 0,
        }
    }

    #[tokio::test]
    async fn upsert_and_list_firewall_rule() {
        let root = std::env::temp_dir().join(format!("rustpanel-security-{}", Uuid::new_v4()));
        let service = SecurityServiceImpl::with_store(SecurityStore::new(root));
        let response = service
            .upsert_firewall_rule(Request::new(UpsertFirewallRuleRequest {
                rule: Some(sample_rule()),
            }))
            .await
            .expect("upsert")
            .into_inner();

        assert_eq!(response.rule.expect("rule").name, "ssh");

        let list = service
            .list_firewall_rules(Request::new(ListFirewallRulesRequest {}))
            .await
            .expect("list")
            .into_inner();

        assert_eq!(list.rules.len(), 1);
        assert_eq!(list.rules[0].source, "10.0.0.0/24");
        assert_eq!(
            list.options.expect("options").last_apply_message,
            "saved; system firewall apply disabled"
        );
    }

    #[test]
    fn rejects_invalid_source_filter() {
        let mut rule = sample_rule();
        rule.source = "not-cidr".to_owned();

        assert_eq!(
            validate_rule(&rule).expect_err("invalid source").code(),
            tonic::Code::InvalidArgument
        );
    }

    #[test]
    fn builds_iptables_tcp_rule_with_source_and_comment() {
        let stored = StoredFirewallRule::from_proto(sample_rule());
        let commands = build_iptables_rule_commands(&stored, FirewallOperation::Add)
            .expect("iptables command");

        assert_eq!(commands[0].program, "iptables");
        assert!(commands[0].args.contains(&"--dport".to_owned()));
        assert!(commands[0].args.contains(&"22".to_owned()));
        assert!(commands[0].args.contains(&"10.0.0.0/24".to_owned()));
        assert!(commands[0].args.contains(&"rustpanel:rule-1".to_owned()));
    }

    #[test]
    fn builds_firewalld_icmp_rule() {
        let mut rule = sample_rule();
        rule.protocol = FirewallProtocol::Icmp.into();
        rule.port_start = 0;
        rule.port_end = 0;
        let stored = StoredFirewallRule::from_proto(rule);

        let rich_rule = firewalld_rich_rule(&stored).expect("rich rule");

        assert!(rich_rule.contains("protocol value=\"icmp\""));
        assert!(rich_rule.contains("source address=\"10.0.0.0/24\""));
    }

    #[test]
    fn normalizes_panel_access_path() {
        assert_eq!(
            normalize_panel_access_path("/secure/").expect("path"),
            "/secure"
        );
        assert!(normalize_panel_access_path("secure").is_err());
        assert!(normalize_panel_access_path("/bad path").is_err());
    }

    #[test]
    fn renders_waf_nginx_config_with_presets() {
        let settings = StoredWafSettings {
            enabled: true,
            cc_protection_enabled: true,
            captcha_challenge_enabled: true,
            ..StoredWafSettings::default()
        };
        let config = render_waf_nginx_config(&settings, &default_waf_rules()).expect("config");

        assert!(config.contains("limit_req_zone"));
        assert!(config.contains("rustpanel-waf-challenge.html"));
        assert!(config.contains("information_schema"));
        assert!(config.contains("wp-config"));
    }

    #[test]
    fn validates_waf_rule_pattern() {
        let rule = WafRule {
            id: String::new(),
            name: "bad".to_owned(),
            kind: WafRuleKind::Xss.into(),
            pattern: "\"bad\"".to_owned(),
            enabled: true,
            scope_domain: String::new(),
            comment: String::new(),
            created_at_seconds: 0,
            updated_at_seconds: 0,
        };

        assert!(validate_waf_rule(&rule).is_err());
    }

    #[test]
    fn renders_ssh_config_with_custom_port_and_password_lock() {
        let settings = StoredSshSettings {
            port: 2222,
            password_login_disabled: true,
            ..StoredSshSettings::default()
        };
        let config = render_ssh_config(&settings);

        assert!(config.contains("Port 2222"));
        assert!(config.contains("PasswordAuthentication no"));
    }

    #[tokio::test]
    async fn auto_bans_ssh_source_after_failed_attempts() {
        let mut state = StoredSecurityState {
            ssh_settings: StoredSshSettings {
                port: 2222,
                failed_attempt_limit: 2,
                failed_attempt_window_seconds: 600,
                auto_ban_enabled: true,
                ..StoredSshSettings::default()
            },
            ..StoredSecurityState::default()
        }
        .with_defaults();
        state.ssh_events.push(StoredSshLoginEvent {
            id: "first".to_owned(),
            username: "root".to_owned(),
            source_ip: "192.0.2.10".to_owned(),
            successful: false,
            auto_banned: false,
            message: String::new(),
            occurred_at_seconds: 100,
        });
        let event = SshLoginEvent {
            id: String::new(),
            username: "root".to_owned(),
            source_ip: "192.0.2.10".to_owned(),
            successful: false,
            auto_banned: false,
            message: String::new(),
            occurred_at_seconds: 200,
        };

        let banned = maybe_auto_ban_ssh_source(&mut state, &event)
            .await
            .expect("auto ban");

        assert!(banned);
        assert_eq!(state.rules.len(), 1);
        assert_eq!(state.rules[0].port_start, 2222);
        assert_eq!(state.rules[0].action, FirewallAction::Deny as i32);
    }
}
