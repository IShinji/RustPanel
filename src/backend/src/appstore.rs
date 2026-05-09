use std::{collections::BTreeMap, env, path::PathBuf};

use serde::Serialize;
use tonic::{Request, Response as GrpcResponse, Status};
use uuid::Uuid;

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        app_store_service_server::AppStoreService, AppTemplate, DeployAppRequest,
        DeployAppResponse, ListAppTemplatesRequest, ListAppTemplatesResponse,
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
        Ok(GrpcResponse::new(ListAppTemplatesResponse {
            status: Some(ok_response("ok")),
            templates: app_templates(),
        }))
    }

    async fn deploy_app(
        &self,
        request: Request<DeployAppRequest>,
    ) -> Result<GrpcResponse<DeployAppResponse>, Status> {
        let request = request.into_inner();
        let template = app_templates()
            .into_iter()
            .find(|template| template.slug == request.slug)
            .ok_or_else(|| Status::not_found("app template not found"))?;
        let app_name = sanitize_app_name(if request.app_name.trim().is_empty() {
            &template.slug
        } else {
            &request.app_name
        })?;
        let compose_yaml = generate_compose_yaml(&template, &app_name)?;
        let compose_path = appstore_root().join(&app_name).join("docker-compose.yml");
        if let Some(parent) = compose_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(io_status)?;
        }
        tokio::fs::write(&compose_path, compose_yaml.as_bytes())
            .await
            .map_err(io_status)?;

        if env::var("RUSTPANEL_APPSTORE_SKIP_COMPOSE").is_err() {
            run_compose_up(&compose_path).await?;
        }

        Ok(GrpcResponse::new(DeployAppResponse {
            status: Some(ok_response("app deployed")),
            compose_path: compose_path.to_string_lossy().to_string(),
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
        },
        AppTemplate {
            slug: "redis".to_owned(),
            name: "Redis".to_owned(),
            description: "Redis 7 cache with append-only persistence".to_owned(),
            image: "redis:7-alpine".to_owned(),
            default_ports: vec!["6379:6379".to_owned()],
        },
        AppTemplate {
            slug: "postgres".to_owned(),
            name: "PostgreSQL".to_owned(),
            description: "PostgreSQL 16 database with persistent storage".to_owned(),
            image: "postgres:16-alpine".to_owned(),
            default_ports: vec!["5432:5432".to_owned()],
        },
    ]
}

fn generate_compose_yaml(template: &AppTemplate, app_name: &str) -> Result<String, Status> {
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
            image: template.image.clone(),
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

async fn run_compose_up(compose_path: &PathBuf) -> Result<(), Status> {
    let output = tokio::process::Command::new("docker")
        .arg("compose")
        .arg("-f")
        .arg(compose_path)
        .arg("up")
        .arg("-d")
        .output()
        .await
        .map_err(io_status)?;

    if output.status.success() {
        Ok(())
    } else {
        Err(Status::unavailable(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ))
    }
}

fn appstore_root() -> PathBuf {
    env::var("RUSTPANEL_APPSTORE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_APPSTORE_ROOT))
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
        let yaml = generate_compose_yaml(&template, "redis").expect("yaml");

        assert!(yaml.contains("rustpanel-redis"));
        assert!(yaml.contains("redis-server --appendonly yes"));
    }
}
