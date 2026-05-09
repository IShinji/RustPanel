use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tonic::{Request, Response as GrpcResponse, Status};
use uuid::Uuid;

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        proxy_service_server::ProxyService, DeleteProxyInstanceRequest,
        DeleteProxyInstanceResponse, DetectVpnCapabilitiesRequest, DetectVpnCapabilitiesResponse,
        GetProxyLogRequest, GetProxyLogResponse, ListProxyInstancesRequest,
        ListProxyInstancesResponse, ListProxyTemplatesRequest, ListProxyTemplatesResponse,
        ProxyInstance, ProxyState, ProxyTemplate, StartProxyInstanceRequest,
        StartProxyInstanceResponse, StopProxyInstanceRequest, StopProxyInstanceResponse,
        UpsertProxyInstanceRequest, UpsertProxyInstanceResponse, VpnCapability,
    },
};

const DEFAULT_PROXY_ROOT: &str = "/tmp/rustpanel/proxy";
const DEFAULT_SS_METHOD: &str = "2022-blake3-aes-128-gcm";

#[derive(Clone)]
pub struct ProxyServiceImpl {
    store: ProxyStore,
    processes: ProcessRegistry,
}

impl ProxyServiceImpl {
    pub fn new() -> Self {
        Self {
            store: ProxyStore::from_env(),
            processes: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Default for ProxyServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl ProxyService for ProxyServiceImpl {
    async fn list_proxy_templates(
        &self,
        _request: Request<ListProxyTemplatesRequest>,
    ) -> Result<GrpcResponse<ListProxyTemplatesResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_PROXY)?;
        Ok(GrpcResponse::new(ListProxyTemplatesResponse {
            status: Some(ok_response("ok")),
            templates: proxy_templates(),
        }))
    }

    async fn list_proxy_instances(
        &self,
        _request: Request<ListProxyInstancesRequest>,
    ) -> Result<GrpcResponse<ListProxyInstancesResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_PROXY)?;
        let mut instances = self.store.load().await?;
        overlay_runtime_state(&mut instances, &self.processes).await;

        Ok(GrpcResponse::new(ListProxyInstancesResponse {
            status: Some(ok_response("ok")),
            instances,
        }))
    }

    async fn upsert_proxy_instance(
        &self,
        request: Request<UpsertProxyInstanceRequest>,
    ) -> Result<GrpcResponse<UpsertProxyInstanceResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_PROXY)?;
        let mut instance = request
            .into_inner()
            .instance
            .ok_or_else(|| Status::invalid_argument("proxy instance is required"))?;
        normalize_instance(&mut instance)?;
        self.store.replace(instance.clone()).await?;

        Ok(GrpcResponse::new(UpsertProxyInstanceResponse {
            status: Some(ok_response("proxy saved")),
            instance: Some(instance),
        }))
    }

    async fn start_proxy_instance(
        &self,
        request: Request<StartProxyInstanceRequest>,
    ) -> Result<GrpcResponse<StartProxyInstanceResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_PROXY)?;
        let id = request.into_inner().id;
        let instance = self.store.load_one(&id).await?;
        let instance = start_proxy(self.store.clone(), self.processes.clone(), instance).await?;

        Ok(GrpcResponse::new(StartProxyInstanceResponse {
            status: Some(ok_response("proxy started")),
            instance: Some(instance),
        }))
    }

    async fn stop_proxy_instance(
        &self,
        request: Request<StopProxyInstanceRequest>,
    ) -> Result<GrpcResponse<StopProxyInstanceResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_PROXY)?;
        let id = request.into_inner().id;
        if let Some(entry) = self.processes.lock().await.remove(&id) {
            terminate_pid(entry.pid).await;
        }
        let instance = self
            .store
            .update_state(&id, ProxyState::Stopped, 0, "stopped")
            .await?;

        Ok(GrpcResponse::new(StopProxyInstanceResponse {
            status: Some(ok_response("proxy stopped")),
            instance: Some(instance),
        }))
    }

    async fn delete_proxy_instance(
        &self,
        request: Request<DeleteProxyInstanceRequest>,
    ) -> Result<GrpcResponse<DeleteProxyInstanceResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_PROXY)?;
        let id = request.into_inner().id;
        if let Some(entry) = self.processes.lock().await.remove(&id) {
            terminate_pid(entry.pid).await;
        }
        self.store.delete(&id).await?;

        Ok(GrpcResponse::new(DeleteProxyInstanceResponse {
            status: Some(ok_response("proxy deleted")),
        }))
    }

    async fn get_proxy_log(
        &self,
        request: Request<GetProxyLogRequest>,
    ) -> Result<GrpcResponse<GetProxyLogResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_PROXY)?;
        let request = request.into_inner();
        let instance = self.store.load_one(&request.id).await?;
        let content = read_tail(
            &PathBuf::from(instance.log_path),
            request.max_bytes.clamp(1, 256 * 1024),
        )
        .await?;

        Ok(GrpcResponse::new(GetProxyLogResponse {
            status: Some(ok_response("ok")),
            content,
        }))
    }

    async fn detect_vpn_capabilities(
        &self,
        _request: Request<DetectVpnCapabilitiesRequest>,
    ) -> Result<GrpcResponse<DetectVpnCapabilitiesResponse>, Status> {
        crate::runtime::ensure_module_enabled(crate::runtime::MODULE_PROXY)?;
        let capabilities = detect_vpn_capabilities();
        let vpn_recommended = capabilities.iter().all(|capability| capability.available);
        let summary = if vpn_recommended {
            "VPN prerequisites look available".to_owned()
        } else {
            "OpenVZ/NAT environment should prefer userspace proxy unless all VPN checks pass"
                .to_owned()
        };

        Ok(GrpcResponse::new(DetectVpnCapabilitiesResponse {
            status: Some(ok_response("ok")),
            capabilities,
            vpn_recommended,
            summary,
        }))
    }
}

