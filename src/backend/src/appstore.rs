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
        let response = match InstallMethod::try_from(template.install_method).unwrap_or_default() {
            InstallMethod::DockerCompose => deploy_via_compose(template, request).await?,
            InstallMethod::BinaryDownload => deploy_via_binary(template, request).await?,
            InstallMethod::NativePackage => deploy_via_apt(template, request).await?,
            other => {
                return Err(Status::unimplemented(format!(
                    "install method {other:?} 暂未实现执行路径"
                )))
            }
        };
        Ok(GrpcResponse::new(response))
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
        let app = load_installed_app(&app_name).await?;
        let template = app_templates()
            .into_iter()
            .find(|template| template.slug == app.slug)
            .ok_or_else(|| Status::not_found("source template not found"))?;
        let response = match InstallMethod::try_from(template.install_method).unwrap_or_default() {
            InstallMethod::DockerCompose => uninstall_via_compose(&app_name).await?,
            InstallMethod::BinaryDownload => uninstall_via_binary(template, &app).await?,
            InstallMethod::NativePackage => uninstall_via_apt(template, &app).await?,
            other => {
                return Err(Status::unimplemented(format!(
                    "install method {other:?} 暂未实现卸载路径"
                )))
            }
        };
        Ok(GrpcResponse::new(response))
    }

    async fn update_app(
        &self,
        request: Request<UpdateAppRequest>,
    ) -> Result<GrpcResponse<UpdateAppResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_APPSTORE)?;
        let request = request.into_inner();
        let app_name = sanitize_app_name(&request.app_name)?;
        let app = load_installed_app(&app_name).await?;
        let template = app_templates()
            .into_iter()
            .find(|template| template.slug == app.slug)
            .ok_or_else(|| Status::not_found("source template not found"))?;
        let response = match InstallMethod::try_from(template.install_method).unwrap_or_default() {
            InstallMethod::DockerCompose => {
                update_via_compose(template, app, &app_name, &request.version).await?
            }
            InstallMethod::BinaryDownload => {
                update_via_binary(template, app, &request.version).await?
            }
            InstallMethod::NativePackage => update_via_apt(template, app).await?,
            other => {
                return Err(Status::unimplemented(format!(
                    "install method {other:?} 暂未实现更新路径"
                )))
            }
        };
        Ok(GrpcResponse::new(response))
    }
}

// ===================================================================
// Phase G executor:DockerCompose 走原来 docker compose,
// BinaryDownload 走 curl + tar + systemctl。
// 两条路径都有 RUSTPANEL_APPSTORE_SKIP_* 干跑开关,测试不动真实主机。
// ===================================================================

async fn deploy_via_compose(
    template: AppTemplate,
    request: DeployAppRequest,
) -> Result<DeployAppResponse, Status> {
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
        slug: template.slug.clone(),
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

    Ok(DeployAppResponse {
        status: Some(ok_response("app deployed")),
        compose_path: compose_path.to_string_lossy().to_string(),
        compose_yaml,
        app: Some(app),
    })
}

async fn uninstall_via_compose(app_name: &str) -> Result<UninstallAppResponse, Status> {
    let app_dir = appstore_root().join(app_name);
    let compose_path = app_dir.join("docker-compose.yml");
    ensure_compose_exists(&compose_path).await?;
    if env::var("RUSTPANEL_APPSTORE_SKIP_COMPOSE").is_err() {
        run_compose(app_name, &compose_path, &["down"]).await?;
    }
    tokio::fs::remove_dir_all(app_dir)
        .await
        .map_err(io_status)?;
    Ok(UninstallAppResponse {
        status: Some(ok_response("app uninstalled")),
    })
}

async fn update_via_compose(
    template: AppTemplate,
    mut app: InstalledApp,
    app_name: &str,
    requested_version: &str,
) -> Result<UpdateAppResponse, Status> {
    let version = resolve_template_version(&template, requested_version)?;
    let compose_yaml = generate_compose_yaml(&template, &version, app_name)?;
    let compose_path = appstore_root().join(app_name).join("docker-compose.yml");
    tokio::fs::write(&compose_path, compose_yaml.as_bytes())
        .await
        .map_err(io_status)?;
    app.version = version.version;
    app.image = version.image;
    app.state = "updated".to_owned();
    app.updated_at_seconds = current_timestamp();
    save_installed_app(&app).await?;
    if env::var("RUSTPANEL_APPSTORE_SKIP_COMPOSE").is_err() {
        run_compose(app_name, &compose_path, &["up", "-d"]).await?;
    }
    Ok(UpdateAppResponse {
        status: Some(ok_response("app updated")),
        app: Some(app),
        compose_yaml,
    })
}

async fn deploy_via_binary(
    template: AppTemplate,
    request: DeployAppRequest,
) -> Result<DeployAppResponse, Status> {
    let plan = phase_g_install_plan(&template.slug).ok_or_else(|| {
        Status::failed_precondition(format!(
            "BinaryDownload 模板 {} 暂无安装计划(只在 Phase G 5 个包里实现)",
            template.slug
        ))
    })?;
    // app_name 默认就是 slug —— 二进制安装是单例(系统里只能有一个
    // /usr/local/bin/<slug> 和 一个 .service),让 app_name 与 slug
    // 对齐,uninstall/update 才能稳定定位。
    let app_name = sanitize_app_name(if request.app_name.trim().is_empty() {
        &template.slug
    } else {
        &request.app_name
    })?;
    let version = resolve_binary_version(&plan, &request.version).await?;
    let asset_name = expand_asset_pattern(plan.asset_pattern, &version);
    let summary = render_install_plan_summary(&plan, &version, &asset_name);
    let unit_path = systemd_unit_dir().join(format!("{}.service", template.slug));

    let state = if env::var("RUSTPANEL_APPSTORE_SKIP_EXECUTE").is_ok() {
        "planned".to_owned()
    } else {
        execute_binary_install(&template.slug, &plan, &version, &asset_name).await?;
        "installed".to_owned()
    };

    let now = current_timestamp();
    let app = InstalledApp {
        app_name: app_name.clone(),
        slug: template.slug.clone(),
        version,
        image: String::new(),
        // 复用 compose_path 字段保存这次安装的 systemd unit 路径,
        // 后续 uninstall / update 能从元数据里直接拿到。
        compose_path: unit_path.to_string_lossy().to_string(),
        state,
        installed_at_seconds: now,
        updated_at_seconds: now,
    };
    save_installed_app(&app).await?;

    Ok(DeployAppResponse {
        status: Some(ok_response("binary app deployed")),
        compose_path: unit_path.to_string_lossy().to_string(),
        // 复用 compose_yaml 字段返回人话版安装计划摘要,前端直接显示
        compose_yaml: summary,
        app: Some(app),
    })
}

