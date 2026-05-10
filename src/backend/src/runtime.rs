use std::{
    collections::HashSet,
    env,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tonic::Status;

use crate::proto::rustpanel::v1::RuntimeModule;

const DEFAULT_RUNTIME_ROOT: &str = "/var/lib/rustpanel/runtime";
const OVERRIDE_CACHE_TTL: Duration = Duration::from_secs(2);

/// 文件级 override:写到 $RUSTPANEL_RUNTIME_ROOT/modules.json。
/// 存在时覆盖 env 变量,允许从面板 UI 改 toggle 即时生效,无需重启。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ModuleOverride {
    /// 显式启用的模块 id;empty 表示"除 disabled 外都启用"。
    #[serde(default)]
    pub enabled: Vec<String>,
    /// 显式禁用的模块 id。优先级高于 enabled。
    #[serde(default)]
    pub disabled: Vec<String>,
}

fn runtime_root() -> PathBuf {
    env::var("RUSTPANEL_RUNTIME_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_RUNTIME_ROOT))
}

fn override_path() -> PathBuf {
    runtime_root().join("modules.json")
}

struct OverrideCache {
    value: Option<ModuleOverride>,
    expires_at: Instant,
}

fn cache() -> &'static Mutex<Option<OverrideCache>> {
    static CACHE: OnceLock<Mutex<Option<OverrideCache>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

fn load_override() -> Option<ModuleOverride> {
    let mut guard = cache().lock().ok()?;
    if let Some(entry) = guard.as_ref() {
        if entry.expires_at > Instant::now() {
            return entry.value.clone();
        }
    }
    let parsed = match std::fs::read_to_string(override_path()) {
        Ok(content) => serde_json::from_str::<ModuleOverride>(&content).ok(),
        Err(_) => None,
    };
    *guard = Some(OverrideCache {
        value: parsed.clone(),
        expires_at: Instant::now() + OVERRIDE_CACHE_TTL,
    });
    parsed
}

/// 把当前 override 写到 modules.json,清缓存让下次读取立刻生效。
pub fn save_override(value: &ModuleOverride) -> Result<(), Status> {
    let path = override_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Status::internal(format!("create runtime root: {e}")))?;
    }
    let serialized = serde_json::to_string_pretty(value)
        .map_err(|e| Status::internal(format!("serialize override: {e}")))?;
    // 写到临时文件再 rename 保证原子性,避免半写状态被读到。
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serialized)
        .map_err(|e| Status::internal(format!("write override tmp: {e}")))?;
    std::fs::rename(&tmp, &path).map_err(|e| Status::internal(format!("rename override: {e}")))?;
    if let Ok(mut guard) = cache().lock() {
        *guard = None; // 立即失效
    }
    Ok(())
}

/// 读当前 override(给 RPC 列表用)。文件不存在返回 None。
pub fn current_override() -> Option<ModuleOverride> {
    load_override()
}

pub const MODULE_CORE: &str = "core";
pub const MODULE_AUDIT: &str = "audit";
pub const MODULE_MONITOR: &str = "monitor";
pub const MODULE_FILES: &str = "files";
pub const MODULE_TERMINAL: &str = "terminal";
pub const MODULE_SECURITY: &str = "security";
pub const MODULE_DOCKER: &str = "docker";
pub const MODULE_APPSTORE: &str = "appstore";
pub const MODULE_SITES: &str = "sites";
pub const MODULE_STATIC_SITES: &str = "static-sites";
pub const MODULE_SSL: &str = "ssl";
pub const MODULE_DATABASE: &str = "database";
pub const MODULE_CRON: &str = "cron";
pub const MODULE_CLUSTER: &str = "cluster";
pub const MODULE_WORKLOADS: &str = "workloads";
pub const MODULE_PROXY: &str = "proxy";

#[derive(Clone, Debug)]
pub struct ModuleDefinition {
    pub id: &'static str,
    pub name: &'static str,
    pub required: bool,
}

#[derive(Clone, Debug)]
pub struct ModuleStatus {
    pub id: &'static str,
    pub name: &'static str,
    pub enabled: bool,
    pub reason: String,
    pub required: bool,
}

#[derive(Clone, Debug)]
pub struct RuntimeModules {
    enabled: Option<HashSet<String>>,
    disabled: HashSet<String>,
    profile: String,
}

impl RuntimeModules {
    pub fn from_env() -> Self {
        let env_enabled = env::var("RUSTPANEL_ENABLED_MODULES")
            .ok()
            .map(|value| parse_module_set(&value));
        let env_disabled = env::var("RUSTPANEL_DISABLED_MODULES")
            .ok()
            .map(|value| parse_module_set(&value))
            .unwrap_or_default();
        let profile = env::var("RUSTPANEL_INSTALL_PROFILE")
            .or_else(|_| env::var("RUSTPANEL_PROFILE"))
            .unwrap_or_else(|_| "custom".to_owned());

        // modules.json override 优先于 env:用户在面板 toggle 后立刻反映,
        // 不需要重启 / 改 .env。文件不存在或读失败时退回 env。
        let (enabled, disabled) = match load_override() {
            Some(over) => {
                let enabled = if over.enabled.is_empty() {
                    env_enabled
                } else {
                    Some(
                        over.enabled
                            .iter()
                            .map(|s| normalize_module_id(s))
                            .collect(),
                    )
                };
                let disabled = if over.disabled.is_empty() {
                    env_disabled
                } else {
                    over.disabled
                        .iter()
                        .map(|s| normalize_module_id(s))
                        .collect()
                };
                (enabled, disabled)
            }
            None => (env_enabled, env_disabled),
        };

        Self {
            enabled,
            disabled,
            profile,
        }
    }

