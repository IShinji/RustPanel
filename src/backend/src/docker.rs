use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    pin::Pin,
    time::{SystemTime, UNIX_EPOCH},
};

use bollard::{
    container::{
        InspectContainerOptions, ListContainersOptions, LogsOptions, PruneContainersOptions,
        RemoveContainerOptions, RestartContainerOptions, StartContainerOptions,
        StopContainerOptions, UpdateContainerOptions,
    },
    image::{CreateImageOptions, ListImagesOptions, PruneImagesOptions, TagImageOptions},
    network::PruneNetworksOptions,
    secret::{ContainerInspectResponse, ContainerSummary, CreateImageInfo, ImageSummary, Port},
    volume::PruneVolumesOptions,
    Docker,
};
use futures_core::Stream;
use futures_util::StreamExt;
use serde_yaml::Value as YamlValue;
use tonic::{Request, Response as GrpcResponse, Status};

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        docker_service_server::DockerService, ComposeProject, ContainerItem, ContainerPort,
        DeployComposeProjectRequest, DeployComposeProjectResponse, ImageItem,
        ListComposeProjectsRequest, ListComposeProjectsResponse, ListContainersRequest,
        ListContainersResponse, ListImagesRequest, ListImagesResponse, PauseContainerRequest,
        PauseContainerResponse, PruneDockerResourcesRequest, PruneDockerResourcesResponse,
        PullImageRequest, PullImageResponse, RemoveComposeProjectRequest,
        RemoveComposeProjectResponse, RemoveContainerRequest, RemoveContainerResponse,
        RestartContainerRequest, RestartContainerResponse, RollbackImageTagRequest,
        RollbackImageTagResponse, SetContainerResourcesRequest, SetContainerResourcesResponse,
        StartContainerRequest, StartContainerResponse, StopContainerRequest, StopContainerResponse,
        UpsertComposeProjectRequest, UpsertComposeProjectResponse, WatchContainerLogsRequest,
        WatchContainerLogsResponse, WatchImagePullRequest, WatchImagePullResponse,
    },
};

const DEFAULT_DOCKER_COMPOSE_ROOT: &str = "/tmp/rustpanel/compose";
const DOCKER_SKIP_COMPOSE_ENV: &str = "RUSTPANEL_DOCKER_SKIP_COMPOSE";

#[derive(Clone, Debug, Default)]
pub struct DockerServiceImpl;

#[tonic::async_trait]
impl DockerService for DockerServiceImpl {
    type WatchContainerLogsStream =
        Pin<Box<dyn Stream<Item = Result<WatchContainerLogsResponse, Status>> + Send>>;
    type WatchImagePullStream =
        Pin<Box<dyn Stream<Item = Result<WatchImagePullResponse, Status>> + Send>>;

    async fn list_containers(
        &self,
        request: Request<ListContainersRequest>,
    ) -> Result<GrpcResponse<ListContainersResponse>, Status> {
        let docker = docker_client()?;
        let summaries = docker
            .list_containers(Some(ListContainersOptions::<String> {
                all: request.into_inner().all,
                ..Default::default()
            }))
            .await
            .map_err(docker_status)?;
        let mut containers = Vec::with_capacity(summaries.len());
        for summary in summaries {
            let mut container = container_summary(summary);
            enrich_container_resources(&docker, &mut container).await;
            containers.push(container);
        }

        Ok(GrpcResponse::new(ListContainersResponse {
            status: Some(ok_response("ok")),
            containers,
        }))
    }

    async fn start_container(
        &self,
        request: Request<StartContainerRequest>,
    ) -> Result<GrpcResponse<StartContainerResponse>, Status> {
        let docker = docker_client()?;
        docker
            .start_container(
                &request.into_inner().container_id,
                None::<StartContainerOptions<String>>,
            )
            .await
            .map_err(docker_status)?;

        Ok(GrpcResponse::new(StartContainerResponse {
            status: Some(ok_response("container started")),
        }))
    }

    async fn stop_container(
        &self,
        request: Request<StopContainerRequest>,
    ) -> Result<GrpcResponse<StopContainerResponse>, Status> {
        let docker = docker_client()?;
        docker
            .stop_container(
                &request.into_inner().container_id,
                None::<StopContainerOptions>,
            )
            .await
            .map_err(docker_status)?;

        Ok(GrpcResponse::new(StopContainerResponse {
            status: Some(ok_response("container stopped")),
        }))
    }

