use std::{
    env,
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use serde::{Deserialize, Serialize};
use tonic::{Request, Response as GrpcResponse, Status};
use uuid::Uuid;

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        backup_service_server::BackupService, BackupRecord, BackupTarget, BackupTargetKind,
        CreateBackupRequest, CreateBackupResponse, DeleteBackupRequest, DeleteBackupResponse,
        DeleteBackupTargetRequest, DeleteBackupTargetResponse, ListBackupTargetsRequest,
        ListBackupTargetsResponse, ListBackupsRequest, ListBackupsResponse, RestoreBackupRequest,
        RestoreBackupResponse, UpsertBackupTargetRequest, UpsertBackupTargetResponse,
    },
};

const DEFAULT_BACKUP_ROOT: &str = "/tmp/rustpanel/backup";
const SECRET_REDACTED: &str = "__rustpanel_secret_kept__";

#[derive(Clone)]
pub struct BackupServiceImpl {
    store: BackupStore,
}

impl BackupServiceImpl {
    pub fn new() -> Self {
        Self {
            store: BackupStore::from_env(),
        }
    }
}

impl Default for BackupServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl BackupService for BackupServiceImpl {
    async fn list_backup_targets(
        &self,
        _request: Request<ListBackupTargetsRequest>,
    ) -> Result<GrpcResponse<ListBackupTargetsResponse>, Status> {
        let state = self.store.load().await?;
        Ok(GrpcResponse::new(ListBackupTargetsResponse {
            status: Some(ok_response("ok")),
            targets: state
                .targets
                .into_iter()
                .map(|target| redact(target.into_proto()))
                .collect(),
        }))
    }

    async fn upsert_backup_target(
        &self,
        request: Request<UpsertBackupTargetRequest>,
    ) -> Result<GrpcResponse<UpsertBackupTargetResponse>, Status> {
        let incoming = request
            .into_inner()
            .target
            .ok_or_else(|| Status::invalid_argument("backup target is required"))?;
        validate_target(&incoming)?;

        let _guard = self.store.write_lock.lock().await;
        let mut state = self.store.load().await?;
        let now = current_timestamp();
        let existing = state.targets.iter().find(|t| t.id == incoming.id).cloned();

        let mut stored = StoredTarget::from_proto(incoming);
        if stored.id.trim().is_empty() {
            stored.id = Uuid::new_v4().to_string();
            stored.created_at_seconds = now;
        } else if let Some(old) = &existing {
            stored.created_at_seconds = old.created_at_seconds;
            if stored.password.trim().is_empty() || stored.password == SECRET_REDACTED {
                stored.password = old.password.clone();
            }
        } else {
            stored.created_at_seconds = now;
        }

        state.targets.retain(|t| t.id != stored.id);
        state.targets.push(stored.clone());
        self.store.save(&state).await?;

        Ok(GrpcResponse::new(UpsertBackupTargetResponse {
            status: Some(ok_response("backup target saved")),
            target: Some(redact(stored.into_proto())),
        }))
    }

    async fn delete_backup_target(
        &self,
        request: Request<DeleteBackupTargetRequest>,
    ) -> Result<GrpcResponse<DeleteBackupTargetResponse>, Status> {
        let id = request.into_inner().id;
        let _guard = self.store.write_lock.lock().await;
        let mut state = self.store.load().await?;
        let before = state.targets.len();
        state.targets.retain(|t| t.id != id);
        if state.targets.len() == before {
            return Err(Status::not_found("backup target not found"));
        }
        self.store.save(&state).await?;
        Ok(GrpcResponse::new(DeleteBackupTargetResponse {
            status: Some(ok_response("backup target deleted")),
        }))
    }

    async fn create_backup(
        &self,
        request: Request<CreateBackupRequest>,
    ) -> Result<GrpcResponse<CreateBackupResponse>, Status> {
        let request = request.into_inner();
        let source = PathBuf::from(request.source_path.trim());
        if source.as_os_str().is_empty() || !source.is_absolute() {
            return Err(Status::invalid_argument(
                "source_path must be an absolute directory path",
            ));
        }
        let metadata = tokio::fs::metadata(&source).await.map_err(io_status)?;
        if !metadata.is_dir() {
            return Err(Status::invalid_argument("source_path must be a directory"));
        }

        let _guard = self.store.write_lock.lock().await;
        let mut state = self.store.load().await?;

        // 解析去向目标(留空 = 仅本地)。
        let target = if request.target_id.trim().is_empty() {
            None
        } else {
            Some(
                state
                    .targets
                    .iter()
                    .find(|t| t.id == request.target_id)
                    .cloned()
                    .ok_or_else(|| Status::not_found("backup target not found"))?,
            )
        };

        let id = Uuid::new_v4().to_string();
        let archive_name = format!("{id}.tar.gz");
        let archive_path = self.store.archive_path(&archive_name);
        let now = current_timestamp();
        let display_name = if request.name.trim().is_empty() {
            let base = source
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "backup".to_owned());
            format!("{base}-{now}")
        } else {
            request.name.trim().to_owned()
        };

