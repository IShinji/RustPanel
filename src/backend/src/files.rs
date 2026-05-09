use std::{
    collections::HashMap,
    env,
    fs::File,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    pin::Pin,
    sync::{Arc, Mutex},
};

use flate2::{write::GzEncoder, Compression};
use futures_core::Stream;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tar::Builder as TarBuilder;
use tokio::io::AsyncWriteExt;
use tokio::sync::broadcast;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tonic::{Request, Response as GrpcResponse, Status};
use uuid::Uuid;
use walkdir::WalkDir;
use zip::{write::SimpleFileOptions, ZipWriter};

use crate::{
    audit, ok_response,
    proto::rustpanel::v1::{
        file_system_service_server::FileSystemService, ArchiveFormat, ArchiveTaskState,
        ChmodRequest, ChmodResponse, ChownRequest, ChownResponse, CreateArchiveRequest,
        CreateArchiveResponse, CreateDirectoryRequest, CreateDirectoryResponse, DeletePathRequest,
        DeletePathResponse, EmptyRecycleBinRequest, EmptyRecycleBinResponse, FileItem, FileKind,
        ListDirectoryRequest, ListDirectoryResponse, ListRecycleBinRequest, ListRecycleBinResponse,
        MovePathRequest, MovePathResponse, ReadFileRequest, ReadFileResponse, RecycleBinItem,
        RestoreRecycleItemRequest, RestoreRecycleItemResponse, SaveFileRequest, SaveFileResponse,
        SearchFilesRequest, SearchFilesResponse, SearchMatch, WatchArchiveProgressRequest,
        WatchArchiveProgressResponse,
    },
};

const ARCHIVE_CHANNEL_SIZE: usize = 16;
const DEFAULT_FILE_ROOT: &str = "/";
const DEFAULT_FILE_STATE_ROOT: &str = "/tmp/rustpanel/files";

#[derive(Clone)]
pub struct FileSystemServiceImpl {
    manager: FileManager,
    archives: ArchiveManager,
}

impl FileSystemServiceImpl {
    pub fn new() -> Self {
        let manager = FileManager::from_env();
        start_integrity_monitor(manager.clone());
        Self {
            manager,
            archives: ArchiveManager::default(),
        }
    }

    pub fn manager(&self) -> FileManager {
        self.manager.clone()
    }
}

impl Default for FileSystemServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

fn start_integrity_monitor(manager: FileManager) {
    let Ok(paths) = env::var("RUSTPANEL_PROTECTED_PATHS") else {
        return;
    };
    let protected_paths = paths
        .split(',')
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if protected_paths.is_empty() || tokio::runtime::Handle::try_current().is_err() {
        return;
    }

    tokio::spawn(async move {
        let mut last_modified = HashMap::<String, u64>::new();
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            for path in &protected_paths {
                let Ok(resolved) = manager.resolve_existing(path) else {
                    continue;
                };
                let Ok(metadata) = tokio::fs::metadata(&resolved).await else {
                    continue;
                };
                let modified = metadata
                    .modified()
                    .ok()
                    .and_then(|modified| modified.elapsed().ok())
                    .map(|elapsed| current_timestamp().saturating_sub(elapsed.as_secs()))
                    .unwrap_or_default();
                if let Some(previous) = last_modified.insert(path.clone(), modified) {
                    if previous != modified {
                        let _ = manager.audit("integrity_change", path).await;
                    }
                }
            }
        }
    });
}

#[tonic::async_trait]
impl FileSystemService for FileSystemServiceImpl {
    type WatchArchiveProgressStream =
        Pin<Box<dyn Stream<Item = Result<WatchArchiveProgressResponse, Status>> + Send>>;

    async fn list_directory(
        &self,
        request: Request<ListDirectoryRequest>,
    ) -> Result<GrpcResponse<ListDirectoryResponse>, Status> {
        let request = request.into_inner();
        let items = self
            .manager
            .list_directory(&request.path, request.recursive)?;

        Ok(GrpcResponse::new(ListDirectoryResponse {
            status: Some(ok_response("ok")),
            items,
        }))
    }