    async fn restart_container(
        &self,
        request: Request<RestartContainerRequest>,
    ) -> Result<GrpcResponse<RestartContainerResponse>, Status> {
        let docker = docker_client()?;
        docker
            .restart_container(
                &request.into_inner().container_id,
                None::<RestartContainerOptions>,
            )
            .await
            .map_err(docker_status)?;

        Ok(GrpcResponse::new(RestartContainerResponse {
            status: Some(ok_response("container restarted")),
        }))
    }

    async fn remove_container(
        &self,
        request: Request<RemoveContainerRequest>,
    ) -> Result<GrpcResponse<RemoveContainerResponse>, Status> {
        let docker = docker_client()?;
        docker
            .remove_container(
                &request.into_inner().container_id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await
            .map_err(docker_status)?;

        Ok(GrpcResponse::new(RemoveContainerResponse {
            status: Some(ok_response("container removed")),
        }))
    }

    async fn pause_container(
        &self,
        request: Request<PauseContainerRequest>,
    ) -> Result<GrpcResponse<PauseContainerResponse>, Status> {
        let docker = docker_client()?;
        docker
            .pause_container(&request.into_inner().container_id)
            .await
            .map_err(docker_status)?;

        Ok(GrpcResponse::new(PauseContainerResponse {
            status: Some(ok_response("container paused")),
        }))
    }

    async fn pull_image(
        &self,
        request: Request<PullImageRequest>,
    ) -> Result<GrpcResponse<PullImageResponse>, Status> {
        let request = request.into_inner();
        let docker = docker_client()?;
        let mut stream = docker.create_image(
            Some(CreateImageOptions {
                from_image: request.image,
                tag: request.tag,
                ..Default::default()
            }),
            None,
            None,
        );

        while let Some(event) = stream.next().await {
            event.map_err(docker_status)?;
        }

        Ok(GrpcResponse::new(PullImageResponse {
            status: Some(ok_response("image pulled")),
        }))
    }

    async fn watch_image_pull(
        &self,
        request: Request<WatchImagePullRequest>,
    ) -> Result<GrpcResponse<Self::WatchImagePullStream>, Status> {
        let request = request.into_inner();
        validate_image_name(&request.image)?;
        let docker = docker_client()?;
        let stream = docker
            .create_image(
                Some(CreateImageOptions {
                    from_image: request.image,
                    tag: request.tag,
                    ..Default::default()
                }),
                None,
                None,
            )
            .map(|event| {
                event
                    .map(image_pull_event)
                    .map_err(docker_status)
                    .and_then(|event| event)
            });

        Ok(GrpcResponse::new(Box::pin(stream)))
    }

    async fn list_images(
        &self,
        request: Request<ListImagesRequest>,
    ) -> Result<GrpcResponse<ListImagesResponse>, Status> {
        let docker = docker_client()?;
        let images = docker
            .list_images(Some(ListImagesOptions::<String> {
                all: request.into_inner().all,
                filters: HashMap::new(),
                digests: true,
            }))
            .await
            .map_err(docker_status)?
            .into_iter()
            .map(image_summary)
            .collect();

        Ok(GrpcResponse::new(ListImagesResponse {
            status: Some(ok_response("ok")),
            images,
        }))
    }

    async fn set_container_resources(
        &self,
        request: Request<SetContainerResourcesRequest>,
    ) -> Result<GrpcResponse<SetContainerResourcesResponse>, Status> {
        let request = request.into_inner();
        if request.container_id.trim().is_empty() {
            return Err(Status::invalid_argument("container id is required"));
        }
        let nano_cpus = nano_cpus_from_cores(request.cpu_limit_cores)?;
        let memory = memory_limit_i64(request.memory_limit_bytes)?;
        let docker = docker_client()?;
        docker
            .update_container(
                &request.container_id,
                UpdateContainerOptions::<String> {
                    nano_cpus,
                    memory,
                    memory_swap: memory,
                    ..Default::default()
                },
            )
            .await
            .map_err(docker_status)?;

        let mut container = docker
            .inspect_container(
                &request.container_id,
                Some(InspectContainerOptions { size: false }),
            )
            .await
            .map(container_item_from_inspect)
            .map_err(docker_status)?;
        enrich_container_resources(&docker, &mut container).await;

        Ok(GrpcResponse::new(SetContainerResourcesResponse {
            status: Some(ok_response("container resources updated")),
            container: Some(container),
        }))
    }

    async fn prune_docker_resources(
        &self,
        request: Request<PruneDockerResourcesRequest>,
    ) -> Result<GrpcResponse<PruneDockerResourcesResponse>, Status> {
        let request = request.into_inner();
        let docker = docker_client()?;
        let mut deleted_count = 0_u32;
        let mut reclaimed = 0_u64;
        let mut parts = Vec::new();

        if request.images {
            let mut filters = HashMap::new();
            if request.all_images {
                filters.insert("dangling".to_owned(), vec!["false".to_owned()]);
            }
            let result = docker
                .prune_images(Some(PruneImagesOptions::<String> { filters }))
                .await
                .map_err(docker_status)?;
            let count = result.images_deleted.unwrap_or_default().len() as u32;
            deleted_count += count;
            reclaimed += positive_i64(result.space_reclaimed);
            parts.push(format!("images={count}"));
        }
        if request.containers {
            let result = docker
                .prune_containers(Some(PruneContainersOptions::<String> {
                    filters: HashMap::new(),
                }))
                .await
                .map_err(docker_status)?;
            let count = result.containers_deleted.unwrap_or_default().len() as u32;
            deleted_count += count;
            reclaimed += positive_i64(result.space_reclaimed);
            parts.push(format!("containers={count}"));
        }
        if request.volumes {
            let result = docker
                .prune_volumes(Some(PruneVolumesOptions::<String> {
                    filters: HashMap::new(),
                }))
                .await
                .map_err(docker_status)?;
            let count = result.volumes_deleted.unwrap_or_default().len() as u32;
            deleted_count += count;
            reclaimed += positive_i64(result.space_reclaimed);
            parts.push(format!("volumes={count}"));
        }
        if request.networks {
            let result = docker
                .prune_networks(Some(PruneNetworksOptions::<String> {
                    filters: HashMap::new(),
                }))
                .await
                .map_err(docker_status)?;
            let count = result.networks_deleted.unwrap_or_default().len() as u32;
            deleted_count += count;
            parts.push(format!("networks={count}"));
        }

        Ok(GrpcResponse::new(PruneDockerResourcesResponse {
            status: Some(ok_response("docker resources pruned")),
            deleted_count,
            space_reclaimed_bytes: reclaimed,
            summary: parts.join(", "),
        }))
    }

    async fn rollback_image_tag(
        &self,
        request: Request<RollbackImageTagRequest>,
    ) -> Result<GrpcResponse<RollbackImageTagResponse>, Status> {
        let request = request.into_inner();
        validate_image_name(&request.source_image)?;
        validate_image_name(&request.target_repository)?;
        if request.target_tag.trim().is_empty() {
            return Err(Status::invalid_argument("target tag is required"));
        }
        let docker = docker_client()?;
        docker
            .tag_image(
                &request.source_image,
                Some(TagImageOptions {
                    repo: request.target_repository,
                    tag: request.target_tag,
                }),
            )
            .await
            .map_err(docker_status)?;

        Ok(GrpcResponse::new(RollbackImageTagResponse {
            status: Some(ok_response("image tag rolled back")),
        }))
    }

    async fn list_compose_projects(
        &self,
        _request: Request<ListComposeProjectsRequest>,
    ) -> Result<GrpcResponse<ListComposeProjectsResponse>, Status> {
        let projects = list_compose_projects_from_disk().await?;

        Ok(GrpcResponse::new(ListComposeProjectsResponse {
            status: Some(ok_response("ok")),
            projects,
        }))
    }

    async fn upsert_compose_project(
        &self,
        request: Request<UpsertComposeProjectRequest>,
    ) -> Result<GrpcResponse<UpsertComposeProjectResponse>, Status> {
        let request = request.into_inner();
        let name = sanitize_project_name(&request.name)?;
        validate_compose_yaml(&request.compose_yaml)?;
        let compose_path = compose_path(&name);
        if let Some(parent) = compose_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(io_status)?;
        }
        tokio::fs::write(&compose_path, request.compose_yaml.as_bytes())
            .await
            .map_err(io_status)?;
        let project = compose_project_from_path(&name, &compose_path, "saved").await?;

        Ok(GrpcResponse::new(UpsertComposeProjectResponse {
            status: Some(ok_response("compose project saved")),
            project: Some(project),
        }))
    }