    pub fn profile(&self) -> &str {
        &self.profile
    }

    pub fn is_enabled(&self, module_id: &str) -> bool {
        let module_id = normalize_module_id(module_id);
        if is_required_module(&module_id) {
            return true;
        }
        if self.disabled.contains(&module_id) {
            return false;
        }
        self.enabled
            .as_ref()
            .map(|enabled| enabled.contains(&module_id))
            .unwrap_or(true)
    }

    pub fn ensure_enabled(&self, module_id: &str) -> Result<(), Status> {
        if self.is_enabled(module_id) {
            Ok(())
        } else {
            Err(Status::failed_precondition(format!(
                "module {module_id} is disabled"
            )))
        }
    }

    pub fn statuses(&self) -> Vec<ModuleStatus> {
        module_catalog()
            .into_iter()
            .map(|definition| {
                let enabled = self.is_enabled(definition.id);
                let reason = if definition.required {
                    "required core module".to_owned()
                } else if enabled {
                    format!("enabled by {} profile", self.profile)
                } else {
                    format!("disabled by {} profile", self.profile)
                };
                ModuleStatus {
                    id: definition.id,
                    name: definition.name,
                    enabled,
                    reason,
                    required: definition.required,
                }
            })
            .collect()
    }
}

impl From<ModuleStatus> for RuntimeModule {
    fn from(status: ModuleStatus) -> Self {
        RuntimeModule {
            id: status.id.to_owned(),
            name: status.name.to_owned(),
            enabled: status.enabled,
            reason: status.reason,
            required: status.required,
        }
    }
}

pub fn from_env() -> RuntimeModules {
    RuntimeModules::from_env()
}

pub fn ensure_module_enabled(module_id: &str) -> Result<(), Status> {
    RuntimeModules::from_env().ensure_enabled(module_id)
}

pub fn module_catalog() -> Vec<ModuleDefinition> {
    vec![
        ModuleDefinition {
            id: MODULE_CORE,
            name: "Core",
            required: true,
        },
        ModuleDefinition {
            id: MODULE_AUDIT,
            name: "Audit",
            required: true,
        },
        ModuleDefinition {
            id: MODULE_MONITOR,
            name: "Monitor",
            required: false,
        },
        ModuleDefinition {
            id: MODULE_FILES,
            name: "Files",
            required: false,
        },
        ModuleDefinition {
            id: MODULE_TERMINAL,
            name: "Terminal",
            required: false,
        },
        ModuleDefinition {
            id: MODULE_SECURITY,
            name: "Security",
            required: false,
        },
        ModuleDefinition {
            id: MODULE_DOCKER,
            name: "Docker",
            required: false,
        },
        ModuleDefinition {
            id: MODULE_APPSTORE,
            name: "App Store",
            required: false,
        },
        ModuleDefinition {
            id: MODULE_SITES,
            name: "Nginx Sites",
            required: false,
        },
        ModuleDefinition {
            id: MODULE_STATIC_SITES,
            name: "Static Sites",
            required: false,
        },
        ModuleDefinition {
            id: MODULE_SSL,
            name: "SSL",
            required: false,
        },
        ModuleDefinition {
            id: MODULE_DATABASE,
            name: "Database",
            required: false,
        },
        ModuleDefinition {
            id: MODULE_CRON,
            name: "Cron",
            required: false,
        },
        ModuleDefinition {
            id: MODULE_CLUSTER,
            name: "Cluster",
            required: false,
        },
        ModuleDefinition {
            id: MODULE_WORKLOADS,
            name: "Workloads",
            required: false,
        },
        ModuleDefinition {
            id: MODULE_PROXY,
            name: "Proxy",
            required: false,
        },
    ]
}

fn is_required_module(module_id: &str) -> bool {
    matches!(module_id, MODULE_CORE | MODULE_AUDIT)
}

fn parse_module_set(value: &str) -> HashSet<String> {
    value
        .split(',')
        .map(normalize_module_id)
        .filter(|value| !value.is_empty())
        .collect()
}

fn normalize_module_id(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('_', "-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabled_list_limits_optional_modules() {
        let modules = RuntimeModules {
            enabled: Some(parse_module_set("files,static-sites")),
            disabled: HashSet::new(),
            profile: "micro".to_owned(),
        };

        assert!(modules.is_enabled(MODULE_CORE));
        assert!(modules.is_enabled(MODULE_FILES));
        assert!(modules.is_enabled(MODULE_STATIC_SITES));
        assert!(!modules.is_enabled(MODULE_DOCKER));
    }

    #[test]
    fn disabled_list_overrides_enabled_list() {
        let modules = RuntimeModules {
            enabled: Some(parse_module_set("docker,files")),
            disabled: parse_module_set("docker"),
            profile: "custom".to_owned(),
        };

        assert!(modules.is_enabled(MODULE_FILES));
        assert!(!modules.is_enabled(MODULE_DOCKER));
        assert!(modules.is_enabled(MODULE_AUDIT));
    }
}