async fn uninstall_via_binary(
    template: AppTemplate,
    app: &InstalledApp,
) -> Result<UninstallAppResponse, Status> {
    let plan = phase_g_install_plan(&template.slug).ok_or_else(|| {
        Status::failed_precondition(format!(
            "BinaryDownload 模板 {} 暂无安装计划",
            template.slug
        ))
    })?;
    if env::var("RUSTPANEL_APPSTORE_SKIP_EXECUTE").is_err() {
        execute_binary_uninstall(&template.slug, &plan).await?;
    }
    // 移除 RustPanel 自身的元数据;config 与数据目录保留,
    // 用户重装时能继续使用之前的配置。
    let app_dir = appstore_root().join(&app.app_name);
    if tokio::fs::try_exists(&app_dir).await.unwrap_or(false) {
        tokio::fs::remove_dir_all(app_dir)
            .await
            .map_err(io_status)?;
    }
    Ok(UninstallAppResponse {
        status: Some(ok_response("binary app uninstalled")),
    })
}

async fn update_via_binary(
    template: AppTemplate,
    mut app: InstalledApp,
    requested_version: &str,
) -> Result<UpdateAppResponse, Status> {
    let plan = phase_g_install_plan(&template.slug).ok_or_else(|| {
        Status::failed_precondition(format!(
            "BinaryDownload 模板 {} 暂无安装计划",
            template.slug
        ))
    })?;
    let new_version = resolve_binary_version(&plan, requested_version).await?;
    let asset_name = expand_asset_pattern(plan.asset_pattern, &new_version);
    let summary = render_install_plan_summary(&plan, &new_version, &asset_name);

    if env::var("RUSTPANEL_APPSTORE_SKIP_EXECUTE").is_err() {
        execute_binary_install(&template.slug, &plan, &new_version, &asset_name).await?;
        // install_atomic 已经走 rename,服务在 restart 时拿到新二进制
        systemctl(&["restart", &format!("{}.service", template.slug)]).await?;
    }

    app.version = new_version;
    app.state = "updated".to_owned();
    app.updated_at_seconds = current_timestamp();
    save_installed_app(&app).await?;

    Ok(UpdateAppResponse {
        status: Some(ok_response("binary app updated")),
        app: Some(app),
        compose_yaml: summary,
    })
}

/// slug → 真实的 apt 包名映射(只覆盖当前 appstore 里走 NativePackage 的 slug)。
/// 没列在这里的 slug 就走 slug 本身作为兜底。pub(crate) 为单测可见。
pub(crate) fn slug_to_apt_package(slug: &str) -> &str {
    match slug {
        "redis-tuned" => "redis-server",
        "postgres-tiny" => "postgresql",
        "sqlite" => "sqlite3",
        "wireguard" => "wireguard",
        // nginx-mainline 走 nginx.org 官方源的 nginx 包(1.27+,内置
        // http_v3_module),pre-install 钩子负责加源 + pinning
        "nginx-mainline" => "nginx",
        // nginx-light / fail2ban / certbot 包名与 slug 一致
        other => other,
    }
}

/// nginx-mainline 的 pre-install 脚本:加 nginx.org GPG key + 官方 deb 源 +
/// apt pin,保证后续 `apt-get install nginx` 装的是 nginx.org 的 1.27+,
/// 而不是发行版自己 ship 的 1.22/1.24(没编译 http_v3_module)。
/// **所有 deb 都是 nginx 团队预编译好的,客户端零编译。**
const NGINX_MAINLINE_PREINSTALL: &str = r#"set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y curl gnupg2 ca-certificates lsb-release debian-archive-keyring
install -d -m 0755 /usr/share/keyrings
curl -fsSL https://nginx.org/keys/nginx_signing.key | gpg --dearmor -o /usr/share/keyrings/nginx-archive-keyring.gpg
DISTRO=$(lsb_release -is | tr '[:upper:]' '[:lower:]')
CODENAME=$(lsb_release -cs)
echo "deb [signed-by=/usr/share/keyrings/nginx-archive-keyring.gpg] http://nginx.org/packages/mainline/${DISTRO} ${CODENAME} nginx" > /etc/apt/sources.list.d/nginx.list
cat > /etc/apt/preferences.d/99nginx <<'PIN'
Package: *
Pin: origin nginx.org
Pin: release o=nginx
Pin-Priority: 900
PIN
"#;

/// slug → 安装前要跑的 shell 脚本(完整可执行 bash 片段)。返回 None
/// 表示无前置步骤,直接走 apt-get install。
pub(crate) fn slug_to_apt_pre_install(slug: &str) -> Option<&'static str> {
    match slug {
        "nginx-mainline" => Some(NGINX_MAINLINE_PREINSTALL),
        _ => None,
    }
}