    async fn deploy_compose_project(
        &self,
        request: Request<DeployComposeProjectRequest>,
    ) -> Result<GrpcResponse<DeployComposeProjectResponse>, Status> {
        let name = sanitize_project_name(&request.into_inner().name)?;
        let compose_path = compose_path(&name);
        ensure_compose_exists(&compose_path).await?;
        if env::var(DOCKER_SKIP_COMPOSE_ENV).is_err() {
            run_compose(&name, &compose_path, &["up", "-d"]).await?;
        }
        let project = compose_project_from_path(&name, &compose_path, "deployed").await?;

        Ok(GrpcResponse::new(DeployComposeProjectResponse {
            status: Some(ok_response("compose project deployed")),
            project: Some(project),
        }))
    }

    async fn remove_compose_project(
        &self,
        request: Request<RemoveComposeProjectRequest>,
    ) -> Result<GrpcResponse<RemoveComposeProjectResponse>, Status> {
        let request = request.into_inner();
        let name = sanitize_project_name(&request.name)?;
        let compose_path = compose_path(&name);
        ensure_compose_exists(&compose_path).await?;
        if env::var(DOCKER_SKIP_COMPOSE_ENV).is_err() {
            run_compose(&name, &compose_path, &["down"]).await?;
        }
        if request.delete_files {
            let project_dir = compose_project_dir(&name);
            tokio::fs::remove_dir_all(project_dir)
                .await
                .map_err(io_status)?;
        }

        Ok(GrpcResponse::new(RemoveComposeProjectResponse {
            status: Some(ok_response("compose project removed")),
        }))
    }

