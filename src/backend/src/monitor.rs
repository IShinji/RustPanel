use std::{
    collections::HashMap,
    env,
    path::PathBuf,
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
        GenerateHealthReportRequest, GenerateHealthReportResponse, GetMetricHistoryRequest,
        GetMetricHistoryResponse, GetProcessSnapshotRequest, GetProcessSnapshotResponse,
        GetSystemStatusRequest, GetSystemStatusResponse, LoadAverageStatus, MemoryStatus,
        NetworkIoStatus, ProcessResourceSnapshot, SystemStatus, WatchSystemStatusRequest,
        WatchSystemStatusResponse,
    },
};

const DEFAULT_STATUS_INTERVAL: Duration = Duration::from_secs(1);
const STATUS_CHANNEL_SIZE: usize = 32;
const HISTORY_SAMPLE_INTERVAL_SECONDS: u64 = 60;
const HISTORY_RETENTION_SECONDS: u64 = 7 * 24 * 60 * 60;
const HISTORY_MAX_SAMPLES: usize = 7 * 24 * 60;
// 进程快照体积大(每条 20 个进程),只留 24 小时
const PROCESS_HISTORY_RETENTION_SECONDS: u64 = 24 * 60 * 60;
const PROCESS_HISTORY_MAX_SAMPLES: usize = 24 * 60;
// 指标历史落盘间隔;空闲退出前也会再落一次
const HISTORY_PERSIST_INTERVAL_SECONDS: u64 = 10 * 60;
const PROCESS_SNAPSHOT_LIMIT: usize = 20;
const PROCESS_SNAPSHOT_MAX_LIMIT: usize = 100;

#[derive(Clone)]
pub struct MonitorServiceImpl {
    collector: Arc<SystemCollector>,
    events: broadcast::Sender<SystemStatus>,
    history: Arc<Mutex<MonitorHistory>>,
}

