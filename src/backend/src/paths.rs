//! 状态目录的统一解析与迁移。
//!
//! 各模块的状态目录优先读自己的 `RUSTPANEL_<X>_ROOT`;没设时落在
//! `$RUSTPANEL_DATA_DIR/<模块>`,再没设就是 `/var/lib/rustpanel/<模块>`。
//!
//! 早期版本把大部分模块的默认目录放在 `/tmp/rustpanel/<模块>`:用户库、通知渠道
//! 凭据、本地备份都在里面,systemd-tmpfiles 清理 / tmpfs 的 /tmp / 容器重建都会让
//! 它们丢失。[`migrate_legacy_state`] 在服务启动时把这些老目录搬到新位置。

use std::{
    env,
    path::{Path, PathBuf},
};

const DEFAULT_BASE: &str = "/var/lib/rustpanel";
const LEGACY_TMP_BASE: &str = "/tmp/rustpanel";

/// (模块目录名, 覆盖用的环境变量)。目录名与老的 /tmp/rustpanel/<名> 一致,迁移按名对应。
pub const MODULES: &[(&str, &str)] = &[
    ("acme", "RUSTPANEL_ACME_ROOT"),
    ("appstore", "RUSTPANEL_APPSTORE_ROOT"),
    ("audit", "RUSTPANEL_AUDIT_ROOT"),
    ("backup", "RUSTPANEL_BACKUP_ROOT"),
    ("capability", "RUSTPANEL_CAPABILITY_ROOT"),
    ("cluster", "RUSTPANEL_CLUSTER_ROOT"),
    ("compose", "RUSTPANEL_DOCKER_COMPOSE_ROOT"),
    ("cron", "RUSTPANEL_CRON_ROOT"),
    ("dns", "RUSTPANEL_DNS_ROOT"),
    ("monitor", "RUSTPANEL_MONITOR_ROOT"),
    ("files", "RUSTPANEL_FILE_STATE_ROOT"),
    ("notification", "RUSTPANEL_NOTIFICATION_ROOT"),
    ("proxy", "RUSTPANEL_PROXY_ROOT"),
    ("rollback", "RUSTPANEL_ROLLBACK_ROOT"),
    ("runtime", "RUSTPANEL_RUNTIME_ROOT"),
    ("security", "RUSTPANEL_SECURITY_ROOT"),
    ("site", "RUSTPANEL_SITE_STATE_ROOT"),
    ("users", "RUSTPANEL_USER_ROOT"),
    ("workloads", "RUSTPANEL_WORKLOAD_ROOT"),
];

fn base_dir() -> PathBuf {
    if let Some(dir) = env::var("RUSTPANEL_DATA_DIR")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    {
        return PathBuf::from(dir);
    }
    if cfg!(test) {
        // 单元测试不能碰 /var/lib;沿用系统临时目录
        return env::temp_dir().join("rustpanel-test-state");
    }
    PathBuf::from(DEFAULT_BASE)
}

/// 模块状态目录的默认值(调用方已先读过自己的 `RUSTPANEL_<X>_ROOT`)。
pub fn default_root(module: &str) -> PathBuf {
    base_dir().join(module)
}

/// 按模块名解析完整状态目录:显式 env 优先,否则默认值。
pub fn state_root(module: &str, env_key: &str) -> PathBuf {
    env::var(env_key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_root(module))
}

/// 把老版本留在 /tmp/rustpanel(以及 DATA_DIR 生效前的 /var/lib/rustpanel)下的
/// 模块目录搬到当前位置。只处理没显式设 env 的模块,且目标还不存在或为空时才搬,
/// 绝不覆盖已有数据。返回搬过的模块名。
pub fn migrate_legacy_state() -> Vec<String> {
    let base = base_dir();
    let mut legacy_bases = vec![PathBuf::from(LEGACY_TMP_BASE)];
    if base != Path::new(DEFAULT_BASE) {
        legacy_bases.push(PathBuf::from(DEFAULT_BASE));
    }
    let mut moved = Vec::new();
    for (module, env_key) in MODULES {
        if env::var(env_key).is_ok_and(|value| !value.trim().is_empty()) {
            continue;
        }
        let target = base.join(module);
        for legacy_base in &legacy_bases {
            let legacy = legacy_base.join(module);
            match migrate_dir(&legacy, &target) {
                Ok(true) => {
                    tracing::info!(
                        module,
                        from = %legacy.display(),
                        to = %target.display(),
                        "migrated legacy state directory"
                    );
                    moved.push((*module).to_owned());
                    break;
                }
                Ok(false) => {}
                Err(error) => tracing::warn!(
                    module,
                    from = %legacy.display(),
                    %error,
                    "failed to migrate legacy state directory"
                ),
            }
        }
    }
    // 搬空了就把老的父目录也收掉(只删空目录)
    let _ = std::fs::remove_dir(LEGACY_TMP_BASE);
    moved
}

