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
        // ====== Phase G:用户可选 Rust 栈(NO SUPPORT VPS 友好) ======
        // 二进制全部保持上游官方版本,RustPanel 只提供配置模板与模块联动,
        // 不 fork、不打补丁,后续上游升级直接替换二进制即可。
        native_template(
            "rpxy",
            "rpxy (Rust 反代)",
            "Rust 写的 HTTPS 反向代理,内置 ACME / 多站点 / h2/h3,常驻 RAM ~15MB。RustPanel 提供配置模板并与 sites / ssl 模块联动,二进制保持上游官方版本。\n\n官网: https://github.com/junkurihara/rust-rpxy",
            AppCategory::WebServer,
            32,
            15,
            15,
            true,
        )
        .with_install(InstallMethod::BinaryDownload)
        .with_versions(native_versions(&[("latest", true)]), "latest"),
        native_template(
            "static-web-server",
            "static-web-server (SWS)",
            "Rust 写的纯静态文件服务器,常驻 RAM ~5MB。可作 rpxy 上游或独立运行,与 static-sites 模块联动。\n\n官网: https://github.com/static-web-server/static-web-server",
            AppCategory::WebServer,
            16,
            8,
            5,
            true,
        )
        .with_install(InstallMethod::BinaryDownload)
        .with_versions(native_versions(&[("latest", true)]), "latest"),
        native_template(
            "leaf",
            "leaf (Rust 多协议代理)",
            "Rust 写的多协议代理,单实例同时暴露 SS / VLESS / Trojan / WireGuard / h2 / ws / tls。常驻 RAM ~25MB。默认 off,在面板软件商店主动启用。\n\n官网: https://github.com/eycorsican/leaf",
            AppCategory::Vpn,
            64,
            20,
            25,
            false,
        )
        .with_install(InstallMethod::BinaryDownload)
        .with_versions(native_versions(&[("latest", true)]), "latest"),
        native_template(
            "vsmtp",
            "vSMTP (Rust 邮件中转)",
            "Rust filter-MTA,做 alias 转发与回复改写;不收件、不存信。出站强制走 SMTP relay (Resend / SES / Postmark),绝不直连 25 端口。常驻 RAM ~35MB。社区维护,默认 off。\n\n官网: https://github.com/viridIT/vSMTP",
            AppCategory::Tool,
            96,
            30,
            35,
            false,
        )
        .with_install(InstallMethod::BinaryDownload)
        .with_versions(native_versions(&[("latest", true)]), "latest"),
        native_template(
            "tuic",
            "TUIC v5 (实验性)",
            "基于 QUIC 的 UDP 代理,作为 leaf 的抗封锁备用线路。要求宿主对 UDP 友好。社区维护,标实验性,默认 off。\n\n官网: https://github.com/EAimTY/tuic",
            AppCategory::Vpn,
            64,
            15,
            20,
            false,
        )
        .with_install(InstallMethod::BinaryDownload)
        .with_versions(native_versions(&[("latest", true)]), "latest"),
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
    fn with_versions(self, versions: Vec<AppVersion>, default: &str) -> Self;
}

impl AppTemplateExt for AppTemplate {
    fn with_install(mut self, method: InstallMethod) -> Self {
        self.install_method = method as i32;
        self
    }

    fn with_versions(mut self, versions: Vec<AppVersion>, default: &str) -> Self {
        self.versions = versions;
        self.default_version = default.to_owned();
        self
    }
}

