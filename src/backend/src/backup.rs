use std::{
    env,
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use futures_util::StreamExt;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use tonic::{Request, Response as GrpcResponse, Status};
use uuid::Uuid;

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        backup_service_server::BackupService, BackupRecord, BackupSourceKind, BackupTarget,
        BackupTargetKind, CreateBackupRequest, CreateBackupResponse, DeleteBackupRequest,
        DeleteBackupResponse, DeleteBackupTargetRequest, DeleteBackupTargetResponse,
        ListBackupTargetsRequest, ListBackupTargetsResponse, ListBackupsRequest,
        ListBackupsResponse, RestoreBackupRequest, RestoreBackupResponse,
        UpsertBackupTargetRequest, UpsertBackupTargetResponse,
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

    /// 保留同一 source_path 的最新 keep 份,其余(本地 + 离站 + 记录)删除。
    async fn prune_to_keep(&self, source_path: &str, keep: u32) -> Result<(), Status> {
        let _guard = self.store.write_lock.lock().await;
        let mut state = self.store.load().await?;
        let mut same: Vec<&StoredRecord> = state
            .records
            .iter()
            .filter(|record| record.source_path == source_path)
            .collect();
        same.sort_by_key(|record| std::cmp::Reverse(record.created_at_seconds));
        let to_delete: Vec<String> = same
            .iter()
            .skip(keep as usize)
            .map(|record| record.id.clone())
            .collect();
        if to_delete.is_empty() {
            return Ok(());
        }
        for id in &to_delete {
            if let Some(record) = state.records.iter().find(|r| &r.id == id).cloned() {
                let archive_path = self.store.archive_path(&record.archive_name);
                let _ = tokio::fs::remove_file(&archive_path).await;
                if record.offsite_uploaded {
                    if let Some(target) = state.targets.iter().find(|t| t.id == record.target_id) {
                        if is_offsite_kind(target.kind) {
                            let _ = offsite_delete(target, &record.archive_name).await;
                        }
                    }
                }
            }
        }
        state
            .records
            .retain(|record| !to_delete.contains(&record.id));
        self.store.save(&state).await
    }
}

impl Default for BackupServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

