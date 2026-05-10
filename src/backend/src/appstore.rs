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
        app_store_service_server::AppStoreService, AppCategory, AppTemplate, AppVersion,
        Capabilities, CompatibilityStatus, DeployAppRequest, DeployAppResponse, InstallMethod,
        InstalledApp, ListAppTemplatesRequest, ListAppTemplatesResponse, ListInstalledAppsRequest,
        ListInstalledAppsResponse, ResourceBudget, UninstallAppRequest, UninstallAppResponse,
        UpdateAppRequest, UpdateAppResponse,
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
        // Phase B:在返回模板前先探一下当前主机能力 + 资源预算,
        // 给每个 entry 填上 compatibility / compatibility_reason,
        // 前端可以直接按状态分组渲染,不需要二次判定。
        let capabilities = crate::capability::probe_capabilities_sync();
        let budget = crate::capability::snapshot_resource_budget_sync();
        let templates = app_templates()
            .into_iter()
            .map(|template| evaluate_compatibility(template, &capabilities, &budget))
            .collect();
        Ok(GrpcResponse::new(ListAppTemplatesResponse {
            status: Some(ok_response("ok")),
            templates,
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
    let mut templates = vec![
        // ====== 轻量 systemd-first(NAT VPS / OpenVZ 友好) ======
        native_template(
            "nginx-light",
            "Nginx (apt)",
            "通过 apt 安装 nginx-light,系统包形式运行,常驻 RAM ~5MB。",
            AppCategory::WebServer,
            10,
            8,
            5,
            true,
        ),
        native_template(
            "caddy",
            "Caddy",
            "Caddy v2 静态二进制,自带 ACME 自动签发 SSL。常驻 RAM ~30MB。",
            AppCategory::WebServer,
            64,
            60,
            30,
            true,
        )
        .with_install(InstallMethod::BinaryDownload),
        native_template(
            "sqlite",
            "SQLite",
            "嵌入式数据库,无需常驻进程,RustPanel 默认推荐。",
            AppCategory::Database,
            0,
            5,
            0,
            true,
        ),
        native_template(
            "redis-tuned",
            "Redis (调优)",
            "apt 安装 redis-server,默认 maxmemory 30MB + LRU 驱逐。常驻 RAM ~10MB。",
            AppCategory::Database,
            32,
            10,
            10,
            true,
        ),
        native_template(
            "postgres-tiny",
            "PostgreSQL (低配版)",
            "apt 安装 postgresql-15,shared_buffers=8MB / max_connections=8。生产建议 ≥ 256MB RAM。",
            AppCategory::Database,
            256,
            120,
            60,
            false,
        ),
        native_template(
            "hugo",
            "Hugo",
            "Go 写的静态站点生成器,一次构建后零运行时占用。",
            AppCategory::Runtime,
            16,
            40,
            0,
            true,
        )
        .with_install(InstallMethod::BinaryDownload),
        native_template(
            "zola",
            "Zola",
            "Rust 写的静态站点生成器,单二进制,零运行时占用。",
            AppCategory::Runtime,
            16,
            40,
            0,
            true,
        )
        .with_install(InstallMethod::BinaryDownload),
        native_template(
            "restic",
            "Restic 备份",
            "Go 写的加密增量备份工具,支持 SFTP/S3/Backblaze 等多种存储后端。",
            AppCategory::Tool,
            16,
            25,
            0,
            true,
        )
        .with_install(InstallMethod::BinaryDownload),
        native_template(
            "rclone",
            "rclone 云存储同步",
            "把文件同步到 S3 / OSS / 七牛 / 阿里云 / Backblaze 等 50+ 云存储。",
            AppCategory::Tool,
            16,
            45,
            0,
            true,
        )
        .with_install(InstallMethod::BinaryDownload),
        native_template(
            "fail2ban",
            "Fail2ban",
            "扫描日志自动封禁恶意 IP。OpenVZ 上需 iptables 模块开放才能工作。",
            AppCategory::Tool,
            32,
            10,
            8,
            true,
        ),
        native_template(
            "wireguard",
            "WireGuard",
            "现代加密 VPN。OpenVZ 通常需要 wireguard-go(用户态),不依赖内核模块。",
            AppCategory::Vpn,
            16,
            8,
            5,
            false,
        ),
        native_template(
            "certbot",
            "Certbot (Let's Encrypt)",
            "ACME 客户端。RustPanel 内置 ACME 客户端,certbot 仅作为兼容备选。",
            AppCategory::Tool,
            32,
            30,
            0,
            false,
        ),
        // ====== Docker 路线(只有 can_run_docker 时才显示) ======
        AppTemplate {
            slug: "mysql".to_owned(),
            name: "MySQL".to_owned(),
            description: "MySQL 8 容器,生产建议 ≥ 1GB RAM。".to_owned(),
            image: "mysql:8.4".to_owned(),
            default_ports: vec!["3306:3306".to_owned()],
            versions: app_versions(&[("8.4", "mysql:8.4", true), ("8.0", "mysql:8.0", false)]),
            default_version: "8.4".to_owned(),
            runtime_kind: "database".to_owned(),
            category: AppCategory::Database as i32,
            install_method: InstallMethod::DockerCompose as i32,
            min_ram_mb: 1024,
            min_disk_mb: 800,
            compatibility: CompatibilityStatus::Unspecified as i32,
            compatibility_reason: String::new(),
            expected_runtime_ram_mb: 600,
            recommended: false,
        },
        AppTemplate {
            slug: "redis".to_owned(),
            name: "Redis (容器)".to_owned(),
            description: "Redis 7 Alpine 容器版本。轻量场景建议改用 redis-tuned。".to_owned(),
            image: "redis:7-alpine".to_owned(),
            default_ports: vec!["6379:6379".to_owned()],
            versions: app_versions(&[
                ("7", "redis:7-alpine", true),
                ("6", "redis:6-alpine", false),
            ]),
            default_version: "7".to_owned(),
            runtime_kind: "cache".to_owned(),
            category: AppCategory::Database as i32,
            install_method: InstallMethod::DockerCompose as i32,
            min_ram_mb: 128,
            min_disk_mb: 50,
            compatibility: CompatibilityStatus::Unspecified as i32,
            compatibility_reason: String::new(),
            expected_runtime_ram_mb: 30,
            recommended: false,
        },
        AppTemplate {
            slug: "postgres".to_owned(),
            name: "PostgreSQL (容器)".to_owned(),
            description: "PostgreSQL 16 Alpine 容器。轻量场景建议改用 postgres-tiny。".to_owned(),
            image: "postgres:16-alpine".to_owned(),
            default_ports: vec!["5432:5432".to_owned()],
            versions: app_versions(&[
                ("16", "postgres:16-alpine", true),
                ("15", "postgres:15-alpine", false),
            ]),
            default_version: "16".to_owned(),
            runtime_kind: "database".to_owned(),
            category: AppCategory::Database as i32,
            install_method: InstallMethod::DockerCompose as i32,
            min_ram_mb: 512,
            min_disk_mb: 400,
            compatibility: CompatibilityStatus::Unspecified as i32,
            compatibility_reason: String::new(),
            expected_runtime_ram_mb: 80,
            recommended: false,
        },
        AppTemplate {
            slug: "nginx".to_owned(),
            name: "Nginx (容器)".to_owned(),
            description: "Nginx 1.27 Alpine 容器。轻量场景建议改用 nginx-light。".to_owned(),
            image: "nginx:1.27-alpine".to_owned(),
            default_ports: vec!["8080:80".to_owned()],
            versions: app_versions(&[
                ("1.27", "nginx:1.27-alpine", true),
                ("1.26", "nginx:1.26-alpine", false),
            ]),
            default_version: "1.27".to_owned(),
            runtime_kind: "web".to_owned(),
            category: AppCategory::WebServer as i32,
            install_method: InstallMethod::DockerCompose as i32,
            min_ram_mb: 64,
            min_disk_mb: 60,
            compatibility: CompatibilityStatus::Unspecified as i32,
            compatibility_reason: String::new(),
            expected_runtime_ram_mb: 25,
            recommended: false,
        },
        AppTemplate {
            slug: "php".to_owned(),
            name: "PHP-FPM (容器)".to_owned(),
            description: "PHP 8 FPM 容器。多版本并存可借此实现。".to_owned(),
            image: "php:8.3-fpm-alpine".to_owned(),
            default_ports: vec![],
            versions: app_versions(&[
                ("8.3", "php:8.3-fpm-alpine", true),
                ("8.2", "php:8.2-fpm-alpine", false),
            ]),
            default_version: "8.3".to_owned(),
            runtime_kind: "runtime".to_owned(),
            category: AppCategory::Runtime as i32,
            install_method: InstallMethod::DockerCompose as i32,
            min_ram_mb: 128,
            min_disk_mb: 200,
            compatibility: CompatibilityStatus::Unspecified as i32,
            compatibility_reason: String::new(),
            expected_runtime_ram_mb: 40,
            recommended: false,
        },
    ];
    // 按分类 + recommended 排序
    templates.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then((!a.recommended).cmp(&!b.recommended))
            .then(a.name.cmp(&b.name))
    });
    templates
}

