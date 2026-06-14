use std::{env, path::PathBuf, sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tonic::{Request, Response as GrpcResponse, Status};

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        dns_service_server::DnsService, DeleteDnsRecordRequest, DeleteDnsRecordResponse,
        DnsProvider, DnsRecord, GetDnsConfigRequest, GetDnsConfigResponse, ListDnsRecordsRequest,
        ListDnsRecordsResponse, SetDnsConfigRequest, SetDnsConfigResponse, UpsertDnsRecordRequest,
        UpsertDnsRecordResponse,
    },
};

const DEFAULT_DNS_ROOT: &str = "/tmp/rustpanel/dns";
const SECRET_REDACTED: &str = "__rustpanel_secret_kept__";
const HTTP_TIMEOUT_SECONDS: u64 = 20;

fn cloudflare_api_base() -> String {
    // 默认官方 API;可经 env 覆盖(自测 / 兼容代理)。
    env::var("RUSTPANEL_CLOUDFLARE_API_BASE")
        .unwrap_or_else(|_| "https://api.cloudflare.com/client/v4".to_owned())
}

#[derive(Clone)]
pub struct DnsServiceImpl {
    store: DnsStore,
}

impl DnsServiceImpl {
    pub fn new() -> Self {
        Self {
            store: DnsStore::from_env(),
        }
    }

    async fn require_config(&self) -> Result<StoredConfig, Status> {
        let config = self.store.load().await?;
        if config.api_token.trim().is_empty() || config.zone_id.trim().is_empty() {
            return Err(Status::failed_precondition(
                "DNS 服务商未配置(请先填写 API token 与 zone id)",
            ));
        }
        Ok(config)
    }
}

impl Default for DnsServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl DnsService for DnsServiceImpl {
    async fn get_dns_config(
        &self,
        _request: Request<GetDnsConfigRequest>,
    ) -> Result<GrpcResponse<GetDnsConfigResponse>, Status> {
        let config = self.store.load().await?;
        Ok(GrpcResponse::new(GetDnsConfigResponse {
            status: Some(ok_response("ok")),
            provider: config.provider,
            zone_id: config.zone_id.clone(),
            configured: !config.api_token.trim().is_empty() && !config.zone_id.trim().is_empty(),
        }))
    }

    async fn set_dns_config(
        &self,
        request: Request<SetDnsConfigRequest>,
    ) -> Result<GrpcResponse<SetDnsConfigResponse>, Status> {
        let request = request.into_inner();
        if DnsProvider::try_from(request.provider).ok() != Some(DnsProvider::Cloudflare) {
            return Err(Status::invalid_argument("当前仅支持 Cloudflare"));
        }
        if request.zone_id.trim().is_empty() {
            return Err(Status::invalid_argument("zone id 不能为空"));
        }

        let _guard = self.store.write_lock.lock().await;
        let mut config = self.store.load().await?;
        config.provider = request.provider;
        config.zone_id = request.zone_id.trim().to_owned();
        // token 留空或占位符=保留原值(避免改 zone 时被迫重输密钥)。
        let token = request.api_token.trim();
        if !token.is_empty() && token != SECRET_REDACTED {
            config.api_token = token.to_owned();
        }
        if config.api_token.trim().is_empty() {
            return Err(Status::invalid_argument("首次配置需提供 API token"));
        }
        self.store.save(&config).await?;
        Ok(GrpcResponse::new(SetDnsConfigResponse {
            status: Some(ok_response("DNS 配置已保存")),
        }))
    }

    async fn list_dns_records(
        &self,
        _request: Request<ListDnsRecordsRequest>,
    ) -> Result<GrpcResponse<ListDnsRecordsResponse>, Status> {
        let config = self.require_config().await?;
        let url = format!(
            "{}?per_page=100",
            cf_records_url(&cloudflare_api_base(), &config.zone_id)
        );
        let json = cf_request(&config.api_token, "GET", &url, None)
            .await
            .map_err(Status::internal)?;
        let records = parse_records(&json).map_err(Status::internal)?;
        Ok(GrpcResponse::new(ListDnsRecordsResponse {
            status: Some(ok_response("ok")),
            records,
        }))
    }