/// 给 native(无 Docker 镜像)的模板构造 versions 列表的简化器:
/// 只接 (version, recommended);image 字段保持空串。
fn native_versions(values: &[(&str, bool)]) -> Vec<AppVersion> {
    values
        .iter()
        .map(|(version, recommended)| AppVersion {
            version: (*version).to_owned(),
            image: String::new(),
            recommended: *recommended,
        })
        .collect()
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

/// Phase G 二进制包的"安装计划"。这里只描述装一个包需要哪些原料,
/// 不执行实际下载、解压、systemctl —— executor 在后续 commit 接进
/// deploy_app。命名为 plan 强调这是数据契约,执行是另一回事。
///
/// 字段语义
/// - `upstream_repo`: owner/repo,executor 拼 GitHub Releases API
/// - `asset_pattern`: release asset 文件名模板,`{version}` 由 executor
///   替换为 GitHub 上拿到的 tag(去掉前缀 v)
/// - `binary_path_in_archive`: 归档内二进制的相对路径;空串表示归档
///   本身就是裸二进制(部分项目用单文件 .gz)
/// - `install_to`: 二进制最终路径
/// - `config_path`: 配置文件最终路径
/// - `systemd_unit` / `config_template`: RustPanel 提供的胶水,
///   保持上游软件自身配置语义不变
/// - `post_install_hint`: 装完后给用户看的一句话下一步指引
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)] // executor 在下一个 commit 接进 deploy_app
pub(crate) struct BinaryInstallPlan {
    pub upstream_repo: &'static str,
    pub asset_pattern: &'static str,
    pub binary_path_in_archive: &'static str,
    pub install_to: &'static str,
    pub config_path: &'static str,
    pub systemd_unit: &'static str,
    pub config_template: &'static str,
    pub post_install_hint: &'static str,
}

/// Phase G 5 个 Rust 栈包的安装计划。
///
/// asset_pattern 锚 linux-amd64 musl 静态链接版本(NAT VPS / OpenVZ
/// 友好,不挑 glibc)。当上游切到 v* tag 但 asset 名沿用裸版本号时,
/// `{version}` 由 executor 在调用时按需去掉前缀 v。
///
/// 当前 patterns 是基于上游 release 命名惯例的最佳估计,executor
/// 接入后会做实际 404 回退(列出 release assets 模糊匹配)。
#[allow(dead_code)] // executor 在下一个 commit 接进 deploy_app
pub(crate) fn phase_g_install_plan(slug: &str) -> Option<BinaryInstallPlan> {
    Some(match slug {
        "rpxy" => BinaryInstallPlan {
            upstream_repo: "junkurihara/rust-rpxy",
            asset_pattern: "rpxy-{version}-x86_64-unknown-linux-musl.tar.gz",
            binary_path_in_archive: "rpxy",
            install_to: "/usr/local/bin/rpxy",
            config_path: "/etc/rpxy/config.toml",
            systemd_unit: RPXY_SERVICE,
            config_template: RPXY_CONFIG,
            post_install_hint: "编辑 /etc/rpxy/config.toml 加入站点条目后 `systemctl restart rpxy`;DNS-01 ACME 走 RustPanel ssl 模块。",
        },
        "static-web-server" => BinaryInstallPlan {
            upstream_repo: "static-web-server/static-web-server",
            asset_pattern: "static-web-server-v{version}-x86_64-unknown-linux-musl.tar.gz",
            binary_path_in_archive: "static-web-server-v{version}-x86_64-unknown-linux-musl/static-web-server",
            install_to: "/usr/local/bin/static-web-server",
            config_path: "/etc/static-web-server/config.toml",
            systemd_unit: SWS_SERVICE,
            config_template: SWS_CONFIG,
            post_install_hint: "把站点根目录挂到 /var/www/<site>,在 /etc/static-web-server/config.toml 指 root 后 `systemctl restart static-web-server`。",
        },
        "leaf" => BinaryInstallPlan {
            upstream_repo: "eycorsican/leaf",
            // leaf 的 release asset 是单文件 .gz,不是 tar.gz
            asset_pattern: "leaf-{version}-x86_64-unknown-linux-gnu.gz",
            binary_path_in_archive: "",
            install_to: "/usr/local/bin/leaf",
            config_path: "/etc/leaf/config.conf",
            systemd_unit: LEAF_SERVICE,
            config_template: LEAF_CONFIG,
            post_install_hint: "在 /etc/leaf/config.conf 配置 inbounds(用 RustPanel security 模块分配的 NAT 端口),`systemctl restart leaf` 后用面板生成的订阅链接接入客户端。",
        },
        "vsmtp" => BinaryInstallPlan {
            upstream_repo: "viridIT/vSMTP",
            asset_pattern: "vsmtp-{version}-x86_64-unknown-linux-musl.tar.gz",
            binary_path_in_archive: "vsmtp",
            install_to: "/usr/local/bin/vsmtp",
            config_path: "/etc/vsmtp/vsmtp.toml",
            systemd_unit: VSMTP_SERVICE,
            config_template: VSMTP_CONFIG,
            post_install_hint: "出站必须配 SMTP relay(Resend/SES/Postmark),不要直连 25;在面板 vSMTP Tab 维护 alias 映射后 `systemctl restart vsmtp`。",
        },
        "tuic" => BinaryInstallPlan {
            upstream_repo: "EAimTY/tuic",
            // TUIC 发布裸二进制(无归档)
            asset_pattern: "tuic-server-{version}-x86_64-unknown-linux-musl",
            binary_path_in_archive: "",
            install_to: "/usr/local/bin/tuic-server",
            config_path: "/etc/tuic/config.json",
            systemd_unit: TUIC_SERVICE,
            config_template: TUIC_CONFIG,
            post_install_hint: "TUIC 是实验性 UDP 备用线;在 /etc/tuic/config.json 填证书与 NAT 端口后 `systemctl restart tuic`。",
        },
        _ => return None,
    })
}