    async fn create_directory(
        &self,
        request: Request<CreateDirectoryRequest>,
    ) -> Result<GrpcResponse<CreateDirectoryResponse>, Status> {
        let path = self.manager.resolve_for_write(&request.into_inner().path)?;
        tokio::fs::create_dir_all(path).await.map_err(io_status)?;

        Ok(GrpcResponse::new(CreateDirectoryResponse {
            status: Some(ok_response("directory created")),
        }))
    }

    async fn move_path(
        &self,
        request: Request<MovePathRequest>,
    ) -> Result<GrpcResponse<MovePathResponse>, Status> {
        let request = request.into_inner();
        let source = self.manager.resolve_existing(&request.source_path)?;
        let target = self.manager.resolve_for_write(&request.target_path)?;
        tokio::fs::rename(source, target).await.map_err(io_status)?;
        self.manager
            .audit(
                "move",
                &format!("{} -> {}", request.source_path, request.target_path),
            )
            .await?;

        Ok(GrpcResponse::new(MovePathResponse {
            status: Some(ok_response("path moved")),
        }))
    }

    async fn delete_path(
        &self,
        request: Request<DeletePathRequest>,
    ) -> Result<GrpcResponse<DeletePathResponse>, Status> {
        let request = request.into_inner();
        let path = self.manager.resolve_existing(&request.path)?;
        if request.permanent {
            let metadata = tokio::fs::metadata(&path).await.map_err(io_status)?;
            if metadata.is_dir() {
                if request.recursive {
                    tokio::fs::remove_dir_all(&path).await.map_err(io_status)?;
                } else {
                    tokio::fs::remove_dir(&path).await.map_err(io_status)?;
                }
            } else {
                tokio::fs::remove_file(&path).await.map_err(io_status)?;
            }
            self.manager
                .audit("delete_permanent", &request.path)
                .await?;
        } else {
            self.manager.move_to_recycle_bin(&path).await?;
            self.manager.audit("delete_recycle", &request.path).await?;
        }

        Ok(GrpcResponse::new(DeletePathResponse {
            status: Some(ok_response("path deleted")),
        }))
    }

    async fn list_recycle_bin(
        &self,
        _request: Request<ListRecycleBinRequest>,
    ) -> Result<GrpcResponse<ListRecycleBinResponse>, Status> {
        Ok(GrpcResponse::new(ListRecycleBinResponse {
            status: Some(ok_response("ok")),
            items: self.manager.recycle_items().await?,
        }))
    }

    async fn restore_recycle_item(
        &self,
        request: Request<RestoreRecycleItemRequest>,
    ) -> Result<GrpcResponse<RestoreRecycleItemResponse>, Status> {
        self.manager
            .restore_recycle_item(&request.into_inner().id)
            .await?;

        Ok(GrpcResponse::new(RestoreRecycleItemResponse {
            status: Some(ok_response("recycle item restored")),
        }))
    }

    async fn empty_recycle_bin(
        &self,
        _request: Request<EmptyRecycleBinRequest>,
    ) -> Result<GrpcResponse<EmptyRecycleBinResponse>, Status> {
        self.manager.empty_recycle_bin().await?;

        Ok(GrpcResponse::new(EmptyRecycleBinResponse {
            status: Some(ok_response("recycle bin emptied")),
        }))
    }

    async fn chmod(
        &self,
        request: Request<ChmodRequest>,
    ) -> Result<GrpcResponse<ChmodResponse>, Status> {
        let request = request.into_inner();
        let path = self.manager.resolve_existing(&request.path)?;
        chmod_path(&path, request.mode).await?;
        self.manager.audit("chmod", &request.path).await?;

        Ok(GrpcResponse::new(ChmodResponse {
            status: Some(ok_response("permissions updated")),
        }))
    }