        // 打 tar.gz(spawn_blocking,避免阻塞 reactor)。
        let source_for_task = source.clone();
        let archive_for_task = archive_path.clone();
        let size_bytes = tokio::task::spawn_blocking(move || {
            create_tar_gz_blocking(&source_for_task, &archive_for_task)
        })
        .await
        .map_err(io_status)?
        .map_err(io_status)?;

        // 离站上传(WebDAV)。失败不丢本地备份,只在 message 里告警。
        let mut offsite_uploaded = false;
        let mut warning = String::new();
        if let Some(target) = &target {
            if target.enabled
                && BackupTargetKind::try_from(target.kind).ok() == Some(BackupTargetKind::Webdav)
            {
                match webdav_upload(target, &archive_name, &archive_path).await {
                    Ok(()) => offsite_uploaded = true,
                    Err(error) => warning = format!("(离站上传失败: {error})"),
                }
            }
        }

        let record = StoredRecord {
            id,
            name: display_name,
            source_path: source.to_string_lossy().to_string(),
            target_id: request.target_id,
            archive_name,
            size_bytes,
            offsite_uploaded,
            created_at_seconds: now,
        };
        state.records.push(record.clone());
        self.store.save(&state).await?;

        Ok(GrpcResponse::new(CreateBackupResponse {
            status: Some(ok_response(&format!("backup created{warning}"))),
            record: Some(record.into_proto()),
        }))
    }

    async fn list_backups(
        &self,
        _request: Request<ListBackupsRequest>,
    ) -> Result<GrpcResponse<ListBackupsResponse>, Status> {
        let mut records = self.store.load().await?.records;
        records.sort_by_key(|record| std::cmp::Reverse(record.created_at_seconds));
        Ok(GrpcResponse::new(ListBackupsResponse {
            status: Some(ok_response("ok")),
            records: records.into_iter().map(StoredRecord::into_proto).collect(),
        }))
    }

    async fn restore_backup(
        &self,
        request: Request<RestoreBackupRequest>,
    ) -> Result<GrpcResponse<RestoreBackupResponse>, Status> {
        let request = request.into_inner();
        let _guard = self.store.write_lock.lock().await;
        let state = self.store.load().await?;
        let record = state
            .records
            .iter()
            .find(|r| r.id == request.id)
            .cloned()
            .ok_or_else(|| Status::not_found("backup not found"))?;

        let restore_path = if request.restore_path.trim().is_empty() {
            PathBuf::from(&record.source_path)
        } else {
            PathBuf::from(request.restore_path.trim())
        };
        if !restore_path.is_absolute() {
            return Err(Status::invalid_argument("restore_path must be absolute"));
        }

        // 本地归档不在(可能只在离站)→ 先从 WebDAV 拉回来。
        let archive_path = self.store.archive_path(&record.archive_name);
        if !tokio::fs::try_exists(&archive_path).await.unwrap_or(false) {
            let target = state
                .targets
                .iter()
                .find(|t| t.id == record.target_id)
                .filter(|t| {
                    BackupTargetKind::try_from(t.kind).ok() == Some(BackupTargetKind::Webdav)
                })
                .ok_or_else(|| {
                    Status::failed_precondition("local archive missing and no offsite target")
                })?;
            webdav_download(target, &record.archive_name, &archive_path)
                .await
                .map_err(Status::internal)?;
        }

        let archive_for_task = archive_path.clone();
        let dest_for_task = restore_path.clone();
        tokio::task::spawn_blocking(move || {
            extract_tar_gz_blocking(&archive_for_task, &dest_for_task)
        })
        .await
        .map_err(io_status)?
        .map_err(io_status)?;

        Ok(GrpcResponse::new(RestoreBackupResponse {
            status: Some(ok_response("backup restored")),
            restored_path: restore_path.to_string_lossy().to_string(),
        }))
    }

    async fn delete_backup(
        &self,
        request: Request<DeleteBackupRequest>,
    ) -> Result<GrpcResponse<DeleteBackupResponse>, Status> {
        let id = request.into_inner().id;
        let _guard = self.store.write_lock.lock().await;
        let mut state = self.store.load().await?;
        let record = state
            .records
            .iter()
            .find(|r| r.id == id)
            .cloned()
            .ok_or_else(|| Status::not_found("backup not found"))?;

        // 删本地归档(不存在也无妨)。
        let archive_path = self.store.archive_path(&record.archive_name);
        let _ = tokio::fs::remove_file(&archive_path).await;

        // 离站副本 best-effort 删除。
        if record.offsite_uploaded {
            if let Some(target) = state.targets.iter().find(|t| t.id == record.target_id) {
                if BackupTargetKind::try_from(target.kind).ok() == Some(BackupTargetKind::Webdav) {
                    let _ = webdav_delete(target, &record.archive_name).await;
                }
            }
        }

        state.records.retain(|r| r.id != id);
        self.store.save(&state).await?;
        Ok(GrpcResponse::new(DeleteBackupResponse {
            status: Some(ok_response("backup deleted")),
        }))
    }
}

