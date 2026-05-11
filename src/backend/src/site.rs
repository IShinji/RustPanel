use std::{
    env,
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use serde::{Deserialize, Serialize};
use tera::{Context, Tera};
use tonic::{Request, Response as GrpcResponse, Status};
use uuid::Uuid;

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        site_service_server::SiteService, CreateSiteRequest, CreateSiteResponse,
        DeleteReverseProxyRuleRequest, DeleteReverseProxyRuleResponse, DeleteSiteRequest,
        DeleteSiteResponse, ListReverseProxyRulesRequest, ListReverseProxyRulesResponse,
        ListRewriteTemplatesRequest, ListRewriteTemplatesResponse, ListSitesRequest,
        ListSitesResponse, ReloadNginxRequest, ReloadNginxResponse, RenderRewriteTemplateRequest,
        RenderRewriteTemplateResponse, ReverseProxyRule, RewriteTemplate, SiteBindKind,
        SiteBinding, SiteItem, SiteKind, SiteTlsStrategy, UpsertReverseProxyRuleRequest,
        UpsertReverseProxyRuleResponse, UpstreamTarget,
    },
};

const DEFAULT_NGINX_SITES_DIR: &str = "/etc/nginx/sites-enabled";
const DEFAULT_SITE_STATE_ROOT: &str = "/tmp/rustpanel/site";
const SITE_ENGINE_ENV: &str = "RUSTPANEL_SITE_ENGINE";
const SITE_ENGINE_BUILTIN: &str = "builtin";
const SITE_ENGINE_NGINX: &str = "nginx";
const NGINX_TEMPLATE: &str = r#"server {
    listen 80;
    server_name {{ domains | join(sep=" ") }};

    location ^~ /.well-known/acme-challenge/ {
        root /var/www/rustpanel-acme;
        default_type "text/plain";
    }

{% if proxy_target != "" %}
    location / {
        proxy_pass {{ proxy_target }};
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
{% else %}
    root {{ root }};
    index index.html index.htm;
    location / {
        try_files $uri $uri/ /index.html;
    }
{% endif %}

{% if ssl_enabled %}
    listen 443 ssl http2;
    ssl_certificate /etc/letsencrypt/live/{{ primary_domain }}/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/{{ primary_domain }}/privkey.pem;
{% endif %}
}
"#;

#[derive(Clone, Debug)]
pub struct SiteServiceImpl {
    store: SiteStore,
}

impl SiteServiceImpl {
    pub fn new() -> Self {
        Self {
            store: SiteStore::from_env(),
        }
    }
}

impl Default for SiteServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl SiteService for SiteServiceImpl {
    async fn list_sites(
        &self,
        _request: Request<ListSitesRequest>,
    ) -> Result<GrpcResponse<ListSitesResponse>, Status> {
        let sites = list_site_configs(&self.store).await?;

        Ok(GrpcResponse::new(ListSitesResponse {
            status: Some(ok_response("ok")),
            sites,
        }))
    }

    async fn create_site(
        &self,
        request: Request<CreateSiteRequest>,
    ) -> Result<GrpcResponse<CreateSiteResponse>, Status> {
        let request = request.into_inner();
        validate_site_request(&request)?;
        let engine = site_engine(&request.engine);
        if engine == SITE_ENGINE_BUILTIN {
            let site = self.store.upsert_builtin_site(request).await?;
            return Ok(GrpcResponse::new(CreateSiteResponse {
                status: Some(ok_response("builtin site created")),
                site: Some(site),
                rendered_config: String::new(),
            }));
        }

        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_SITES)?;