    async fn chown(
        &self,
        request: Request<ChownRequest>,
    ) -> Result<GrpcResponse<ChownResponse>, Status> {
        let request = request.into_inner();
        let path = self.manager.resolve_existing(&request.path)?;
        chown_path(&path, &request.owner, &request.group).await?;
        self.manager.audit("chown", &request.path).await?;

        Ok(GrpcResponse::new(ChownResponse {
            status: Some(ok_response("ownership updated")),
        }))
    }

    async fn read_file(
        &self,
        request: Request<ReadFileRequest>,
    ) -> Result<GrpcResponse<ReadFileResponse>, Status> {
        let path = self.manager.resolve_existing(&request.into_inner().path)?;
        let content = tokio::fs::read(path).await.map_err(io_status)?;

        Ok(GrpcResponse::new(ReadFileResponse {
            status: Some(ok_response("ok")),
            content,
        }))
    }

    async fn save_file(
        &self,
        request: Request<SaveFileRequest>,
    ) -> Result<GrpcResponse<SaveFileResponse>, Status> {
        let request = request.into_inner();
        let path = self.manager.resolve_for_write(&request.path)?;
        tokio::fs::write(&path, request.content)
            .await
            .map_err(io_status)?;
        self.manager.audit("save", &request.path).await?;

        Ok(GrpcResponse::new(SaveFileResponse {
            status: Some(ok_response("file saved")),
        }))
    }

    async fn search_files(
        &self,
        request: Request<SearchFilesRequest>,
    ) -> Result<GrpcResponse<SearchFilesResponse>, Status> {
        let matches = self.manager.search_files(request.into_inner())?;

        Ok(GrpcResponse::new(SearchFilesResponse {
            status: Some(ok_response("ok")),
            matches,
        }))
    }

    async fn create_archive(
        &self,
        request: Request<CreateArchiveRequest>,
    ) -> Result<GrpcResponse<CreateArchiveResponse>, Status> {
        let task_id = self
            .archives
            .create(request.into_inner(), self.manager.clone())?;

        Ok(GrpcResponse::new(CreateArchiveResponse {
            status: Some(ok_response("archive task started")),
            task_id,
        }))
    }

    async fn watch_archive_progress(
        &self,
        request: Request<WatchArchiveProgressRequest>,
    ) -> Result<GrpcResponse<Self::WatchArchiveProgressStream>, Status> {
        let task_id = request.into_inner().task_id;
        let receiver = self.archives.subscribe(&task_id)?;
        let stream = BroadcastStream::new(receiver).filter_map(|event| match event {
            Ok(progress) => Some(Ok(progress)),
            Err(error) => Some(Err(Status::internal(error.to_string()))),
        });

        Ok(GrpcResponse::new(Box::pin(stream)))
    }
}

#[derive(Clone, Debug)]
pub struct FileManager {
    root: Arc<PathBuf>,
    state_root: Arc<PathBuf>,
}

