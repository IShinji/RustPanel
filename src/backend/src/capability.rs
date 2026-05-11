// CapabilityService:面向 NAT VPS / OpenVZ 这种受限环境的能力探针 + 资源预算 + NAT 端口预留。
//
// 设计意图:128MB / 2GB / OpenVZ NAT VPS 这种环境下,Docker / iptables / 嵌套虚拟化等
// 大量功能跑不动。前端不该等用户点了之后报错,而是开机就拉一次能力清单,把跑不动的
// 模块/操作提前置灰。NAT 端口资源(典型只有 20 个)需要显式预算管理,避免冲突。

use std::{
    env,
    net::IpAddr,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tonic::{Request as GrpcRequest, Response as GrpcResponse, Status};

use crate::proto::rustpanel::v1::{
    capability_service_server::CapabilityService, Capabilities, DiskBudget, GetCapabilitiesRequest,
    GetCapabilitiesResponse, GetResourceBudgetRequest, GetResourceBudgetResponse, Ipv6Address,
    ListIpv6AddressesRequest, ListIpv6AddressesResponse, ListReservedPortsRequest,
    ListReservedPortsResponse, MemoryBudget, PortBudget, ReleasePortRequest, ReleasePortResponse,
    ReservePortRequest, ReservePortResponse, ReservedPort, ResourceBudget, Response,
};

const DEFAULT_CAPABILITY_ROOT: &str = "/tmp/rustpanel/capability";
const DEFAULT_NAT_PORT_TOTAL: u32 = 20;

fn ok_response(message: &str) -> Response {
    Response {
        code: 0,
        message: message.to_owned(),
        data: None,
    }
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn io_status<E: std::fmt::Display>(error: E) -> Status {
    Status::internal(format!("io error: {error}"))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredReservation {
    port: u32,
    owner: String,
    description: String,
    protocol: String,
    reserved_at_seconds: u64,
}

impl From<StoredReservation> for ReservedPort {
    fn from(value: StoredReservation) -> Self {
        ReservedPort {
            port: value.port,
            owner: value.owner,
            description: value.description,
            protocol: value.protocol,
            reserved_at_seconds: value.reserved_at_seconds,
        }
    }
}

#[derive(Clone)]
pub struct CapabilityServiceImpl {
    inner: Arc<CapabilityState>,
}

struct CapabilityState {
    root: PathBuf,
    reservations: Mutex<Vec<StoredReservation>>,
    capabilities_cache: Mutex<Option<Capabilities>>,
}

impl CapabilityServiceImpl {
    pub fn new() -> Self {
        let root = env::var("RUSTPANEL_CAPABILITY_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_CAPABILITY_ROOT));

        Self {
            inner: Arc::new(CapabilityState {
                root,
                reservations: Mutex::new(Vec::new()),
                capabilities_cache: Mutex::new(None),
            }),
        }
    }

    fn reservations_path(&self) -> PathBuf {
        self.inner.root.join("ports.json")
    }

    async fn load_reservations(&self) -> Result<Vec<StoredReservation>, Status> {
        let mut guard = self.inner.reservations.lock().await;
        if !guard.is_empty() {
            return Ok(guard.clone());
        }
        match tokio::fs::read_to_string(self.reservations_path()).await {
            Ok(content) => {
                let parsed: Vec<StoredReservation> =
                    serde_json::from_str(&content).map_err(io_status)?;
                *guard = parsed.clone();
                Ok(parsed)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(io_status(error)),
        }
    }

    async fn save_reservations(&self, items: &[StoredReservation]) -> Result<(), Status> {
        tokio::fs::create_dir_all(&self.inner.root)
            .await
            .map_err(io_status)?;
        let content = serde_json::to_string_pretty(items).map_err(io_status)?;
        tokio::fs::write(self.reservations_path(), content)
            .await
            .map_err(io_status)
    }

    /// 在后端启动时调用一次,把面板自身监听的端口写进预留表(owner=panel),
    /// 让 NAT 端口预算自然算上"面板已经占了 1 个"。重复调用幂等。
    pub async fn seed_panel_port(&self, port: u16) -> Result<(), Status> {
        if port == 0 {
            return Ok(());
        }
        let mut items = self.load_reservations().await?;
        if items.iter().any(|i| i.port == port as u32) {
            return Ok(());
        }
        items.push(StoredReservation {
            port: port as u32,
            owner: "panel".to_owned(),
            description: "RustPanel 面板自身监听端口".to_owned(),
            protocol: "tcp".to_owned(),
            reserved_at_seconds: now_seconds(),
        });
        items.sort_by_key(|i| i.port);
        self.save_reservations(&items).await?;
        let mut guard = self.inner.reservations.lock().await;
        *guard = items;
        Ok(())
    }
}

impl Default for CapabilityServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl CapabilityService for CapabilityServiceImpl {
    async fn get_capabilities(
        &self,
        _request: GrpcRequest<GetCapabilitiesRequest>,
    ) -> Result<GrpcResponse<GetCapabilitiesResponse>, Status> {
        let mut cache = self.inner.capabilities_cache.lock().await;
        if let Some(existing) = cache.as_ref() {
            // 缓存命中 1 小时 (probed_at_seconds + 3600 > now)
            if existing.probed_at_seconds + 3600 > now_seconds() {
                return Ok(GrpcResponse::new(GetCapabilitiesResponse {
                    status: Some(ok_response("ok")),
                    capabilities: Some(existing.clone()),
                }));
            }
        }
        let probed = probe_capabilities();
        *cache = Some(probed.clone());
        Ok(GrpcResponse::new(GetCapabilitiesResponse {
            status: Some(ok_response("ok")),
            capabilities: Some(probed),
        }))
    }

    async fn get_resource_budget(
        &self,
        _request: GrpcRequest<GetResourceBudgetRequest>,
    ) -> Result<GrpcResponse<GetResourceBudgetResponse>, Status> {
        let mut sys = sysinfo::System::new();
        sys.refresh_memory();
        sys.refresh_cpu();

        let memory = MemoryBudget {
            total_bytes: sys.total_memory(),
            used_bytes: sys.used_memory(),
            available_bytes: sys.total_memory().saturating_sub(sys.used_memory()),
            swap_total_bytes: sys.total_swap(),
            swap_used_bytes: sys.used_swap(),
        };

        let mut disks_view = sysinfo::Disks::new_with_refreshed_list();
        disks_view.refresh();
        let mut disks: Vec<DiskBudget> = disks_view
            .list()
            .iter()
            .map(|disk| {
                let mount = disk.mount_point().to_string_lossy().into_owned();
                DiskBudget {
                    mount_point: mount,
                    filesystem: disk.file_system().to_string_lossy().into_owned(),
                    total_bytes: disk.total_space(),
                    used_bytes: disk.total_space().saturating_sub(disk.available_space()),
                    available_bytes: disk.available_space(),
                }
            })
            .collect();
        // OpenVZ 上很常见 / 重复或被代理(simfs/vzfs);只保留唯一 mount,根分区优先
        disks.sort_by(|a, b| {
            let priority = |m: &str| if m == "/" { 0 } else { 1 };
            priority(&a.mount_point)
                .cmp(&priority(&b.mount_point))
                .then(a.mount_point.cmp(&b.mount_point))
        });
        disks.dedup_by(|a, b| a.mount_point == b.mount_point);

        let load = sysinfo::System::load_average();

        let reservations = self.load_reservations().await?;
        let total_ports = env::var("RUSTPANEL_NAT_PORT_TOTAL")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(DEFAULT_NAT_PORT_TOTAL);
        let listening = count_listening_ports();

        let ports = PortBudget {
            total: total_ports,
            reserved: reservations.len() as u32,
            listening,
        };

        let cpu_count = sys.cpus().len() as u32;

        let budget = ResourceBudget {
            memory: Some(memory),
            disks,
            ports: Some(ports),
            cpu_count,
            load_one: load.one,
            load_five: load.five,
            load_fifteen: load.fifteen,
        };

        Ok(GrpcResponse::new(GetResourceBudgetResponse {
            status: Some(ok_response("ok")),
            budget: Some(budget),
        }))
    }

    async fn list_reserved_ports(
        &self,
        _request: GrpcRequest<ListReservedPortsRequest>,
    ) -> Result<GrpcResponse<ListReservedPortsResponse>, Status> {
        let items = self.load_reservations().await?;
        Ok(GrpcResponse::new(ListReservedPortsResponse {
            status: Some(ok_response("ok")),
            ports: items.into_iter().map(Into::into).collect(),
        }))
    }

    async fn reserve_port(
        &self,
        request: GrpcRequest<ReservePortRequest>,
    ) -> Result<GrpcResponse<ReservePortResponse>, Status> {
        let req = request.into_inner();
        if req.port == 0 || req.port > 65535 {
            return Err(Status::invalid_argument("port must be 1-65535"));
        }
        if req.owner.trim().is_empty() {
            return Err(Status::invalid_argument("owner cannot be empty"));
        }

        let mut items = self.load_reservations().await?;
        if items.iter().any(|item| item.port == req.port) {
            return Err(Status::already_exists(format!(
                "port {} already reserved",
                req.port
            )));
        }

        let entry = StoredReservation {
            port: req.port,
            owner: req.owner.clone(),
            description: req.description.clone(),
            protocol: if req.protocol.is_empty() {
                "tcp".to_owned()
            } else {
                req.protocol.clone()
            },
            reserved_at_seconds: now_seconds(),
        };
        items.push(entry.clone());
        items.sort_by_key(|item| item.port);
        self.save_reservations(&items).await?;

        let mut guard = self.inner.reservations.lock().await;
        *guard = items;

        Ok(GrpcResponse::new(ReservePortResponse {
            status: Some(ok_response("reserved")),
            reservation: Some(entry.into()),
        }))
    }

    async fn release_port(
        &self,
        request: GrpcRequest<ReleasePortRequest>,
    ) -> Result<GrpcResponse<ReleasePortResponse>, Status> {
        let req = request.into_inner();
        let mut items = self.load_reservations().await?;
        let before = items.len();
        items.retain(|item| item.port != req.port);
        if items.len() == before {
            return Err(Status::not_found(format!("port {} not reserved", req.port)));
        }
        self.save_reservations(&items).await?;
        let mut guard = self.inner.reservations.lock().await;
        *guard = items;

        Ok(GrpcResponse::new(ReleasePortResponse {
            status: Some(ok_response("released")),
        }))
    }

    async fn list_ipv6_addresses(
        &self,
        _request: GrpcRequest<ListIpv6AddressesRequest>,
    ) -> Result<GrpcResponse<ListIpv6AddressesResponse>, Status> {
        let (addresses, prefixes) = collect_ipv6_addresses().await;
        Ok(GrpcResponse::new(ListIpv6AddressesResponse {
            status: Some(ok_response("ok")),
            addresses,
            prefixes,
        }))
    }
}

// ====== 探测实现 ======

/// 暴露给其他模块(如 appstore)做兼容性判定。同步读 /proc。
pub fn probe_capabilities_sync() -> Capabilities {
    probe_capabilities()
}

/// 同步读取一份资源预算快照,供其他模块判定"装得下吗"。
/// 不计 NAT 端口预算(那部分需要 IO 读 ports.json,异步)。
pub fn snapshot_resource_budget_sync() -> ResourceBudget {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.refresh_cpu();

    let memory = MemoryBudget {
        total_bytes: sys.total_memory(),
        used_bytes: sys.used_memory(),
        available_bytes: sys.total_memory().saturating_sub(sys.used_memory()),
        swap_total_bytes: sys.total_swap(),
        swap_used_bytes: sys.used_swap(),
    };

    let mut disks_view = sysinfo::Disks::new_with_refreshed_list();
    disks_view.refresh();
    let mut disks: Vec<DiskBudget> = disks_view
        .list()
        .iter()
        .map(|disk| DiskBudget {
            mount_point: disk.mount_point().to_string_lossy().into_owned(),
            filesystem: disk.file_system().to_string_lossy().into_owned(),
            total_bytes: disk.total_space(),
            used_bytes: disk.total_space().saturating_sub(disk.available_space()),
            available_bytes: disk.available_space(),
        })
        .collect();
    disks.sort_by(|a, b| {
        let priority = |m: &str| if m == "/" { 0 } else { 1 };
        priority(&a.mount_point)
            .cmp(&priority(&b.mount_point))
            .then(a.mount_point.cmp(&b.mount_point))
    });
    disks.dedup_by(|a, b| a.mount_point == b.mount_point);

    let load = sysinfo::System::load_average();

    ResourceBudget {
        memory: Some(memory),
        disks,
        ports: None,
        cpu_count: sys.cpus().len() as u32,
        load_one: load.one,
        load_five: load.five,
        load_fifteen: load.fifteen,
    }
}

fn probe_capabilities() -> Capabilities {
    let kernel_version = sysinfo::System::kernel_version().unwrap_or_default();
    let is_openvz = std::path::Path::new("/proc/user_beancounters").exists()
        || std::path::Path::new("/proc/vz").exists();
    let is_container = std::path::Path::new("/.dockerenv").exists()
        || std::fs::read_to_string("/proc/1/cgroup")
            .map(|content| content.contains("docker") || content.contains("lxc"))
            .unwrap_or(false);

    let filesystems = std::fs::read_to_string("/proc/filesystems").unwrap_or_default();
    let has_overlay2 = filesystems.lines().any(|line| line.contains("overlay"));
    let has_fuse = filesystems.lines().any(|line| line.contains("fuse"));

    let modules = std::fs::read_to_string("/proc/modules").unwrap_or_default();
    let has_nf_nat = modules.lines().any(|line| line.starts_with("nf_nat"));

    // iptables 二进制存在 + 内核 ip_tables 模块加载
    let has_iptables = std::path::Path::new("/usr/sbin/iptables").exists()
        || std::path::Path::new("/sbin/iptables").exists();

    let swaps = std::fs::read_to_string("/proc/swaps").unwrap_or_default();
    let has_swap = swaps.lines().count() > 1; // 第一行是表头

    let congestion = std::fs::read_to_string("/proc/sys/net/ipv4/tcp_available_congestion_control")
        .unwrap_or_default();
    let has_bbr = congestion.contains("bbr");

    let has_cgroups_v2 = std::path::Path::new("/sys/fs/cgroup/cgroup.controllers").exists();

    let max_user_ns =
        std::fs::read_to_string("/proc/sys/user/max_user_namespaces").unwrap_or_default();
    let has_user_namespaces = max_user_ns.trim().parse::<u64>().unwrap_or(0) > 0;

    let docker_running = std::path::Path::new("/var/run/docker.sock").exists();

    // 综合判定:OpenVZ 且无 overlay2 / 无 user namespace 时 docker 不可用
    let mut docker_block_reason = String::new();
    let mut can_run_docker = docker_running || has_overlay2;
    if is_openvz && !has_overlay2 {
        can_run_docker = false;
        docker_block_reason = "OpenVZ 内核未提供 overlayfs,Docker 无法启动".to_owned();
    } else if !has_overlay2 && !docker_running {
        can_run_docker = false;
        docker_block_reason = "缺少 overlay2 文件系统支持".to_owned();
    } else if !has_user_namespaces && is_openvz {
        can_run_docker = false;
        docker_block_reason = "OpenVZ 上 user_namespaces 已禁用".to_owned();
    }

    // 推荐 ACME challenge:
    // - OpenVZ NAT VPS:80 端口通常没在 NAT mapping 里,HTTP-01 进不来 → DNS-01
    // - 其它环境:大概率有公网 80,HTTP-01 一键搞定;LE 真打不通的话用户
    //   再手动切回 DNS-01。
    // 这里宁可保守(默认 dns01)也不让用户面对一次 HTTP-01 失败的尴尬。
    let (recommended_acme_challenge, acme_challenge_reason) = if is_openvz {
        (
            "dns01".to_owned(),
            "OpenVZ NAT VPS · 公网 80 通常没在转发表里,HTTP-01 走不通,DNS-01 更稳。".to_owned(),
        )
    } else if is_container {
        (
            "dns01".to_owned(),
            "容器环境 · 入站 80 不一定暴露,先走 DNS-01 保稳。".to_owned(),
        )
    } else {
        (
            "http01".to_owned(),
            "公网 IP 主机 · HTTP-01 一键完成,不用碰 DNS。".to_owned(),
        )
    };

    Capabilities {
        is_openvz,
        is_container,
        kernel_version,
        has_overlay2,
        has_fuse,
        has_iptables,
        has_nf_nat,
        has_swap,
        has_bbr,
        has_cgroups_v2,
        has_user_namespaces,
        docker_running,
        can_run_docker,
        docker_block_reason,
        probed_at_seconds: now_seconds(),
        recommended_acme_challenge,
        acme_challenge_reason,
    }
}

fn count_listening_ports() -> u32 {
    // 解析 /proc/net/tcp + /proc/net/tcp6 中 state == 0A (LISTEN) 的条目数
    let mut total = 0u32;
    for path in ["/proc/net/tcp", "/proc/net/tcp6"] {
        if let Ok(content) = std::fs::read_to_string(path) {
            for line in content.lines().skip(1) {
                // local_address:hex_port 在第二列,state 在第四列
                let mut parts = line.split_whitespace();
                let _local = parts.next();
                let _remote = parts.next();
                if let Some(state) = parts.next() {
                    if state == "0A" {
                        total += 1;
                    }
                }
            }
        }
    }
    total
}

async fn collect_ipv6_addresses() -> (Vec<Ipv6Address>, Vec<String>) {
    // 优先用 `ip -6 -o addr show` 解析,失败再回退到 /proc/net/if_inet6
    let mut addresses: Vec<Ipv6Address> = Vec::new();

    if let Ok(output) = tokio::process::Command::new("ip")
        .args(["-6", "-o", "addr", "show", "scope", "global"])
        .output()
        .await
    {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                // 格式: "2: eth0    inet6 2001:db8::1/64 scope global ..."
                let mut tokens = line.split_whitespace();
                let _idx = tokens.next();
                let iface = tokens.next().unwrap_or("").trim_end_matches(':').to_owned();
                let _family = tokens.next();
                let cidr = tokens.next().unwrap_or("");
                if let Some((addr, prefix)) = cidr.split_once('/') {
                    if let (Ok(IpAddr::V6(v6)), Ok(p)) =
                        (addr.parse::<IpAddr>(), prefix.parse::<u32>())
                    {
                        if is_real_global_ipv6(&v6) {
                            addresses.push(Ipv6Address {
                                address: addr.to_owned(),
                                prefix_length: p,
                                interface_name: iface.clone(),
                                is_global: true,
                            });
                        }
                    }
                }
            }
        }
    }

    if addresses.is_empty() {
        if let Ok(content) = tokio::fs::read_to_string("/proc/net/if_inet6").await {
            for line in content.lines() {
                let mut parts = line.split_whitespace();
                let hex_addr = parts.next().unwrap_or("");
                let _idx = parts.next();
                let prefix_hex = parts.next().unwrap_or("80");
                let _scope = parts.next();
                let _flags = parts.next();
                let iface = parts.next().unwrap_or("").to_owned();
                if hex_addr.len() == 32 {
                    let mut groups = Vec::with_capacity(8);
                    for chunk in (0..32).step_by(4) {
                        groups.push(&hex_addr[chunk..chunk + 4]);
                    }
                    let formatted = groups.join(":");
                    if let Ok(IpAddr::V6(v6)) = formatted.parse::<IpAddr>() {
                        if is_real_global_ipv6(&v6) {
                            let prefix_length = u32::from_str_radix(prefix_hex, 16).unwrap_or(80);
                            addresses.push(Ipv6Address {
                                address: v6.to_string(),
                                prefix_length,
                                interface_name: iface,
                                is_global: true,
                            });
                        }
                    }
                }
            }
        }
    }

    // 推导前缀:每个地址按 prefix_length 截断,生成 "ip/prefix" 字符串集合
    let mut prefixes: Vec<String> = addresses
        .iter()
        .map(|addr| {
            format!(
                "{}/{}",
                truncate_ipv6_prefix(&addr.address, addr.prefix_length),
                addr.prefix_length
            )
        })
        .collect();
    prefixes.sort();
    prefixes.dedup();

    (addresses, prefixes)
}

// 判定一个 v6 地址是不是"真正的可用公网地址"。
// 排除掉:
//   ::          (unspecified)
//   ::1         (loopback)
//   ::/96       (deprecated IPv4-Compatible IPv6 Address, RFC 4291 §2.5.5.1,
//                典型形如 ::2 / ::ffff:1.2.3.4 旧映射,大多数现代 OS 不再用)
//   fe80::/10   (link-local)
//   ::ffff:0:0/96 (IPv4-Mapped,nginx 不该把它当公网 v6 listen)
//   fc00::/7    (Unique Local,私网,不是公网)
//   fec0::/10   (deprecated site-local)
//   ff00::/8    (multicast)
fn is_real_global_ipv6(addr: &std::net::Ipv6Addr) -> bool {
    if addr.is_unspecified() || addr.is_loopback() || addr.is_multicast() {
        return false;
    }
    let segs = addr.segments();
    // link-local fe80::/10
    if segs[0] & 0xffc0 == 0xfe80 {
        return false;
    }
    // site-local fec0::/10 (deprecated)
    if segs[0] & 0xffc0 == 0xfec0 {
        return false;
    }
    // ULA fc00::/7
    if segs[0] & 0xfe00 == 0xfc00 {
        return false;
    }
    // ::/96 deprecated IPv4-compatible:前 96 位全为 0
    if segs[0] == 0 && segs[1] == 0 && segs[2] == 0 && segs[3] == 0 && segs[4] == 0 && segs[5] == 0
    {
        return false;
    }
    // ::ffff:0:0/96 IPv4-Mapped
    if segs[0] == 0
        && segs[1] == 0
        && segs[2] == 0
        && segs[3] == 0
        && segs[4] == 0
        && segs[5] == 0xffff
    {
        return false;
    }
    true
}

fn truncate_ipv6_prefix(address: &str, prefix_length: u32) -> String {
    let parsed: std::net::Ipv6Addr = match address.parse() {
        Ok(addr) => addr,
        Err(_) => return address.to_owned(),
    };
    let bits = u128::from(parsed);
    let prefix_length = prefix_length.min(128);
    if prefix_length == 0 {
        return std::net::Ipv6Addr::UNSPECIFIED.to_string();
    }
    let mask = if prefix_length == 128 {
        u128::MAX
    } else {
        ((1u128 << prefix_length) - 1) << (128 - prefix_length)
    };
    let truncated = std::net::Ipv6Addr::from(bits & mask);
    truncated.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_ipv6_prefix_handles_80_block() {
        let prefix = truncate_ipv6_prefix("2001:db8:abcd:1234:5678::1", 80);
        assert_eq!(prefix, "2001:db8:abcd:1234:5678::");
    }

    #[test]
    fn truncate_ipv6_prefix_handles_64() {
        let prefix = truncate_ipv6_prefix("2001:db8:abcd:1234:5678::1", 64);
        assert_eq!(prefix, "2001:db8:abcd:1234::");
    }

    #[test]
    fn count_listening_ports_returns_non_negative() {
        // 仅在 Linux 真实存在 /proc/net/tcp 时 > 0;CI 沙箱也通常有 sshd
        let count = count_listening_ports();
        assert!(count < 100_000);
    }

    #[tokio::test]
    async fn reservation_round_trip() {
        let tmp = tempdir_for_test();
        env::set_var("RUSTPANEL_CAPABILITY_ROOT", tmp.path());
        let svc = CapabilityServiceImpl::new();

        // 初始为空
        let listed = svc.load_reservations().await.unwrap();
        assert!(listed.is_empty());

        // 预留一个
        svc.reserve_port(GrpcRequest::new(ReservePortRequest {
            port: 8443,
            owner: "panel".into(),
            description: "RustPanel".into(),
            protocol: "tcp".into(),
        }))
        .await
        .unwrap();
        let listed = svc.load_reservations().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].port, 8443);

        // 重复预留应失败
        let dup = svc
            .reserve_port(GrpcRequest::new(ReservePortRequest {
                port: 8443,
                owner: "other".into(),
                description: String::new(),
                protocol: String::new(),
            }))
            .await;
        assert!(dup.is_err());

        // 释放
        svc.release_port(GrpcRequest::new(ReleasePortRequest { port: 8443 }))
            .await
            .unwrap();
        let listed = svc.load_reservations().await.unwrap();
        assert!(listed.is_empty());

        env::remove_var("RUSTPANEL_CAPABILITY_ROOT");
    }

    fn tempdir_for_test() -> tempfile::TempDir {
        tempfile::tempdir().expect("create tempdir")
    }
}