fn is_empty_or_missing(dir: &Path) -> bool {
    match std::fs::read_dir(dir) {
        Ok(mut entries) => entries.next().is_none(),
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
    }
}

fn migrate_dir(legacy: &Path, target: &Path) -> std::io::Result<bool> {
    if legacy == target || !legacy.is_dir() || !is_empty_or_missing(target) {
        return Ok(false);
    }
    if let Some(parent) = target.parent() {
        create_private_dir_all(parent)?;
    }
    if target.exists() {
        std::fs::remove_dir(target)?;
    }
    // 同一文件系统直接 rename;/tmp 是 tmpfs 等跨设备情况下退回复制后删除
    if std::fs::rename(legacy, target).is_err() {
        copy_dir_all(legacy, target)?;
        std::fs::remove_dir_all(legacy)?;
    }
    Ok(true)
}

fn create_private_dir_all(dir: &Path) -> std::io::Result<()> {
    if dir.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn copy_dir_all(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    std::fs::set_permissions(to, std::fs::metadata(from)?.permissions())?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let source = entry.path();
        let dest = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_dir_all(&source, &dest)?;
        } else if kind.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(std::fs::read_link(&source)?, &dest)?;
        } else {
            // fs::copy 连权限位一起复制(私钥等 0600 文件保持 0600)
            std::fs::copy(&source, &dest)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_moves_legacy_dir_and_keeps_permissions() {
        let root = tempfile::tempdir().expect("tempdir");
        let legacy = root.path().join("tmp/users");
        std::fs::create_dir_all(&legacy).expect("legacy");
        std::fs::write(legacy.join("users.json"), b"[]").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                legacy.join("users.json"),
                std::fs::Permissions::from_mode(0o600),
            )
            .expect("chmod");
        }
        let target = root.path().join("data/users");

        assert!(migrate_dir(&legacy, &target).expect("migrate"));
        assert!(!legacy.exists());
        assert_eq!(
            std::fs::read(target.join("users.json")).expect("read"),
            b"[]"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(target.join("users.json"))
                .expect("meta")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn migrate_never_overwrites_existing_data() {
        let root = tempfile::tempdir().expect("tempdir");
        let legacy = root.path().join("tmp/users");
        let target = root.path().join("data/users");
        std::fs::create_dir_all(&legacy).expect("legacy");
        std::fs::write(legacy.join("users.json"), b"old").expect("write old");
        std::fs::create_dir_all(&target).expect("target");
        std::fs::write(target.join("users.json"), b"new").expect("write new");

        assert!(!migrate_dir(&legacy, &target).expect("migrate"));
        assert_eq!(
            std::fs::read(target.join("users.json")).expect("read"),
            b"new"
        );
        assert!(legacy.exists(), "legacy kept when target already has data");
    }

    #[test]
    fn migrate_into_empty_target_dir() {
        let root = tempfile::tempdir().expect("tempdir");
        let legacy = root.path().join("tmp/cron");
        let target = root.path().join("data/cron");
        std::fs::create_dir_all(legacy.join("logs")).expect("legacy");
        std::fs::write(legacy.join("tasks.json"), b"[]").expect("write");
        std::fs::create_dir_all(&target).expect("empty target");

        assert!(migrate_dir(&legacy, &target).expect("migrate"));
        assert!(target.join("tasks.json").exists());
        assert!(target.join("logs").is_dir());
    }

    #[test]
    fn copy_dir_all_copies_nested_tree() {
        let root = tempfile::tempdir().expect("tempdir");
        let from = root.path().join("a");
        std::fs::create_dir_all(from.join("b/c")).expect("tree");
        std::fs::write(from.join("b/c/f.txt"), b"x").expect("write");
        copy_dir_all(&from, &root.path().join("z")).expect("copy");
        assert_eq!(
            std::fs::read(root.path().join("z/b/c/f.txt")).expect("read"),
            b"x"
        );
    }

    #[test]
    fn every_module_has_unique_name_and_env() {
        let mut names: Vec<_> = MODULES.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), MODULES.len());
    }
}