/// 一次性备份(CLI / cron 定时调度用):跑一次 create_backup,再按 keep 保留同源最新 N 份。
pub async fn run_oneshot_backup(
    source_path: String,
    target_id: String,
    name: String,
    keep: u32,
) -> Result<(), String> {
    use crate::proto::rustpanel::v1::backup_service_server::BackupService;
    let service = BackupServiceImpl::new();
    let response = service
        .create_backup(Request::new(CreateBackupRequest {
            source_path: source_path.clone(),
            name,
            target_id,
            source_kind: BackupSourceKind::Directory as i32,
            source_dsn: String::new(),
        }))
        .await
        .map_err(|status| status.message().to_owned())?;
    if let Some(record) = response.into_inner().record {
        tracing::info!(
            target = "backup.oneshot",
            id = %record.id,
            size = record.size_bytes,
            offsite = record.offsite_uploaded,
            "oneshot backup created"
        );
    }
    if keep > 0 {
        service
            .prune_to_keep(&source_path, keep)
            .await
            .map_err(|status| status.message().to_owned())?;
    }
    Ok(())
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
        // UNSPECIFIED(旧请求)按目录处理;仅 DATABASE 走数据库分支。
        let is_database = BackupSourceKind::try_from(request.source_kind).ok()
            == Some(BackupSourceKind::Database);

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

        // 按来源确定"要打包的目录" + 记录里的 source_path + 默认显示名。
        // 数据库备份先把 dump 落到临时目录,再 tar 该目录(打完即删)。
        let mut tmp_dir: Option<PathBuf> = None;
        let (tar_source, record_source_path, display_base) = if is_database {
            let dsn = request.source_dsn.trim();
            if dsn.is_empty() {
                return Err(Status::invalid_argument(
                    "source_dsn is required for database backup",
                ));
            }
            let dir = self.store.root.join("tmp").join(&id);
            tokio::fs::create_dir_all(&dir).await.map_err(io_status)?;
            dump_database(dsn, &dir).await?;
            tmp_dir = Some(dir.clone());
            (dir, redact_dsn(dsn), database_name(dsn))
        } else {
            let source = PathBuf::from(request.source_path.trim());
            if source.as_os_str().is_empty() || !source.is_absolute() {
                return Err(Status::invalid_argument(
                    "source_path must be an absolute directory path",
                ));
            }
            if !tokio::fs::metadata(&source)
                .await
                .map_err(io_status)?
                .is_dir()
            {
                return Err(Status::invalid_argument("source_path must be a directory"));
            }
            let base = source
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "backup".to_owned());
            (source.clone(), source.to_string_lossy().to_string(), base)
        };

        let display_name = if request.name.trim().is_empty() {
            format!("{display_base}-{now}")
        } else {
            request.name.trim().to_owned()
        };

        // 打 tar.gz(spawn_blocking,避免阻塞 reactor)。
        let source_for_task = tar_source.clone();
        let archive_for_task = archive_path.clone();
        let tar_result = tokio::task::spawn_blocking(move || {
            create_tar_gz_blocking(&source_for_task, &archive_for_task)
        })
        .await
        .map_err(io_status)?;
        if let Some(dir) = &tmp_dir {
            let _ = tokio::fs::remove_dir_all(dir).await;
        }
        let size_bytes = tar_result.map_err(io_status)?;

        // 离站上传(WebDAV / S3)。失败不丢本地备份,只在 message 里告警。
        let mut offsite_uploaded = false;
        let mut warning = String::new();
        if let Some(target) = &target {
            if target.enabled && is_offsite_kind(target.kind) {
                match offsite_upload(target, &archive_name, &archive_path).await {
                    Ok(()) => offsite_uploaded = true,
                    Err(error) => warning = format!("(离站上传失败: {error})"),
                }
            }
        }

        let record = StoredRecord {
            id,
            name: display_name,
            source_path: record_source_path,
            target_id: request.target_id,
            archive_name,
            size_bytes,
            offsite_uploaded,
            created_at_seconds: now,
            source_kind: if is_database {
                BackupSourceKind::Database as i32
            } else {
                BackupSourceKind::Directory as i32
            },
        };
        state.records.push(record.clone());
        self.store.save(&state).await?;

        Ok(GrpcResponse::new(CreateBackupResponse {
            status: Some(ok_response(format!("backup created{warning}"))),
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

        // 数据库备份的 source_path 是脱敏 DSN(非路径),还原只能解出 dump,
        // 必须显式给目标目录,不自动应用回库(避免误覆盖)。
        if record.source_kind == BackupSourceKind::Database as i32
            && request.restore_path.trim().is_empty()
        {
            return Err(Status::invalid_argument(
                "数据库备份请指定还原目录(将 dump.sql 解出到该目录,再自行导入)",
            ));
        }
        let restore_path = if request.restore_path.trim().is_empty() {
            PathBuf::from(&record.source_path)
        } else {
            PathBuf::from(request.restore_path.trim())
        };
        if !restore_path.is_absolute() {
            return Err(Status::invalid_argument("restore_path must be absolute"));
        }

        // 本地归档不在(可能只在离站)→ 先从离站(WebDAV / S3)拉回来。
        let archive_path = self.store.archive_path(&record.archive_name);
        if !tokio::fs::try_exists(&archive_path).await.unwrap_or(false) {
            let target = state
                .targets
                .iter()
                .find(|t| t.id == record.target_id)
                .filter(|t| is_offsite_kind(t.kind))
                .ok_or_else(|| {
                    Status::failed_precondition("local archive missing and no offsite target")
                })?;
            offsite_download(target, &record.archive_name, &archive_path)
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
                if is_offsite_kind(target.kind) {
                    let _ = offsite_delete(target, &record.archive_name).await;
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
    // 流式上传:归档以 stream 喂给 body,不整档读进内存(低配主机备份几百 MB
    // 也不 OOM)。
    let file = tokio::fs::File::open(archive_path)
        .await
        .map_err(|error| error.to_string())?;
    let body = reqwest::Body::wrap_stream(ReaderStream::new(file));
    let client = reqwest::Client::new();
    let response = client
        .put(webdav_url(target, archive_name))
        .basic_auth(&target.username, Some(&target.password))
        .body(body)
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
    if let Some(parent) = archive_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| error.to_string())?;
    }
    // 流式落盘:逐块写,不把整个下载缓冲进内存。
    let mut file = tokio::fs::File::create(archive_path)
        .await
        .map_err(|error| error.to_string())?;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| error.to_string())?;
        file.write_all(&chunk)
            .await
            .map_err(|error| error.to_string())?;
    }
    file.flush().await.map_err(|error| error.to_string())
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

fn is_offsite_kind(kind: i32) -> bool {
    matches!(
        BackupTargetKind::try_from(kind).ok(),
        Some(BackupTargetKind::Webdav) | Some(BackupTargetKind::S3)
    )
}

async fn offsite_upload(target: &StoredTarget, name: &str, path: &Path) -> Result<(), String> {
    match BackupTargetKind::try_from(target.kind).unwrap_or(BackupTargetKind::Unspecified) {
        BackupTargetKind::Webdav => webdav_upload(target, name, path).await,
        BackupTargetKind::S3 => s3_upload(target, name, path).await,
        _ => Err("target is not an offsite kind".to_owned()),
    }
}

async fn offsite_download(target: &StoredTarget, name: &str, path: &Path) -> Result<(), String> {
    match BackupTargetKind::try_from(target.kind).unwrap_or(BackupTargetKind::Unspecified) {
        BackupTargetKind::Webdav => webdav_download(target, name, path).await,
        BackupTargetKind::S3 => s3_download(target, name, path).await,
        _ => Err("target is not an offsite kind".to_owned()),
    }
}

async fn offsite_delete(target: &StoredTarget, name: &str) -> Result<(), String> {
    match BackupTargetKind::try_from(target.kind).unwrap_or(BackupTargetKind::Unspecified) {
        BackupTargetKind::Webdav => webdav_delete(target, name).await,
        BackupTargetKind::S3 => s3_delete(target, name).await,
        _ => Err("target is not an offsite kind".to_owned()),
    }
}

// ===== S3 兼容(SigV4 + 路径风格 + 流式;UNSIGNED-PAYLOAD 免哈希整档,低配友好) =====

type HmacSha256 = Hmac<Sha256>;

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex_lower(&hasher.finalize())
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// RFC 3986 编码 S3 路径(encode_slash=false 时保留 '/')。
fn s3_uri_encode(input: &str, encode_slash: bool) -> String {
    let mut out = String::new();
    for &byte in input.as_bytes() {
        let ch = byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | '_' | '~') {
            out.push(ch);
        } else if ch == '/' && !encode_slash {
            out.push('/');
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

struct S3Signed {
    url: String,
    // 除 host 外要显式设的头(host 由 reqwest 按 URL 自动发,且已纳入签名)。
    headers: Vec<(String, String)>,
}

/// 计算单次 S3 请求的 SigV4 签名(路径风格 {endpoint}/{bucket}/{key})。
fn s3_sign(
    target: &StoredTarget,
    method: &str,
    key: &str,
    payload_hash: &str,
) -> Result<S3Signed, String> {
    let endpoint = target.endpoint.trim().trim_end_matches('/');
    if endpoint.is_empty() {
        return Err("s3 endpoint is empty".to_owned());
    }
    let region = if target.region.trim().is_empty() {
        "us-east-1"
    } else {
        target.region.trim()
    };
    let host = endpoint
        .strip_prefix("https://")
        .or_else(|| endpoint.strip_prefix("http://"))
        .unwrap_or(endpoint)
        .split('/')
        .next()
        .unwrap_or("")
        .to_owned();
    if host.is_empty() {
        return Err("s3 endpoint has no host".to_owned());
    }

    let object_path = format!("{}/{}", target.bucket.trim().trim_matches('/'), key);
    let encoded_path = s3_uri_encode(&object_path, false);
    let canonical_uri = format!("/{encoded_path}");
    let url = format!("{endpoint}/{encoded_path}");

    let now = chrono::Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();

    let canonical_headers =
        format!("host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n");
    let signed_headers = "host;x-amz-content-sha256;x-amz-date";
    let canonical_request = format!(
        "{method}\n{canonical_uri}\n\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
    );
    let scope = format!("{date_stamp}/{region}/s3/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );

    let k_date = hmac_sha256(
        format!("AWS4{}", target.password).as_bytes(),
        date_stamp.as_bytes(),
    );
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, b"s3");
    let k_signing = hmac_sha256(&k_service, b"aws4_request");
    let signature = hex_lower(&hmac_sha256(&k_signing, string_to_sign.as_bytes()));

    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
        target.username.trim()
    );
    Ok(S3Signed {
        url,
        headers: vec![
            ("x-amz-content-sha256".to_owned(), payload_hash.to_owned()),
            ("x-amz-date".to_owned(), amz_date),
            ("authorization".to_owned(), authorization),
        ],
    })
}

async fn s3_upload(target: &StoredTarget, key: &str, archive_path: &Path) -> Result<(), String> {
    let signed = s3_sign(target, "PUT", key, "UNSIGNED-PAYLOAD")?;
    let file = tokio::fs::File::open(archive_path)
        .await
        .map_err(|error| error.to_string())?;
    let body = reqwest::Body::wrap_stream(ReaderStream::new(file));
    let client = reqwest::Client::new();
    let mut request = client.put(&signed.url).body(body);
    for (name, value) in &signed.headers {
        request = request.header(name.as_str(), value.as_str());
    }
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
            .take(300)
            .collect();
        Err(format!("S3 PUT {status}: {detail}"))
    }
}

async fn s3_download(target: &StoredTarget, key: &str, archive_path: &Path) -> Result<(), String> {
    let signed = s3_sign(target, "GET", key, "UNSIGNED-PAYLOAD")?;
    let client = reqwest::Client::new();
    let mut request = client.get(&signed.url);
    for (name, value) in &signed.headers {
        request = request.header(name.as_str(), value.as_str());
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("S3 GET {status}"));
    }
    if let Some(parent) = archive_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| error.to_string())?;
    }
    let mut file = tokio::fs::File::create(archive_path)
        .await
        .map_err(|error| error.to_string())?;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| error.to_string())?;
        file.write_all(&chunk)
            .await
            .map_err(|error| error.to_string())?;
    }
    file.flush().await.map_err(|error| error.to_string())
}

