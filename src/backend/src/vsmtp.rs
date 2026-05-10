//! vSMTP 别名转发表(Mail Alias)的 CRUD 后端。
//!
//! 设计要点
//! - **只管表,不管邮件**:实际收信→改写→转发由 vSMTP 进程做,
//!   本模块只是把"用户在面板上点的转发规则"持久化下来并维持唯一性。
//! - **存储**:JSON 文件落到 `$RUSTPANEL_RUNTIME_ROOT/vsmtp/aliases.json`,
//!   与 runtime.rs modules.json 同套规则(tmp + rename 原子写)。
//!   单家面板用户量级别有几十条 alias,JSON 完全够。
//! - **rhai 规则生成**:这一轮**不做** —— vSMTP 还没接进面板真正
//!   起服务,先把表稳了。等接入时,从 list() 结果渲染 rhai 即可。
//! - **门控**:依托 `MODULE_APPSTORE`,vSMTP 是 appstore 装上去的,
//!   关掉 appstore 时管理别名也没意义。
//!
//! 不在范围内
//! - 不验证 forward_to 是不是真的邮箱(SMTP 探活、MX 查询等),
//!   交给 vSMTP 自己发信时报错;面板层只做最基本格式校验。
//! - 不写 vSMTP 自身配置(/etc/vsmtp/vsmtp.toml 那一份),
//!   仍由 appstore 的安装计划负责。

use std::{
    env,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tonic::{Request, Response as GrpcResponse, Status};

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        vsmtp_alias_service_server::VsmtpAliasService, DeleteVsmtpAliasRequest,
        DeleteVsmtpAliasResponse, ListVsmtpAliasesRequest, ListVsmtpAliasesResponse,
        UpsertVsmtpAliasRequest, UpsertVsmtpAliasResponse, VsmtpAlias,
    },
};

const DEFAULT_RUNTIME_ROOT: &str = "/var/lib/rustpanel/runtime";
const DEFAULT_VSMTP_CONFIG_DIR: &str = "/etc/vsmtp";

#[derive(Clone, Debug, Default)]
pub struct VsmtpAliasServiceImpl;

#[tonic::async_trait]
impl VsmtpAliasService for VsmtpAliasServiceImpl {
    async fn list_vsmtp_aliases(
        &self,
        _request: Request<ListVsmtpAliasesRequest>,
    ) -> Result<GrpcResponse<ListVsmtpAliasesResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_APPSTORE)?;
        let aliases = load_aliases().await?;
        Ok(GrpcResponse::new(ListVsmtpAliasesResponse {
            status: Some(ok_response("ok")),
            aliases,
        }))
    }

    async fn upsert_vsmtp_alias(
        &self,
        request: Request<UpsertVsmtpAliasRequest>,
    ) -> Result<GrpcResponse<UpsertVsmtpAliasResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_APPSTORE)?;
        let alias = request
            .into_inner()
            .alias
            .ok_or_else(|| Status::invalid_argument("alias is required"))?;
        let normalized_local = validate_alias_local(&alias.alias)?;
        let normalized_target = validate_forward_to(&alias.forward_to)?;
        let now = current_timestamp();
        let mut aliases = load_aliases().await?;
        let existing_created = aliases
            .iter()
            .find(|item| item.alias == normalized_local)
            .map(|item| item.created_at_seconds)
            .unwrap_or(now);
        aliases.retain(|item| item.alias != normalized_local);
        let stored = VsmtpAlias {
            alias: normalized_local.clone(),
            forward_to: normalized_target.clone(),
            note: alias.note.trim().to_owned(),
            created_at_seconds: existing_created,
            updated_at_seconds: now,
        };
        aliases.push(stored.clone());
        sort_aliases(&mut aliases);
        save_aliases(&aliases).await?;
        apply_vsmtp_runtime(&aliases).await?;
        Ok(GrpcResponse::new(UpsertVsmtpAliasResponse {
            status: Some(ok_response("alias saved")),
            alias: Some(stored),
        }))
    }

    async fn delete_vsmtp_alias(
        &self,
        request: Request<DeleteVsmtpAliasRequest>,
    ) -> Result<GrpcResponse<DeleteVsmtpAliasResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_APPSTORE)?;
        let alias = validate_alias_local(&request.into_inner().alias)?;
        let mut aliases = load_aliases().await?;
        let before = aliases.len();
        aliases.retain(|item| item.alias != alias);
        if aliases.len() == before {
            return Err(Status::not_found("alias not found"));
        }
        save_aliases(&aliases).await?;
        apply_vsmtp_runtime(&aliases).await?;
        Ok(GrpcResponse::new(DeleteVsmtpAliasResponse {
            status: Some(ok_response("alias deleted")),
        }))
    }
}