        // nginx 缺就自动从 nginx.org 装 mainline(预编译 deb)。失败不
        // 致命:vhost 文件还是会写,用户事后自己 apt 装也行;但若装了的话
        // 会立刻拿到带 --with-http_v3_module 的 1.27+,HTTP/3 自动生效。
        // 把"装了" / "没装上"的结果先存着,最后塞进 CreateSiteResponse.status.message
        // 让前端 banner 直接显示,不再只在后端日志里 warn 用户根本看不到。
        let mut bootstrap_notes: Vec<String> = Vec::new();
        match crate::appstore::ensure_nginx_installed().await {
            Ok(true) => {
                tracing::info!(
                    target = "site.bootstrap",
                    "nginx 不存在,已自动装 nginx-mainline"
                );
                bootstrap_notes
                    .push("已自动安装 nginx-mainline(nginx.org 官方源,带 HTTP/3)".to_owned());
            }
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(
                    target = "site.bootstrap",
                    error = %error,
                    "auto-install nginx-mainline failed (non-fatal; user can install manually)"
                );
                bootstrap_notes.push(format!(
                    "⚠️ nginx 自动安装失败:{}(站点配置已写到 nginx 配置目录,等你手动装好 nginx 后 reload 即生效)",
                    error.message()
                ));
            }
        }

        // Phase C:如果传了 kind/binding,渲染 v6/NAT 端口感知的 vhost,
        // 同时调用 capability 预留对应端口/v6 地址,避免冲突。
        let kind = SiteKind::try_from(request.kind).unwrap_or(SiteKind::Unspecified);
        let binding = request.binding.clone();
        let tls = SiteTlsStrategy::try_from(request.tls_strategy).unwrap_or_default();
        let rendered_config = if matches!(
            kind,
            SiteKind::Static | SiteKind::RustBinary | SiteKind::ReverseProxy
        ) {
            render_phase_c_site(&request, kind, binding.as_ref(), tls)?
        } else {
            render_site_config(&request)?
        };

        // 启用 SSL 但还没签真证书时,先在 ssl_certificate 路径写一份自签
        // 兜底,这样 nginx -t / reload 立刻通过;后续 ACME 完成会覆盖。
        // 多域名时按 primary domain 来,与 render_phase_c_site 里 tls_block
        // 的 ssl_certificate 路径对齐。
        if (request.ssl_enabled || tls != SiteTlsStrategy::None) && !request.domains.is_empty() {
            let primary = &request.domains[0];
            if let Err(error) = crate::ssl::bootstrap_self_signed_if_missing(primary).await {
                tracing::warn!(
                    target = "site.ssl-bootstrap",
                    domain = %primary,
                    error = %error,
                    "snakeoil bootstrap failed; nginx -t may reject vhost until real cert is issued"
                );
            }
        }

        let config_path =
            nginx_sites_dir().join(format!("rustpanel-{}.conf", safe_name(&request.name)?));
        if let Some(parent) = config_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(io_status)?;
        }
        tokio::fs::write(&config_path, rendered_config.as_bytes())
            .await
            .map_err(io_status)?;

        let systemd_unit = if kind == SiteKind::RustBinary {
            format!("rustpanel-site-{}.service", safe_name(&request.name)?)
        } else {
            String::new()
        };
        // RustBinary 内部 nginx → 本地 127.0.0.1:internal_port,默认 9100+ 区段。
        // Static 在 sws 装上时也分配一个 internal_port(8200+ 区段),
        // 让"装上 SWS 后切到 SWS 后端"成为零额外配置的事;没装 SWS 时
        // 这个端口什么都不占,nginx vhost 照常服务静态文件。
        let internal_port = match kind {
            SiteKind::RustBinary => 9100 + (hash_name_to_port_offset(&request.name) % 800),
            SiteKind::Static => 8200 + (hash_name_to_port_offset(&request.name) % 800),
            _ => 0,
        };

        let site = SiteItem {
            name: request.name,
            domains: request.domains,
            root: request.root,
            proxy_target: request.proxy_target,
            ssl_enabled: request.ssl_enabled || tls != SiteTlsStrategy::None,
            config_path: config_path.to_string_lossy().to_string(),
            engine,
            public_path: String::new(),
            listen_addr: request.listen_addr,
            kind: kind as i32,
            binding,
            tls_strategy: tls as i32,
            systemd_unit,
            internal_port,
        };

        // 落 sidecar 元数据:list_site_configs 之后回到详情抽屉能拿到
        // 完整的 domains/kind/root/binding,SSL Tab 也能拿到 primaryDomain
        // 去签证书。失败不阻塞创建(只 log),但这是软伤,正常应当成功。
        if let Err(error) = self.store.save_nginx_site_metadata(&site).await {
            tracing::warn!(
                target = "site.metadata",
                site = %site.name,
                error = %error,
                "save nginx site metadata failed (non-fatal; list_sites will fall back to stub)"
            );
        }

        // 机会主义起 sws@<name>.service:Static 站点 + SWS 已装时,
        // 写 per-site 配置并 enable --now;SWS 没装就跳过,什么都不留下。
        // 失败不阻塞站点创建。
        if let Some((root, port)) = crate::appstore::static_site_to_sws_args(&site) {
            let safe = safe_name(&site.name).unwrap_or_else(|_| site.name.clone());
            match crate::appstore::start_sws_for_site(&safe, &root, port).await {
                Ok(true) => tracing::info!(
                    target = "site.sws",
                    site = %site.name,
                    port = port,
                    "sws@instance started"
                ),
                Ok(false) => {} // SWS 没装,静默跳过
                Err(error) => tracing::warn!(
                    target = "site.sws",
                    site = %site.name,
                    error = %error,
                    "sws@instance start failed (non-fatal)"
                ),
            }
        }

        // 机会主义写一份 rpxy 站点片段:
        // - rpxy 没装 / 没启用时,文件静静躺在 sites.d 里,装上 rpxy
        //   后它会自动读到(rpxy 默认 watch 该目录)
        // - 写失败不阻塞 site 创建 —— 用户已经看到 nginx vhost 落了盘,
        //   rpxy 这条腿就算暂时 broken,不影响主流程
        // - Static 站点 site_to_rpxy_app_block 返回 None(rpxy 不直接
        //   服务静态文件,需 sws 上游配合,见 static_site_to_sws_args)
        if let Some(block) = crate::appstore::site_to_rpxy_app_block(&site) {
            let safe = safe_name(&site.name).unwrap_or_else(|_| site.name.clone());
            if let Err(error) = crate::appstore::write_rpxy_site_fragment(&safe, &block).await {
                tracing::warn!(
                    target = "site.rpxy",
                    site = %site.name,
                    error = %error,
                    "rpxy site fragment write failed (non-fatal)"
                );
            } else if let Err(error) = crate::appstore::reload_rpxy_if_running().await {
                tracing::warn!(
                    target = "site.rpxy",
                    site = %site.name,
                    error = %error,
                    "rpxy reload failed (non-fatal)"
                );
            }
        }

        // 站点创建成功的同时,如果 bootstrap 阶段产生了 notes(nginx 自动
        // 安装结果之类),把它合进 status.message —— 前端 message banner
        // 就能直接展示,无需走 logs。
        let status_message = if bootstrap_notes.is_empty() {
            "站点已创建".to_owned()
        } else {
            format!("站点已创建 · {}", bootstrap_notes.join(" · "))
        };

        Ok(GrpcResponse::new(CreateSiteResponse {
            status: Some(ok_response(&status_message)),
            site: Some(site),
            rendered_config,
        }))
    }

    async fn delete_site(
        &self,
        request: Request<DeleteSiteRequest>,
    ) -> Result<GrpcResponse<DeleteSiteResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_SITES)?;
        let name = request.into_inner().name;
        if name.trim().is_empty() {
            return Err(Status::invalid_argument("name is required"));
        }
        let safe = safe_name(&name)?;
        let mut cleaned: Vec<String> = Vec::new();

        // 1) nginx vhost 配置文件
        let conf_path = nginx_sites_dir().join(format!("rustpanel-{safe}.conf"));
        if tokio::fs::try_exists(&conf_path).await.unwrap_or(false) {
            tokio::fs::remove_file(&conf_path)
                .await
                .map_err(io_status)?;
            cleaned.push(conf_path.to_string_lossy().to_string());
        }

        // 2) sidecar 元数据(create_site 时落的 StoredSite JSON)
        let sidecar_path = self.store.nginx_metadata_path(&safe);
        if tokio::fs::try_exists(&sidecar_path).await.unwrap_or(false) {
            tokio::fs::remove_file(&sidecar_path)
                .await
                .map_err(io_status)?;
            cleaned.push(sidecar_path.to_string_lossy().to_string());
        }

        // 3) rpxy 站点片段(opportunistic;rpxy 可能没装,fragment 可能不存在)
        if crate::appstore::remove_rpxy_site_fragment(&safe)
            .await
            .is_ok()
        {
            cleaned.push(format!("rpxy fragment for {safe}"));
        }
        let _ = crate::appstore::reload_rpxy_if_running().await;

        // 4) sws per-site 配置 + 停 instance
        if crate::appstore::remove_sws_site_config(&safe).await.is_ok() {
            cleaned.push(format!("sws config for {safe}"));
        }
        if env::var("RUSTPANEL_APPSTORE_SKIP_EXECUTE").is_err() {
            let unit = format!("sws@{safe}.service");
            // disable --now 即使 unit 不存在也容错;失败不阻塞
            let _ = tokio::process::Command::new("systemctl")
                .args(["disable", "--now", &unit])
                .output()
                .await;
        }

        // 5) builtin sites.json 里的记录(engine = builtin 的路径)
        let mut builtin = self.store.load_builtin_sites().await?;
        let before = builtin.len();
        builtin.retain(|item| safe_name(&item.name).ok().as_deref() != Some(&safe));
        if builtin.len() != before {
            self.store.save_builtin_sites(&builtin).await?;
            cleaned.push("builtin sites.json entry".to_owned());
        }

        // 6) 重载 nginx —— 已删配置文件,需要让 nginx 真正 unregister vhost。
        //    -s reload 失败说明可能 nginx 没装/没起;静默忽略,不阻塞删除流程。
        if env::var("RUSTPANEL_APPSTORE_SKIP_EXECUTE").is_err() {
            let _ = tokio::process::Command::new("nginx")
                .args(["-s", "reload"])
                .output()
                .await;
        }

        Ok(GrpcResponse::new(DeleteSiteResponse {
            status: Some(ok_response("site deleted")),
            cleaned_paths: cleaned,
        }))
    }

    async fn reload_nginx(
        &self,
        _request: Request<ReloadNginxRequest>,
    ) -> Result<GrpcResponse<ReloadNginxResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_SITES)?;
        let test = tokio::process::Command::new("nginx")
            .arg("-t")
            .output()
            .await
            .map_err(io_status)?;
        if !test.status.success() {
            return Ok(GrpcResponse::new(ReloadNginxResponse {
                status: Some(crate::error_response(
                    1,
                    String::from_utf8_lossy(&test.stderr).to_string(),
                )),
                output: String::from_utf8_lossy(&test.stderr).to_string(),
            }));
        }

        let reload = tokio::process::Command::new("nginx")
            .arg("-s")
            .arg("reload")
            .output()
            .await
            .map_err(io_status)?;
        let output = if reload.status.success() {
            String::from_utf8_lossy(&reload.stdout).to_string()
        } else {
            String::from_utf8_lossy(&reload.stderr).to_string()
        };

        Ok(GrpcResponse::new(ReloadNginxResponse {
            status: Some(if reload.status.success() {
                ok_response("nginx reloaded")
            } else {
                crate::error_response(1, output.clone())
            }),
            output,
        }))
    }

    async fn list_rewrite_templates(
        &self,
        _request: Request<ListRewriteTemplatesRequest>,
    ) -> Result<GrpcResponse<ListRewriteTemplatesResponse>, Status> {
        Ok(GrpcResponse::new(ListRewriteTemplatesResponse {
            status: Some(ok_response("ok")),
            templates: rewrite_templates(),
        }))
    }

    async fn render_rewrite_template(
        &self,
        request: Request<RenderRewriteTemplateRequest>,
    ) -> Result<GrpcResponse<RenderRewriteTemplateResponse>, Status> {
        let request = request.into_inner();
        let rendered_config = if request.custom_content.trim().is_empty() {
            rewrite_templates()
                .into_iter()
                .find(|template| template.id == request.template_id)
                .map(|template| template.content)
                .ok_or_else(|| Status::not_found("rewrite template not found"))?
        } else {
            request.custom_content
        };

        Ok(GrpcResponse::new(RenderRewriteTemplateResponse {
            status: Some(ok_response("rewrite template rendered")),
            rendered_config,
        }))
    }

    async fn list_reverse_proxy_rules(
        &self,
        _request: Request<ListReverseProxyRulesRequest>,
    ) -> Result<GrpcResponse<ListReverseProxyRulesResponse>, Status> {
        Ok(GrpcResponse::new(ListReverseProxyRulesResponse {
            status: Some(ok_response("ok")),
            rules: self
                .store
                .load_proxy_rules()
                .await?
                .into_iter()
                .map(StoredReverseProxyRule::into_proto)
                .collect(),
        }))
    }

    async fn upsert_reverse_proxy_rule(
        &self,
        request: Request<UpsertReverseProxyRuleRequest>,
    ) -> Result<GrpcResponse<UpsertReverseProxyRuleResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_SITES)?;
        let mut rule = request
            .into_inner()
            .rule
            .ok_or_else(|| Status::invalid_argument("reverse proxy rule is required"))?;
        validate_proxy_rule(&rule)?;
        let now = current_timestamp();
        let mut rules = self.store.load_proxy_rules().await?;
        let old = rules.iter().find(|stored| stored.id == rule.id).cloned();
        if rule.id.trim().is_empty() {
            rule.id = Uuid::new_v4().to_string();
            rule.created_at_seconds = now;
        } else if let Some(old) = &old {
            rule.created_at_seconds = old.created_at_seconds;
        } else {
            rule.created_at_seconds = now;
        }
        rule.updated_at_seconds = now;
        let config_path =
            nginx_sites_dir().join(format!("rustpanel-proxy-{}.conf", safe_name(&rule.name)?));
        rule.config_path = config_path.to_string_lossy().to_string();
        let rendered_config = render_proxy_rule(&rule)?;
        if let Some(parent) = config_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(io_status)?;
        }
        tokio::fs::write(&config_path, rendered_config.as_bytes())
            .await
            .map_err(io_status)?;
        rules.retain(|stored| stored.id != rule.id);
        rules.push(StoredReverseProxyRule::from_proto(rule.clone()));
        self.store.save_proxy_rules(&rules).await?;

        Ok(GrpcResponse::new(UpsertReverseProxyRuleResponse {
            status: Some(ok_response("reverse proxy rule saved")),
            rule: Some(rule),
            rendered_config,
        }))
    }

    async fn delete_reverse_proxy_rule(
        &self,
        request: Request<DeleteReverseProxyRuleRequest>,
    ) -> Result<GrpcResponse<DeleteReverseProxyRuleResponse>, Status> {
        let id = request.into_inner().id;
        let mut rules = self.store.load_proxy_rules().await?;
        let removed = rules
            .iter()
            .find(|stored| stored.id == id)
            .cloned()
            .ok_or_else(|| Status::not_found("reverse proxy rule not found"))?;
        if !removed.config_path.trim().is_empty() {
            let _ = tokio::fs::remove_file(&removed.config_path).await;
        }
        rules.retain(|stored| stored.id != id);
        self.store.save_proxy_rules(&rules).await?;

        Ok(GrpcResponse::new(DeleteReverseProxyRuleResponse {
            status: Some(ok_response("reverse proxy rule deleted")),
        }))
    }
}