type ProcessRegistry = Arc<Mutex<HashMap<String, ProcessEntry>>>;

#[derive(Clone, Debug)]
struct ProcessEntry {
    pid: i32,
}

async fn start_proxy(
    store: ProxyStore,
    processes: ProcessRegistry,
    mut instance: ProxyInstance,
) -> Result<ProxyInstance, Status> {
    normalize_instance(&mut instance)?;
    if processes.lock().await.contains_key(&instance.id) {
        return Err(Status::already_exists("proxy is already running"));
    }
    let binary = ssserver_binary();
    if !binary.exists() {
        return Err(Status::failed_precondition(format!(
            "shadowsocks-rust ssserver not found at {}; install micro proxy runtime first",
            binary.display()
        )));
    }

    tokio::fs::create_dir_all(store.log_dir())
        .await
        .map_err(io_status)?;
    let stdout = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&instance.log_path)
        .map_err(io_status)?;
    let stderr = stdout.try_clone().map_err(io_status)?;
    let listen = format!("{}:{}", instance.listen_host, instance.listen_port);
    let mut child = tokio::process::Command::new(binary)
        .arg("-s")
        .arg(listen)
        .arg("-m")
        .arg(&instance.method)
        .arg("-k")
        .arg(&instance.password)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(io_status)?;
    let pid = child
        .id()
        .and_then(|pid| i32::try_from(pid).ok())
        .ok_or_else(|| Status::internal("spawned proxy has no pid"))?;
    instance.state = ProxyState::Running.into();
    instance.pid = pid;
    instance.updated_at_seconds = current_timestamp();
    store.replace(instance.clone()).await?;
    processes
        .lock()
        .await
        .insert(instance.id.clone(), ProcessEntry { pid });

    let instance_id = instance.id.clone();
    tokio::spawn(async move {
        let status = child.wait().await;
        processes.lock().await.remove(&instance_id);
        let state = if status.as_ref().is_ok_and(|status| status.success()) {
            ProxyState::Stopped
        } else {
            ProxyState::Failed
        };
        let message = status
            .map(|status| format!("exited with {status}"))
            .unwrap_or_else(|error| error.to_string());
        let _ = store.update_state(&instance_id, state, 0, &message).await;
    });

    Ok(instance)
}

#[derive(Clone, Debug)]
struct ProxyStore {
    root: Arc<PathBuf>,
}

