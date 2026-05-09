use std::{env, path::PathBuf, sync::Arc};

use serde::{Deserialize, Serialize};
use tera::{Context, Tera};
use tonic::{Request, Response as GrpcResponse, Status};
use uuid::Uuid;

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        site_service_server::SiteService, CreateSiteRequest, CreateSiteResponse,
        DeleteReverseProxyRuleRequest, DeleteReverseProxyRuleResponse,
        ListReverseProxyRulesRequest, ListReverseProxyRulesResponse, ListRewriteTemplatesRequest,
        ListRewriteTemplatesResponse, ListSitesRequest, ListSitesResponse, ReloadNginxRequest,
        ReloadNginxResponse, RenderRewriteTemplateRequest, RenderRewriteTemplateResponse,
        ReverseProxyRule, RewriteTemplate, SiteItem, UpsertReverseProxyRuleRequest,
        UpsertReverseProxyRuleResponse, UpstreamTarget,
    },
};

const DEFAULT_NGINX_SITES_DIR: &str = "/etc/nginx/sites-enabled";
const DEFAULT_SITE_STATE_ROOT: &str = "/tmp/rustpanel/site";
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
        let sites = list_site_configs().await?;

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
        let rendered_config = render_site_config(&request)?;
        let config_path =
            nginx_sites_dir().join(format!("rustpanel-{}.conf", safe_name(&request.name)?));
        if let Some(parent) = config_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(io_status)?;
        }
        tokio::fs::write(&config_path, rendered_config.as_bytes())
            .await
            .map_err(io_status)?;
        let site = SiteItem {
            name: request.name,
            domains: request.domains,
            root: request.root,
            proxy_target: request.proxy_target,
            ssl_enabled: request.ssl_enabled,
            config_path: config_path.to_string_lossy().to_string(),
        };

        Ok(GrpcResponse::new(CreateSiteResponse {
            status: Some(ok_response("site created")),
            site: Some(site),
            rendered_config,
        }))
    }

    async fn reload_nginx(
        &self,
        _request: Request<ReloadNginxRequest>,
    ) -> Result<GrpcResponse<ReloadNginxResponse>, Status> {
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

async fn list_site_configs() -> Result<Vec<SiteItem>, Status> {
    let mut sites = Vec::new();
    let mut entries = match tokio::fs::read_dir(nginx_sites_dir()).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(sites),
        Err(error) => return Err(io_status(error)),
    };

    while let Some(entry) = entries.next_entry().await.map_err(io_status)? {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) == Some("conf") {
            sites.push(SiteItem {
                name: path
                    .file_stem()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default(),
                domains: Vec::new(),
                root: String::new(),
                proxy_target: String::new(),
                ssl_enabled: false,
                config_path: path.to_string_lossy().to_string(),
            });
        }
    }

    Ok(sites)
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
    if request.domains.is_empty() {
        return Err(Status::invalid_argument("at least one domain is required"));
    }
    if request.root.trim().is_empty() && request.proxy_target.trim().is_empty() {
        return Err(Status::invalid_argument(
            "either static root or proxy target is required",
        ));
    }
    Ok(())
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