impl FileManager {
    pub fn from_env() -> Self {
        let root = env::var("RUSTPANEL_FS_ROOT").unwrap_or_else(|_| DEFAULT_FILE_ROOT.to_owned());
        Self::new(root)
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let root = root.canonicalize().unwrap_or(root);

        let state_root = env::var("RUSTPANEL_FILE_STATE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_FILE_STATE_ROOT));

        Self {
            root: Arc::new(root),
            state_root: Arc::new(state_root),
        }
    }

    pub fn list_directory(&self, path: &str, recursive: bool) -> Result<Vec<FileItem>, Status> {
        let root = self.resolve_existing(path)?;
        let walker = if recursive {
            WalkDir::new(&root).min_depth(1)
        } else {
            WalkDir::new(&root).min_depth(1).max_depth(1)
        };

        walker
            .into_iter()
            .map(|entry| {
                let entry = entry.map_err(|error| Status::internal(error.to_string()))?;
                self.file_item(entry.path())
            })
            .collect()
    }

    pub fn resolve_existing(&self, requested: &str) -> Result<PathBuf, Status> {
        let path = self.root.join(safe_relative_path(requested)?);
        let canonical = path.canonicalize().map_err(io_status)?;
        ensure_inside_root(&self.root, &canonical)?;

        Ok(canonical)
    }

    pub fn resolve_for_write(&self, requested: &str) -> Result<PathBuf, Status> {
        let path = self.root.join(safe_relative_path(requested)?);
        let parent = path
            .parent()
            .ok_or_else(|| Status::invalid_argument("path has no parent"))?;
        let parent = parent.canonicalize().map_err(io_status)?;
        ensure_inside_root(&self.root, &parent)?;

        Ok(path)
    }

    pub fn public_path(&self, path: &Path) -> String {
        path.strip_prefix(self.root.as_ref())
            .map(|path| format!("/{}", path.to_string_lossy()))
            .unwrap_or_else(|_| path.to_string_lossy().to_string())
    }

    async fn move_to_recycle_bin(&self, path: &Path) -> Result<(), Status> {
        let id = Uuid::new_v4().to_string();
        let recycle_dir = self.recycle_dir();
        tokio::fs::create_dir_all(&recycle_dir)
            .await
            .map_err(io_status)?;
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "item".to_owned());
        let recycle_path = recycle_dir.join(format!("{id}-{file_name}"));
        let item = StoredRecycleItem {
            id,
            original_path: self.public_path(path),
            recycle_path: recycle_path.to_string_lossy().to_string(),
            deleted_at_seconds: current_timestamp(),
        };
        tokio::fs::rename(path, &recycle_path)
            .await
            .map_err(io_status)?;
        let mut items = self.recycle_items_stored().await?;
        items.push(item);
        self.save_recycle_items(&items).await
    }

    async fn recycle_items(&self) -> Result<Vec<RecycleBinItem>, Status> {
        Ok(self
            .recycle_items_stored()
            .await?
            .into_iter()
            .map(StoredRecycleItem::into_proto)
            .collect())
    }