// 轻量 native 模板的快捷构造器:apt-style,Docker 字段留空。
#[allow(clippy::too_many_arguments)]
fn native_template(
    slug: &str,
    name: &str,
    description: &str,
    category: AppCategory,
    min_ram_mb: u32,
    min_disk_mb: u32,
    expected_runtime_ram_mb: u32,
    recommended: bool,
) -> AppTemplate {
    AppTemplate {
        slug: slug.to_owned(),
        name: name.to_owned(),
        description: description.to_owned(),
        image: String::new(),
        default_ports: Vec::new(),
        versions: Vec::new(),
        default_version: String::new(),
        runtime_kind: "native".to_owned(),
        category: category as i32,
        install_method: InstallMethod::NativePackage as i32,
        min_ram_mb,
        min_disk_mb,
        compatibility: CompatibilityStatus::Unspecified as i32,
        compatibility_reason: String::new(),
        expected_runtime_ram_mb,
        recommended,
    }
}

trait AppTemplateExt {
    fn with_install(self, method: InstallMethod) -> Self;
}

impl AppTemplateExt for AppTemplate {
    fn with_install(mut self, method: InstallMethod) -> Self {
        self.install_method = method as i32;
        self
    }
}

// 结合主机能力 + 资源预算,给模板打上 compatibility 状态。
fn evaluate_compatibility(
    mut template: AppTemplate,
    capabilities: &Capabilities,
    budget: &ResourceBudget,
) -> AppTemplate {
    let install_method = InstallMethod::try_from(template.install_method).unwrap_or_default();

    // 1. Docker 路线但 Docker 不可用 → NEEDS_DOCKER
    if install_method == InstallMethod::DockerCompose && !capabilities.can_run_docker {
        template.compatibility = CompatibilityStatus::NeedsDocker as i32;
        template.compatibility_reason = if capabilities.docker_block_reason.is_empty() {
            "本机未启用 Docker".to_owned()
        } else {
            capabilities.docker_block_reason.clone()
        };
        return template;
    }

    // 2. fail2ban 在 OpenVZ 且无 iptables 时 → KERNEL_UNSUPPORTED
    if template.slug == "fail2ban" && capabilities.is_openvz && !capabilities.has_iptables {
        template.compatibility = CompatibilityStatus::KernelUnsupported as i32;
        template.compatibility_reason = "OpenVZ 上 iptables 不可写,fail2ban 无法工作".to_owned();
        return template;
    }

    // 3. wireguard 内核模块在 OpenVZ 上多半缺,但提示用 wireguard-go(用户态)
    if template.slug == "wireguard" && capabilities.is_openvz {
        template.compatibility = CompatibilityStatus::KernelUnsupported as i32;
        template.compatibility_reason =
            "OpenVZ 内核通常无 wireguard 模块,可改装 wireguard-go(用户态)".to_owned();
        return template;
    }

    // 4. RAM 不够
    if template.min_ram_mb > 0 {
        if let Some(memory) = budget.memory.as_ref() {
            let total_mb = (memory.total_bytes / 1024 / 1024) as u32;
            if total_mb < template.min_ram_mb {
                template.compatibility = CompatibilityStatus::ResourceShort as i32;
                template.compatibility_reason = format!(
                    "本机仅 {} MB RAM,该应用建议 ≥ {} MB",
                    total_mb, template.min_ram_mb
                );
                return template;
            }
        }
    }

    // 5. Disk 不够
    if template.min_disk_mb > 0 {
        if let Some(root) = budget
            .disks
            .iter()
            .find(|d| d.mount_point == "/")
            .or_else(|| budget.disks.first())
        {
            let avail_mb = (root.available_bytes / 1024 / 1024) as u32;
            if avail_mb < template.min_disk_mb {
                template.compatibility = CompatibilityStatus::ResourceShort as i32;
                template.compatibility_reason = format!(
                    "{} 可用 {} MB,该应用建议 ≥ {} MB",
                    root.mount_point, avail_mb, template.min_disk_mb
                );
                return template;
            }
        }
    }

    template.compatibility = CompatibilityStatus::Compatible as i32;
    template
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