async fn s3_delete(target: &StoredTarget, key: &str) -> Result<(), String> {
    let signed = s3_sign(target, "DELETE", key, "UNSIGNED-PAYLOAD")?;
    let client = reqwest::Client::new();
    let mut request = client.delete(&signed.url);
    for (name, value) in &signed.headers {
        request = request.header(name.as_str(), value.as_str());
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status();
    if status.is_success() {
        Ok(())
    } else {
        Err(format!("S3 DELETE {status}"))
    }
}

struct SqlDsn {
    user: String,
    pass: String,
    host: String,
    port: String,
    db: String,
}

/// 解析 mysql:// / postgres:// DSN 为各部件(v1:不做 %xx 解码)。
fn parse_sql_dsn(dsn: &str) -> Option<SqlDsn> {
    let rest = dsn.split_once("://")?.1;
    let (userinfo, hostpart) = rest.split_once('@')?;
    let (user, pass) = userinfo.split_once(':').unwrap_or((userinfo, ""));
    let (hostport, dbpart) = hostpart.split_once('/').unwrap_or((hostpart, ""));
    let db = dbpart.split(['?', '&']).next().unwrap_or("").to_owned();
    let (host, port) = hostport.split_once(':').unwrap_or((hostport, ""));
    if host.is_empty() || db.is_empty() {
        return None;
    }
    Some(SqlDsn {
        user: user.to_owned(),
        pass: pass.to_owned(),
        host: host.to_owned(),
        port: port.to_owned(),
        db,
    })
}

fn sqlite_path(dsn: &str) -> Option<PathBuf> {
    let rest = dsn.strip_prefix("sqlite:")?;
    let rest = rest.strip_prefix("//").unwrap_or(rest);
    let path = rest.split('?').next().unwrap_or("");
    if path.is_empty() || path == ":memory:" {
        return None;
    }
    Some(PathBuf::from(path))
}

/// 抹掉 DSN 里的密码用于展示/记录。
fn redact_dsn(dsn: &str) -> String {
    if let Some((scheme, rest)) = dsn.split_once("://") {
        if let Some((userinfo, hostpart)) = rest.split_once('@') {
            let user = userinfo.split_once(':').map(|(u, _)| u).unwrap_or(userinfo);
            return format!("{scheme}://{user}:***@{hostpart}");
        }
    }
    dsn.to_owned()
}

fn database_name(dsn: &str) -> String {
    parse_sql_dsn(dsn)
        .map(|parts| parts.db)
        .filter(|db| !db.is_empty())
        .or_else(|| {
            sqlite_path(dsn).and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().to_string())
            })
        })
        .unwrap_or_else(|| "db".to_owned())
}

