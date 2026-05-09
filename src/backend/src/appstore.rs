use std::{
    collections::BTreeMap,
    env,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tonic::{Request, Response as GrpcResponse, Status};
use uuid::Uuid;

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        app_store_service_server::AppStoreService, AppTemplate, AppVersion, DeployAppRequest,
        DeployAppResponse, InstalledApp, ListAppTemplatesRequest, ListAppTemplatesResponse,
        ListInstalledAppsRequest, ListInstalledAppsResponse, UninstallAppRequest,
        UninstallAppResponse, UpdateAppRequest, UpdateAppResponse,
    },
};

const DEFAULT_APPSTORE_ROOT: &str = "/tmp/rustpanel/appstore";

#[derive(Clone, Debug, Default)]
pub struct AppStoreServiceImpl;

#[tonic::async_trait]
impl AppStoreService for AppStoreServiceImpl {
    async fn list_app_templates(
        &self,
        _request: Request<ListAppTemplatesRequest>,
    ) -> Result<GrpcResponse<ListAppTemplatesResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_APPSTORE)?;
        Ok(GrpcResponse::new(ListAppTemplatesResponse {
            status: Some(ok_response("ok")),
            templates: app_templates(),
        }))
    }

    async fn deploy_app(
        &self,
        request: Request<DeployAppRequest>,
    ) -> Result<GrpcResponse<DeployAppResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_APPSTORE)?;
        let request = request.into_inner();
        let template = app_templates()
            .into_iter()
            .find(|template| template.slug == request.slug)
            .ok_or_else(|| Status::not_found("app template not found"))?;
        let version = resolve_template_version(&template, &request.version)?;
        let default_app_name;
        let app_name = sanitize_app_name(if request.app_name.trim().is_empty() {
            default_app_name = format!("{}-{}", template.slug, version.version);
            &default_app_name
        } else {
            &request.app_name
        })?;
        let compose_yaml = generate_compose_yaml(&template, &version, &app_name)?;
        let compose_path = appstore_root().join(&app_name).join("docker-compose.yml");
        if let Some(parent) = compose_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(io_status)?;
        }
        tokio::fs::write(&compose_path, compose_yaml.as_bytes())
            .await
            .map_err(io_status)?;
        let now = current_timestamp();
        let app = InstalledApp {
            app_name: app_name.clone(),
            slug: template.slug,
            version: version.version,
            image: version.image,
            compose_path: compose_path.to_string_lossy().to_string(),
            state: "installed".to_owned(),
            installed_at_seconds: now,
            updated_at_seconds: now,
        };
        save_installed_app(&app).await?;

        if env::var("RUSTPANEL_APPSTORE_SKIP_COMPOSE").is_err() {
            run_compose(&app_name, &compose_path, &["up", "-d"]).await?;
        }

        Ok(GrpcResponse::new(DeployAppResponse {
            status: Some(ok_response("app deployed")),
            compose_path: compose_path.to_string_lossy().to_string(),
            compose_yaml,
            app: Some(app),
        }))
    }

    async fn list_installed_apps(
        &self,
        _request: Request<ListInstalledAppsRequest>,
    ) -> Result<GrpcResponse<ListInstalledAppsResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_APPSTORE)?;
        Ok(GrpcResponse::new(ListInstalledAppsResponse {
            status: Some(ok_response("ok")),
            apps: list_installed_apps().await?,
        }))
    }

    async fn uninstall_app(
        &self,
        request: Request<UninstallAppRequest>,
    ) -> Result<GrpcResponse<UninstallAppResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_APPSTORE)?;
        let app_name = sanitize_app_name(&request.into_inner().app_name)?;
        let app_dir = appstore_root().join(&app_name);
        let compose_path = app_dir.join("docker-compose.yml");
        ensure_compose_exists(&compose_path).await?;
        if env::var("RUSTPANEL_APPSTORE_SKIP_COMPOSE").is_err() {
            run_compose(&app_name, &compose_path, &["down"]).await?;
        }
        tokio::fs::remove_dir_all(app_dir)
            .await
            .map_err(io_status)?;

        Ok(GrpcResponse::new(UninstallAppResponse {
            status: Some(ok_response("app uninstalled")),
        }))
    }

    async fn update_app(
        &self,
        request: Request<UpdateAppRequest>,
    ) -> Result<GrpcResponse<UpdateAppResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_APPSTORE)?;
        let request = request.into_inner();
        let app_name = sanitize_app_name(&request.app_name)?;
        let mut app = load_installed_app(&app_name).await?;
        let template = app_templates()
            .into_iter()
            .find(|template| template.slug == app.slug)
            .ok_or_else(|| Status::not_found("app template not found"))?;
        let version = resolve_template_version(&template, &request.version)?;
        let compose_yaml = generate_compose_yaml(&template, &version, &app_name)?;
        let compose_path = appstore_root().join(&app_name).join("docker-compose.yml");
        tokio::fs::write(&compose_path, compose_yaml.as_bytes())
            .await
            .map_err(io_status)?;
        app.version = version.version;
        app.image = version.image;
        app.state = "updated".to_owned();
        app.updated_at_seconds = current_timestamp();
        save_installed_app(&app).await?;
        if env::var("RUSTPANEL_APPSTORE_SKIP_COMPOSE").is_err() {
            run_compose(&app_name, &compose_path, &["up", "-d"]).await?;
        }

        Ok(GrpcResponse::new(UpdateAppResponse {
            status: Some(ok_response("app updated")),
            app: Some(app),
            compose_yaml,
        }))
    }
}

