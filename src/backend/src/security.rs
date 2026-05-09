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
    ok_response,
    proto::rustpanel::v1::{
        security_service_server::SecurityService, DeleteFirewallRuleRequest,
        DeleteFirewallRuleResponse, ExportFirewallRulesRequest, ExportFirewallRulesResponse,
        FirewallAction, FirewallBackend, FirewallDirection, FirewallProtocol, FirewallRule,
        ImportFirewallRulesRequest, ImportFirewallRulesResponse, ListFirewallRulesRequest,
        ListFirewallRulesResponse, SecurityOptions, SetFirewallRuleEnabledRequest,
        SetFirewallRuleEnabledResponse, UpdateSecurityOptionsRequest,
        UpdateSecurityOptionsResponse, UpsertFirewallRuleRequest, UpsertFirewallRuleResponse,
    },
};

const DEFAULT_SECURITY_ROOT: &str = "/tmp/rustpanel/security";
const APPLY_ENV: &str = "RUSTPANEL_SECURITY_APPLY";
const DEFAULT_SCAN_BURST: u32 = 20;
const DEFAULT_SCAN_WINDOW_SECONDS: u32 = 60;
const DEFAULT_PANEL_ACCESS_PATH: &str = "/";

#[derive(Clone, Debug)]
pub struct SecurityServiceImpl {
    store: SecurityStore,
}

impl SecurityServiceImpl {
    pub fn new() -> Self {
        Self {
            store: SecurityStore::from_env(),
        }
    }

    #[cfg(test)]
    fn with_store(store: SecurityStore) -> Self {
        Self { store }
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

        let mut state = self.store.load().await?;
        state.options = StoredSecurityOptions::from_proto(options);
        apply_options(&mut state.options).await?;
        self.store.save(&state).await?;

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
        let mut current = self.store.load().await?;
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
        apply_imported_state(&current, &mut options).await?;
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
}

#[derive(Clone, Debug)]
struct SecurityStore {
    root: Arc<PathBuf>,
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
        }
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
        tokio::fs::write(self.state_path(), content)
            .await
            .map_err(io_status)
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
    if old_rule.is_some_and(|rule| rule.enabled) {
        let rule = old_rule.expect("checked old rule");
        let commands = build_rule_commands(backend, rule, FirewallOperation::Delete)?;
        run_commands(commands).await?;
        messages.push(format!("removed old rule via {}", backend_name(backend)));
    }
    if new_rule.is_some_and(|rule| rule.enabled) {
        let rule = new_rule.expect("checked new rule");
        let commands = build_rule_commands(backend, rule, FirewallOperation::Add)?;
        run_commands(commands).await?;
        messages.push(format!("applied rule via {}", backend_name(backend)));
    }
    if messages.is_empty() {
        messages.push("rule stored without active firewall change".to_owned());
    }
    options.last_apply_message = messages.join("; ");
    Ok(())
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

async fn apply_imported_state(
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
}