    async fn restore_recycle_item(&self, id: &str) -> Result<(), Status> {
        let mut items = self.recycle_items_stored().await?;
        let item = items
            .iter()
            .find(|item| item.id == id)
            .cloned()
            .ok_or_else(|| Status::not_found("recycle item not found"))?;
        let target = self.resolve_for_write(&item.original_path)?;
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(io_status)?;
        }
        tokio::fs::rename(&item.recycle_path, target)
            .await
            .map_err(io_status)?;
        items.retain(|stored| stored.id != id);
        self.save_recycle_items(&items).await
    }

    async fn empty_recycle_bin(&self) -> Result<(), Status> {
        let recycle_dir = self.recycle_dir();
        if tokio::fs::try_exists(&recycle_dir)
            .await
            .map_err(io_status)?
        {
            tokio::fs::remove_dir_all(&recycle_dir)
                .await
                .map_err(io_status)?;
        }
        self.save_recycle_items(&[]).await
    }

    fn search_files(&self, request: SearchFilesRequest) -> Result<Vec<SearchMatch>, Status> {
        if request.query.trim().is_empty() {
            return Err(Status::invalid_argument("search query is required"));
        }
        let root = self.resolve_existing(&request.root_path)?;
        let matcher = SearchMatcher::new(&request.query, request.regex)?;
        let max_results = request.max_results.clamp(1, 500) as usize;
        let mut matches = Vec::new();
        for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
            if matches.len() >= max_results || !entry.file_type().is_file() {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            for (index, line) in content.lines().enumerate() {
                if matcher.is_match(line) {
                    matches.push(SearchMatch {
                        path: self.public_path(entry.path()),
                        line_number: (index + 1) as u32,
                        line: line.chars().take(240).collect(),
                    });
                    if matches.len() >= max_results {
                        break;
                    }
                }
            }
        }
        Ok(matches)
    }

    async fn audit(&self, action: &str, path: &str) -> Result<(), Status> {
        tokio::fs::create_dir_all(self.state_root.as_ref())
            .await
            .map_err(io_status)?;
        let entry = FileAuditEntry {
            action: action.to_owned(),
            path: path.to_owned(),
            timestamp_seconds: current_timestamp(),
        };
        let mut line = serde_json::to_string(&entry).map_err(io_status)?;
        line.push('\n');
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.audit_path())
            .await
            .map_err(io_status)?;
        file.write_all(line.as_bytes()).await.map_err(io_status)?;
        let _ =
            audit::append_audit_event("files", action, format!("file {action}: {path}"), "grpc")
                .await;
        Ok(())
    }

    async fn recycle_items_stored(&self) -> Result<Vec<StoredRecycleItem>, Status> {
        match tokio::fs::read_to_string(self.recycle_index_path()).await {
            Ok(content) => serde_json::from_str(&content).map_err(io_status),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(io_status(error)),
        }
    }

    async fn save_recycle_items(&self, items: &[StoredRecycleItem]) -> Result<(), Status> {
        tokio::fs::create_dir_all(self.state_root.as_ref())
            .await
            .map_err(io_status)?;
        let content = serde_json::to_string_pretty(items).map_err(io_status)?;
        tokio::fs::write(self.recycle_index_path(), content)
            .await
            .map_err(io_status)
    }

    fn recycle_dir(&self) -> PathBuf {
        self.state_root.join("recycle")
    }

    fn recycle_index_path(&self) -> PathBuf {
        self.state_root.join("recycle.json")
    }

    fn audit_path(&self) -> PathBuf {
        self.state_root.join("audit.jsonl")
    }

    fn file_item(&self, path: &Path) -> Result<FileItem, Status> {
        let metadata = std::fs::symlink_metadata(path).map_err(io_status)?;
        let kind = if metadata.file_type().is_symlink() {
            FileKind::Symlink
        } else if metadata.is_dir() {
            FileKind::Directory
        } else if metadata.is_file() {
            FileKind::File
        } else {
            FileKind::Other
        };

        Ok(FileItem {
            path: self.public_path(path),
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default(),
            kind: kind.into(),
            size_bytes: metadata.len(),
            permissions: file_mode(&metadata),
            owner: file_owner(&metadata),
            group: file_group(&metadata),
            modified_at_seconds: metadata
                .modified()
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .map(|elapsed| current_timestamp().saturating_sub(elapsed.as_secs()))
                .unwrap_or_default(),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredRecycleItem {
    id: String,
    original_path: String,
    recycle_path: String,
    deleted_at_seconds: u64,
}

impl StoredRecycleItem {
    fn into_proto(self) -> RecycleBinItem {
        RecycleBinItem {
            id: self.id,
            original_path: self.original_path,
            recycle_path: self.recycle_path,
            deleted_at_seconds: self.deleted_at_seconds,
        }
    }
}

#[derive(Debug, Serialize)]
struct FileAuditEntry {
    action: String,
    path: String,
    timestamp_seconds: u64,
}

enum SearchMatcher {
    Plain(String),
    Regex(Regex),
}

impl SearchMatcher {
    fn new(query: &str, regex: bool) -> Result<Self, Status> {
        if regex {
            Regex::new(query)
                .map(Self::Regex)
                .map_err(|error| Status::invalid_argument(error.to_string()))
        } else {
            Ok(Self::Plain(query.to_lowercase()))
        }
    }

    fn is_match(&self, line: &str) -> bool {
        match self {
            Self::Plain(query) => line.to_lowercase().contains(query),
            Self::Regex(regex) => regex.is_match(line),
        }
    }
}

#[derive(Clone, Default)]
struct ArchiveManager {
    tasks: Arc<Mutex<HashMap<String, broadcast::Sender<WatchArchiveProgressResponse>>>>,
}

impl ArchiveManager {
    fn create(
        &self,
        request: CreateArchiveRequest,
        manager: FileManager,
    ) -> Result<String, Status> {
        if request.source_paths.is_empty() {
            return Err(Status::invalid_argument(
                "archive requires at least one source path",
            ));
        }
        let sources = request
            .source_paths
            .iter()
            .map(|source| manager.resolve_existing(source))
            .collect::<Result<Vec<_>, _>>()?;
        let archive_path = manager.resolve_for_write(&request.archive_path)?;
        let format = ArchiveFormat::try_from(request.format)
            .ok()
            .filter(|format| *format != ArchiveFormat::Unspecified)
            .ok_or_else(|| Status::invalid_argument("archive format is required"))?;
        let task_id = Uuid::new_v4().to_string();
        let (sender, _) = broadcast::channel(ARCHIVE_CHANNEL_SIZE);
        self.tasks
            .lock()
            .map_err(|_| Status::internal("archive task lock poisoned"))?
            .insert(task_id.clone(), sender.clone());
        let task_id_for_task = task_id.clone();

        tokio::task::spawn_blocking(move || {
            send_archive_progress(
                &sender,
                &task_id_for_task,
                ArchiveTaskState::Running,
                5,
                "running",
            );
            let result = create_archive_file(&sources, &archive_path, format);
            match result {
                Ok(()) => send_archive_progress(
                    &sender,
                    &task_id_for_task,
                    ArchiveTaskState::Succeeded,
                    100,
                    "archive created",
                ),
                Err(error) => send_archive_progress(
                    &sender,
                    &task_id_for_task,
                    ArchiveTaskState::Failed,
                    100,
                    &error.to_string(),
                ),
            }
        });

        Ok(task_id)
    }

    fn subscribe(
        &self,
        task_id: &str,
    ) -> Result<broadcast::Receiver<WatchArchiveProgressResponse>, Status> {
        let sender = self
            .tasks
            .lock()
            .map_err(|_| Status::internal("archive task lock poisoned"))?
            .get(task_id)
            .cloned()
            .ok_or_else(|| Status::not_found("archive task not found"))?;

        Ok(sender.subscribe())
    }
}

fn create_archive_file(
    sources: &[PathBuf],
    archive_path: &Path,
    format: ArchiveFormat,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(parent) = archive_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    match format {
        ArchiveFormat::Zip => create_zip_archive(sources, archive_path),
        ArchiveFormat::TarGz => create_tar_gz_archive(sources, archive_path),
        ArchiveFormat::Unspecified => Err("archive format is required".into()),
    }
}

fn create_zip_archive(
    sources: &[PathBuf],
    archive_path: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let file = File::create(archive_path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    for source in sources {
        append_path_to_zip(&mut zip, source, source, options)?;
    }
    zip.finish()?;
    Ok(())
}

fn append_path_to_zip(
    zip: &mut ZipWriter<File>,
    root: &Path,
    path: &Path,
    options: SimpleFileOptions,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let relative = path
        .strip_prefix(root.parent().unwrap_or(root))
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let metadata = std::fs::metadata(path)?;

    if metadata.is_dir() {
        zip.add_directory(format!("{relative}/"), options)?;
        for entry in std::fs::read_dir(path)? {
            append_path_to_zip(zip, root, &entry?.path(), options)?;
        }
    } else {
        zip.start_file(relative, options)?;
        let mut file = File::open(path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;
        zip.write_all(&buffer)?;
    }

    Ok(())
}

fn create_tar_gz_archive(
    sources: &[PathBuf],
    archive_path: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let file = File::create(archive_path)?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut tar = TarBuilder::new(encoder);

    for source in sources {
        let name = source
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("archive"));
        if source.is_dir() {
            tar.append_dir_all(name, source)?;
        } else {
            tar.append_path_with_name(source, name)?;
        }
    }
    tar.finish()?;
    Ok(())
}

fn send_archive_progress(
    sender: &broadcast::Sender<WatchArchiveProgressResponse>,
    task_id: &str,
    state: ArchiveTaskState,
    percent: u32,
    message: &str,
) {
    let _ = sender.send(WatchArchiveProgressResponse {
        status: Some(ok_response(message)),
        task_id: task_id.to_owned(),
        state: state.into(),
        percent,
        message: message.to_owned(),
    });
}

fn safe_relative_path(requested: &str) -> Result<PathBuf, Status> {
    let path = Path::new(requested);
    let mut relative = PathBuf::new();

    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::ParentDir => {
                return Err(Status::invalid_argument("path traversal is not allowed"));
            }
            Component::RootDir | Component::CurDir => {}
            Component::Normal(part) => relative.push(part),
        }
    }

    Ok(relative)
}

fn ensure_inside_root(root: &Path, path: &Path) -> Result<(), Status> {
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(Status::permission_denied(
            "path escapes configured file root",
        ))
    }
}

fn io_status(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
}

#[cfg(unix)]
fn file_mode(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o7777
}

#[cfg(not(unix))]
fn file_mode(_metadata: &std::fs::Metadata) -> u32 {
    0
}

#[cfg(unix)]
fn file_owner(metadata: &std::fs::Metadata) -> String {
    use std::os::unix::fs::MetadataExt;

    metadata.uid().to_string()
}

#[cfg(not(unix))]
fn file_owner(_metadata: &std::fs::Metadata) -> String {
    "unknown".to_owned()
}

#[cfg(unix)]
fn file_group(metadata: &std::fs::Metadata) -> String {
    use std::os::unix::fs::MetadataExt;

    metadata.gid().to_string()
}

#[cfg(not(unix))]
fn file_group(_metadata: &std::fs::Metadata) -> String {
    "unknown".to_owned()
}

#[cfg(unix)]
async fn chmod_path(path: &Path, mode: u32) -> Result<(), Status> {
    use std::os::unix::fs::PermissionsExt;

    let permissions = std::fs::Permissions::from_mode(mode);
    tokio::fs::set_permissions(path, permissions)
        .await
        .map_err(io_status)
}

#[cfg(not(unix))]
async fn chmod_path(_path: &Path, _mode: u32) -> Result<(), Status> {
    Err(Status::unimplemented("chmod is only supported on Unix"))
}

#[cfg(unix)]
async fn chown_path(path: &Path, owner: &str, group: &str) -> Result<(), Status> {
    if owner.trim().is_empty() && group.trim().is_empty() {
        return Err(Status::invalid_argument("owner or group is required"));
    }
    let target = if group.trim().is_empty() {
        owner.to_owned()
    } else if owner.trim().is_empty() {
        format!(":{group}")
    } else {
        format!("{owner}:{group}")
    };
    let output = tokio::process::Command::new("chown")
        .arg(target)
        .arg(path)
        .output()
        .await
        .map_err(io_status)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(Status::internal(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ))
    }
}