    async fn watch_container_logs(
        &self,
        request: Request<WatchContainerLogsRequest>,
    ) -> Result<GrpcResponse<Self::WatchContainerLogsStream>, Status> {
        let request = request.into_inner();
        let docker = docker_client()?;
        let tail = request.tail_lines.max(100).to_string();
        let stream = docker
            .logs(
                &request.container_id,
                Some(LogsOptions::<String> {
                    follow: true,
                    stdout: true,
                    stderr: true,
                    tail,
                    ..Default::default()
                }),
            )
            .map(|event| {
                event
                    .map(|output| WatchContainerLogsResponse {
                        status: Some(ok_response("ok")),
                        line: log_output_text(output),
                    })
                    .map_err(docker_status)
            });

        Ok(GrpcResponse::new(Box::pin(stream)))
    }
}

fn docker_client() -> Result<Docker, Status> {
    Docker::connect_with_local_defaults().map_err(docker_status)
}

fn docker_status(error: impl std::fmt::Display) -> Status {
    Status::unavailable(error.to_string())
}

fn io_status(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
}

fn container_summary(summary: ContainerSummary) -> ContainerItem {
    ContainerItem {
        id: summary.id.unwrap_or_default(),
        name: summary
            .names
            .and_then(|names| names.into_iter().next())
            .unwrap_or_default()
            .trim_start_matches('/')
            .to_owned(),
        image: summary.image.unwrap_or_default(),
        state: summary.state.unwrap_or_default(),
        status_text: summary.status.unwrap_or_default(),
        ports: summary
            .ports
            .unwrap_or_default()
            .into_iter()
            .map(container_port)
            .collect(),
        created_at_seconds: summary.created.unwrap_or_default(),
        cpu_limit_cores: 0.0,
        memory_limit_bytes: 0,
    }
}

fn container_item_from_inspect(inspect: ContainerInspectResponse) -> ContainerItem {
    ContainerItem {
        id: inspect.id.unwrap_or_default(),
        name: inspect
            .name
            .unwrap_or_default()
            .trim_start_matches('/')
            .to_owned(),
        image: inspect
            .config
            .as_ref()
            .and_then(|config| config.image.clone())
            .or(inspect.image)
            .unwrap_or_default(),
        state: inspect
            .state
            .as_ref()
            .and_then(|state| state.status.as_ref())
            .map(|status| format!("{status:?}").to_ascii_lowercase())
            .unwrap_or_default(),
        status_text: inspect
            .state
            .as_ref()
            .and_then(|state| state.status.as_ref())
            .map(|status| format!("{status:?}").to_ascii_lowercase())
            .unwrap_or_default(),
        ports: Vec::new(),
        created_at_seconds: 0,
        cpu_limit_cores: inspect
            .host_config
            .as_ref()
            .and_then(|config| config.nano_cpus)
            .map(cores_from_nano_cpus)
            .unwrap_or_default(),
        memory_limit_bytes: inspect
            .host_config
            .as_ref()
            .and_then(|config| config.memory)
            .and_then(non_negative_i64)
            .unwrap_or_default(),
    }
}