/// 数据库逻辑备份:mysqldump / pg_dump 落 dump.sql,或 SQLite 拷文件。工具缺失给清晰报错。
async fn dump_database(dsn: &str, out_dir: &Path) -> Result<(), Status> {
    if dsn.starts_with("mysql://") {
        let parts =
            parse_sql_dsn(dsn).ok_or_else(|| Status::invalid_argument("invalid mysql dsn"))?;
        let out = out_dir.join("dump.sql");
        let mut command = tokio::process::Command::new("mysqldump");
        command
            .env("MYSQL_PWD", &parts.pass)
            .arg("-h")
            .arg(&parts.host)
            .arg("-u")
            .arg(&parts.user)
            .arg(format!("--result-file={}", out.display()));
        if !parts.port.is_empty() {
            command.arg("-P").arg(&parts.port);
        }
        command.arg(&parts.db);
        run_dump(command, "mysqldump").await
    } else if dsn.starts_with("postgres://") || dsn.starts_with("postgresql://") {
        let parts =
            parse_sql_dsn(dsn).ok_or_else(|| Status::invalid_argument("invalid postgres dsn"))?;
        let out = out_dir.join("dump.sql");
        let mut command = tokio::process::Command::new("pg_dump");
        command
            .env("PGPASSWORD", &parts.pass)
            .arg("-h")
            .arg(&parts.host)
            .arg("-U")
            .arg(&parts.user)
            .arg("-f")
            .arg(&out)
            .arg("-d")
            .arg(&parts.db);
        if !parts.port.is_empty() {
            command.arg("-p").arg(&parts.port);
        }
        run_dump(command, "pg_dump").await
    } else if dsn.starts_with("sqlite:") {
        let path = sqlite_path(dsn).ok_or_else(|| {
            Status::invalid_argument("invalid sqlite dsn (:memory: not supported)")
        })?;
        tokio::fs::copy(&path, out_dir.join("dump.sqlite"))
            .await
            .map_err(io_status)?;
        Ok(())
    } else {
        Err(Status::invalid_argument(
            "dsn must start with mysql:// / postgres:// / sqlite:",
        ))
    }
}

