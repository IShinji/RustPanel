//! 状态文件原子写助手。
//!
//! 全仓 JSON 状态文件此前各写各的 tmp + rename,含机密的几处(备份去向凭据、
//! 通知渠道 token、用户口令哈希、ACME 账号私钥、集群 node_secret)落盘时没有
//! 收紧权限,root 默认 umask 下会写成 0644 —— 本机任意用户/容器都能读走。
//! 这里统一成两个入口:普通状态用 [`write_atomic`],含机密的用
//! [`write_secret_atomic`](见 `AGENTS.md` 机密约束)。
//!
//! 权限在 rename **之前**打到 tmp 上,所以目标路径从不出现「先 0644 后 chmod」
//! 的宽权限窗口;rename 失败时清掉 tmp,不留垃圾。

use std::path::{Path, PathBuf};

/// tmp + rename 原子写普通状态文件。
pub async fn write_atomic(path: &Path, content: impl AsRef<[u8]>) -> std::io::Result<()> {
    write_inner(path, content.as_ref(), false).await
}

/// tmp + rename 原子写机密状态文件,落盘权限 0600。
pub async fn write_secret_atomic(path: &Path, content: impl AsRef<[u8]>) -> std::io::Result<()> {
    write_inner(path, content.as_ref(), true).await
}

async fn write_inner(path: &Path, content: &[u8], secret: bool) -> std::io::Result<()> {
    let tmp = tmp_path(path);
    tokio::fs::write(&tmp, content).await?;
    if secret {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(error) =
                tokio::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600)).await
            {
                let _ = tokio::fs::remove_file(&tmp).await;
                return Err(error);
            }
        }
    }
    match tokio::fs::rename(&tmp, path).await {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = tokio::fs::remove_file(&tmp).await;
            Err(error)
        }
    }
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmp_path_appends_suffix_without_eating_extension() {
        assert_eq!(
            tmp_path(Path::new("/var/rustpanel/state.json")),
            PathBuf::from("/var/rustpanel/state.json.tmp")
        );
    }

    #[tokio::test]
    async fn write_atomic_replaces_content_and_removes_tmp() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("state.json");
        write_atomic(&path, b"{\"a\":1}").await.expect("first");
        write_atomic(&path, b"{\"a\":2}").await.expect("second");

        assert_eq!(std::fs::read(&path).expect("read"), b"{\"a\":2}");
        assert!(!tmp_path(&path).exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn write_secret_atomic_lands_on_0600() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("secret.json");
        write_secret_atomic(&path, b"{\"token\":\"x\"}")
            .await
            .expect("write");

        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rewriting_secret_keeps_0600() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("secret.json");
        write_secret_atomic(&path, b"one").await.expect("first");
        write_secret_atomic(&path, b"two").await.expect("second");

        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(std::fs::read(&path).expect("read"), b"two");
    }
}