fn runtime_root() -> PathBuf {
    env::var("RUSTPANEL_RUNTIME_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_RUNTIME_ROOT))
}

fn aliases_path() -> PathBuf {
    runtime_root().join("vsmtp").join("aliases.json")
}

/// alias 本地部分校验:RFC5321 local-part 的子集,只允许小写字母 +
/// 数字 + . _ -,长度 1..=64。比标准更严是因为面板生成的 alias 还要
/// 进 systemd / rhai,简单字符集省去 escape 的麻烦。
fn validate_alias_local(input: &str) -> Result<String, Status> {
    let trimmed = input.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return Err(Status::invalid_argument("alias is required"));
    }
    if trimmed.len() > 64 {
        return Err(Status::invalid_argument("alias too long (max 64 chars)"));
    }
    let ok = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !ok {
        return Err(Status::invalid_argument(
            "alias may only contain a-z, 0-9, '.', '_', '-'",
        ));
    }
    Ok(trimmed)
}

/// forward_to 校验:必须像邮箱(local@domain),不做 DNS 查询。
fn validate_forward_to(input: &str) -> Result<String, Status> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(Status::invalid_argument("forward_to is required"));
    }
    if trimmed.len() > 254 {
        return Err(Status::invalid_argument("forward_to too long"));
    }
    let mut parts = trimmed.splitn(2, '@');
    let local = parts.next().unwrap_or("");
    let domain = parts.next().unwrap_or("");
    if local.is_empty() || domain.is_empty() {
        return Err(Status::invalid_argument(
            "forward_to must look like name@example.com",
        ));
    }
    // 域名段至少有一个点,粗略过滤"name@localhost"这类无 DNS 解析能力的目标
    if !domain.contains('.') {
        return Err(Status::invalid_argument(
            "forward_to domain must include a TLD",
        ));
    }
    Ok(trimmed.to_owned())
}

fn sort_aliases(aliases: &mut [VsmtpAlias]) {
    aliases.sort_by(|left, right| left.alias.cmp(&right.alias));
}

async fn load_aliases() -> Result<Vec<VsmtpAlias>, Status> {
    let path = aliases_path();
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let stored: Vec<StoredAlias> = serde_json::from_slice(&bytes).map_err(io_status)?;
            let mut aliases: Vec<VsmtpAlias> =
                stored.into_iter().map(StoredAlias::into_proto).collect();
            sort_aliases(&mut aliases);
            Ok(aliases)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(io_status(error)),
    }
}

async fn save_aliases(aliases: &[VsmtpAlias]) -> Result<(), Status> {
    let path = aliases_path();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(io_status)?;
    }
    let stored: Vec<StoredAlias> = aliases
        .iter()
        .cloned()
        .map(StoredAlias::from_proto)
        .collect();
    let body = serde_json::to_vec_pretty(&stored).map_err(io_status)?;
    let tmp = path.with_extension("json.rustpanel-tmp");
    tokio::fs::write(&tmp, body).await.map_err(io_status)?;
    tokio::fs::rename(&tmp, &path).await.map_err(io_status)?;
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredAlias {
    alias: String,
    forward_to: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    created_at_seconds: i64,
    #[serde(default)]
    updated_at_seconds: i64,
}