async fn enrich_container_resources(docker: &Docker, container: &mut ContainerItem) {
    if container.id.is_empty() {
        return;
    }
    let Ok(inspect) = docker
        .inspect_container(&container.id, Some(InspectContainerOptions { size: false }))
        .await
    else {
        return;
    };
    if let Some(host_config) = inspect.host_config {
        container.cpu_limit_cores = host_config
            .nano_cpus
            .map(cores_from_nano_cpus)
            .unwrap_or_default();
        container.memory_limit_bytes = host_config
            .memory
            .and_then(non_negative_i64)
            .unwrap_or_default();
    }
}

fn image_summary(summary: ImageSummary) -> ImageItem {
    ImageItem {
        id: summary.id,
        repo_tags: summary.repo_tags,
        created_at_seconds: summary.created,
        size_bytes: summary.size,
        containers: summary.containers,
    }
}

fn container_port(port: Port) -> ContainerPort {
    ContainerPort {
        ip: port.ip.unwrap_or_default(),
        private_port: u32::from(port.private_port),
        public_port: port.public_port.map(u32::from).unwrap_or_default(),
        r#type: port
            .typ
            .map(|port_type| format!("{port_type:?}").to_ascii_lowercase())
            .unwrap_or_default(),
    }
}

fn image_pull_event(info: CreateImageInfo) -> Result<WatchImagePullResponse, Status> {
    if let Some(error) = info.error {
        return Err(Status::unavailable(error));
    }
    let status_text = info.status.unwrap_or_default();
    let done = status_text.contains("Downloaded newer image")
        || status_text.contains("Image is up to date")
        || status_text.contains("Status: Downloaded");
    let (current_bytes, total_bytes) = info
        .progress_detail
        .map(|detail| (positive_i64(detail.current), positive_i64(detail.total)))
        .unwrap_or_default();

    Ok(WatchImagePullResponse {
        status: Some(ok_response("ok")),
        image_id: info.id.unwrap_or_default(),
        status_text,
        progress: info.progress.unwrap_or_default(),
        current_bytes,
        total_bytes,
        done,
    })
}

fn nano_cpus_from_cores(cores: f64) -> Result<Option<i64>, Status> {
    if cores < 0.0 || !cores.is_finite() {
        return Err(Status::invalid_argument(
            "cpu limit must be a positive number",
        ));
    }
    if cores == 0.0 {
        return Ok(Some(0));
    }

    Ok(Some((cores * 1_000_000_000.0).round() as i64))
}

fn cores_from_nano_cpus(nano_cpus: i64) -> f64 {
    if nano_cpus <= 0 {
        0.0
    } else {
        nano_cpus as f64 / 1_000_000_000.0
    }
}

fn memory_limit_i64(memory_limit_bytes: u64) -> Result<Option<i64>, Status> {
    if memory_limit_bytes > i64::MAX as u64 {
        return Err(Status::invalid_argument("memory limit is too large"));
    }
    Ok(Some(memory_limit_bytes as i64))
}

fn non_negative_i64(value: i64) -> Option<u64> {
    if value > 0 {
        Some(value as u64)
    } else {
        None
    }
}

fn positive_i64(value: Option<i64>) -> u64 {
    value.filter(|value| *value > 0).unwrap_or_default() as u64
}

fn validate_image_name(image: &str) -> Result<(), Status> {
    let image = image.trim();
    if image.is_empty() {
        return Err(Status::invalid_argument("image is required"));
    }
    if image.contains(char::is_whitespace) {
        return Err(Status::invalid_argument(
            "image must not contain whitespace",
        ));
    }
    Ok(())
}

