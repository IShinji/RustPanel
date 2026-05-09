use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures_core::Stream;
use sysinfo::{Disks, Networks, System};
use tokio::sync::broadcast;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tonic::{Request, Response as GrpcResponse, Status};
use tracing::warn;

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        monitor_service_server::MonitorService, CpuCoreStatus, DiskIoStatus,
        GetSystemStatusRequest, GetSystemStatusResponse, LoadAverageStatus, MemoryStatus,
        NetworkIoStatus, SystemStatus, WatchSystemStatusRequest, WatchSystemStatusResponse,
    },
};

const DEFAULT_STATUS_INTERVAL: Duration = Duration::from_secs(1);
const STATUS_CHANNEL_SIZE: usize = 32;

#[derive(Clone)]
pub struct MonitorServiceImpl {
    collector: Arc<SystemCollector>,
    events: broadcast::Sender<SystemStatus>,
}

impl MonitorServiceImpl {
    pub fn new() -> Self {
        let collector = Arc::new(SystemCollector::new());
        let (events, _) = broadcast::channel(STATUS_CHANNEL_SIZE);
        start_monitor_loop(collector.clone(), events.clone());

        Self { collector, events }
    }

    pub fn collector(&self) -> Arc<SystemCollector> {
        self.collector.clone()
    }
}

impl Default for MonitorServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl MonitorService for MonitorServiceImpl {
    type WatchSystemStatusStream =
        std::pin::Pin<Box<dyn Stream<Item = Result<WatchSystemStatusResponse, Status>> + Send>>;

    async fn get_system_status(
        &self,
        _request: Request<GetSystemStatusRequest>,
    ) -> Result<GrpcResponse<GetSystemStatusResponse>, Status> {
        let system_status = self.collector.snapshot()?;

        Ok(GrpcResponse::new(GetSystemStatusResponse {
            status: Some(ok_response("ok")),
            system_status: Some(system_status),
        }))
    }

    async fn watch_system_status(
        &self,
        request: Request<WatchSystemStatusRequest>,
    ) -> Result<GrpcResponse<Self::WatchSystemStatusStream>, Status> {
        let interval = request.into_inner().interval_seconds.max(1);
        let receiver = self.events.subscribe();
        let stream = BroadcastStream::new(receiver).filter_map(move |event| {
            let interval = interval as u64;
            match event {
                Ok(system_status)
                    if system_status.timestamp_seconds % interval == 0 || interval == 1 =>
                {
                    Some(Ok(WatchSystemStatusResponse {
                        status: Some(ok_response("ok")),
                        system_status: Some(system_status),
                    }))
                }
                Ok(_) => None,
                Err(error) => Some(Err(Status::internal(error.to_string()))),
            }
        });

        Ok(GrpcResponse::new(Box::pin(stream)))
    }
}

#[derive(Debug)]
pub struct SystemCollector {
    inner: Mutex<CollectorState>,
}

#[derive(Debug)]
struct CollectorState {
    system: System,
    networks: Networks,
    disks: Disks,
    disk_counters: DiskCounters,
}

impl SystemCollector {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(CollectorState {
                system: System::new_all(),
                networks: Networks::new_with_refreshed_list(),
                disks: Disks::new_with_refreshed_list(),
                disk_counters: DiskCounters::read(),
            }),
        }
    }

    pub fn snapshot(&self) -> Result<SystemStatus, Status> {
        let mut state = self
            .inner
            .lock()
            .map_err(|_| Status::internal("system collector lock poisoned"))?;

        state.system.refresh_cpu();
        state.system.refresh_memory();
        state.networks.refresh();
        state.disks.refresh();

        let previous_disk_counters = state.disk_counters.clone();
        state.disk_counters = DiskCounters::read();
        let load = System::load_average();
        let cpu_cores = state
            .system
            .cpus()
            .iter()
            .enumerate()
            .map(|(core_id, cpu)| CpuCoreStatus {
                core_id: core_id as u32,
                usage_percent: cpu.cpu_usage(),
                frequency_mhz: cpu.frequency(),
            })
            .collect::<Vec<_>>();
        let cpu_usage_percent = if cpu_cores.is_empty() {
            0.0
        } else {
            cpu_cores.iter().map(|core| core.usage_percent).sum::<f32>() / cpu_cores.len() as f32
        };
        let networks = state
            .networks
            .iter()
            .map(|(interface_name, data)| NetworkIoStatus {
                interface_name: interface_name.to_string(),
                received_bytes: data.total_received(),
                transmitted_bytes: data.total_transmitted(),
            })
            .collect::<Vec<_>>();
        let disks = state
            .disks
            .iter()
            .map(|disk| {
                let disk_name = disk.name().to_string_lossy().to_string();
                let (read_bytes, written_bytes) = state
                    .disk_counters
                    .delta_for(&previous_disk_counters, &disk_name);

                DiskIoStatus {
                    disk_name,
                    mount_point: disk.mount_point().to_string_lossy().to_string(),
                    total_space_bytes: disk.total_space(),
                    available_space_bytes: disk.available_space(),
                    read_bytes,
                    written_bytes,
                }
            })
            .collect::<Vec<_>>();

        Ok(SystemStatus {
            timestamp_seconds: unix_timestamp(),
            cpu_usage_percent,
            cpu_cores,
            memory: Some(MemoryStatus {
                total_bytes: state.system.total_memory(),
                used_bytes: state.system.used_memory(),
                available_bytes: state.system.available_memory(),
                swap_total_bytes: state.system.total_swap(),
                swap_used_bytes: state.system.used_swap(),
            }),
            load_average: Some(LoadAverageStatus {
                one_minute: load.one,
                five_minutes: load.five,
                fifteen_minutes: load.fifteen,
            }),
            networks,
            disks,
            uptime_seconds: System::uptime(),
        })
    }
}