#[cfg(not(unix))]
async fn chown_path(_path: &Path, _owner: &str, _group: &str) -> Result<(), Status> {
    Err(Status::unimplemented("chown is only supported on Unix"))
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
    fn rejects_parent_path_components() {
        let result = safe_relative_path("../etc/passwd");

        assert!(result.is_err());
    }

    #[test]
    fn resolves_path_inside_root() {
        let manager = FileManager::new(env::temp_dir());
        let path = manager.resolve_for_write("rustpanel-test-file.txt");

        assert!(path.is_ok());
    }

    #[test]
    fn searches_plain_and_regex_content() {
        let root = env::temp_dir().join(format!("rustpanel-files-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("root");
        std::fs::write(root.join("app.log"), "alpha\nerror: failed\n").expect("file");
        let manager = FileManager::new(&root);

        let plain = manager
            .search_files(SearchFilesRequest {
                root_path: "/".to_owned(),
                query: "ERROR".to_owned(),
                regex: false,
                max_results: 10,
            })
            .expect("plain search");
        let regex = manager
            .search_files(SearchFilesRequest {
                root_path: "/".to_owned(),
                query: "error: .*".to_owned(),
                regex: true,
                max_results: 10,
            })
            .expect("regex search");

        assert_eq!(plain.len(), 1);
        assert_eq!(regex[0].line_number, 2);
    }
}