/// 检查 nginx 二进制是否在 $PATH 里。create_site 用来决定要不要自动
/// 装 nginx-mainline。command -v 命中即认作存在 —— 不区分版本。
pub(crate) async fn is_nginx_installed() -> bool {
    tokio::process::Command::new("sh")
        .args(["-c", "command -v nginx"])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 缺 nginx 时自动装 nginx-mainline(nginx.org 官方预编译 deb)。
/// 已装则什么也不做。返回 true 表示本次走过安装,false 表示已经装好。
/// SKIP_EXECUTE 干跑跳过实际安装,直接返回 false 当作"已装"。
pub(crate) async fn ensure_nginx_installed() -> Result<bool, Status> {
    if is_nginx_installed().await {
        return Ok(false);
    }
    if env::var("RUSTPANEL_APPSTORE_SKIP_EXECUTE").is_ok() {
        return Ok(false);
    }
    // 先加 nginx.org 源 + GPG key + pinning,然后 apt-get install nginx
    if let Some(script) = slug_to_apt_pre_install("nginx-mainline") {
        run_pre_install(script).await?;
    }
    execute_apt_install("nginx").await?;
    Ok(true)
}

/// 跑 pre-install 脚本(bash -c)。SKIP_EXECUTE 时干跑跳过。
async fn run_pre_install(script: &str) -> Result<(), Status> {
    if env::var("RUSTPANEL_APPSTORE_SKIP_EXECUTE").is_ok() {
        return Ok(());
    }
    let output = tokio::process::Command::new("bash")
        .arg("-c")
        .arg(script)
        .env("DEBIAN_FRONTEND", "noninteractive")
        .output()
        .await
        .map_err(io_status)?;
    if !output.status.success() {
        return Err(Status::unavailable(format!(
            "apt pre-install script failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}

async fn deploy_via_apt(
    template: AppTemplate,
    request: DeployAppRequest,
) -> Result<DeployAppResponse, Status> {
    let app_name = sanitize_app_name(if request.app_name.trim().is_empty() {
        &template.slug
    } else {
        &request.app_name
    })?;
    let pkg = slug_to_apt_package(&template.slug).to_owned();
    let pre_install = slug_to_apt_pre_install(&template.slug);
    let pre_note = if pre_install.is_some() {
        "\n前置: 已添加 nginx.org 官方 apt 源 + GPG key + pinning(预编译 deb,无本地编译)"
    } else {
        ""
    };
    let summary = format!(
        "apt 包: {pkg}\n安装命令: apt-get install -y {pkg}{pre_note}\n备注: 由 apt 控制启动 / 服务状态,RustPanel 不接管 systemd 单元。\n下一步: 若包提供服务(redis-server / postgresql / nginx),`systemctl status {pkg}` 查看运行状况。"
    );

    let state = if env::var("RUSTPANEL_APPSTORE_SKIP_EXECUTE").is_ok() {
        "planned".to_owned()
    } else {
        // 有 pre-install 脚本就先跑(添加 nginx.org apt 源 / GPG key / pinning),
        // 然后再 apt-get install。两步分开:pre-install 失败时给出明确的
        // "添加源失败" 而不是被 apt-get install 的 "Unable to locate package" 误导。
        if let Some(script) = slug_to_apt_pre_install(&template.slug) {
            run_pre_install(script).await?;
        }
        execute_apt_install(&pkg).await?;
        "installed".to_owned()
    };

    let now = current_timestamp();
    let app = InstalledApp {
        app_name: app_name.clone(),
        slug: template.slug,
        version: "system".to_owned(),
        image: String::new(),
        // apt 模式下没有专属 unit / compose,把 apt 包名塞进 compose_path,
        // 后续 uninstall 反查时直接拿。
        compose_path: format!("apt:{pkg}"),
        state,
        installed_at_seconds: now,
        updated_at_seconds: now,
    };
    save_installed_app(&app).await?;

    Ok(DeployAppResponse {
        status: Some(ok_response("apt app deployed")),
        compose_path: format!("apt:{pkg}"),
        compose_yaml: summary,
        app: Some(app),
    })
}

async fn uninstall_via_apt(
    template: AppTemplate,
    app: &InstalledApp,
) -> Result<UninstallAppResponse, Status> {
    let pkg = slug_to_apt_package(&template.slug).to_owned();
    if env::var("RUSTPANEL_APPSTORE_SKIP_EXECUTE").is_err() {
        execute_apt_remove(&pkg).await?;
    }
    let app_dir = appstore_root().join(&app.app_name);
    if tokio::fs::try_exists(&app_dir).await.unwrap_or(false) {
        tokio::fs::remove_dir_all(app_dir)
            .await
            .map_err(io_status)?;
    }
    Ok(UninstallAppResponse {
        status: Some(ok_response("apt app uninstalled")),
    })
}

async fn update_via_apt(
    template: AppTemplate,
    mut app: InstalledApp,
) -> Result<UpdateAppResponse, Status> {
    let pkg = slug_to_apt_package(&template.slug).to_owned();
    let summary = format!(
        "apt 升级: apt-get install --only-upgrade -y {pkg}\n备注: 系统包升级由 apt 源决定可获取版本,RustPanel 不强制锁定版本号。"
    );
    if env::var("RUSTPANEL_APPSTORE_SKIP_EXECUTE").is_err() {
        execute_apt_upgrade(&pkg).await?;
    }
    app.state = "updated".to_owned();
    app.updated_at_seconds = current_timestamp();
    save_installed_app(&app).await?;
    Ok(UpdateAppResponse {
        status: Some(ok_response("apt app updated")),
        app: Some(app),
        compose_yaml: summary,
    })
}

async fn ensure_apt_available() -> Result<(), Status> {
    let output = tokio::process::Command::new("sh")
        .args(["-c", "command -v apt-get"])
        .output()
        .await
        .map_err(io_status)?;
    if !output.status.success() {
        return Err(Status::failed_precondition(
            "当前主机没有 apt-get,NativePackage 路径仅支持 Debian / Ubuntu 系",
        ));
    }
    Ok(())
}

async fn run_apt(args: &[&str]) -> Result<(), Status> {
    let output = tokio::process::Command::new("apt-get")
        .args(args)
        .env("DEBIAN_FRONTEND", "noninteractive")
        .output()
        .await
        .map_err(io_status)?;
    if !output.status.success() {
        return Err(Status::unavailable(format!(
            "apt-get {} 失败: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}

async fn execute_apt_install(pkg: &str) -> Result<(), Status> {
    ensure_apt_available().await?;
    // 先 update 一次 source list,避免缓存过期装到不存在的旧版
    run_apt(&["update"]).await?;
    run_apt(&["install", "-y", "--no-install-recommends", pkg]).await?;
    Ok(())
}

async fn execute_apt_remove(pkg: &str) -> Result<(), Status> {
    ensure_apt_available().await?;
    // 只 remove 不 purge:用户的配置 / 数据保留,与 BinaryDownload 路径
    // "保留 config" 的语义一致。
    run_apt(&["remove", "-y", pkg]).await?;
    Ok(())
}

async fn execute_apt_upgrade(pkg: &str) -> Result<(), Status> {
    ensure_apt_available().await?;
    run_apt(&["update"]).await?;
    run_apt(&["install", "--only-upgrade", "-y", pkg]).await?;
    Ok(())
}

// =====================================================================
// Phase G 后续 · sites 模块 ↔ rpxy 集成的数据层与执行胶水。
// 这一层只负责"把一条站点变成 rpxy 配置片段并落盘 / 删除 / reload",
// 不动 site.rs 现有 create_site 流程 —— 该流程默认仍走 nginx,后续
// commit 在 site.rs 里加 "backend = rpxy" 的可选分支时再消费这些函数。
// =====================================================================

use crate::proto::rustpanel::v1::{SiteItem, SiteKind};

const RPXY_FRAGMENT_DIR: &str = "/etc/rpxy/sites.d";

/// 把一条 SiteItem 翻译成 rpxy 的 `[apps.<name>]` 配置片段。
/// 返回 None 表示这种站点不适合直接走 rpxy(典型是纯静态站,
/// 需要 sws 作上游配合,见 static_site_to_sws_args)。
///
/// 同一 site 多 domain 时,**只取第一个作为 server_name** —— rpxy
/// 单个 app 块只支持一个 SNI;多域名要在调用方为每个 domain 生成
/// 独立的 app 块。
pub(crate) fn site_to_rpxy_app_block(site: &SiteItem) -> Option<String> {
    let kind = SiteKind::try_from(site.kind).unwrap_or(SiteKind::Unspecified);
    let primary_domain = site.domains.first()?;
    if primary_domain.trim().is_empty() {
        return None;
    }
    let upstream = match kind {
        SiteKind::ReverseProxy => {
            let target = site.proxy_target.trim();
            if target.is_empty() {
                return None;
            }
            target.to_owned()
        }
        SiteKind::RustBinary => {
            if site.internal_port == 0 {
                return None;
            }
            format!("127.0.0.1:{}", site.internal_port)
        }
        // 纯静态站 / Unspecified:rpxy 自己不服务静态文件,需 sws 配合,
        // 调用方应当走 static_site_to_sws_args 这条路径。
        _ => return None,
    };
    // ssl_enabled 时显式指向 RustPanel ssl 模块按域签下的证书,**不让 rpxy
    // 自己跑 ACME** —— NAT VPS 没有 80/443,只能走 DNS-01,ACME 客户端
    // 已经是 ssl 模块的工作。证书是否实际存在由调用方在写入前自行验证;
    // 这里只发出"路径合约"。
    let tls_line = if site.ssl_enabled {
        let (cert, key) = crate::ssl::acme_cert_paths(primary_domain);
        format!(
            "tls = {{ https_redirection = true, tls_cert_path = \"{}\", tls_cert_key_path = \"{}\" }}\n",
            cert.display(),
            key.display(),
        )
    } else {
        String::new()
    };
    Some(format!(
        "[apps.\"{name}\"]\nserver_name = \"{domain}\"\nreverse_proxy = [{{ location = \"/\", upstream = [{{ location = \"{upstream}\" }}] }}]\n{tls}",
        name = site.name,
        domain = primary_domain,
        upstream = upstream,
        tls = tls_line,
    ))
}

/// 给静态站点生成 systemd template-unit 调用所需的"--root + --port"
/// 参数对,供未来 sws@<site>.service 的 EnvironmentFile / drop-in 用。
/// Static 之外的 kind 返回 None。
pub(crate) fn static_site_to_sws_args(site: &SiteItem) -> Option<(String, u16)> {
    let kind = SiteKind::try_from(site.kind).unwrap_or(SiteKind::Unspecified);
    if kind != SiteKind::Static {
        return None;
    }
    let root = site.root.trim();
    if root.is_empty() {
        return None;
    }
    // internal_port 0 时,后续 commit 在分配真实端口时再补;
    // 干净的 None 让上层判断"还没就绪"。
    if site.internal_port == 0 {
        return None;
    }
    let port: u16 = site.internal_port.try_into().ok()?;
    Some((root.to_owned(), port))
}

/// rpxy 站点片段目录,RUSTPANEL_RPXY_FRAGMENT_DIR 可覆盖(测试用)。
fn rpxy_fragment_dir() -> PathBuf {
    env::var("RUSTPANEL_RPXY_FRAGMENT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(RPXY_FRAGMENT_DIR))
}

fn rpxy_fragment_path(site_name: &str) -> PathBuf {
    rpxy_fragment_dir().join(format!("{site_name}.toml"))
}

/// 把 site_to_rpxy_app_block 生成的片段原子写入 sites.d 目录。
/// 调用方应当确保 site_name 已经做过 sanitize_app_name。
pub(crate) async fn write_rpxy_site_fragment(
    site_name: &str,
    block: &str,
) -> Result<PathBuf, Status> {
    let dir = rpxy_fragment_dir();
    tokio::fs::create_dir_all(&dir).await.map_err(io_status)?;
    let path = rpxy_fragment_path(site_name);
    let tmp = path.with_extension("toml.rustpanel-tmp");
    tokio::fs::write(&tmp, block.as_bytes())
        .await
        .map_err(io_status)?;
    tokio::fs::rename(&tmp, &path).await.map_err(io_status)?;
    Ok(path)
}

#[allow(dead_code)]
pub(crate) async fn remove_rpxy_site_fragment(site_name: &str) -> Result<(), Status> {
    let path = rpxy_fragment_path(site_name);
    if tokio::fs::try_exists(&path).await.unwrap_or(false) {
        tokio::fs::remove_file(&path).await.map_err(io_status)?;
    }
    Ok(())
}

/// rpxy reload —— 容错 systemctl,unit 不存在时返回 failed_precondition
/// 提示用户先在软件商店启用 rpxy。
pub(crate) async fn reload_rpxy_if_running() -> Result<(), Status> {
    if env::var("RUSTPANEL_APPSTORE_SKIP_EXECUTE").is_ok() {
        return Ok(());
    }
    let is_active = tokio::process::Command::new("systemctl")
        .args(["is-active", "--quiet", "rpxy.service"])
        .status()
        .await
        .map_err(io_status)?;
    if !is_active.success() {
        // 没装 / 没启用,直接返回 OK —— 站点片段已写好,装上 rpxy 后
        // 它启动时会自动读到。这里报错只会让 site 操作失败。
        return Ok(());
    }
    systemctl(&["reload", "rpxy.service"]).await
}

// =====================================================================
// static-sites ↔ SWS:每个静态站对应一个 sws@<name>.service instance。
// 用 systemd template unit + per-site toml 配置,跟 rpxy fragment 同形。
// =====================================================================

const SWS_CONFIG_DIR: &str = "/etc/sws";

const SWS_TEMPLATE_UNIT: &str = r#"[Unit]
Description=static-web-server (RustPanel) instance %i
Documentation=https://static-web-server.net/
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/static-web-server --config-file /etc/sws/%i.toml
Restart=on-failure
RestartSec=3s

[Install]
WantedBy=multi-user.target
"#;

/// 纯函数:给一个静态站生成 SWS per-site TOML 配置文本。
/// 监听 127.0.0.1 —— 公网入口由 rpxy 反代,SWS 自己不绑外。
#[allow(dead_code)]
pub(crate) fn render_sws_site_config(root: &str, port: u16) -> String {
    format!(
        "[general]\nhost = \"127.0.0.1\"\nport = {port}\nroot = \"{root}\"\nlog-level = \"info\"\ncompression = true\ncache-control-headers = true\n",
        port = port,
        root = root,
    )
}

fn sws_config_dir() -> PathBuf {
    env::var("RUSTPANEL_SWS_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(SWS_CONFIG_DIR))
}

fn sws_site_config_path(site_name: &str) -> PathBuf {
    sws_config_dir().join(format!("{site_name}.toml"))
}

/// 写 SWS systemd template unit(只在不存在时写)。每台机器一份即可,
/// 后续 sws@<site>.service 都共享它。
pub(crate) async fn ensure_sws_template_unit() -> Result<PathBuf, Status> {
    let dir = systemd_unit_dir();
    tokio::fs::create_dir_all(&dir).await.map_err(io_status)?;
    let path = dir.join("sws@.service");
    if tokio::fs::try_exists(&path).await.unwrap_or(false) {
        return Ok(path);
    }
    let tmp = path.with_extension("service.rustpanel-tmp");
    tokio::fs::write(&tmp, SWS_TEMPLATE_UNIT.as_bytes())
        .await
        .map_err(io_status)?;
    tokio::fs::rename(&tmp, &path).await.map_err(io_status)?;
    Ok(path)
}

/// 写一份 per-site SWS 配置;后续调用方再 `systemctl enable --now
/// sws@<name>.service` 把 instance 拉起来。
pub(crate) async fn write_sws_site_config(
    site_name: &str,
    root: &str,
    port: u16,
) -> Result<PathBuf, Status> {
    let dir = sws_config_dir();
    tokio::fs::create_dir_all(&dir).await.map_err(io_status)?;
    let path = sws_site_config_path(site_name);
    let body = render_sws_site_config(root, port);
    let tmp = path.with_extension("toml.rustpanel-tmp");
    tokio::fs::write(&tmp, body.as_bytes())
        .await
        .map_err(io_status)?;
    tokio::fs::rename(&tmp, &path).await.map_err(io_status)?;
    Ok(path)
}

#[allow(dead_code)]
pub(crate) async fn remove_sws_site_config(site_name: &str) -> Result<(), Status> {
    let path = sws_site_config_path(site_name);
    if tokio::fs::try_exists(&path).await.unwrap_or(false) {
        tokio::fs::remove_file(&path).await.map_err(io_status)?;
    }
    Ok(())
}

/// 检查 static-web-server 二进制是否已安装,作为"要不要起 sws@instance"
/// 的判断条件。没装的话,site.rs 那边的 hook 会直接跳过,不会留下
/// orphan systemd unit。
pub(crate) async fn is_sws_installed() -> bool {
    tokio::fs::try_exists("/usr/local/bin/static-web-server")
        .await
        .unwrap_or(false)
}

/// 给一个静态站点起 sws@<name>.service:写 template unit(幂等)+ per-site
/// 配置 + daemon-reload + enable --now。SKIP_EXECUTE 干跑跳过 systemctl。
/// 返回 Ok(true) 表示已起,Ok(false) 表示 SWS 没装 / 跳过。
pub(crate) async fn start_sws_for_site(
    site_name: &str,
    root: &str,
    port: u16,
) -> Result<bool, Status> {
    if !is_sws_installed().await {
        return Ok(false);
    }
    ensure_sws_template_unit().await?;
    write_sws_site_config(site_name, root, port).await?;
    if env::var("RUSTPANEL_APPSTORE_SKIP_EXECUTE").is_err() {
        systemctl(&["daemon-reload"]).await?;
        systemctl(&["enable", "--now", &format!("sws@{site_name}.service")]).await?;
    }
    Ok(true)
}

pub fn app_templates() -> Vec<AppTemplate> {
    let mut templates = vec![
        // ====== 轻量 systemd-first(NAT VPS / OpenVZ 友好) ======
        native_template(
            "nginx-light",
            "Nginx (apt 发行版默认)",
            "通过 apt 安装 nginx-light(发行版默认源,Debian 12 / Ubuntu 24.04 大概率是 1.22 / 1.24,**没有 HTTP/3**)。常驻 RAM ~5MB。要 HTTP/3 请装 nginx-mainline。",
            AppCategory::WebServer,
            10,
            8,
            5,
            false,
        )
        .with_homepage("https://nginx.org/"),
        native_template(
            "nginx-mainline",
            "Nginx mainline 1.27+(HTTP/3)",
            "从 nginx.org **官方 apt 源**装最新 mainline(deb 由 nginx 团队预编译,客户端零本地编译)。带 --with-http_v3_module,vhost 启用 SSL 时 RustPanel 自动 emit `listen ... quic reuseport;` 让 HTTP/3 真正生效。安装会自动加 nginx.org GPG key + 源 + apt pinning。常驻 RAM ~5MB。",
            AppCategory::WebServer,
            10,
            12,
            5,
            true,
        )
        .with_homepage("https://nginx.org/"),
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
        .with_install(InstallMethod::BinaryDownload)
        .with_homepage("https://caddyserver.com/"),
        native_template(
            "sqlite",
            "SQLite",
            "嵌入式数据库,无需常驻进程,RustPanel 默认推荐。",
            AppCategory::Database,
            0,
            5,
            0,
            true,
        )
        .with_homepage("https://www.sqlite.org/"),
        native_template(
            "redis-tuned",
            "Redis (调优)",
            "apt 安装 redis-server,默认 maxmemory 30MB + LRU 驱逐。常驻 RAM ~10MB。",
            AppCategory::Database,
            32,
            10,
            10,
            true,
        )
        .with_homepage("https://redis.io/"),
        native_template(
            "postgres-tiny",
            "PostgreSQL (低配版)",
            "apt 安装 postgresql-15,shared_buffers=8MB / max_connections=8。生产建议 ≥ 256MB RAM。",
            AppCategory::Database,
            256,
            120,
            60,
            false,
        )
        .with_homepage("https://www.postgresql.org/"),
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
        .with_install(InstallMethod::BinaryDownload)
        .with_homepage("https://gohugo.io/"),
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
        .with_install(InstallMethod::BinaryDownload)
        .with_homepage("https://www.getzola.org/"),
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
        .with_install(InstallMethod::BinaryDownload)
        .with_homepage("https://restic.net/"),
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
        .with_install(InstallMethod::BinaryDownload)
        .with_homepage("https://rclone.org/"),
        native_template(
            "fail2ban",
            "Fail2ban",
            "扫描日志自动封禁恶意 IP。OpenVZ 上需 iptables 模块开放才能工作。",
            AppCategory::Tool,
            32,
            10,
            8,
            true,
        )
        .with_homepage("https://www.fail2ban.org/"),
        native_template(
            "wireguard",
            "WireGuard",
            "现代加密 VPN。OpenVZ 通常需要 wireguard-go(用户态),不依赖内核模块。",
            AppCategory::Vpn,
            16,
            8,
            5,
            false,
        )
        .with_homepage("https://www.wireguard.com/"),
        native_template(
            "certbot",
            "Certbot (Let's Encrypt)",
            "ACME 客户端。RustPanel 内置 ACME 客户端,certbot 仅作为兼容备选。",
            AppCategory::Tool,
            32,
            30,
            0,
            false,
        )
        .with_homepage("https://certbot.eff.org/"),
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
        .with_versions(native_versions(&[("latest", true)]), "latest")
        .with_homepage("https://github.com/junkurihara/rust-rpxy"),
        native_template(
            "static-web-server",
            "static-web-server (SWS)",
            "Rust 写的纯静态文件服务器,常驻 RAM ~5MB。可作 rpxy 上游或独立运行,与 static-sites 模块联动。\n\n官网: https://static-web-server.net/",
            AppCategory::WebServer,
            16,
            8,
            5,
            true,
        )
        .with_install(InstallMethod::BinaryDownload)
        .with_versions(native_versions(&[("latest", true)]), "latest")
        .with_homepage("https://static-web-server.net/"),
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
        .with_versions(native_versions(&[("latest", true)]), "latest")
        .with_homepage("https://github.com/eycorsican/leaf"),
        native_template(
            "vsmtp",
            "vSMTP (Rust 邮件中转)",
            "Rust filter-MTA,做 alias 转发与回复改写;不收件、不存信。出站强制走 SMTP relay (Resend / SES / Postmark),绝不直连 25 端口。常驻 RAM ~35MB。社区维护,默认 off。\n\n官网: https://www.vsmtp.rs/",
            AppCategory::Tool,
            96,
            30,
            35,
            false,
        )
        .with_install(InstallMethod::BinaryDownload)
        .with_versions(native_versions(&[("latest", true)]), "latest")
        .with_homepage("https://www.vsmtp.rs/"),
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
        .with_versions(native_versions(&[("latest", true)]), "latest")
        .with_homepage("https://github.com/EAimTY/tuic"),
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
            homepage: "https://www.mysql.com/".to_owned(),
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
            homepage: "https://redis.io/".to_owned(),
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
            homepage: "https://www.postgresql.org/".to_owned(),
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
            homepage: "https://nginx.org/".to_owned(),
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
            homepage: "https://www.php.net/".to_owned(),
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
        homepage: String::new(),
    }
}

trait AppTemplateExt {
    fn with_install(self, method: InstallMethod) -> Self;
    fn with_versions(self, versions: Vec<AppVersion>, default: &str) -> Self;
    fn with_homepage(self, url: &str) -> Self;
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

    fn with_homepage(mut self, url: &str) -> Self {
        self.homepage = url.to_owned();
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

// =====================================================================
// BinaryDownload executor 的底层工具:
// - 网络:shell out 到 curl(避免引入 reqwest 这个重依赖)
// - 解压:flate2 + tar(项目已用)
// - systemd:Command::new("systemctl")
// 所有外部副作用调用都被 RUSTPANEL_APPSTORE_SKIP_EXECUTE 包住,
// 测试在 skip 模式下走纯数据路径。
// =====================================================================

/// /etc/systemd/system 的写入目录,RUSTPANEL_SYSTEMD_DIR env 可覆盖
/// (测试 / 容器内沙箱用)。
fn systemd_unit_dir() -> PathBuf {
    env::var("RUSTPANEL_SYSTEMD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/etc/systemd/system"))
}

/// 把 BinaryInstallPlan.asset_pattern 里的 {version} 占位符替换为
/// 解析出来的 bare 版本。pattern 自己显式写 `v{version}` 时,
/// 上层不再二次加 'v',避免出现 vv0.10.0 这种重复。
fn expand_asset_pattern(pattern: &str, version: &str) -> String {
    pattern.replace("{version}", version)
}

/// 构造 release 下载 URL。GitHub 的 release tag 通常以 v 开头
/// (`v0.10.0`),所以这里我们用 tag 而不是 bare version。
fn asset_download_url(repo: &str, tag: &str, asset: &str) -> String {
    format!("https://github.com/{repo}/releases/download/{tag}/{asset}")
}

/// 给前端 / 用户的"装这个包会做什么"摘要 —— 用纯文本而不是 JSON,
/// 复用 DeployAppResponse.compose_yaml 字段返回,前端直接显示。
fn render_install_plan_summary(plan: &BinaryInstallPlan, version: &str, asset: &str) -> String {
    format!(
        "上游: {repo}\n版本: {version}\nasset: {asset}\n二进制安装到: {install_to}\nsystemd unit: {unit}\n配置: {config}\n下一步: {hint}",
        repo = plan.upstream_repo,
        version = version,
        asset = asset,
        install_to = plan.install_to,
        unit = systemd_unit_dir()
            .join(format!(
                "{slug}.service",
                slug = plan
                    .install_to
                    .rsplit('/')
                    .next()
                    .unwrap_or("app")
            ))
            .to_string_lossy(),
        config = plan.config_path,
        hint = plan.post_install_hint,
    )
}

/// 解析二进制版本:
/// - 用户传 "" 或 "latest" → 调 GitHub API 取最新 release.tag_name
/// - 用户传具体版本(如 "0.10.0" 或 "v0.10.0")→ 直接用
///
/// 返回 bare 版本(去掉前导 v),供 expand_asset_pattern 用。
async fn resolve_binary_version(
    plan: &BinaryInstallPlan,
    requested: &str,
) -> Result<String, Status> {
    let trimmed = requested.trim();
    if !trimmed.is_empty() && trimmed != "latest" {
        return Ok(trimmed.trim_start_matches('v').to_owned());
    }
    if env::var("RUSTPANEL_APPSTORE_SKIP_EXECUTE").is_ok() {
        // 干跑模式无网络,固定回填一个占位 tag,让 plan summary 仍可渲染
        return Ok("latest".to_owned());
    }
    let api_url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        plan.upstream_repo
    );
    let output = tokio::process::Command::new("curl")
        .args([
            "-fsSL",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: rustpanel-appstore",
            "--retry",
            "3",
            &api_url,
        ])
        .output()
        .await
        .map_err(io_status)?;
    if !output.status.success() {
        return Err(Status::unavailable(format!(
            "GitHub Releases API 拉取失败: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(io_status)?;
    let tag = parsed
        .get("tag_name")
        .and_then(|tag| tag.as_str())
        .ok_or_else(|| Status::internal("GitHub Releases API 响应缺 tag_name"))?;
    Ok(tag.trim_start_matches('v').to_owned())
}

/// 走 curl 下载 asset 到 dest;父目录会自动建好。
async fn download_asset(url: &str, dest: &Path) -> Result<(), Status> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(io_status)?;
    }
    let output = tokio::process::Command::new("curl")
        .args([
            "-fSL",
            "--retry",
            "3",
            "--retry-delay",
            "2",
            "-H",
            "User-Agent: rustpanel-appstore",
            "-o",
        ])
        .arg(dest)
        .arg(url)
        .output()
        .await
        .map_err(io_status)?;
    if !output.status.success() {
        return Err(Status::unavailable(format!(
            "下载失败 ({url}): {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}

/// 解压 / 单文件 gunzip / 原样 copy,统一返回"可执行二进制源路径"。
/// 调用方再根据 plan.binary_path_in_archive 决定走哪个子文件。
async fn extract_archive(archive: &Path, work_dir: &Path) -> Result<PathBuf, Status> {
    tokio::fs::create_dir_all(work_dir)
        .await
        .map_err(io_status)?;
    let name = archive
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        let archive = archive.to_path_buf();
        let dest = work_dir.to_path_buf();
        let dest_for_task = dest.clone();
        tokio::task::spawn_blocking(move || -> Result<(), std::io::Error> {
            let file = std::fs::File::open(&archive)?;
            let gz = flate2::read::GzDecoder::new(file);
            let mut tar = tar::Archive::new(gz);
            tar.unpack(&dest_for_task)?;
            Ok(())
        })
        .await
        .map_err(|join| Status::internal(format!("解压 task panicked: {join}")))?
        .map_err(io_status)?;
        Ok(dest)
    } else if name.ends_with(".gz") {
        // 单文件 gzip;落到 work_dir/<去掉 .gz 的同名>
        let stem = name.strip_suffix(".gz").unwrap_or(&name).to_owned();
        let out_path = work_dir.join(&stem);
        let archive = archive.to_path_buf();
        let out_clone = out_path.clone();
        tokio::task::spawn_blocking(move || -> Result<(), std::io::Error> {
            let in_file = std::fs::File::open(&archive)?;
            let mut gz = flate2::read::GzDecoder::new(in_file);
            let mut out_file = std::fs::File::create(&out_clone)?;
            std::io::copy(&mut gz, &mut out_file)?;
            Ok(())
        })
        .await
        .map_err(|join| Status::internal(format!("解压 task panicked: {join}")))?
        .map_err(io_status)?;
        Ok(out_path)
    } else {
        // 裸二进制(TUIC 那种),不解压,直接当作"已就位"
        Ok(archive.to_path_buf())
    }
}

/// 把 src 二进制安装到 dest:chmod +x,先写 .tmp 再 rename 保证原子。
/// dest 已存在时被覆盖(rename 在同一文件系统下是原子的)。
async fn install_binary_atomic(src: &Path, dest: &Path) -> Result<(), Status> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(io_status)?;
    }
    let tmp = dest.with_extension("rustpanel-tmp");
    tokio::fs::copy(src, &tmp).await.map_err(io_status)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = tokio::fs::metadata(&tmp)
            .await
            .map_err(io_status)?
            .permissions();
        perms.set_mode(0o755);
        tokio::fs::set_permissions(&tmp, perms)
            .await
            .map_err(io_status)?;
    }
    tokio::fs::rename(&tmp, dest).await.map_err(io_status)?;
    Ok(())
}

/// 写 systemd unit 到 RUSTPANEL_SYSTEMD_DIR(默认 /etc/systemd/system),
/// 同样 tmp + rename 保证原子。
async fn write_systemd_unit(slug: &str, content: &str) -> Result<PathBuf, Status> {
    let dir = systemd_unit_dir();
    tokio::fs::create_dir_all(&dir).await.map_err(io_status)?;
    let path = dir.join(format!("{slug}.service"));
    let tmp = path.with_extension("service.rustpanel-tmp");
    tokio::fs::write(&tmp, content.as_bytes())
        .await
        .map_err(io_status)?;
    tokio::fs::rename(&tmp, &path).await.map_err(io_status)?;
    Ok(path)
}

/// 写配置文件 —— **只在不存在时写**,绝不覆盖用户已经手改过的内容。
async fn write_config_if_missing(path: &Path, content: &str) -> Result<(), Status> {
    if tokio::fs::try_exists(path).await.unwrap_or(false) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(io_status)?;
    }
    tokio::fs::write(path, content.as_bytes())
        .await
        .map_err(io_status)?;
    Ok(())
}

/// 包一层 systemctl —— 失败时把 stderr 直接当作 Status::unavailable 抛出。
async fn systemctl(args: &[&str]) -> Result<(), Status> {
    let output = tokio::process::Command::new("systemctl")
        .args(args)
        .output()
        .await
        .map_err(io_status)?;
    if !output.status.success() {
        return Err(Status::unavailable(format!(
            "systemctl {} 失败: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}

/// 全流程:下载 → 解压 → 安装二进制 → 写 unit → 写配置 → enable+start。
/// 调用前确保 RUSTPANEL_APPSTORE_SKIP_EXECUTE 未设置。
async fn execute_binary_install(
    slug: &str,
    plan: &BinaryInstallPlan,
    version: &str,
    asset_name: &str,
) -> Result<(), Status> {
    let work_root = appstore_root().join(slug);
    let cache_dir = work_root.join("cache");
    let work_dir = work_root.join("work");
    tokio::fs::create_dir_all(&cache_dir)
        .await
        .map_err(io_status)?;
    tokio::fs::create_dir_all(&work_dir)
        .await
        .map_err(io_status)?;

    let archive_path = cache_dir.join(asset_name);
    let tag = format!("v{version}");
    let download_url = asset_download_url(plan.upstream_repo, &tag, asset_name);
    download_asset(&download_url, &archive_path).await?;

    let extract_root = extract_archive(&archive_path, &work_dir).await?;
    let binary_src = if plan.binary_path_in_archive.is_empty() {
        extract_root
    } else {
        let rel = expand_asset_pattern(plan.binary_path_in_archive, version);
        // 不论 extract_root 是目录(tar.gz 解出来的根)还是文件(.gz / 裸二进制),
        // 都按相对路径在 work_dir 里找。
        if extract_root.is_dir() {
            extract_root.join(rel)
        } else {
            work_dir.join(rel)
        }
    };

    install_binary_atomic(&binary_src, Path::new(plan.install_to)).await?;
    write_systemd_unit(slug, plan.systemd_unit).await?;
    write_config_if_missing(Path::new(plan.config_path), plan.config_template).await?;
    systemctl(&["daemon-reload"]).await?;
    systemctl(&["enable", "--now", &format!("{slug}.service")]).await?;
    Ok(())
}

/// 卸载:停服务、disable、删 unit、删二进制;config / 数据保留。
async fn execute_binary_uninstall(slug: &str, plan: &BinaryInstallPlan) -> Result<(), Status> {
    let service = format!("{slug}.service");
    // 停 + disable 即使 unit 已经不存在也不该 fatal —— 容错处理。
    let _ = systemctl(&["disable", "--now", &service]).await;
    let unit_path = systemd_unit_dir().join(&service);
    if tokio::fs::try_exists(&unit_path).await.unwrap_or(false) {
        tokio::fs::remove_file(&unit_path)
            .await
            .map_err(io_status)?;
    }
    let binary_path = Path::new(plan.install_to);
    if tokio::fs::try_exists(binary_path).await.unwrap_or(false) {
        tokio::fs::remove_file(binary_path)
            .await
            .map_err(io_status)?;
    }
    let _ = systemctl(&["daemon-reload"]).await;
    Ok(())
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
        // 都至少给出 latest 这一项可选版本,且 homepage 字段精确匹配。
        let templates = app_templates();
        let expected = [
            (
                "rpxy",
                AppCategory::WebServer,
                "https://github.com/junkurihara/rust-rpxy",
            ),
            (
                "static-web-server",
                AppCategory::WebServer,
                "https://static-web-server.net/",
            ),
            (
                "leaf",
                AppCategory::Vpn,
                "https://github.com/eycorsican/leaf",
            ),
            ("vsmtp", AppCategory::Tool, "https://www.vsmtp.rs/"),
            ("tuic", AppCategory::Vpn, "https://github.com/EAimTY/tuic"),
        ];
        for (slug, category, homepage) in expected {
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
            assert_eq!(
                template.homepage, homepage,
                "{slug} homepage field mismatch"
            );
        }
    }

    #[test]
    fn every_template_has_homepage() {
        // 软件商店所有模板都必须填 homepage,前端卡片才能渲染上游官网入口。
        for template in app_templates() {
            assert!(
                !template.homepage.is_empty(),
                "template `{}` is missing homepage",
                template.slug
            );
            assert!(
                template.homepage.starts_with("http://")
                    || template.homepage.starts_with("https://"),
                "template `{}` homepage should be a URL, got: {}",
                template.slug,
                template.homepage
            );
        }
    }

    fn make_site(kind: SiteKind, name: &str, domain: &str) -> SiteItem {
        SiteItem {
            name: name.to_owned(),
            domains: vec![domain.to_owned()],
            root: String::new(),
            proxy_target: String::new(),
            ssl_enabled: false,
            config_path: String::new(),
            engine: String::new(),
            public_path: String::new(),
            listen_addr: String::new(),
            kind: kind as i32,
            binding: None,
            tls_strategy: 0,
            systemd_unit: String::new(),
            internal_port: 0,
        }
    }

    #[test]
    fn site_to_rpxy_app_block_for_reverse_proxy_includes_upstream() {
        let mut site = make_site(SiteKind::ReverseProxy, "blog", "blog.example.com");
        site.proxy_target = "127.0.0.1:8080".to_owned();
        site.ssl_enabled = true;
        let block = site_to_rpxy_app_block(&site).expect("reverse-proxy 块应生成");
        assert!(block.contains("[apps.\"blog\"]"));
        assert!(block.contains("server_name = \"blog.example.com\""));
        assert!(block.contains("location = \"127.0.0.1:8080\""));
        // ssl_enabled → 显式指向 ssl 模块按域签下的证书,不让 rpxy 自跑 ACME
        assert!(block.contains("tls_cert_path = "));
        assert!(block.contains("tls_cert_key_path = "));
        assert!(block.contains("blog.example.com/fullchain.pem"));
        assert!(block.contains("blog.example.com/privkey.pem"));
        assert!(!block.contains("acme = true"));
    }

    #[test]
    fn site_to_rpxy_app_block_for_rust_binary_uses_loopback_port() {
        let mut site = make_site(SiteKind::RustBinary, "api", "api.example.com");
        site.internal_port = 4321;
        let block = site_to_rpxy_app_block(&site).expect("rust-binary 块应生成");
        assert!(block.contains("location = \"127.0.0.1:4321\""));
        // 默认 ssl_enabled=false 时不出 tls 段
        assert!(!block.contains("tls_cert_path"));
        assert!(!block.contains("tls = "));
    }

    #[test]
    fn site_to_rpxy_app_block_skips_pure_static() {
        // 纯静态站需要 sws 上游配合,rpxy 自己不服务文件 → None
        let mut site = make_site(SiteKind::Static, "docs", "docs.example.com");
        site.root = "/var/www/docs".to_owned();
        assert!(site_to_rpxy_app_block(&site).is_none());
    }

    #[test]
    fn site_to_rpxy_app_block_skips_missing_upstream() {
        // ReverseProxy 但 proxy_target 空 → None
        let site = make_site(SiteKind::ReverseProxy, "bare", "bare.example.com");
        assert!(site_to_rpxy_app_block(&site).is_none());
        // RustBinary 但 internal_port=0 → None
        let site = make_site(SiteKind::RustBinary, "bare", "bare.example.com");
        assert!(site_to_rpxy_app_block(&site).is_none());
        // 无 domain → None
        let mut site = make_site(SiteKind::ReverseProxy, "nodom", "");
        site.domains.clear();
        site.proxy_target = "127.0.0.1:1".to_owned();
        assert!(site_to_rpxy_app_block(&site).is_none());
    }

    #[test]
    fn render_sws_site_config_emits_required_fields() {
        let config = render_sws_site_config("/var/www/blog", 9001);
        assert!(config.contains("host = \"127.0.0.1\""));
        assert!(config.contains("port = 9001"));
        assert!(config.contains("root = \"/var/www/blog\""));
        // 默认开压缩与 cache-control,符合 NAT VPS 带宽紧张的场景
        assert!(config.contains("compression = true"));
        assert!(config.contains("cache-control-headers = true"));
    }

    #[test]
    fn static_site_to_sws_args_only_when_ready() {
        let mut site = make_site(SiteKind::Static, "docs", "docs.example.com");
        site.root = "/var/www/docs".to_owned();
        // internal_port=0:还没分配,返回 None
        assert!(static_site_to_sws_args(&site).is_none());
        site.internal_port = 9001;
        assert_eq!(
            static_site_to_sws_args(&site),
            Some(("/var/www/docs".to_owned(), 9001))
        );
        // 非 Static 的 kind 永远 None
        let mut site = make_site(SiteKind::ReverseProxy, "x", "x.example.com");
        site.root = "/var/www/x".to_owned();
        site.internal_port = 9001;
        assert!(static_site_to_sws_args(&site).is_none());
    }

    #[test]
    fn slug_to_apt_package_maps_known_slugs_and_falls_back() {
        // 命名不一致的:走 map
        assert_eq!(slug_to_apt_package("redis-tuned"), "redis-server");
        assert_eq!(slug_to_apt_package("postgres-tiny"), "postgresql");
        assert_eq!(slug_to_apt_package("sqlite"), "sqlite3");
        // 命名一致的:slug 兜底
        assert_eq!(slug_to_apt_package("nginx-light"), "nginx-light");
        assert_eq!(slug_to_apt_package("fail2ban"), "fail2ban");
        assert_eq!(slug_to_apt_package("certbot"), "certbot");
        // nginx-mainline → 包名 nginx(nginx.org 官方源里就叫这个)
        assert_eq!(slug_to_apt_package("nginx-mainline"), "nginx");
        // 未知 slug 也兜底,executor 端的 apt-get 自己报"无此包"
        assert_eq!(slug_to_apt_package("nonexistent"), "nonexistent");
    }

    #[test]
    fn nginx_mainline_has_apt_repo_pre_install_script() {
        // nginx-mainline 必须配套 pre-install 脚本,否则只能装到发行版
        // 老 nginx,装个 HTTP/3 寂寞。脚本里应当出现 nginx.org 官方源、
        // GPG key 路径和 apt pinning。
        let script = slug_to_apt_pre_install("nginx-mainline").expect("脚本必须有");
        assert!(script.contains("nginx.org"));
        assert!(script.contains("nginx_signing.key"));
        assert!(script.contains("nginx-archive-keyring.gpg"));
        assert!(script.contains("/etc/apt/sources.list.d/nginx.list"));
        assert!(script.contains("/etc/apt/preferences.d/99nginx"));
        // 其它 slug 不应有 pre-install
        assert!(slug_to_apt_pre_install("nginx-light").is_none());
        assert!(slug_to_apt_pre_install("redis-tuned").is_none());
    }

    #[test]
    fn expand_asset_pattern_substitutes_version_and_handles_v_prefix() {
        // bare {version} 替换
        assert_eq!(
            expand_asset_pattern("rpxy-{version}-x86_64-linux.tar.gz", "0.10.0"),
            "rpxy-0.10.0-x86_64-linux.tar.gz"
        );
        // pattern 自己写了 v 前缀:不再重复加,版本数字直接进去
        assert_eq!(
            expand_asset_pattern("sws-v{version}-musl.tar.gz", "2.32.0"),
            "sws-v2.32.0-musl.tar.gz"
        );
        // binary_path_in_archive 也能复用同一替换器
        assert_eq!(
            expand_asset_pattern("sws-v{version}-musl/sws", "2.32.0"),
            "sws-v2.32.0-musl/sws"
        );
        // 没占位时原样返回
        assert_eq!(expand_asset_pattern("tuic-server", "0.0.1"), "tuic-server");
    }

    #[test]
    fn asset_download_url_matches_github_release_layout() {
        assert_eq!(
            asset_download_url(
                "junkurihara/rust-rpxy",
                "v0.10.0",
                "rpxy-0.10.0-x86_64-unknown-linux-musl.tar.gz",
            ),
            "https://github.com/junkurihara/rust-rpxy/releases/download/v0.10.0/rpxy-0.10.0-x86_64-unknown-linux-musl.tar.gz",
        );
    }

    #[test]
    fn render_install_plan_summary_lists_key_fields() {
        let plan = phase_g_install_plan("rpxy").expect("rpxy plan");
        let summary =
            render_install_plan_summary(&plan, "0.10.0", "rpxy-0.10.0-x86_64-linux.tar.gz");
        assert!(summary.contains("junkurihara/rust-rpxy"));
        assert!(summary.contains("0.10.0"));
        assert!(summary.contains("rpxy-0.10.0-x86_64-linux.tar.gz"));
        assert!(summary.contains("/usr/local/bin/rpxy"));
        assert!(summary.contains("/etc/rpxy/config.toml"));
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