// systemd unit 模板 —— 保持最小,刻意不开 ProtectSystem=strict / PrivateNetwork
// 等沙箱选项,128MB OpenVZ 上启用这些会和服务自身的写盘/绑端口冲突,
// 先稳后紧。每个 unit 自身的 ExecStart 参数对齐上游 README 的默认调用。

const RPXY_SERVICE: &str = r#"[Unit]
Description=rpxy reverse proxy (RustPanel managed)
Documentation=https://github.com/junkurihara/rust-rpxy
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/rpxy --config /etc/rpxy/config.toml
Restart=on-failure
RestartSec=3s

[Install]
WantedBy=multi-user.target
"#;

const RPXY_CONFIG: &str = r#"# RustPanel 默认 rpxy 配置骨架。
# 站点条目由 sites 模块在用户增删站点时自动写入,这里只放全局段。
listen_port = 8080
listen_port_tls = 8443

[apps]
# 例:
# [apps."example"]
# server_name = "example.com"
# reverse_proxy = [{ location = "/", upstream = [{ location = "127.0.0.1:3000" }] }]
# tls = { https_redirection = true, acme = true }
"#;

const SWS_SERVICE: &str = r#"[Unit]
Description=static-web-server (RustPanel managed)
Documentation=https://github.com/static-web-server/static-web-server
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/static-web-server --config-file /etc/static-web-server/config.toml
Restart=on-failure
RestartSec=3s

[Install]
WantedBy=multi-user.target
"#;

const SWS_CONFIG: &str = r#"# RustPanel 默认 SWS 配置骨架。
# 多站点建议每个站起独立 unit(static-web-server@<site>.service),
# 这里是单实例模式的默认值。
[general]
host = "127.0.0.1"
port = 8081
root = "/var/www/default"
log-level = "info"
compression = true
cache-control-headers = true
"#;

const LEAF_SERVICE: &str = r#"[Unit]
Description=leaf multi-protocol proxy (RustPanel managed)
Documentation=https://github.com/eycorsican/leaf
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/leaf -c /etc/leaf/config.conf
Restart=on-failure
RestartSec=3s

[Install]
WantedBy=multi-user.target
"#;

const LEAF_CONFIG: &str = r#"# RustPanel 默认 leaf 配置骨架(占位)。
# 实际 inbounds / outbounds 由 RustPanel 在用户启用代理协议时生成,
# 这里只是单元能起来不报错的最小可加载形态。
[General]
loglevel = "info"

[Proxy]
Direct = direct
"#;

const VSMTP_SERVICE: &str = r#"[Unit]
Description=vSMTP filter MTA (RustPanel managed)
Documentation=https://github.com/viridIT/vSMTP
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/vsmtp --config /etc/vsmtp/vsmtp.toml
Restart=on-failure
RestartSec=3s

[Install]
WantedBy=multi-user.target
"#;

const VSMTP_CONFIG: &str = r#"# RustPanel 默认 vSMTP 配置骨架。
# alias 表与 rhai 规则由 RustPanel vSMTP Tab 在用户操作时生成。
# 出站强制走 relay,**绝不直连 25**。
[server]
domain = "rustpanel.local"

[server.system]
group_local = "vsmtp"
user_local = "vsmtp"

[app]
dirpath = "/var/spool/vsmtp"

# relay 凭证由面板向导写入对应 secret 文件,不放在这里。
"#;