async fn list_site_configs(store: &SiteStore) -> Result<Vec<SiteItem>, Status> {
    let mut sites = Vec::new();
    sites.extend(store.load_builtin_sites().await?);
    if !crate::runtime::from_env().is_enabled(crate::runtime::MODULE_SITES) {
        return Ok(sites);
    }

    let mut entries = match tokio::fs::read_dir(nginx_sites_dir()).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(sites),
        Err(error) => return Err(io_status(error)),
    };

    while let Some(entry) = entries.next_entry().await.map_err(io_status)? {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("conf") {
            continue;
        }
        // 文件名形如 rustpanel-<safe_name>.conf 或 rustpanel-proxy-<safe>.conf;
        // 反代规则的 .conf 不属于 site,跳过。
        let stem = path
            .file_stem()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        let safe_name_part = match stem.strip_prefix("rustpanel-") {
            Some(rest) if !rest.starts_with("proxy-") => rest.to_owned(),
            _ => {
                // 不认识前缀的 .conf 当作 legacy stub 推一份,name 用文件 stem
                sites.push(legacy_site_stub(&path, stem.clone()));
                continue;
            }
        };
        // 优先用 sidecar JSON;没找到就 fallback 到 legacy stub。
        // legacy stub 的 name **必须**是 safe_name_part(去掉 rustpanel- 前缀
        // 的纯名字),否则前端发回 delete 时会把整个文件 stem 当 name,
        // 后端二次拼 `rustpanel-<那玩意>.conf` 找不到文件,删了个寂寞。
        if let Some(mut full) = store.load_nginx_site_metadata(&safe_name_part).await {
            // 万一两边对不上(sidecar 里旧 config_path),以磁盘当前文件为准
            full.config_path = path.to_string_lossy().to_string();
            sites.push(full);
        } else {
            sites.push(legacy_site_stub(&path, safe_name_part));
        }
    }

    Ok(sites)
}