impl Default for SystemCollector {
    fn default() -> Self {
        Self::new()
    }
}

fn start_monitor_loop(collector: Arc<SystemCollector>, events: broadcast::Sender<SystemStatus>) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(DEFAULT_STATUS_INTERVAL);
        loop {
            ticker.tick().await;
            match collector.snapshot() {
                Ok(snapshot) => {
                    let _ = events.send(snapshot);
                }
                Err(error) => warn!(%error, "failed to collect system status"),
            }
        }
    });
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[derive(Clone, Debug, Default)]
struct DiskCounters {
    counters: HashMap<String, DiskCounter>,
}

#[derive(Clone, Debug, Default)]
struct DiskCounter {
    read_bytes: u64,
    written_bytes: u64,
}

impl DiskCounters {
    fn read() -> Self {
        #[cfg(target_os = "linux")]
        {
            return Self::read_linux();
        }

        #[allow(unreachable_code)]
        Self::default()
    }

    #[cfg(target_os = "linux")]
    fn read_linux() -> Self {
        let contents = std::fs::read_to_string("/proc/diskstats").unwrap_or_default();
        let counters = contents
            .lines()
            .filter_map(parse_linux_diskstats_line)
            .collect::<HashMap<_, _>>();

        Self { counters }
    }

    fn delta_for(&self, previous: &Self, disk_name: &str) -> (u64, u64) {
        let Some(current) = self.counters.get(disk_name) else {
            return (0, 0);
        };
        let Some(previous) = previous.counters.get(disk_name) else {
            return (0, 0);
        };

        (
            current.read_bytes.saturating_sub(previous.read_bytes),
            current.written_bytes.saturating_sub(previous.written_bytes),
        )
    }
}

#[cfg(target_os = "linux")]
fn parse_linux_diskstats_line(line: &str) -> Option<(String, DiskCounter)> {
    let fields = line.split_whitespace().collect::<Vec<_>>();
    let name = fields.get(2)?.to_string();
    let sectors_read = fields.get(5)?.parse::<u64>().ok()?;
    let sectors_written = fields.get(9)?.parse::<u64>().ok()?;
    const SECTOR_BYTES: u64 = 512;

    Some((
        name,
        DiskCounter {
            read_bytes: sectors_read.saturating_mul(SECTOR_BYTES),
            written_bytes: sectors_written.saturating_mul(SECTOR_BYTES),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn parses_linux_diskstats_line() {
        let parsed = parse_linux_diskstats_line("   8       0 sda 100 0 200 0 300 0 400 0 0 0 0 0")
            .expect("diskstats");

        assert_eq!(parsed.0, "sda");
        assert_eq!(parsed.1.read_bytes, 200 * 512);
        assert_eq!(parsed.1.written_bytes, 400 * 512);
    }

    #[tokio::test]
    async fn collector_returns_timestamped_snapshot() {
        let collector = SystemCollector::new();
        let snapshot = collector.snapshot().expect("snapshot");

        assert!(snapshot.timestamp_seconds > 0);
        assert!(snapshot.memory.is_some());
    }
}
