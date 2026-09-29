// App.tsx 的 Dashboard,以及被它和 NetworkPage 共用的 BudgetBars/BudgetRow/
// MetricCard/NetworkMetricCard/ServerInfoCell 几个小组件。
export const dashboard = {
  subtitle: "服务器实时状态总览",
  rangeCustom: "自定义",
  offline: "离线",
  online: "运行中",

  hostname: "主机名",
  os: "操作系统",
  kernel: "内核",
  arch: "架构",
  uptime: "运行时间",
  load: "负载",

  cpuUsage: "CPU 使用率",
  cores: "{count} 核心",
  memoryUsage: "内存使用",
  diskUsage: "磁盘使用",
  noDiskData: "无磁盘数据",
  networkThroughput: "网络吞吐",
  download: "下行",
  upload: "上行",

  budgetTitle: "资源预算",
  openvzContainer: "OpenVZ 容器",
  memory: "内存",
  disk: "磁盘",
  natPorts: "NAT 端口",
  natReserved: "{reserved} / {total} 已预留",
  natNotConfigured: "未配置 NAT 端口预算",
  dockerUnavailable: "Docker 在本机不可用 —— {reason}",
  missingKernelCapability: "缺少必要内核能力",

  cpuMemoryTrend: "CPU / 内存趋势",
  refreshHistory: "刷新历史",
  startTime: "开始时间",
  endTime: "结束时间",
  chartLoading: "图表加载中…",

  installedApps: "已安装软件",
  installedAppsDesc: "当前面板部署的应用与运行状态",
  noAppsInstalled: "尚未安装任何应用",

  processSnapshot: "异常时刻进程",
  clickChartHint: "点击趋势图查看该时刻进程资源",
  colProcess: "进程",
  colCpu: "CPU",
  colMemory: "内存",
  notSelected: "未选择",

  reportTitle: "运行报告",
  reportDesc: "面板自动汇总日报或周报",
  daily: "日报",
  weekly: "周报",
  generate: "生成",
  noReportYet: "暂无报告"
};

export type DashboardMessages = typeof dashboard;