/// 没有 sidecar 元数据(老站点 / 手动添加)时的兜底 stub —— 至少让
/// 主表看到这一行,知道有这么个 vhost 在,虽然 domains 等字段空。
fn legacy_site_stub(path: &std::path::Path, stem: String) -> SiteItem {
    SiteItem {
        name: stem,
        domains: Vec::new(),
        root: String::new(),
        proxy_target: String::new(),
        ssl_enabled: false,
        config_path: path.to_string_lossy().to_string(),
        engine: SITE_ENGINE_NGINX.to_owned(),
        public_path: String::new(),
        listen_addr: String::new(),
        kind: 0,
        binding: None,
        tls_strategy: 0,
        systemd_unit: String::new(),
        internal_port: 0,
    }
}

#[derive(Clone, Debug)]
struct SiteStore {
    root: Arc<PathBuf>,
}

impl SiteStore {
    fn from_env() -> Self {
        let root = env::var("RUSTPANEL_SITE_STATE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_SITE_STATE_ROOT));
        Self {
            root: Arc::new(root),
        }
    }

    async fn load_proxy_rules(&self) -> Result<Vec<StoredReverseProxyRule>, Status> {
        match tokio::fs::read_to_string(self.proxy_rules_path()).await {
            Ok(content) => serde_json::from_str(&content).map_err(io_status),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(io_status(error)),
        }
    }

    async fn save_proxy_rules(&self, rules: &[StoredReverseProxyRule]) -> Result<(), Status> {
        tokio::fs::create_dir_all(self.root.as_ref())
            .await
            .map_err(io_status)?;
        let content = serde_json::to_string_pretty(rules).map_err(io_status)?;
        tokio::fs::write(self.proxy_rules_path(), content)
            .await
            .map_err(io_status)
    }

    fn proxy_rules_path(&self) -> PathBuf {
        self.root.join("reverse-proxy-rules.json")
    }

    async fn load_builtin_sites(&self) -> Result<Vec<SiteItem>, Status> {
        match tokio::fs::read_to_string(self.sites_path()).await {
            Ok(content) => serde_json::from_str::<Vec<StoredSite>>(&content)
                .map_err(io_status)
                .map(|sites| sites.into_iter().map(StoredSite::into_proto).collect()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(io_status(error)),
        }
    }

    async fn save_builtin_sites(&self, sites: &[SiteItem]) -> Result<(), Status> {
        tokio::fs::create_dir_all(self.root.as_ref())
            .await
            .map_err(io_status)?;
        let stored = sites
            .iter()
            .cloned()
            .map(StoredSite::from_proto)
            .collect::<Vec<_>>();
        let content = serde_json::to_string_pretty(&stored).map_err(io_status)?;
        tokio::fs::write(self.sites_path(), content)
            .await
            .map_err(io_status)
    }

    async fn upsert_builtin_site(&self, request: CreateSiteRequest) -> Result<SiteItem, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_STATIC_SITES)?;
        let name = safe_name(&request.name)?;
        let mut sites = self.load_builtin_sites().await?;
        let site = SiteItem {
            name: name.clone(),
            domains: request.domains,
            root: request.root,
            proxy_target: String::new(),
            ssl_enabled: false,
            config_path: self.sites_path().to_string_lossy().to_string(),
            engine: SITE_ENGINE_BUILTIN.to_owned(),
            public_path: format!("/sites/{name}/"),
            listen_addr: request.listen_addr,
            kind: SiteKind::Static as i32,
            binding: request.binding.clone(),
            tls_strategy: request.tls_strategy,
            systemd_unit: String::new(),
            internal_port: 0,
        };
        sites.retain(|stored| stored.name != site.name);
        sites.push(site.clone());
        self.save_builtin_sites(&sites).await?;

        Ok(site)
    }