impl StoredAlias {
    fn from_proto(value: VsmtpAlias) -> Self {
        Self {
            alias: value.alias,
            forward_to: value.forward_to,
            note: value.note,
            created_at_seconds: value.created_at_seconds,
            updated_at_seconds: value.updated_at_seconds,
        }
    }

    fn into_proto(self) -> VsmtpAlias {
        VsmtpAlias {
            alias: self.alias,
            forward_to: self.forward_to,
            note: self.note,
            created_at_seconds: self.created_at_seconds,
            updated_at_seconds: self.updated_at_seconds,
        }
    }
}

fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn io_status(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
}

// =====================================================================
// vSMTP runtime artifact:把 alias 表落到 vSMTP 进程可读的两份文件:
// - /etc/vsmtp/aliases.json:紧凑表数据,适合脚本 / debug
// - /etc/vsmtp/rules/aliases.rhai:Rhai 常量 map,vSMTP 的 .rhai 主
//   规则文件可以直接 import 这一份 const ALIASES
// 两份都原子写。reload vSMTP 不在这里做(独立 lifecycle,且 vSMTP
// 一般 watch 配置文件自动 reload)。
// =====================================================================

fn vsmtp_config_dir() -> PathBuf {
    env::var("RUSTPANEL_VSMTP_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_VSMTP_CONFIG_DIR))
}

/// 纯函数:把 alias 表序列化成给外部脚本读的紧凑 JSON。
pub(crate) fn render_vsmtp_aliases_json(aliases: &[VsmtpAlias]) -> String {
    let stored: Vec<StoredAlias> = aliases
        .iter()
        .cloned()
        .map(StoredAlias::from_proto)
        .collect();
    serde_json::to_string_pretty(&stored).unwrap_or_else(|_| "[]".to_owned())
}

/// 纯函数:把 alias 表生成 Rhai const map。vSMTP 主脚本里
/// `import "aliases" as a;` 然后 `a::ALIASES["amazon"]` 拿目标地址。
/// 转发链路逻辑(改 Reply-To / 走 relay)在主脚本里写,这里只供数据。
pub(crate) fn render_vsmtp_rhai(aliases: &[VsmtpAlias]) -> String {
    let mut body = String::new();
    body.push_str("// RustPanel-managed vSMTP alias map. Regenerated on every UI change.\n");
    body.push_str("// Do not edit by hand — changes will be overwritten.\n");
    body.push_str("const ALIASES = #{\n");
    // 已按 alias 升序持久化,这里也按入参顺序输出,保证 diff 稳定
    for alias in aliases {
        body.push_str("    ");
        // alias 已经过 validate_alias_local 校验,只含安全字符;
        // 仍然走标准转义防御性 escape。
        body.push_str(&format!(
            "\"{}\": \"{}\",\n",
            escape_rhai_string(&alias.alias),
            escape_rhai_string(&alias.forward_to)
        ));
    }
    body.push_str("};\n");
    body
}

fn escape_rhai_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

async fn write_atomic(path: PathBuf, body: &[u8]) -> Result<(), Status> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(io_status)?;
    }
    let tmp = path.with_extension("rustpanel-tmp");
    tokio::fs::write(&tmp, body).await.map_err(io_status)?;
    tokio::fs::rename(&tmp, &path).await.map_err(io_status)
}