impl ProxyStore {
    fn from_env() -> Self {
        let root = env::var("RUSTPANEL_PROXY_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_PROXY_ROOT));
        Self {
            root: Arc::new(root),
        }
    }

    async fn load(&self) -> Result<Vec<ProxyInstance>, Status> {
        match tokio::fs::read_to_string(self.instances_path()).await {
            Ok(content) => serde_json::from_str::<Vec<StoredProxyInstance>>(&content)
                .map_err(io_status)
                .map(|items| {
                    items
                        .into_iter()
                        .map(StoredProxyInstance::into_proto)
                        .collect()
                }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(io_status(error)),
        }
    }

    async fn load_one(&self, id: &str) -> Result<ProxyInstance, Status> {
        self.load()
            .await?
            .into_iter()
            .find(|instance| instance.id == id)
            .ok_or_else(|| Status::not_found("proxy not found"))
    }

    async fn replace(&self, instance: ProxyInstance) -> Result<(), Status> {
        let mut instances = self.load().await?;
        instances.retain(|stored| stored.id != instance.id);
        instances.push(instance);
        self.save(&instances).await
    }

    async fn save(&self, instances: &[ProxyInstance]) -> Result<(), Status> {
        tokio::fs::create_dir_all(self.root.as_ref())
            .await
            .map_err(io_status)?;
        let stored = instances
            .iter()
            .cloned()
            .map(StoredProxyInstance::from_proto)
            .collect::<Vec<_>>();
        let content = serde_json::to_string_pretty(&stored).map_err(io_status)?;
        tokio::fs::write(self.instances_path(), content)
            .await
            .map_err(io_status)
    }

    async fn update_state(
        &self,
        id: &str,
        state: ProxyState,
        pid: i32,
        message: &str,
    ) -> Result<ProxyInstance, Status> {
        let mut instances = self.load().await?;
        let Some(instance) = instances.iter_mut().find(|instance| instance.id == id) else {
            return Err(Status::not_found("proxy not found"));
        };
        instance.state = state.into();
        instance.pid = pid;
        instance.last_message = message.to_owned();
        instance.updated_at_seconds = current_timestamp();
        let updated = instance.clone();
        self.save(&instances).await?;
        Ok(updated)
    }

    async fn delete(&self, id: &str) -> Result<(), Status> {
        let mut instances = self.load().await?;
        let before = instances.len();
        instances.retain(|instance| instance.id != id);
        if instances.len() == before {
            return Err(Status::not_found("proxy not found"));
        }
        self.save(&instances).await
    }

    fn instances_path(&self) -> PathBuf {
        self.root.join("instances.json")
    }

    fn log_dir(&self) -> PathBuf {
        self.root.join("logs")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredProxyInstance {
    id: String,
    name: String,
    template_id: String,
    listen_host: String,
    listen_port: u32,
    method: String,
    password: String,
    state: i32,
    pid: i32,
    log_path: String,
    last_message: String,
    updated_at_seconds: u64,
}

impl StoredProxyInstance {
    fn from_proto(instance: ProxyInstance) -> Self {
        Self {
            id: instance.id,
            name: instance.name,
            template_id: instance.template_id,
            listen_host: instance.listen_host,
            listen_port: instance.listen_port,
            method: instance.method,
            password: instance.password,
            state: instance.state,
            pid: instance.pid,
            log_path: instance.log_path,
            last_message: instance.last_message,
            updated_at_seconds: instance.updated_at_seconds,
        }
    }

    fn into_proto(self) -> ProxyInstance {
        ProxyInstance {
            id: self.id,
            name: self.name,
            template_id: self.template_id,
            listen_host: self.listen_host,
            listen_port: self.listen_port,
            method: self.method,
            password: self.password,
            state: self.state,
            pid: self.pid,
            log_path: self.log_path,
            last_message: self.last_message,
            updated_at_seconds: self.updated_at_seconds,
        }
    }
}

fn normalize_instance(instance: &mut ProxyInstance) -> Result<(), Status> {
    if instance.name.trim().is_empty() {
        return Err(Status::invalid_argument("proxy name is required"));
    }
    if instance.id.trim().is_empty() {
        instance.id = Uuid::new_v4().to_string();
    }
    if instance.template_id.trim().is_empty() {
        instance.template_id = "shadowsocks-rust".to_owned();
    }
    if instance.template_id != "shadowsocks-rust" {
        return Err(Status::invalid_argument(
            "only shadowsocks-rust proxy template is supported",
        ));
    }
    if instance.listen_host.trim().is_empty() {
        instance.listen_host = "0.0.0.0".to_owned();
    }
    if instance.listen_port == 0 || instance.listen_port > u16::MAX as u32 {
        return Err(Status::invalid_argument(
            "listen_port must be between 1 and 65535",
        ));
    }
    if instance.method.trim().is_empty() {
        instance.method = DEFAULT_SS_METHOD.to_owned();
    }
    if instance.password.trim().is_empty() {
        return Err(Status::invalid_argument("proxy password is required"));
    }
    if instance.state == ProxyState::Unspecified as i32 {
        instance.state = ProxyState::Stopped.into();
    }
    if instance.log_path.trim().is_empty() {
        instance.log_path = ProxyStore::from_env()
            .root
            .join("logs")
            .join(format!("{}.log", instance.id))
            .to_string_lossy()
            .to_string();
    }
    instance.updated_at_seconds = current_timestamp();
    Ok(())
}

fn proxy_templates() -> Vec<ProxyTemplate> {
    vec![ProxyTemplate {
        id: "shadowsocks-rust".to_owned(),
        name: "shadowsocks-rust".to_owned(),
        runtime: "userspace-proxy".to_owned(),
        description: "Low-memory userspace proxy for NAT/OpenVZ micro servers".to_owned(),
        default_port: 8388,
    }]
}

fn detect_vpn_capabilities() -> Vec<VpnCapability> {
    vec![
        VpnCapability {
            id: "tun".to_owned(),
            name: "/dev/net/tun".to_owned(),
            available: Path::new("/dev/net/tun").exists(),
            reason: if Path::new("/dev/net/tun").exists() {
                "TUN device exists".to_owned()
            } else {
                "TUN device is missing; common on restricted OpenVZ".to_owned()
            },
        },
        VpnCapability {
            id: "cap-net-admin".to_owned(),
            name: "CAP_NET_ADMIN".to_owned(),
            available: has_cap_net_admin(),
            reason: if has_cap_net_admin() {
                "process has CAP_NET_ADMIN".to_owned()
            } else {
                "process lacks CAP_NET_ADMIN".to_owned()
            },
        },
        VpnCapability {
            id: "ip-tool".to_owned(),
            name: "iproute2".to_owned(),
            available: command_exists("ip"),
            reason: if command_exists("ip") {
                "ip command is available".to_owned()
            } else {
                "ip command is missing".to_owned()
            },
        },
    ]
}

async fn overlay_runtime_state(instances: &mut [ProxyInstance], processes: &ProcessRegistry) {
    let processes = processes.lock().await;
    for instance in instances {
        if let Some(entry) = processes.get(&instance.id) {
            instance.pid = entry.pid;
            instance.state = ProxyState::Running.into();
        } else if instance.state == ProxyState::Running as i32 {
            instance.state = ProxyState::Stopped.into();
            instance.pid = 0;
        }
    }
}

fn ssserver_binary() -> PathBuf {
    if let Ok(path) = env::var("RUSTPANEL_SHADOWSOCKS_SERVER_BIN") {
        return PathBuf::from(path);
    }
    env::var("RUSTPANEL_PROXY_BIN_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/usr/local/bin"))
        .join("ssserver")
}

fn has_cap_net_admin() -> bool {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return false;
    };
    let Some(line) = status.lines().find(|line| line.starts_with("CapEff:")) else {
        return false;
    };
    let Some(hex) = line.split_whitespace().nth(1) else {
        return false;
    };
    u64::from_str_radix(hex, 16)
        .map(|caps| caps & (1 << 12) != 0)
        .unwrap_or(false)
}

fn command_exists(command: &str) -> bool {
    env::var_os("PATH")
        .is_some_and(|paths| env::split_paths(&paths).any(|path| path.join(command).exists()))
}

async fn terminate_pid(pid: i32) {
    let _ = tokio::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .output()
        .await;
}

async fn read_tail(path: &Path, max_bytes: u64) -> Result<String, Status> {
    let bytes = tokio::fs::read(path).await.unwrap_or_default();
    let start = bytes.len().saturating_sub(max_bytes as usize);
    Ok(String::from_utf8_lossy(&bytes[start..]).to_string())
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn io_status(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_defaults_to_shadowsocks() {
        let mut instance = ProxyInstance {
            name: "edge".to_owned(),
            listen_port: 8388,
            password: "secret".to_owned(),
            ..Default::default()
        };

        normalize_instance(&mut instance).expect("normalized");

        assert_eq!(instance.template_id, "shadowsocks-rust");
        assert_eq!(instance.listen_host, "0.0.0.0");
        assert_eq!(instance.method, DEFAULT_SS_METHOD);
    }
}