    fn sites_path(&self) -> PathBuf {
        self.root.join("sites.json")
    }

    /// nginx 路径站点的元数据 sidecar 目录。list_site_configs 只能从
    /// /etc/nginx 文件名反推一个名字,domains/root/kind/binding 这些
    /// 创建时填的字段无法从 nginx 配置里"逆向"出来 —— 所以 create_site
    /// 时落一份 JSON sidecar,list 时再合并回 SiteItem。
    fn nginx_metadata_dir(&self) -> PathBuf {
        self.root.join("nginx-sites")
    }

    fn nginx_metadata_path(&self, safe_name: &str) -> PathBuf {
        self.nginx_metadata_dir().join(format!("{safe_name}.json"))
    }

    async fn save_nginx_site_metadata(&self, site: &SiteItem) -> Result<(), Status> {
        let safe = safe_name(&site.name)?;
        let dir = self.nginx_metadata_dir();
        tokio::fs::create_dir_all(&dir).await.map_err(io_status)?;
        let path = self.nginx_metadata_path(&safe);
        let stored = StoredSite::from_proto(site.clone());
        let body = serde_json::to_vec_pretty(&stored).map_err(io_status)?;
        let tmp = path.with_extension("json.rustpanel-tmp");
        tokio::fs::write(&tmp, body).await.map_err(io_status)?;
        tokio::fs::rename(&tmp, &path).await.map_err(io_status)?;
        Ok(())
    }

