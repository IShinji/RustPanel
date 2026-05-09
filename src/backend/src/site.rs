use std::{env, path::PathBuf};

use tera::{Context, Tera};
use tonic::{Request, Response as GrpcResponse, Status};

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        site_service_server::SiteService, CreateSiteRequest, CreateSiteResponse, ListSitesRequest,
        ListSitesResponse, ReloadNginxRequest, ReloadNginxResponse, SiteItem,
    },
};

const DEFAULT_NGINX_SITES_DIR: &str = "/etc/nginx/sites-enabled";
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

#[derive(Clone, Debug, Default)]
pub struct SiteServiceImpl;

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
}