fn compose_root() -> PathBuf {
    env::var("RUSTPANEL_DOCKER_COMPOSE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_DOCKER_COMPOSE_ROOT))
}

fn compose_project_dir(name: &str) -> PathBuf {
    compose_root().join(name)
}

fn compose_path(name: &str) -> PathBuf {
    compose_project_dir(name).join("docker-compose.yml")
}

fn sanitize_project_name(name: &str) -> Result<String, Status> {
    let sanitized = name
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

    if sanitized.is_empty() {
        Err(Status::invalid_argument("compose project name is required"))
    } else {
        Ok(sanitized)
    }
}

fn validate_compose_yaml(compose_yaml: &str) -> Result<(), Status> {
    let value = serde_yaml::from_str::<YamlValue>(compose_yaml).map_err(io_status)?;
    let services = value
        .get("services")
        .and_then(YamlValue::as_mapping)
        .ok_or_else(|| Status::invalid_argument("compose yaml must contain services"))?;
    if services.is_empty() {
        return Err(Status::invalid_argument(
            "compose yaml must contain at least one service",
        ));
    }
    Ok(())
}

async fn ensure_compose_exists(path: &Path) -> Result<(), Status> {
    tokio::fs::metadata(path).await.map_err(io_status)?;
    Ok(())
}

async fn list_compose_projects_from_disk() -> Result<Vec<ComposeProject>, Status> {
    let root = compose_root();
    let mut projects = Vec::new();
    let mut entries = match tokio::fs::read_dir(&root).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(projects),
        Err(error) => return Err(io_status(error)),
    };

    while let Some(entry) = entries.next_entry().await.map_err(io_status)? {
        if !entry.file_type().await.map_err(io_status)?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path().join("docker-compose.yml");
        if tokio::fs::metadata(&path).await.is_ok() {
            projects.push(compose_project_from_path(&name, &path, "saved").await?);
        }
    }
    projects.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(projects)
}

async fn compose_project_from_path(
    name: &str,
    path: &Path,
    status_text: &str,
) -> Result<ComposeProject, Status> {
    let compose_yaml = tokio::fs::read_to_string(path).await.map_err(io_status)?;
    let metadata = tokio::fs::metadata(path).await.map_err(io_status)?;
    let service_names = compose_service_names(&compose_yaml)?;
    let updated_at_seconds = metadata
        .modified()
        .ok()
        .and_then(system_time_seconds)
        .unwrap_or_default() as i64;

    Ok(ComposeProject {
        name: name.to_owned(),
        compose_path: path.to_string_lossy().to_string(),
        compose_yaml,
        service_names,
        status_text: status_text.to_owned(),
        updated_at_seconds,
    })
}

fn compose_service_names(compose_yaml: &str) -> Result<Vec<String>, Status> {
    let value = serde_yaml::from_str::<YamlValue>(compose_yaml).map_err(io_status)?;
    let services = value
        .get("services")
        .and_then(YamlValue::as_mapping)
        .ok_or_else(|| Status::invalid_argument("compose yaml must contain services"))?;
    Ok(services
        .keys()
        .filter_map(YamlValue::as_str)
        .map(ToOwned::to_owned)
        .collect())
}

fn system_time_seconds(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

async fn run_compose(name: &str, compose_path: &Path, args: &[&str]) -> Result<(), Status> {
    let mut command = tokio::process::Command::new("docker");
    command.arg("compose");
    command.arg("-p").arg(format!("rustpanel-{name}"));
    command.arg("-f").arg(compose_path);
    for arg in args {
        command.arg(arg);
    }
    let output = command.output().await.map_err(io_status)?;

    if output.status.success() {
        Ok(())
    } else {
        Err(Status::unavailable(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ))
    }
}

fn log_output_text(output: bollard::container::LogOutput) -> String {
    match output {
        bollard::container::LogOutput::StdOut { message }
        | bollard::container::LogOutput::StdErr { message }
        | bollard::container::LogOutput::StdIn { message }
        | bollard::container::LogOutput::Console { message } => {
            String::from_utf8_lossy(&message).to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_container_port_defaults() {
        let port = container_port(Port {
            ip: Some("127.0.0.1".to_owned()),
            private_port: 80,
            public_port: Some(8080),
            typ: None,
        });

        assert_eq!(port.private_port, 80);
        assert_eq!(port.public_port, 8080);
        assert_eq!(port.r#type, "");
    }
}