pub fn app_templates() -> Vec<AppTemplate> {
    vec![
        AppTemplate {
            slug: "mysql".to_owned(),
            name: "MySQL".to_owned(),
            description: "MySQL 8 database with persistent storage".to_owned(),
            image: "mysql:8.4".to_owned(),
            default_ports: vec!["3306:3306".to_owned()],
            versions: app_versions(&[("8.4", "mysql:8.4", true), ("8.0", "mysql:8.0", false)]),
            default_version: "8.4".to_owned(),
            runtime_kind: "database".to_owned(),
        },
        AppTemplate {
            slug: "redis".to_owned(),
            name: "Redis".to_owned(),
            description: "Redis 7 cache with append-only persistence".to_owned(),
            image: "redis:7-alpine".to_owned(),
            default_ports: vec!["6379:6379".to_owned()],
            versions: app_versions(&[
                ("7", "redis:7-alpine", true),
                ("6", "redis:6-alpine", false),
            ]),
            default_version: "7".to_owned(),
            runtime_kind: "cache".to_owned(),
        },
        AppTemplate {
            slug: "postgres".to_owned(),
            name: "PostgreSQL".to_owned(),
            description: "PostgreSQL 16 database with persistent storage".to_owned(),
            image: "postgres:16-alpine".to_owned(),
            default_ports: vec!["5432:5432".to_owned()],
            versions: app_versions(&[
                ("16", "postgres:16-alpine", true),
                ("15", "postgres:15-alpine", false),
            ]),
            default_version: "16".to_owned(),
            runtime_kind: "database".to_owned(),
        },
        AppTemplate {
            slug: "nginx".to_owned(),
            name: "Nginx".to_owned(),
            description: "Nginx runtime with version-pinned image".to_owned(),
            image: "nginx:1.27-alpine".to_owned(),
            default_ports: vec!["8080:80".to_owned()],
            versions: app_versions(&[
                ("1.27", "nginx:1.27-alpine", true),
                ("1.26", "nginx:1.26-alpine", false),
            ]),
            default_version: "1.27".to_owned(),
            runtime_kind: "web".to_owned(),
        },
        AppTemplate {
            slug: "php".to_owned(),
            name: "PHP".to_owned(),
            description: "PHP FPM runtime for multi-version application hosting".to_owned(),
            image: "php:8.3-fpm-alpine".to_owned(),
            default_ports: vec![],
            versions: app_versions(&[
                ("8.3", "php:8.3-fpm-alpine", true),
                ("8.2", "php:8.2-fpm-alpine", false),
            ]),
            default_version: "8.3".to_owned(),
            runtime_kind: "runtime".to_owned(),
        },
    ]
}

fn app_versions(values: &[(&str, &str, bool)]) -> Vec<AppVersion> {
    values
        .iter()
        .map(|(version, image, recommended)| AppVersion {
            version: (*version).to_owned(),
            image: (*image).to_owned(),
            recommended: *recommended,
        })
        .collect()
}

fn resolve_template_version(template: &AppTemplate, requested: &str) -> Result<AppVersion, Status> {
    let wanted = if requested.trim().is_empty() {
        &template.default_version
    } else {
        requested.trim()
    };

    template
        .versions
        .iter()
        .find(|version| version.version == wanted)
        .cloned()
        .ok_or_else(|| Status::invalid_argument("unsupported app version"))
}

fn generate_compose_yaml(
    template: &AppTemplate,
    version: &AppVersion,
    app_name: &str,
) -> Result<String, Status> {
    let password = Uuid::new_v4().simple().to_string();
    let mut environment = BTreeMap::new();
    match template.slug.as_str() {
        "mysql" => {
            environment.insert("MYSQL_ROOT_PASSWORD".to_owned(), password);
            environment.insert("MYSQL_DATABASE".to_owned(), "rustpanel".to_owned());
        }
        "postgres" => {
            environment.insert("POSTGRES_PASSWORD".to_owned(), password);
            environment.insert("POSTGRES_DB".to_owned(), "rustpanel".to_owned());
        }
        "redis" => {}
        _ => return Err(Status::invalid_argument("unsupported app template")),
    }

    let service_name = format!("rustpanel-{app_name}");
    let mut services = BTreeMap::new();
    services.insert(
        service_name.clone(),
        ComposeService {
            image: version.image.clone(),
            container_name: service_name,
            restart: "unless-stopped".to_owned(),
            ports: template.default_ports.clone(),
            environment,
            volumes: vec![format!("rustpanel-{app_name}-data:/data")],
            command: if template.slug == "redis" {
                Some("redis-server --appendonly yes".to_owned())
            } else {
                None
            },
        },
    );
    let compose = ComposeFile {
        services,
        volumes: BTreeMap::from([(format!("rustpanel-{app_name}-data"), BTreeMap::new())]),
    };

    serde_yaml::to_string(&compose).map_err(io_status)
}