const TUIC_SERVICE: &str = r#"[Unit]
Description=TUIC v5 QUIC proxy (RustPanel managed, experimental)
Documentation=https://github.com/EAimTY/tuic
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/tuic-server -c /etc/tuic/config.json
Restart=on-failure
RestartSec=3s

[Install]
WantedBy=multi-user.target
"#;

const TUIC_CONFIG: &str = r#"{
  "_comment": "RustPanel 默认 TUIC v5 骨架;证书路径与监听端口由面板向导填入。",
  "server": "[::]:443",
  "users": {},
  "certificate": "/etc/tuic/cert.pem",
  "private_key": "/etc/tuic/key.pem",
  "congestion_control": "bbr",
  "alpn": ["h3"],
  "udp_relay_ipv6": true,
  "zero_rtt_handshake": false,
  "auth_timeout": "3s",
  "task_negotiation_timeout": "3s",
  "max_idle_time": "10s",
  "max_external_packet_size": 1500,
  "gc_interval": "3s",
  "gc_lifetime": "15s",
  "log_level": "info"
}
"#;

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

    #[test]
    fn phase_g_rust_stack_templates_present() {
        // Phase G:5 个 Rust 栈包必须全部出现在 appstore,都走 BinaryDownload,
        // 都至少给出 latest 这一项可选版本,description 末尾带官网链接。
        let templates = app_templates();
        let expected = [
            (
                "rpxy",
                AppCategory::WebServer,
                "github.com/junkurihara/rust-rpxy",
            ),
            (
                "static-web-server",
                AppCategory::WebServer,
                "github.com/static-web-server/static-web-server",
            ),
            ("leaf", AppCategory::Vpn, "github.com/eycorsican/leaf"),
            ("vsmtp", AppCategory::Tool, "github.com/viridIT/vSMTP"),
            ("tuic", AppCategory::Vpn, "github.com/EAimTY/tuic"),
        ];
        for (slug, category, homepage_substr) in expected {
            let template = templates
                .iter()
                .find(|template| template.slug == slug)
                .unwrap_or_else(|| panic!("missing phase-g template: {slug}"));
            assert_eq!(
                template.install_method,
                InstallMethod::BinaryDownload as i32,
                "{slug} must install from upstream binary"
            );
            assert_eq!(
                template.category, category as i32,
                "{slug} category mismatch"
            );
            assert_eq!(
                template.default_version, "latest",
                "{slug} should default to latest"
            );
            assert!(
                template
                    .versions
                    .iter()
                    .any(|version| version.version == "latest" && version.recommended),
                "{slug} should expose recommended latest version"
            );
            assert!(
                template.description.contains(homepage_substr),
                "{slug} description should contain upstream URL: {homepage_substr}"
            );
        }
    }

    #[test]
    fn phase_g_install_plans_present() {
        // 每个 Phase G slug 都必须有一份 BinaryInstallPlan,且关键字段
        // 非空、asset_pattern 包含 {version} 占位、install_to 是绝对路径。
        // executor 后续 commit 会消费这份数据。
        let slugs = ["rpxy", "static-web-server", "leaf", "vsmtp", "tuic"];
        for slug in slugs {
            let plan = phase_g_install_plan(slug)
                .unwrap_or_else(|| panic!("no install plan for slug: {slug}"));
            assert!(
                !plan.upstream_repo.is_empty(),
                "{slug} plan must set upstream_repo"
            );
            assert!(
                plan.asset_pattern.contains("{version}"),
                "{slug} asset_pattern must contain {{version}} placeholder"
            );
            assert!(
                plan.install_to.starts_with('/'),
                "{slug} install_to must be absolute path"
            );
            assert!(
                plan.config_path.starts_with('/'),
                "{slug} config_path must be absolute path"
            );
            assert!(
                plan.systemd_unit.contains("[Service]"),
                "{slug} systemd_unit must contain [Service] section"
            );
            assert!(
                plan.systemd_unit.contains(plan.install_to),
                "{slug} systemd_unit ExecStart should reference install_to"
            );
            assert!(
                !plan.post_install_hint.is_empty(),
                "{slug} should provide a post-install hint"
            );
        }
        assert!(
            phase_g_install_plan("not-a-real-slug").is_none(),
            "unknown slugs must return None"
        );
    }
}