impl MonitorServiceImpl {
    pub fn new() -> Self {
        let collector = Arc::new(SystemCollector::new());
        let (events, _) = broadcast::channel(STATUS_CHANNEL_SIZE);
        let history = Arc::new(Mutex::new(MonitorHistory::load_persisted()));
        let _ = HISTORY_HANDLE.set(history.clone());
        start_monitor_loop(collector.clone(), events.clone(), history.clone());

        Self {
            collector,
            events,
            history,
        }
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

    async fn get_metric_history(
        &self,
        request: Request<GetMetricHistoryRequest>,
    ) -> Result<GrpcResponse<GetMetricHistoryResponse>, Status> {
        let request = request.into_inner();
        let end_seconds = if request.end_seconds == 0 {
            unix_timestamp()
        } else {
            request.end_seconds
        };
        let start_seconds = if request.start_seconds == 0 {
            end_seconds.saturating_sub(60 * 60)
        } else {
            request.start_seconds
        };

        let mut samples = self
            .history
            .lock()
            .map_err(|_| Status::internal("monitor history lock poisoned"))?
            .metric_samples(start_seconds, end_seconds);

        if samples.is_empty() {
            samples.push(self.collector.snapshot()?);
        }

        Ok(GrpcResponse::new(GetMetricHistoryResponse {
            status: Some(ok_response("ok")),
            samples,
        }))
    }

    async fn get_process_snapshot(
        &self,
        request: Request<GetProcessSnapshotRequest>,
    ) -> Result<GrpcResponse<GetProcessSnapshotResponse>, Status> {
        let request = request.into_inner();
        let timestamp_seconds = if request.timestamp_seconds == 0 {
            unix_timestamp()
        } else {
            request.timestamp_seconds
        };
        let limit = bounded_process_limit(request.limit);
        let processes = self
            .history
            .lock()
            .map_err(|_| Status::internal("monitor history lock poisoned"))?
            .nearest_process_snapshot(timestamp_seconds, limit)
            .unwrap_or_else(|| collect_process_snapshot(limit));

        Ok(GrpcResponse::new(GetProcessSnapshotResponse {
            status: Some(ok_response("ok")),
            processes,
        }))
    }

    async fn generate_health_report(
        &self,
        request: Request<GenerateHealthReportRequest>,
    ) -> Result<GrpcResponse<GenerateHealthReportResponse>, Status> {
        let period = normalize_report_period(&request.into_inner().period);
        let now = unix_timestamp();
        let start_seconds = now.saturating_sub(report_period_seconds(period));
        let mut samples = self
            .history
            .lock()
            .map_err(|_| Status::internal("monitor history lock poisoned"))?
            .metric_samples(start_seconds, now);

        if samples.is_empty() {
            samples.push(self.collector.snapshot()?);
        }

        let security = SecurityReportCounters::read_since(start_seconds);
        let report = render_health_report(period, start_seconds, now, &samples, security);

        Ok(GrpcResponse::new(GenerateHealthReportResponse {
            status: Some(ok_response("ok")),
            report,
        }))
    }
}

/// 进程内唯一的监控历史,供空闲退出前落盘([`flush_history`])。
static HISTORY_HANDLE: std::sync::OnceLock<Arc<Mutex<MonitorHistory>>> = std::sync::OnceLock::new();

/// 指标历史的紧凑形态:图表与健康报告只用到这些字段。完整 SystemStatus 带每核 CPU、
/// 每块盘、每张网卡,7 天 × 每分钟一条在内存里能涨到几十 MB(128MB 小鸡扛不住)。
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
struct CompactSample {
    t: u64,
    cpu: f32,
    cores: u32,
    mem_total: u64,
    mem_used: u64,
    mem_avail: u64,
    load: [f64; 3],
    /// 占用率最高那块盘(报告里只看峰值磁盘占用)
    disk_total: u64,
    disk_avail: u64,
    rx: u64,
    tx: u64,
}

impl CompactSample {
    fn from_status(status: &SystemStatus) -> Self {
        let memory = status.memory.unwrap_or_default();
        let load = status.load_average.unwrap_or_default();
        let fullest = status
            .disks
            .iter()
            .max_by(|a, b| disk_usage_percent(a).total_cmp(&disk_usage_percent(b)));
        Self {
            t: status.timestamp_seconds,
            cpu: status.cpu_usage_percent,
            cores: u32::try_from(status.cpu_cores.len()).unwrap_or(u32::MAX),
            mem_total: memory.total_bytes,
            mem_used: memory.used_bytes,
            mem_avail: memory.available_bytes,
            load: [load.one_minute, load.five_minutes, load.fifteen_minutes],
            disk_total: fullest.map_or(0, |disk| disk.total_space_bytes),
            disk_avail: fullest.map_or(0, |disk| disk.available_space_bytes),
            rx: status.networks.iter().map(|net| net.received_bytes).sum(),
            tx: status
                .networks
                .iter()
                .map(|net| net.transmitted_bytes)
                .sum(),
        }
    }