fn create_tar_gz_blocking(source: &Path, archive: &Path) -> std::io::Result<u64> {
    if let Some(parent) = archive.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = File::create(archive)?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut builder = tar::Builder::new(encoder);
    // 以 "." 为前缀打包目录内容,还原时直接解到目标目录即可重建。
    builder.append_dir_all(".", source)?;
    let encoder = builder.into_inner()?;
    encoder.finish()?;
    Ok(std::fs::metadata(archive)?.len())
}

fn extract_tar_gz_blocking(archive: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    let file = File::open(archive)?;
    let decoder = GzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);
    // unpack_in 会过滤 ".." / 绝对路径(返回 false 跳过),防 tar-slip。
    for entry in tar.entries()? {
        let mut entry = entry?;
        entry.unpack_in(dest)?;
    }
    Ok(())
}

fn webdav_url(target: &StoredTarget, archive_name: &str) -> String {
    format!("{}/{archive_name}", target.endpoint.trim_end_matches('/'))
}

async fn webdav_upload(
    target: &StoredTarget,
    archive_name: &str,
    archive_path: &Path,
) -> Result<(), String> {
    let bytes = tokio::fs::read(archive_path)
        .await
        .map_err(|error| error.to_string())?;
    let client = reqwest::Client::new();
    let response = client
        .put(webdav_url(target, archive_name))
        .basic_auth(&target.username, Some(&target.password))
        .body(bytes)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {}", response.status()))
    }
}

async fn webdav_download(
    target: &StoredTarget,
    archive_name: &str,
    archive_path: &Path,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let response = client
        .get(webdav_url(target, archive_name))
        .basic_auth(&target.username, Some(&target.password))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let bytes = response.bytes().await.map_err(|error| error.to_string())?;
    if let Some(parent) = archive_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| error.to_string())?;
    }
    tokio::fs::write(archive_path, &bytes)
        .await
        .map_err(|error| error.to_string())
}