async fn run_dump(mut command: tokio::process::Command, tool: &str) -> Result<(), Status> {
    let output = match command.output().await {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(Status::failed_precondition(format!(
                "{tool} 未安装,无法做数据库逻辑备份(可改用目录/卷备份)"
            )));
        }
        Err(error) => return Err(io_status(error)),
    };
    if output.status.success() {
        Ok(())
    } else {
        let detail: String = String::from_utf8_lossy(&output.stderr)
            .trim()
            .chars()
            .take(300)
            .collect();
        Err(Status::internal(format!("{tool}: {detail}")))
    }
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
        BackupTargetKind::S3 => {
            if target.endpoint.trim().is_empty()
                || target.bucket.trim().is_empty()
                || target.region.trim().is_empty()
                || target.username.trim().is_empty()
            {
                Err(Status::invalid_argument(
                    "s3 target needs endpoint / bucket / region / access key",
                ))
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
    #[serde(default)]
    region: String,
    #[serde(default)]
    bucket: String,
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
            region: target.region,
            bucket: target.bucket,
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
            region: self.region,
            bucket: self.bucket,
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
    #[serde(default)]
    source_kind: i32,
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
            source_kind: self.source_kind,
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
            region: String::new(),
            bucket: String::new(),
        };
        assert_eq!(
            webdav_url(&target, "x.tar.gz"),
            "https://dav.example.com/backups/x.tar.gz"
        );
    }

    #[test]
    fn sha256_hex_of_empty_matches_known_vector() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sigv4_signing_key_chain_is_stable() {
        // SigV4 派生链(kDate→kRegion→kService→kSigning)的确定性/回归保护:
        // 哈希原语已由 sha256_hex 空串向量独立验证,HMAC 用标准 hmac crate(与
        // TOTP 同),此处固定 (secret, 20120215, us-east-1, iam) 的派生结果防链路被
        // 误改;真实端到端正确性以 MinIO / R2 实测为终检。
        let secret = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), b"20120215");
        let k_region = hmac_sha256(&k_date, b"us-east-1");
        let k_service = hmac_sha256(&k_region, b"iam");
        let k_signing = hmac_sha256(&k_service, b"aws4_request");
        assert_eq!(
            hex_lower(&k_signing),
            "004aa806e13dae88b9032d9261bcb04c67d023afadd221e6b0d206e1760e0b5e"
        );
    }

    #[test]
    fn s3_uri_encode_keeps_or_encodes_slash() {
        assert_eq!(s3_uri_encode("a/b c", false), "a/b%20c");
        assert_eq!(s3_uri_encode("a/b", true), "a%2Fb");
    }

    #[test]
    fn parses_and_redacts_sql_dsn() {
        let parsed = parse_sql_dsn("mysql://app:secret@db.local:3306/shop?x=1").expect("dsn");
        assert_eq!(parsed.user, "app");
        assert_eq!(parsed.pass, "secret");
        assert_eq!(parsed.host, "db.local");
        assert_eq!(parsed.port, "3306");
        assert_eq!(parsed.db, "shop");
        assert_eq!(
            redact_dsn("mysql://app:secret@db.local:3306/shop"),
            "mysql://app:***@db.local:3306/shop"
        );
        assert_eq!(database_name("postgres://u:p@h/orders"), "orders");
    }

    #[test]
    fn sqlite_path_rejects_memory() {
        assert_eq!(
            sqlite_path("sqlite:/var/lib/app.db"),
            Some(PathBuf::from("/var/lib/app.db"))
        );
        assert!(sqlite_path("sqlite::memory:").is_none());
    }
}