    fn to_status(&self) -> SystemStatus {
        SystemStatus {
            timestamp_seconds: self.t,
            cpu_usage_percent: self.cpu,
            cpu_cores: (0..self.cores)
                .map(|core_id| CpuCoreStatus {
                    core_id,
                    ..CpuCoreStatus::default()
                })
                .collect(),
            memory: Some(MemoryStatus {
                total_bytes: self.mem_total,
                used_bytes: self.mem_used,
                available_bytes: self.mem_avail,
                swap_total_bytes: 0,
                swap_used_bytes: 0,
            }),
            load_average: Some(LoadAverageStatus {
                one_minute: self.load[0],
                five_minutes: self.load[1],
                fifteen_minutes: self.load[2],
            }),
            networks: vec![NetworkIoStatus {
                interface_name: "total".to_owned(),
                received_bytes: self.rx,
                transmitted_bytes: self.tx,
            }],
            disks: vec![DiskIoStatus {
                disk_name: "fullest".to_owned(),
                mount_point: String::new(),
                total_space_bytes: self.disk_total,
                available_space_bytes: self.disk_avail,
                read_bytes: 0,
                written_bytes: 0,
            }],
            uptime_seconds: 0,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct MonitorHistory {
    metric_samples: Vec<CompactSample>,
    process_samples: Vec<ProcessHistorySample>,
    last_sample_seconds: u64,
    last_persist_seconds: u64,
}

impl MonitorHistory {
    fn push(
        &mut self,
        sample: SystemStatus,
        processes: Vec<ProcessResourceSnapshot>,
        now_seconds: u64,
    ) {
        if self.last_sample_seconds != 0
            && now_seconds.saturating_sub(self.last_sample_seconds)
                < HISTORY_SAMPLE_INTERVAL_SECONDS
        {
            return;
        }

        self.last_sample_seconds = now_seconds;
        self.metric_samples
            .push(CompactSample::from_status(&sample));
        self.process_samples.push(ProcessHistorySample {
            timestamp_seconds: now_seconds,
            processes,
        });
        self.prune(now_seconds);
    }

    fn metric_samples(&self, start_seconds: u64, end_seconds: u64) -> Vec<SystemStatus> {
        self.metric_samples
            .iter()
            .filter(|sample| sample.t >= start_seconds && sample.t <= end_seconds)
            .map(CompactSample::to_status)
            .collect()
    }

    /// 下一次采样到点了吗(没人订阅实时流时,只在到点时才采集)。
    fn sample_due(&self, now_seconds: u64) -> bool {
        self.last_sample_seconds == 0
            || now_seconds.saturating_sub(self.last_sample_seconds)
                >= HISTORY_SAMPLE_INTERVAL_SECONDS
    }

    fn history_path() -> PathBuf {
        crate::paths::state_root("monitor", "RUSTPANEL_MONITOR_ROOT").join("history.json")
    }

    /// 启动时读回上次落盘的指标历史(过期的丢掉);读不到就从空开始。
    fn load_persisted() -> Self {
        let mut history = Self::default();
        if let Ok(content) = std::fs::read(Self::history_path()) {
            if let Ok(samples) = serde_json::from_slice::<Vec<CompactSample>>(&content) {
                history.metric_samples = samples;
                history.prune(unix_timestamp());
            }
        }
        history.last_persist_seconds = unix_timestamp();
        history
    }

    fn persist(&mut self, now_seconds: u64) {
        self.last_persist_seconds = now_seconds;
        let path = Self::history_path();
        let Ok(content) = serde_json::to_vec(&self.metric_samples) else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(error) = crate::statefile::write_atomic_blocking(&path, content) {
            warn!(%error, "failed to persist monitor history");
        }
    }

    fn nearest_process_snapshot(
        &self,
        timestamp_seconds: u64,
        limit: usize,
    ) -> Option<Vec<ProcessResourceSnapshot>> {
        self.process_samples
            .iter()
            .min_by_key(|sample| sample.timestamp_seconds.abs_diff(timestamp_seconds))
            .map(|sample| {
                sample
                    .processes
                    .iter()
                    .take(limit)
                    .cloned()
                    .collect::<Vec<_>>()
            })
    }

    fn prune(&mut self, now_seconds: u64) {
        let oldest = now_seconds.saturating_sub(HISTORY_RETENTION_SECONDS);
        self.metric_samples.retain(|sample| sample.t >= oldest);
        let oldest_process = now_seconds.saturating_sub(PROCESS_HISTORY_RETENTION_SECONDS);
        self.process_samples
            .retain(|sample| sample.timestamp_seconds >= oldest_process);

        if self.metric_samples.len() > HISTORY_MAX_SAMPLES {
            let drain_count = self.metric_samples.len() - HISTORY_MAX_SAMPLES;
            self.metric_samples.drain(0..drain_count);
        }
        if self.process_samples.len() > PROCESS_HISTORY_MAX_SAMPLES {
            let drain_count = self.process_samples.len() - PROCESS_HISTORY_MAX_SAMPLES;
            self.process_samples.drain(0..drain_count);
        }
    }
}

#[derive(Clone, Debug)]
struct ProcessHistorySample {
    timestamp_seconds: u64,
    processes: Vec<ProcessResourceSnapshot>,
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

fn start_monitor_loop(
    collector: Arc<SystemCollector>,
    events: broadcast::Sender<SystemStatus>,
    history: Arc<Mutex<MonitorHistory>>,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(DEFAULT_STATUS_INTERVAL);
        loop {
            ticker.tick().await;
            let now_seconds = unix_timestamp();
            let history_due = history
                .lock()
                .map(|history| history.sample_due(now_seconds))
                .unwrap_or(false);
            // 没人看实时流、历史也没到点:什么都不采(每秒全量采集在小鸡上是纯浪费)
            if events.receiver_count() == 0 && !history_due {
                continue;
            }
            match collector.snapshot() {
                Ok(snapshot) => {
                    if history_due {
                        // 进程快照只在落历史时采,别每秒扫一遍 /proc
                        let processes = collect_process_snapshot(PROCESS_SNAPSHOT_LIMIT);
                        match history.lock() {
                            Ok(mut history) => {
                                history.push(
                                    snapshot.clone(),
                                    processes,
                                    snapshot.timestamp_seconds,
                                );
                                if now_seconds.saturating_sub(history.last_persist_seconds)
                                    >= HISTORY_PERSIST_INTERVAL_SECONDS
                                {
                                    history.persist(now_seconds);
                                }
                            }
                            Err(_) => warn!("monitor history lock poisoned"),
                        }
                    }
                    if events.receiver_count() > 0 {
                        let _ = events.send(snapshot);
                    }
                }
                Err(error) => warn!(%error, "failed to collect system status"),
            }
        }
    });
}

/// 进程退出前(空闲退出 / 正常停止)把指标历史落盘,下次唤醒接着画。
pub fn flush_history() {
    if let Some(history) = HISTORY_HANDLE.get() {
        if let Ok(mut history) = history.lock() {
            history.persist(unix_timestamp());
        }
    }
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn bounded_process_limit(limit: u32) -> usize {
    if limit == 0 {
        PROCESS_SNAPSHOT_LIMIT
    } else {
        (limit as usize).min(PROCESS_SNAPSHOT_MAX_LIMIT)
    }
}

fn collect_process_snapshot(limit: usize) -> Vec<ProcessResourceSnapshot> {
    let system = System::new_all();
    let mut processes = system
        .processes()
        .iter()
        .map(|(pid, process)| ProcessResourceSnapshot {
            pid: pid.to_string(),
            name: process.name().to_owned(),
            cpu_usage_percent: process.cpu_usage(),
            memory_bytes: process.memory(),
        })
        .collect::<Vec<_>>();

    processes.sort_by(|left, right| {
        right
            .cpu_usage_percent
            .total_cmp(&left.cpu_usage_percent)
            .then_with(|| right.memory_bytes.cmp(&left.memory_bytes))
    });
    processes.truncate(limit);
    processes
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReportPeriod {
    Daily,
    Weekly,
}

fn normalize_report_period(period: &str) -> ReportPeriod {
    match period.trim().to_ascii_lowercase().as_str() {
        "weekly" | "week" | "7d" => ReportPeriod::Weekly,
        _ => ReportPeriod::Daily,
    }
}

fn report_period_seconds(period: ReportPeriod) -> u64 {
    match period {
        ReportPeriod::Daily => 24 * 60 * 60,
        ReportPeriod::Weekly => 7 * 24 * 60 * 60,
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct SecurityReportCounters {
    waf_blocks: usize,
    ssh_auto_bans: usize,
}

impl SecurityReportCounters {
    fn read_since(start_seconds: u64) -> Self {
        let path = security_state_path();
        let Ok(content) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
            return Self::default();
        };

        let waf_blocks = value
            .get("waf_events")
            .and_then(serde_json::Value::as_array)
            .map(|events| {
                events
                    .iter()
                    .filter(|event| {
                        event
                            .get("occurred_at_seconds")
                            .and_then(serde_json::Value::as_u64)
                            .is_some_and(|timestamp| timestamp >= start_seconds)
                    })
                    .count()
            })
            .unwrap_or_default();
        let ssh_auto_bans = value
            .get("ssh_events")
            .and_then(serde_json::Value::as_array)
            .map(|events| {
                events
                    .iter()
                    .filter(|event| {
                        let in_window = event
                            .get("occurred_at_seconds")
                            .and_then(serde_json::Value::as_u64)
                            .is_some_and(|timestamp| timestamp >= start_seconds);
                        let auto_banned = event
                            .get("auto_banned")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false);
                        in_window && auto_banned
                    })
                    .count()
            })
            .unwrap_or_default();

        Self {
            waf_blocks,
            ssh_auto_bans,
        }
    }
}

fn security_state_path() -> PathBuf {
    env::var("RUSTPANEL_SECURITY_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| crate::paths::default_root("security"))
        .join("state.json")
}

/// 健康度判定结果。之前直接拿展示文案("健康"/"需关注")当控制流用
/// (`health_advice` 里 `if health != "健康"`),字符串一改文案就跟着断;
/// i18n 引入双语文案后这个隐患会被放大,所以先用枚举把"判定"和"怎么显示"分开。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HealthLevel {
    Healthy,
    NeedsAttention,
}

impl HealthLevel {
    fn from_summary(summary: &HealthSummary) -> Self {
        if summary.peak_cpu_percent >= 90.0
            || summary.peak_memory_percent >= 90.0
            || summary.peak_load_one >= f64::from(summary.cpu_cores.max(1)) * 2.0
        {
            Self::NeedsAttention
        } else {
            Self::Healthy
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Healthy => crate::i18n::tr("健康", "Healthy"),
            Self::NeedsAttention => crate::i18n::tr("需关注", "Needs Attention"),
        }
    }
}

fn render_health_report(
    period: ReportPeriod,
    start_seconds: u64,
    end_seconds: u64,
    samples: &[SystemStatus],
    security: SecurityReportCounters,
) -> String {
    let summary = summarize_samples(samples);
    let period_label = match period {
        ReportPeriod::Daily => crate::i18n::tr("日报", "daily report"),
        ReportPeriod::Weekly => crate::i18n::tr("周报", "weekly report"),
    };
    let health = HealthLevel::from_summary(&summary);

    crate::trf!(
        "RustPanel 运行{period_label}\n\
         时间范围: {start_seconds} - {end_seconds}\n\
         采样点: {sample_count}\n\
         健康度: {health_label}\n\
         资源峰值: CPU {peak_cpu:.1}%, 内存 {peak_memory:.1}%, 1分钟负载 {peak_load:.2}, 磁盘占用 {peak_disk:.1}%\n\
         资源均值: CPU {avg_cpu:.1}%, 内存 {avg_memory:.1}%\n\
         流量增量: 入站 {network_in}, 出站 {network_out}\n\
         安全拦截: WAF {waf_blocks} 次, SSH 自动封禁 {ssh_auto_bans} 次\n\
         建议: {advice}",
        "RustPanel {period_label}\n\
         Time range: {start_seconds} - {end_seconds}\n\
         Samples: {sample_count}\n\
         Health: {health_label}\n\
         Peak usage: CPU {peak_cpu:.1}%, Memory {peak_memory:.1}%, 1-min load {peak_load:.2}, Disk {peak_disk:.1}%\n\
         Average usage: CPU {avg_cpu:.1}%, Memory {avg_memory:.1}%\n\
         Traffic delta: In {network_in}, Out {network_out}\n\
         Security blocks: WAF {waf_blocks}, SSH auto-bans {ssh_auto_bans}\n\
         Advice: {advice}",
        period_label = period_label,
        start_seconds = start_seconds,
        end_seconds = end_seconds,
        sample_count = summary.sample_count,
        health_label = health.label(),
        peak_cpu = summary.peak_cpu_percent,
        peak_memory = summary.peak_memory_percent,
        peak_load = summary.peak_load_one,
        peak_disk = summary.peak_disk_usage_percent,
        avg_cpu = summary.average_cpu_percent,
        avg_memory = summary.average_memory_percent,
        network_in = format_bytes(summary.network_received_delta),
        network_out = format_bytes(summary.network_transmitted_delta),
        waf_blocks = security.waf_blocks,
        ssh_auto_bans = security.ssh_auto_bans,
        advice = health_advice(health, security),
    )
}

fn health_advice(health: HealthLevel, security: SecurityReportCounters) -> &'static str {
    if health == HealthLevel::NeedsAttention {
        crate::i18n::tr(
            "检查高峰时段进程快照，必要时扩容或限制异常进程。",
            "Check the process snapshot during peak periods, and scale up or throttle abnormal processes if needed.",
        )
    } else if security.waf_blocks > 0 || security.ssh_auto_bans > 0 {
        crate::i18n::tr(
            "关注安全中心攻击来源排行，并保持 WAF 与 SSH 防护开启。",
            "Keep an eye on the attack source ranking in the Security Center, and keep WAF and SSH protection enabled.",
        )
    } else {
        crate::i18n::tr(
            "当前周期资源与安全事件稳定。",
            "Resource usage and security events were stable during this period.",
        )
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct HealthSummary {
    sample_count: usize,
    cpu_cores: u32,
    peak_cpu_percent: f32,
    average_cpu_percent: f32,
    peak_memory_percent: f32,
    average_memory_percent: f32,
    peak_load_one: f64,
    peak_disk_usage_percent: f32,
    network_received_delta: u64,
    network_transmitted_delta: u64,
}

fn summarize_samples(samples: &[SystemStatus]) -> HealthSummary {
    let mut summary = HealthSummary {
        sample_count: samples.len(),
        ..HealthSummary::default()
    };
    if samples.is_empty() {
        return summary;
    }

    let mut cpu_total = 0.0_f32;
    let mut memory_total = 0.0_f32;
    for sample in samples {
        summary.cpu_cores = summary.cpu_cores.max(sample.cpu_cores.len() as u32);
        summary.peak_cpu_percent = summary.peak_cpu_percent.max(sample.cpu_usage_percent);
        cpu_total += sample.cpu_usage_percent;

        let memory_percent = sample
            .memory
            .as_ref()
            .map(memory_usage_percent)
            .unwrap_or_default();
        summary.peak_memory_percent = summary.peak_memory_percent.max(memory_percent);
        memory_total += memory_percent;

        let load_one = sample
            .load_average
            .as_ref()
            .map(|load| load.one_minute)
            .unwrap_or_default();
        summary.peak_load_one = summary.peak_load_one.max(load_one);

        for disk in &sample.disks {
            summary.peak_disk_usage_percent = summary
                .peak_disk_usage_percent
                .max(disk_usage_percent(disk));
        }
    }

    summary.average_cpu_percent = cpu_total / samples.len() as f32;
    summary.average_memory_percent = memory_total / samples.len() as f32;
    summary.network_received_delta = network_delta(samples, |network| network.received_bytes);
    summary.network_transmitted_delta = network_delta(samples, |network| network.transmitted_bytes);
    summary
}

fn memory_usage_percent(memory: &MemoryStatus) -> f32 {
    if memory.total_bytes == 0 {
        return 0.0;
    }
    (memory.used_bytes as f32 / memory.total_bytes as f32) * 100.0
}

fn disk_usage_percent(disk: &DiskIoStatus) -> f32 {
    if disk.total_space_bytes == 0 {
        return 0.0;
    }
    let used = disk
        .total_space_bytes
        .saturating_sub(disk.available_space_bytes);
    (used as f32 / disk.total_space_bytes as f32) * 100.0
}

fn network_delta<F>(samples: &[SystemStatus], value_for: F) -> u64
where
    F: Fn(&NetworkIoStatus) -> u64,
{
    let Some(first) = samples.first() else {
        return 0;
    };
    let Some(last) = samples.last() else {
        return 0;
    };

    let first_total = first.networks.iter().map(&value_for).sum::<u64>();
    let last_total = last.networks.iter().map(&value_for).sum::<u64>();
    last_total.saturating_sub(first_total)
}

fn format_bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = value as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{value} {}", UNITS[unit])
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
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

    #[test]
    fn history_filters_metric_samples_by_time_window() {
        let mut history = MonitorHistory::default();
        history.push(test_sample(100, 10.0, 20, 100), Vec::new(), 100);
        history.last_sample_seconds = 0;
        history.push(test_sample(200, 80.0, 40, 500), Vec::new(), 200);

        let samples = history.metric_samples(150, 250);

        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].timestamp_seconds, 200);
    }

    #[test]
    fn compact_sample_round_trips_report_fields() {
        let original = test_sample(100, 42.0, 60, 5_000);
        let restored = CompactSample::from_status(&original).to_status();
        assert_eq!(restored.timestamp_seconds, 100);
        assert_eq!(restored.cpu_usage_percent, 42.0);
        assert_eq!(restored.cpu_cores.len(), original.cpu_cores.len());
        assert_eq!(restored.memory, original.memory);
        assert_eq!(restored.load_average, original.load_average);
        // 报告只关心峰值磁盘占用与网络总量
        let original_summary = summarize_samples(std::slice::from_ref(&original));
        let restored_summary = summarize_samples(&[restored]);
        assert_eq!(
            original_summary.peak_disk_usage_percent,
            restored_summary.peak_disk_usage_percent
        );
        assert_eq!(original_summary.cpu_cores, restored_summary.cpu_cores);
    }

    #[test]
    fn history_prunes_processes_sooner_than_metrics() {
        let mut history = MonitorHistory::default();
        let now = 10 * 24 * 60 * 60;
        history.metric_samples.push(CompactSample {
            t: now - 2 * 24 * 60 * 60,
            ..CompactSample::default()
        });
        history.process_samples.push(ProcessHistorySample {
            timestamp_seconds: now - 2 * 24 * 60 * 60,
            processes: Vec::new(),
        });
        history.prune(now);
        assert_eq!(history.metric_samples.len(), 1, "metrics kept for 7 days");
        assert!(history.process_samples.is_empty(), "processes kept for 24h");
        assert!(history.sample_due(now));
    }

    #[test]
    fn summarizes_peak_average_and_network_delta() {
        let samples = vec![
            test_sample(100, 20.0, 50, 1_000),
            test_sample(200, 80.0, 80, 4_000),
        ];

        let summary = summarize_samples(&samples);

        assert_eq!(summary.sample_count, 2);
        assert_eq!(summary.cpu_cores, 2);
        assert_eq!(summary.peak_cpu_percent, 80.0);
        assert_eq!(summary.average_cpu_percent, 50.0);
        assert_eq!(summary.peak_memory_percent, 80.0);
        assert_eq!(summary.network_received_delta, 3_000);
    }

    #[test]
    fn health_level_reflects_peak_thresholds_not_a_display_string() {
        let calm = summarize_samples(&[test_sample(100, 20.0, 20, 0)]);
        assert_eq!(HealthLevel::from_summary(&calm), HealthLevel::Healthy);

        let cpu_spike = summarize_samples(&[test_sample(100, 95.0, 20, 0)]);
        assert_eq!(
            HealthLevel::from_summary(&cpu_spike),
            HealthLevel::NeedsAttention
        );

        // memory_used_bytes / total(100) >= 90% 也算需关注
        let memory_spike = summarize_samples(&[test_sample(100, 20.0, 95, 0)]);
        assert_eq!(
            HealthLevel::from_summary(&memory_spike),
            HealthLevel::NeedsAttention
        );
    }

    #[test]
    fn health_advice_branches_on_level_not_label_text() {
        let none = SecurityReportCounters {
            waf_blocks: 0,
            ssh_auto_bans: 0,
        };
        let attacked = SecurityReportCounters {
            waf_blocks: 3,
            ssh_auto_bans: 0,
        };

        assert_eq!(
            health_advice(HealthLevel::NeedsAttention, attacked),
            "检查高峰时段进程快照，必要时扩容或限制异常进程。"
        );
        assert_eq!(
            health_advice(HealthLevel::Healthy, attacked),
            "关注安全中心攻击来源排行，并保持 WAF 与 SSH 防护开启。"
        );
        assert_eq!(
            health_advice(HealthLevel::Healthy, none),
            "当前周期资源与安全事件稳定。"
        );
        assert_eq!(HealthLevel::Healthy.label(), "健康");
        assert_eq!(HealthLevel::NeedsAttention.label(), "需关注");
    }

    #[tokio::test]
    async fn health_report_switches_to_english_under_the_en_locale() {
        crate::i18n::scope(crate::i18n::Locale::En, async {
            let none = SecurityReportCounters {
                waf_blocks: 0,
                ssh_auto_bans: 0,
            };
            assert_eq!(HealthLevel::Healthy.label(), "Healthy");
            assert_eq!(HealthLevel::NeedsAttention.label(), "Needs Attention");
            assert_eq!(
                health_advice(HealthLevel::Healthy, none),
                "Resource usage and security events were stable during this period."
            );

            let samples = [test_sample(100, 20.0, 20, 0)];
            let report = render_health_report(ReportPeriod::Daily, 0, 100, &samples, none);
            assert!(report.contains("RustPanel daily report"), "{report}");
            assert!(report.contains("Health: Healthy"), "{report}");
            assert!(
                report
                    .contains("Resource usage and security events were stable during this period."),
                "{report}"
            );
        })
        .await;
    }

    fn test_sample(
        timestamp_seconds: u64,
        cpu_usage_percent: f32,
        memory_used_bytes: u64,
        received_bytes: u64,
    ) -> SystemStatus {
        SystemStatus {
            timestamp_seconds,
            cpu_usage_percent,
            cpu_cores: vec![
                CpuCoreStatus {
                    core_id: 0,
                    usage_percent: cpu_usage_percent,
                    frequency_mhz: 2_400,
                },
                CpuCoreStatus {
                    core_id: 1,
                    usage_percent: cpu_usage_percent,
                    frequency_mhz: 2_400,
                },
            ],
            memory: Some(MemoryStatus {
                total_bytes: 100,
                used_bytes: memory_used_bytes,
                available_bytes: 100_u64.saturating_sub(memory_used_bytes),
                swap_total_bytes: 0,
                swap_used_bytes: 0,
            }),
            load_average: Some(LoadAverageStatus {
                one_minute: 1.5,
                five_minutes: 1.0,
                fifteen_minutes: 0.5,
            }),
            networks: vec![NetworkIoStatus {
                interface_name: "eth0".to_owned(),
                received_bytes,
                transmitted_bytes: received_bytes / 2,
            }],
            disks: vec![DiskIoStatus {
                disk_name: "sda".to_owned(),
                mount_point: "/".to_owned(),
                total_space_bytes: 1_000,
                available_space_bytes: 250,
                read_bytes: 0,
                written_bytes: 0,
            }],
            uptime_seconds: 1_000,
        }
    }
}