    async fn load_nginx_site_metadata(&self, safe_name: &str) -> Option<SiteItem> {
        let path = self.nginx_metadata_path(safe_name);
        let bytes = tokio::fs::read(&path).await.ok()?;
        let stored: StoredSite = serde_json::from_slice(&bytes).ok()?;
        Some(stored.into_proto())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredSite {
    name: String,
    domains: Vec<String>,
    root: String,
    proxy_target: String,
    ssl_enabled: bool,
    config_path: String,
    engine: String,
    public_path: String,
    listen_addr: String,
    // === Phase C 扩展(默认 0/空,旧记录读出后字段自然为默认值) ===
    #[serde(default)]
    kind: i32,
    #[serde(default)]
    binding_kind: i32,
    #[serde(default)]
    nat_port: u32,
    #[serde(default)]
    ipv6_address: String,
    #[serde(default)]
    tls_strategy: i32,
    #[serde(default)]
    systemd_unit: String,
    #[serde(default)]
    internal_port: u32,
}

impl StoredSite {
    fn from_proto(site: SiteItem) -> Self {
        let (binding_kind, nat_port, ipv6_address) = site
            .binding
            .as_ref()
            .map(|b| (b.kind, b.nat_port, b.ipv6_address.clone()))
            .unwrap_or((0, 0, String::new()));
        Self {
            name: site.name,
            domains: site.domains,
            root: site.root,
            proxy_target: site.proxy_target,
            ssl_enabled: site.ssl_enabled,
            config_path: site.config_path,
            engine: site.engine,
            public_path: site.public_path,
            listen_addr: site.listen_addr,
            kind: site.kind,
            binding_kind,
            nat_port,
            ipv6_address,
            tls_strategy: site.tls_strategy,
            systemd_unit: site.systemd_unit,
            internal_port: site.internal_port,
        }
    }

    fn into_proto(self) -> SiteItem {
        let binding = if self.binding_kind != 0 {
            Some(SiteBinding {
                kind: self.binding_kind,
                nat_port: self.nat_port,
                ipv6_address: self.ipv6_address,
            })
        } else {
            None
        };
        SiteItem {
            name: self.name,
            domains: self.domains,
            root: self.root,
            proxy_target: self.proxy_target,
            ssl_enabled: self.ssl_enabled,
            config_path: self.config_path,
            engine: self.engine,
            public_path: self.public_path,
            listen_addr: self.listen_addr,
            kind: self.kind,
            binding,
            tls_strategy: self.tls_strategy,
            systemd_unit: self.systemd_unit,
            internal_port: self.internal_port,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredReverseProxyRule {
    id: String,
    name: String,
    domain: String,
    path_prefix: String,
    targets: Vec<StoredUpstreamTarget>,
    load_balance_method: String,
    cache_enabled: bool,
    rate_limit_per_minute: u32,
    enabled: bool,
    config_path: String,
    created_at_seconds: u64,
    updated_at_seconds: u64,
}

impl StoredReverseProxyRule {
    fn from_proto(rule: ReverseProxyRule) -> Self {
        Self {
            id: rule.id,
            name: rule.name,
            domain: rule.domain,
            path_prefix: rule.path_prefix,
            targets: rule
                .targets
                .into_iter()
                .map(StoredUpstreamTarget::from_proto)
                .collect(),
            load_balance_method: rule.load_balance_method,
            cache_enabled: rule.cache_enabled,
            rate_limit_per_minute: rule.rate_limit_per_minute,
            enabled: rule.enabled,
            config_path: rule.config_path,
            created_at_seconds: rule.created_at_seconds,
            updated_at_seconds: rule.updated_at_seconds,
        }
    }

    fn into_proto(self) -> ReverseProxyRule {
        ReverseProxyRule {
            id: self.id,
            name: self.name,
            domain: self.domain,
            path_prefix: self.path_prefix,
            targets: self
                .targets
                .into_iter()
                .map(StoredUpstreamTarget::into_proto)
                .collect(),
            load_balance_method: self.load_balance_method,
            cache_enabled: self.cache_enabled,
            rate_limit_per_minute: self.rate_limit_per_minute,
            enabled: self.enabled,
            config_path: self.config_path,
            created_at_seconds: self.created_at_seconds,
            updated_at_seconds: self.updated_at_seconds,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredUpstreamTarget {
    url: String,
    weight: u32,
    healthy: bool,
}

impl StoredUpstreamTarget {
    fn from_proto(target: UpstreamTarget) -> Self {
        Self {
            url: target.url,
            weight: target.weight,
            healthy: target.healthy,
        }
    }

    fn into_proto(self) -> UpstreamTarget {
        UpstreamTarget {
            url: self.url,
            weight: self.weight,
            healthy: self.healthy,
        }
    }
}

fn rewrite_templates() -> Vec<RewriteTemplate> {
    vec![
        RewriteTemplate {
            id: "wordpress".to_owned(),
            name: "WordPress".to_owned(),
            stack: "PHP".to_owned(),
            content: "location / {\n    try_files $uri $uri/ /index.php?$args;\n}".to_owned(),
        },
        RewriteTemplate {
            id: "laravel".to_owned(),
            name: "Laravel".to_owned(),
            stack: "PHP".to_owned(),
            content:
                "location / {\n    try_files $uri $uri/ /index.php?$query_string;\n}".to_owned(),
        },
        RewriteTemplate {
            id: "thinkphp".to_owned(),
            name: "ThinkPHP".to_owned(),
            stack: "PHP".to_owned(),
            content: "location / {\n    if (!-e $request_filename) {\n        rewrite ^(.*)$ /index.php?s=$1 last;\n    }\n}".to_owned(),
        },
    ]
}

fn validate_proxy_rule(rule: &ReverseProxyRule) -> Result<(), Status> {
    safe_name(&rule.name)?;
    validate_domain(&rule.domain)?;
    if !rule.path_prefix.starts_with('/') {
        return Err(Status::invalid_argument("path_prefix must start with /"));
    }
    if rule.targets.is_empty() {
        return Err(Status::invalid_argument(
            "reverse proxy requires at least one target",
        ));
    }
    for target in &rule.targets {
        validate_proxy_target(&target.url)?;
    }
    if !matches!(
        rule.load_balance_method.as_str(),
        "" | "round_robin" | "least_conn" | "ip_hash"
    ) {
        return Err(Status::invalid_argument(
            "load_balance_method must be round_robin, least_conn, or ip_hash",
        ));
    }
    Ok(())
}

fn render_proxy_rule(rule: &ReverseProxyRule) -> Result<String, Status> {
    validate_proxy_rule(rule)?;
    let upstream_name = format!("rustpanel_{}", safe_name(&rule.name)?.replace('-', "_"));
    let mut config = String::new();
    if rule.enabled {
        config.push_str(&format!("upstream {upstream_name} {{\n"));
        if rule.load_balance_method == "least_conn" {
            config.push_str("    least_conn;\n");
        } else if rule.load_balance_method == "ip_hash" {
            config.push_str("    ip_hash;\n");
        }
        for target in &rule.targets {
            config.push_str(&format!(
                "    server {} weight={};\n",
                target
                    .url
                    .trim_start_matches("http://")
                    .trim_start_matches("https://"),
                target.weight.max(1)
            ));
        }
        config.push_str("}\n\n");
    }
    config.push_str("server {\n");
    config.push_str("    listen 80;\n");
    config.push_str(&format!("    server_name {};\n", rule.domain));
    if rule.cache_enabled {
        config.push_str("    proxy_cache rustpanel_proxy_cache;\n");
    }
    if rule.rate_limit_per_minute > 0 {
        config.push_str(&format!(
            "    limit_req_zone $binary_remote_addr zone={upstream_name}_limit:10m rate={}r/m;\n",
            rule.rate_limit_per_minute
        ));
        config.push_str(&format!(
            "    limit_req zone={upstream_name}_limit burst=20 nodelay;\n"
        ));
    }
    config.push_str(&format!("    location {} {{\n", rule.path_prefix));
    if rule.enabled {
        config.push_str(&format!("        proxy_pass http://{upstream_name};\n"));
    } else {
        config.push_str("        return 503;\n");
    }
    config.push_str("        proxy_http_version 1.1;\n");
    config.push_str("        proxy_set_header Host $host;\n");
    config.push_str("        proxy_set_header X-Real-IP $remote_addr;\n");
    config.push_str("        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;\n");
    config.push_str("        proxy_set_header X-Forwarded-Proto $scheme;\n");
    config.push_str("    }\n");
    config.push_str("}\n");
    Ok(config)
}

fn render_site_config(request: &CreateSiteRequest) -> Result<String, Status> {
    let mut context = Context::new();
    context.insert("domains", &request.domains);
    context.insert(
        "primary_domain",
        request
            .domains
            .first()
            .ok_or_else(|| Status::invalid_argument("at least one domain is required"))?,
    );
    context.insert("root", &request.root);
    context.insert("proxy_target", &request.proxy_target);
    context.insert("ssl_enabled", &request.ssl_enabled);

    Tera::one_off(NGINX_TEMPLATE, &context, false).map_err(io_status)
}

// Phase C:渲染对 NAT 端口 / IPv6 / 不同 SiteKind 感知的 nginx vhost。
// 不依赖 Tera 模板,直接拼字符串以便处理 listen 行的多种变体。
fn render_phase_c_site(
    request: &CreateSiteRequest,
    kind: SiteKind,
    binding: Option<&SiteBinding>,
    tls: SiteTlsStrategy,
) -> Result<String, Status> {
    if request.domains.is_empty() {
        return Err(Status::invalid_argument("at least one domain is required"));
    }
    let primary = request
        .domains
        .first()
        .cloned()
        .unwrap_or_else(|| "_".to_owned());
    let server_names = request.domains.join(" ");

    let bind_kind = binding
        .map(|b| SiteBindKind::try_from(b.kind).unwrap_or_default())
        .unwrap_or_default();

    // 生成 listen 指令:
    //   NAT_PORT      → listen <port>; listen [::]:<port>;
    //   IPV6_ADDRESS  → listen [<addr>]:443 ssl http2;(走 v6 直接绑)
    //   未指定        → 退回 listen 80;
    let listen_lines = match (bind_kind, binding) {
        (SiteBindKind::NatPort, Some(b)) if b.nat_port > 0 => {
            let p = b.nat_port;
            format!("    listen {p};\n    listen [::]:{p};\n")
        }
        (SiteBindKind::Ipv6Address, Some(b)) if !b.ipv6_address.is_empty() => {
            // **总是**监听 80 端口:
            //   - HTTP-01 ACME 验证必须能从 80 端口拉 challenge 文件
            //   - 没 SSL 时直接服务 HTTP
            //   - 有 SSL 时承担 http → https 重定向
            // 然后,启用 SSL 时再叠加 443 ssl/quic 监听。配套的 ssl_certificate
            // 文件由 ssl::bootstrap_self_signed_if_missing 在 create_site 阶段
            // 先用自签兜底,保证 nginx -t 不会因为找不到证书拒绝启动;
            // 真证书签发后会原地覆盖。
            // HTTP/3:nginx 1.25+ 编译了 --with-http_v3_module 才支持 `quic`
            // 这个 listen 参数;探测不到就只走 HTTP/2,避免老 nginx -t 报错。
            let mut lines = format!("    listen [{addr}]:80;\n", addr = b.ipv6_address);
            if tls != SiteTlsStrategy::None {
                lines.push_str(&format!(
                    "    listen [{addr}]:443 ssl http2;\n",
                    addr = b.ipv6_address
                ));
                if nginx_supports_http3() {
                    lines.push_str(&format!(
                        "    listen [{addr}]:443 quic reuseport;\n",
                        addr = b.ipv6_address
                    ));
                }
            }
            lines
        }
        _ => "    listen 80;\n    listen [::]:80;\n".to_owned(),
    };

    let internal_port = 9100 + (hash_name_to_port_offset(&request.name) % 800);

    let body = match kind {
        SiteKind::Static => {
            let root = if request.root.trim().is_empty() {
                "/var/www/html".to_owned()
            } else {
                request.root.clone()
            };
            format!(
                "    root {root};\n    index index.html index.htm;\n    location / {{\n        try_files $uri $uri/ /index.html;\n    }}\n"
            )
        }
        SiteKind::RustBinary => format!(
            "    # Rust 二进制由 systemd 监听 127.0.0.1:{internal_port}\n    location / {{\n        proxy_pass http://127.0.0.1:{internal_port};\n        proxy_http_version 1.1;\n        proxy_set_header Host $host;\n        proxy_set_header X-Real-IP $remote_addr;\n        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;\n        proxy_set_header X-Forwarded-Proto $scheme;\n    }}\n"
        ),
        SiteKind::ReverseProxy => {
            let upstream = if request.proxy_target.trim().is_empty() {
                return Err(Status::invalid_argument(
                    "ReverseProxy 类型必须填写 proxy_target",
                ));
            } else {
                request.proxy_target.clone()
            };
            format!(
                "    location / {{\n        proxy_pass {upstream};\n        proxy_http_version 1.1;\n        proxy_set_header Host $host;\n        proxy_set_header X-Real-IP $remote_addr;\n        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;\n        proxy_set_header X-Forwarded-Proto $scheme;\n    }}\n"
            )
        }
        _ => unreachable!("render_phase_c_site only handles Phase C kinds"),
    };

    // ssl_certificate 路径指向 ssl 模块实际写证书的位置(RUSTPANEL_CERT_ROOT,
    // 默认 /var/lib/rustpanel/acme/<domain>/),而不是 /etc/letsencrypt/live。
    // 之前的硬编码导致 ACME 签好的证书 nginx 根本读不到,反代 vhost 立即 500。
    // 顺手:把 ssl_protocols 显式钉到 TLS 1.2/1.3(HTTP/3 强制 TLS 1.3);
    // 若 nginx 支持 HTTP/3,在 vhost 加一条 Alt-Svc 头让现代浏览器自动
    // 切到 h3。
    let tls_block = match tls {
        SiteTlsStrategy::LetsencryptDns01 | SiteTlsStrategy::Imported => {
            let (cert_path, key_path) = crate::ssl::acme_cert_paths(&primary);
            let mut block = format!(
                "    ssl_certificate {};\n    ssl_certificate_key {};\n    ssl_protocols TLSv1.2 TLSv1.3;\n",
                cert_path.display(),
                key_path.display(),
            );
            if nginx_supports_http3() {
                block.push_str(
                    "    add_header Alt-Svc 'h3=\":443\"; ma=86400' always;\n    add_header X-Quic-Status $http3 always;\n",
                );
            }
            block
        }
        _ => String::new(),
    };

    let acme_block = if matches!(tls, SiteTlsStrategy::LetsencryptDns01) {
        // DNS-01 不需要 .well-known/acme-challenge,但保留 root 兼容混合场景
        String::new()
    } else {
        "    location ^~ /.well-known/acme-challenge/ {\n        root /var/www/rustpanel-acme;\n        default_type \"text/plain\";\n    }\n"
            .to_owned()
    };

    Ok(format!(
        "# RustPanel Phase C site (kind={kind:?}, bind={bind_kind:?}, tls={tls:?})\nserver {{\n{listen_lines}    server_name {server_names};\n\n{acme_block}\n{body}\n{tls_block}}}\n"
    ))
}

// 简单的字符串哈希,把站点名映射到 0..800 的偏移,作为内部 port 的稳定基。
// 不要求加密强度,只要保证同名站点拿到同样端口、不同名站点尽量不冲突。
fn hash_name_to_port_offset(name: &str) -> u32 {
    let mut hash: u32 = 5381;
    for byte in name.as_bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(*byte as u32);
    }
    hash
}

fn validate_domain(domain: &str) -> Result<(), Status> {
    let valid = !domain.trim().is_empty()
        && domain
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || matches!(char, '-' | '.'))
        && domain.contains('.');
    if valid {
        Ok(())
    } else {
        Err(Status::invalid_argument("valid domain is required"))
    }
}

fn validate_proxy_target(target: &str) -> Result<(), Status> {
    let valid = (target.starts_with("http://") || target.starts_with("https://"))
        && !target.contains(char::is_whitespace);
    if valid {
        Ok(())
    } else {
        Err(Status::invalid_argument(
            "target url must start with http:// or https://",
        ))
    }
}

fn validate_site_request(request: &CreateSiteRequest) -> Result<(), Status> {
    safe_name(&request.name)?;
    let engine = site_engine(&request.engine);
    if engine != SITE_ENGINE_BUILTIN && request.domains.is_empty() {
        return Err(Status::invalid_argument("at least one domain is required"));
    }
    if request.root.trim().is_empty()
        && (engine == SITE_ENGINE_BUILTIN || request.proxy_target.trim().is_empty())
    {
        return Err(Status::invalid_argument(
            "either static root or proxy target is required",
        ));
    }
    Ok(())
}

pub async fn builtin_site_file(path: &str) -> Result<Option<PathBuf>, Status> {
    crate::runtime::ensure_module_enabled(crate::runtime::MODULE_STATIC_SITES)?;
    let mut parts = path.trim_start_matches('/').splitn(2, '/');
    let Some(name) = parts.next().filter(|name| !name.is_empty()) else {
        return Ok(None);
    };
    let name = safe_name(name)?;
    let relative = parts.next().unwrap_or_default();
    let store = SiteStore::from_env();
    let Some(site) = store
        .load_builtin_sites()
        .await?
        .into_iter()
        .find(|site| site.name == name && site.engine == SITE_ENGINE_BUILTIN)
    else {
        return Ok(None);
    };
    let root = PathBuf::from(site.root);
    let root = tokio::fs::canonicalize(&root).await.map_err(io_status)?;
    let relative = safe_relative_path(relative);
    let mut candidate = root.join(relative);
    if candidate.is_dir() {
        candidate = candidate.join("index.html");
    }
    if tokio::fs::metadata(&candidate).await.is_err() {
        candidate = root.join("index.html");
    }
    let candidate = tokio::fs::canonicalize(candidate)
        .await
        .map_err(io_status)?;
    if !candidate.starts_with(&root) {
        return Err(Status::permission_denied("site path escapes site root"));
    }

    Ok(Some(candidate))
}

fn safe_relative_path(path: &str) -> PathBuf {
    let mut result = PathBuf::new();
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            continue;
        }
        result.push(segment);
    }
    result
}

fn site_engine(request_engine: &str) -> String {
    let value = if request_engine.trim().is_empty() {
        env::var(SITE_ENGINE_ENV).unwrap_or_else(|_| SITE_ENGINE_NGINX.to_owned())
    } else {
        request_engine.to_owned()
    };
    match value.trim().to_ascii_lowercase().as_str() {
        SITE_ENGINE_BUILTIN => SITE_ENGINE_BUILTIN.to_owned(),
        _ => SITE_ENGINE_NGINX.to_owned(),
    }
}

fn safe_name(name: &str) -> Result<String, Status> {
    let safe = name
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
    if safe.is_empty() {
        Err(Status::invalid_argument("site name is required"))
    } else {
        Ok(safe)
    }
}

/// 探测本机 nginx 是否编译了 HTTP/3(QUIC)模块。结果缓存进 OnceLock,
/// 进程生命周期内只跑一次 `nginx -V`。env `RUSTPANEL_SITE_HTTP3` 可强制:
///   - "off" / "0" / "false"  → 始终关
///   - "on"  / "1" / "true"   → 始终开(即使没有模块,emit 出去 nginx 会拒)
///   - 其它 / 未设置          → 自动探测
fn nginx_supports_http3() -> bool {
    static CACHE: OnceLock<bool> = OnceLock::new();
    *CACHE.get_or_init(|| {
        if let Ok(value) = env::var("RUSTPANEL_SITE_HTTP3") {
            let lower = value.to_ascii_lowercase();
            match lower.as_str() {
                "off" | "0" | "false" | "no" => return false,
                "on" | "1" | "true" | "yes" => return true,
                _ => {}
            }
        }
        // nginx -V 把所有编译参数打到 stderr;mainline 1.25+ 默认含
        // --with-http_v3_module。距离 stable 还远的 1.22/1.24 没这个
        // 模块,emit `quic` 会让 nginx -t 直接拒绝。
        std::process::Command::new("nginx")
            .arg("-V")
            .output()
            .ok()
            .map(|output| {
                let combined = format!(
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                combined.contains("--with-http_v3_module")
            })
            .unwrap_or(false)
    })
}

fn nginx_sites_dir() -> PathBuf {
    env::var("RUSTPANEL_NGINX_SITES_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_NGINX_SITES_DIR))
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
    fn renders_proxy_site_config() {
        let config = render_site_config(&CreateSiteRequest {
            name: "demo".to_owned(),
            domains: vec!["example.com".to_owned()],
            root: String::new(),
            proxy_target: "http://127.0.0.1:3000".to_owned(),
            ssl_enabled: true,
            engine: SITE_ENGINE_NGINX.to_owned(),
            listen_addr: String::new(),
            kind: 0,
            binding: None,
            tls_strategy: 0,
            binary_path: String::new(),
        })
        .expect("config");

        assert!(config.contains("server_name example.com"));
        assert!(config.contains("proxy_pass http://127.0.0.1:3000"));
        assert!(config.contains(".well-known/acme-challenge"));
    }

    #[test]
    fn exposes_common_rewrite_templates() {
        let templates = rewrite_templates();

        assert!(templates.iter().any(|template| template.id == "wordpress"));
        assert!(templates.iter().any(|template| template.id == "laravel"));
        assert!(templates.iter().any(|template| template.id == "thinkphp"));
    }

    #[test]
    fn renders_reverse_proxy_upstream() {
        let config = render_proxy_rule(&ReverseProxyRule {
            id: String::new(),
            name: "api".to_owned(),
            domain: "example.com".to_owned(),
            path_prefix: "/api/".to_owned(),
            targets: vec![
                UpstreamTarget {
                    url: "http://127.0.0.1:3000".to_owned(),
                    weight: 2,
                    healthy: true,
                },
                UpstreamTarget {
                    url: "http://127.0.0.1:3001".to_owned(),
                    weight: 1,
                    healthy: true,
                },
            ],
            load_balance_method: "least_conn".to_owned(),
            cache_enabled: true,
            rate_limit_per_minute: 120,
            enabled: true,
            config_path: String::new(),
            created_at_seconds: 0,
            updated_at_seconds: 0,
        })
        .expect("proxy config");

        assert!(config.contains("least_conn"));
        assert!(config.contains("server 127.0.0.1:3000 weight=2"));
        assert!(config.contains("proxy_pass http://rustpanel_api"));
    }
}