/// 把 alias 表同时落到 aliases.json + rules/aliases.rhai。
async fn apply_vsmtp_runtime(aliases: &[VsmtpAlias]) -> Result<(), Status> {
    let dir = vsmtp_config_dir();
    let json_path = dir.join("aliases.json");
    let rhai_path = dir.join("rules").join("aliases.rhai");
    let json_body = render_vsmtp_aliases_json(aliases);
    let rhai_body = render_vsmtp_rhai(aliases);
    write_atomic(json_path, json_body.as_bytes()).await?;
    write_atomic(rhai_path, rhai_body.as_bytes()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_local_lowercases_and_validates_charset() {
        assert_eq!(validate_alias_local("Amazon").unwrap(), "amazon");
        assert_eq!(
            validate_alias_local(" hello.world ").unwrap(),
            "hello.world"
        );
        assert!(validate_alias_local("").is_err());
        assert!(validate_alias_local("has space").is_err());
        assert!(validate_alias_local("has+plus").is_err());
        assert!(validate_alias_local(&"a".repeat(65)).is_err());
    }

    #[test]
    fn forward_to_requires_email_shape_with_tld() {
        assert!(validate_forward_to("me@example.com").is_ok());
        assert!(validate_forward_to("me@sub.example.com").is_ok());
        // 无域、无 TLD 都拒
        assert!(validate_forward_to("plain-string").is_err());
        assert!(validate_forward_to("me@localhost").is_err());
        assert!(validate_forward_to("@example.com").is_err());
        assert!(validate_forward_to("me@").is_err());
    }

    #[test]
    fn render_vsmtp_rhai_emits_const_alias_map() {
        let aliases = vec![
            VsmtpAlias {
                alias: "amazon".to_owned(),
                forward_to: "me@example.com".to_owned(),
                note: String::new(),
                created_at_seconds: 0,
                updated_at_seconds: 0,
            },
            VsmtpAlias {
                alias: "github".to_owned(),
                forward_to: "other@example.com".to_owned(),
                note: String::new(),
                created_at_seconds: 0,
                updated_at_seconds: 0,
            },
        ];
        let rhai = render_vsmtp_rhai(&aliases);
        assert!(rhai.contains("const ALIASES = #{"));
        assert!(rhai.contains("\"amazon\": \"me@example.com\""));
        assert!(rhai.contains("\"github\": \"other@example.com\""));
        // 空表也要是合法 rhai —— 至少有 const ALIASES = #{};
        let empty = render_vsmtp_rhai(&[]);
        assert!(empty.contains("const ALIASES = #{"));
        assert!(empty.contains("};"));
    }

    #[test]
    fn escape_rhai_string_handles_quotes_and_backslashes() {
        assert_eq!(escape_rhai_string("abc"), "abc");
        assert_eq!(escape_rhai_string("a\"b"), "a\\\"b");
        assert_eq!(escape_rhai_string("a\\b"), "a\\\\b");
        assert_eq!(escape_rhai_string("line1\nline2"), "line1\\nline2");
    }

    #[test]
    fn render_vsmtp_aliases_json_round_trips() {
        let aliases = vec![VsmtpAlias {
            alias: "amazon".to_owned(),
            forward_to: "me@example.com".to_owned(),
            note: "amazon 注册".to_owned(),
            created_at_seconds: 1234,
            updated_at_seconds: 5678,
        }];
        let json = render_vsmtp_aliases_json(&aliases);
        let parsed: Vec<StoredAlias> =
            serde_json::from_str(&json).expect("regenerated json must be parseable back");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].alias, "amazon");
        assert_eq!(parsed[0].forward_to, "me@example.com");
        assert_eq!(parsed[0].note, "amazon 注册");
        assert_eq!(parsed[0].created_at_seconds, 1234);
        assert_eq!(parsed[0].updated_at_seconds, 5678);
    }

    #[test]
    fn stored_alias_round_trips_through_proto() {
        let original = VsmtpAlias {
            alias: "amazon".to_owned(),
            forward_to: "me@example.com".to_owned(),
            note: "amazon 注册".to_owned(),
            created_at_seconds: 100,
            updated_at_seconds: 200,
        };
        let stored = StoredAlias::from_proto(original.clone());
        let restored = stored.into_proto();
        assert_eq!(restored.alias, original.alias);
        assert_eq!(restored.forward_to, original.forward_to);
        assert_eq!(restored.note, original.note);
        assert_eq!(restored.created_at_seconds, original.created_at_seconds);
        assert_eq!(restored.updated_at_seconds, original.updated_at_seconds);
    }
}