async fn webdav_delete(target: &StoredTarget, archive_name: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    client
        .delete(webdav_url(target, archive_name))
        .basic_auth(&target.username, Some(&target.password))
        .send()
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn validate_target(target: &BackupTarget) -> Result<(), Status> {
    if target.name.trim().is_empty() {
        return Err(Status::invalid_argument("target name is required"));
    }
    let kind = BackupTargetKind::try_from(target.kind).unwrap_or(BackupTargetKind::Unspecified);
    match kind {
        BackupTargetKind::Local => Ok(()),
        BackupTargetKind::Webdav => {
            if target.endpoint.trim().is_empty() {
                Err(Status::invalid_argument("webdav endpoint is required"))
            } else {
                Ok(())
            }
        }
        BackupTargetKind::Unspecified => Err(Status::invalid_argument("target kind is required")),
    }
}

fn redact(mut target: BackupTarget) -> BackupTarget {
    if !target.password.is_empty() {
        target.password = SECRET_REDACTED.to_owned();
    }
    target
}

#[derive(Clone, Debug)]
struct BackupStore {
    root: Arc<PathBuf>,
    write_lock: Arc<tokio::sync::Mutex<()>>,
}

impl BackupStore {
    fn from_env() -> Self {
        let root = env::var("RUSTPANEL_BACKUP_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_BACKUP_ROOT));
        Self {
            root: Arc::new(root),
            write_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    fn archive_path(&self, archive_name: &str) -> PathBuf {
        self.root.join("archives").join(archive_name)
    }

    fn state_path(&self) -> PathBuf {
        self.root.join("state.json")
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
        let path = self.state_path();
        let tmp = path.with_extension("json.tmp");
        tokio::fs::write(&tmp, content).await.map_err(io_status)?;
        tokio::fs::rename(&tmp, &path).await.map_err(io_status)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct StoredState {
    #[serde(default)]
    targets: Vec<StoredTarget>,
    #[serde(default)]
    records: Vec<StoredRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredTarget {
    id: String,
    name: String,
    kind: i32,
    endpoint: String,
    username: String,
    password: String,
    enabled: bool,
    created_at_seconds: u64,
}

impl StoredTarget {
    fn from_proto(target: BackupTarget) -> Self {
        Self {
            id: target.id,
            name: target.name,
            kind: target.kind,
            endpoint: target.endpoint,
            username: target.username,
            password: target.password,
            enabled: target.enabled,
            created_at_seconds: target.created_at_seconds,
        }
    }

    fn into_proto(self) -> BackupTarget {
        BackupTarget {
            id: self.id,
            name: self.name,
            kind: self.kind,
            endpoint: self.endpoint,
            username: self.username,
            password: self.password,
            enabled: self.enabled,
            created_at_seconds: self.created_at_seconds,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredRecord {
    id: String,
    name: String,
    source_path: String,
    target_id: String,
    archive_name: String,
    size_bytes: u64,
    #[serde(default)]
    offsite_uploaded: bool,
    created_at_seconds: u64,
}

impl StoredRecord {
    fn into_proto(self) -> BackupRecord {
        BackupRecord {
            id: self.id,
            name: self.name,
            source_path: self.source_path,
            target_id: self.target_id,
            archive_name: self.archive_name,
            size_bytes: self.size_bytes,
            offsite_uploaded: self.offsite_uploaded,
            created_at_seconds: self.created_at_seconds,
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
    fn tar_gz_roundtrip_preserves_files() {
        let base = env::temp_dir().join(format!("rustpanel-backup-test-{}", Uuid::new_v4()));
        let source = base.join("src");
        let archive = base.join("backup.tar.gz");
        let restore = base.join("restore");
        std::fs::create_dir_all(source.join("nested")).expect("mkdir");
        std::fs::write(source.join("a.txt"), b"alpha").expect("a");
        std::fs::write(source.join("nested/b.txt"), b"beta").expect("b");

        let size = create_tar_gz_blocking(&source, &archive).expect("create");
        assert!(size > 0);
        extract_tar_gz_blocking(&archive, &restore).expect("extract");

        assert_eq!(std::fs::read(restore.join("a.txt")).expect("a"), b"alpha");
        assert_eq!(
            std::fs::read(restore.join("nested/b.txt")).expect("b"),
            b"beta"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn validate_target_checks_kind_and_endpoint() {
        let mut target = BackupTarget {
            name: "nas".to_owned(),
            kind: BackupTargetKind::Webdav as i32,
            endpoint: "https://dav.example.com/backups".to_owned(),
            ..Default::default()
        };
        assert!(validate_target(&target).is_ok());

        target.endpoint = String::new();
        assert!(validate_target(&target).is_err());

        target.kind = BackupTargetKind::Local as i32;
        assert!(validate_target(&target).is_ok());

        target.kind = BackupTargetKind::Unspecified as i32;
        assert!(validate_target(&target).is_err());
    }

    #[test]
    fn redact_hides_password() {
        let target = BackupTarget {
            password: "hunter2".to_owned(),
            ..Default::default()
        };
        assert_eq!(redact(target).password, SECRET_REDACTED);
    }

    #[test]
    fn webdav_url_joins_endpoint_and_name() {
        let target = StoredTarget {
            id: String::new(),
            name: String::new(),
            kind: BackupTargetKind::Webdav as i32,
            endpoint: "https://dav.example.com/backups/".to_owned(),
            username: String::new(),
            password: String::new(),
            enabled: true,
            created_at_seconds: 0,
        };
        assert_eq!(
            webdav_url(&target, "x.tar.gz"),
            "https://dav.example.com/backups/x.tar.gz"
        );
    }
}