async fn run_compose(app_name: &str, compose_path: &Path, args: &[&str]) -> Result<(), Status> {
    let mut command = tokio::process::Command::new("docker");
    command.arg("compose");
    command.arg("-p").arg(format!("rustpanel-{app_name}"));
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

async fn ensure_compose_exists(compose_path: &Path) -> Result<(), Status> {
    tokio::fs::metadata(compose_path).await.map_err(io_status)?;
    Ok(())
}

fn appstore_root() -> PathBuf {
    env::var("RUSTPANEL_APPSTORE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_APPSTORE_ROOT))
}

fn metadata_path(app_name: &str) -> PathBuf {
    appstore_root().join(app_name).join("rustpanel-app.json")
}

async fn save_installed_app(app: &InstalledApp) -> Result<(), Status> {
    let path = metadata_path(&app.app_name);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(io_status)?;
    }
    let stored = StoredInstalledApp::from_proto(app.clone());
    let content = serde_json::to_string_pretty(&stored).map_err(io_status)?;
    tokio::fs::write(path, content).await.map_err(io_status)
}

async fn load_installed_app(app_name: &str) -> Result<InstalledApp, Status> {
    let content = tokio::fs::read_to_string(metadata_path(app_name))
        .await
        .map_err(io_status)?;
    serde_json::from_str::<StoredInstalledApp>(&content)
        .map(StoredInstalledApp::into_proto)
        .map_err(io_status)
}

async fn list_installed_apps() -> Result<Vec<InstalledApp>, Status> {
    let root = appstore_root();
    let mut apps = Vec::new();
    let mut entries = match tokio::fs::read_dir(&root).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(apps),
        Err(error) => return Err(io_status(error)),
    };

    while let Some(entry) = entries.next_entry().await.map_err(io_status)? {
        if !entry.file_type().await.map_err(io_status)?.is_dir() {
            continue;
        }
        let app_name = entry.file_name().to_string_lossy().to_string();
        if let Ok(app) = load_installed_app(&app_name).await {
            apps.push(app);
        }
    }
    apps.sort_by(|left, right| left.app_name.cmp(&right.app_name));
    Ok(apps)
}

fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn sanitize_app_name(name: &str) -> Result<String, Status> {
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
        Err(Status::invalid_argument("app name is required"))
    } else {
        Ok(sanitized)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredInstalledApp {
    app_name: String,
    slug: String,
    version: String,
    image: String,
    compose_path: String,
    state: String,
    installed_at_seconds: i64,
    updated_at_seconds: i64,
}

impl StoredInstalledApp {
    fn from_proto(app: InstalledApp) -> Self {
        Self {
            app_name: app.app_name,
            slug: app.slug,
            version: app.version,
            image: app.image,
            compose_path: app.compose_path,
            state: app.state,
            installed_at_seconds: app.installed_at_seconds,
            updated_at_seconds: app.updated_at_seconds,
        }
    }

    fn into_proto(self) -> InstalledApp {
        InstalledApp {
            app_name: self.app_name,
            slug: self.slug,
            version: self.version,
            image: self.image,
            compose_path: self.compose_path,
            state: self.state,
            installed_at_seconds: self.installed_at_seconds,
            updated_at_seconds: self.updated_at_seconds,
        }
    }
}

fn io_status(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
}

#[derive(Debug, Serialize)]
struct ComposeFile {
    services: BTreeMap<String, ComposeService>,
    volumes: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Debug, Serialize)]
struct ComposeService {
    image: String,
    container_name: String,
    restart: String,
    ports: Vec<String>,
    environment: BTreeMap<String, String>,
    volumes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_compose_uses_rustpanel_names() {
        let template = app_templates()
            .into_iter()
            .find(|template| template.slug == "redis")
            .expect("redis template");
        let version = resolve_template_version(&template, "").expect("version");
        let yaml = generate_compose_yaml(&template, &version, "redis").expect("yaml");

        assert!(yaml.contains("rustpanel-redis"));
        assert!(yaml.contains("redis-server --appendonly yes"));
    }

    #[test]
    fn templates_expose_multiple_runtime_versions() {
        let php = app_templates()
            .into_iter()
            .find(|template| template.slug == "php")
            .expect("php template");

        assert_eq!(php.default_version, "8.3");
        assert!(php.versions.iter().any(|version| version.version == "8.2"));
    }
}