    async fn upsert_dns_record(
        &self,
        request: Request<UpsertDnsRecordRequest>,
    ) -> Result<GrpcResponse<UpsertDnsRecordResponse>, Status> {
        let record = request
            .into_inner()
            .record
            .ok_or_else(|| Status::invalid_argument("record is required"))?;
        validate_record(&record)?;
        let config = self.require_config().await?;
        let base = cloudflare_api_base();
        let body = record_to_body(&record);

        let json = if record.id.trim().is_empty() {
            cf_request(
                &config.api_token,
                "POST",
                &cf_records_url(&base, &config.zone_id),
                Some(body),
            )
            .await
        } else {
            cf_request(
                &config.api_token,
                "PUT",
                &cf_record_url(&base, &config.zone_id, record.id.trim()),
                Some(body),
            )
            .await
        }
        .map_err(Status::internal)?;

        let saved = parse_single(&json).map_err(Status::internal)?;
        Ok(GrpcResponse::new(UpsertDnsRecordResponse {
            status: Some(ok_response("解析记录已保存")),
            record: Some(saved),
        }))
    }

    async fn delete_dns_record(
        &self,
        request: Request<DeleteDnsRecordRequest>,
    ) -> Result<GrpcResponse<DeleteDnsRecordResponse>, Status> {
        let id = request.into_inner().id;
        if id.trim().is_empty() {
            return Err(Status::invalid_argument("record id is required"));
        }
        let config = self.require_config().await?;
        let url = cf_record_url(&cloudflare_api_base(), &config.zone_id, id.trim());
        cf_request(&config.api_token, "DELETE", &url, None)
            .await
            .map_err(Status::internal)?;
        Ok(GrpcResponse::new(DeleteDnsRecordResponse {
            status: Some(ok_response("解析记录已删除")),
        }))
    }
}

fn validate_record(record: &DnsRecord) -> Result<(), Status> {
    if record.r#type.trim().is_empty() {
        return Err(Status::invalid_argument("记录类型不能为空"));
    }
    if record.name.trim().is_empty() {
        return Err(Status::invalid_argument("记录名不能为空"));
    }
    if record.content.trim().is_empty() {
        return Err(Status::invalid_argument("记录值不能为空"));
    }
    Ok(())
}

// ===== Cloudflare API 纯函数(可单测,不依赖网络) =====

fn cf_records_url(base: &str, zone_id: &str) -> String {
    format!(
        "{}/zones/{}/dns_records",
        base.trim_end_matches('/'),
        zone_id
    )
}

fn cf_record_url(base: &str, zone_id: &str, record_id: &str) -> String {
    format!("{}/{record_id}", cf_records_url(base, zone_id))
}

fn record_to_body(record: &DnsRecord) -> Value {
    serde_json::json!({
        "type": record.r#type.trim(),
        "name": record.name.trim(),
        "content": record.content.trim(),
        // ttl=1 表示 automatic;0 当作未填,落 1。
        "ttl": if record.ttl == 0 { 1 } else { record.ttl },
        "proxied": record.proxied,
    })
}

fn json_to_record(value: &Value) -> DnsRecord {
    DnsRecord {
        id: value
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        r#type: value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        name: value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        content: value
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        ttl: value
            .get("ttl")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .min(u64::from(u32::MAX)) as u32,
        proxied: value
            .get("proxied")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

fn parse_records(json: &Value) -> Result<Vec<DnsRecord>, String> {
    let array = json
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| "cloudflare 响应缺少 result 数组".to_owned())?;
    Ok(array.iter().map(json_to_record).collect())
}

fn parse_single(json: &Value) -> Result<DnsRecord, String> {
    let result = json
        .get("result")
        .ok_or_else(|| "cloudflare 响应缺少 result".to_owned())?;
    Ok(json_to_record(result))
}

/// 提取 Cloudflare 错误信息(errors[].message 拼接)。
fn cf_error_message(json: &Value) -> String {
    json.get("errors")
        .and_then(Value::as_array)
        .map(|errors| {
            errors
                .iter()
                .filter_map(|error| error.get("message").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("; ")
        })
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| "cloudflare 返回失败".to_owned())
}

async fn cf_request(
    token: &str,
    method: &str,
    url: &str,
    body: Option<Value>,
) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECONDS))
        .build()
        .map_err(|error| error.to_string())?;
    let mut builder = match method {
        "GET" => client.get(url),
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "DELETE" => client.delete(url),
        other => return Err(format!("unsupported method {other}")),
    }
    .bearer_auth(token);
    if let Some(body) = body {
        builder = builder.json(&body);
    }
    let response = builder.send().await.map_err(|error| error.to_string())?;
    let status = response.status();
    let json: Value = response
        .json()
        .await
        .map_err(|error| format!("解析 cloudflare 响应失败: {error}"))?;
    let success = json
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if status.is_success() && success {
        Ok(json)
    } else {
        Err(cf_error_message(&json))
    }
}

#[derive(Clone, Debug)]
struct DnsStore {
    root: Arc<PathBuf>,
    write_lock: Arc<tokio::sync::Mutex<()>>,
}

impl DnsStore {
    fn from_env() -> Self {
        let root = env::var("RUSTPANEL_DNS_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_DNS_ROOT));
        Self {
            root: Arc::new(root),
            write_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    fn config_path(&self) -> PathBuf {
        self.root.join("config.json")
    }

    async fn load(&self) -> Result<StoredConfig, Status> {
        match tokio::fs::read_to_string(self.config_path()).await {
            Ok(content) => serde_json::from_str(&content).map_err(io_status),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(StoredConfig::default())
            }
            Err(error) => Err(io_status(error)),
        }
    }

    async fn save(&self, config: &StoredConfig) -> Result<(), Status> {
        tokio::fs::create_dir_all(self.root.as_ref())
            .await
            .map_err(io_status)?;
        let content = serde_json::to_string_pretty(config).map_err(io_status)?;
        let path = self.config_path();
        let tmp = path.with_extension("json.tmp");
        tokio::fs::write(&tmp, content).await.map_err(io_status)?;
        // token 是密钥,落盘按 0600。
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = tokio::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600)).await;
        }
        tokio::fs::rename(&tmp, &path).await.map_err(io_status)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredConfig {
    #[serde(default)]
    provider: i32,
    #[serde(default)]
    api_token: String,
    #[serde(default)]
    zone_id: String,
}

impl Default for StoredConfig {
    fn default() -> Self {
        Self {
            provider: DnsProvider::Cloudflare as i32,
            api_token: String::new(),
            zone_id: String::new(),
        }
    }
}

fn io_status(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_cloudflare_urls() {
        let base = "https://api.cloudflare.com/client/v4";
        assert_eq!(
            cf_records_url(base, "zone1"),
            "https://api.cloudflare.com/client/v4/zones/zone1/dns_records"
        );
        assert_eq!(
            cf_record_url(base, "zone1", "rec9"),
            "https://api.cloudflare.com/client/v4/zones/zone1/dns_records/rec9"
        );
        // 结尾斜杠不应产生双斜杠。
        assert_eq!(
            cf_records_url("https://x/", "z"),
            "https://x/zones/z/dns_records"
        );
    }

    #[test]
    fn record_body_defaults_ttl_and_trims() {
        let record = DnsRecord {
            id: String::new(),
            r#type: " A ".to_owned(),
            name: " www.example.com ".to_owned(),
            content: " 1.2.3.4 ".to_owned(),
            ttl: 0,
            proxied: true,
        };
        let body = record_to_body(&record);
        assert_eq!(body["type"], "A");
        assert_eq!(body["name"], "www.example.com");
        assert_eq!(body["content"], "1.2.3.4");
        assert_eq!(body["ttl"], 1); // 0 → automatic(1)
        assert_eq!(body["proxied"], true);
    }

    #[test]
    fn parses_record_list_and_single() {
        let json = serde_json::json!({
            "success": true,
            "result": [
                {"id":"a1","type":"A","name":"x.example.com","content":"1.1.1.1","ttl":120,"proxied":false},
                {"id":"a2","type":"TXT","name":"_acme.example.com","content":"token","ttl":1,"proxied":false}
            ]
        });
        let records = parse_records(&json).expect("parse");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].id, "a1");
        assert_eq!(records[0].ttl, 120);
        assert_eq!(records[1].r#type, "TXT");

        let single = serde_json::json!({
            "success": true,
            "result": {"id":"a3","type":"CNAME","name":"www","content":"example.com","ttl":1,"proxied":true}
        });
        let record = parse_single(&single).expect("single");
        assert_eq!(record.id, "a3");
        assert!(record.proxied);
    }

    #[test]
    fn extracts_cloudflare_error_message() {
        let json = serde_json::json!({
            "success": false,
            "errors": [{"code": 1004, "message": "DNS Validation Error"}]
        });
        assert_eq!(cf_error_message(&json), "DNS Validation Error");
        // 无 errors 时回退默认文案。
        assert_eq!(
            cf_error_message(&serde_json::json!({"success": false})),
            "cloudflare 返回失败"
        );
    }

    #[test]
    fn parse_records_rejects_missing_result() {
        assert!(parse_records(&serde_json::json!({"success": true})).is_err());
    }
}
