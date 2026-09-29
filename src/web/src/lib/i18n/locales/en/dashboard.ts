import type { DashboardMessages } from "../zh-CN/dashboard";

export const dashboard: DashboardMessages = {
  subtitle: "Real-time server status overview",
  rangeCustom: "Custom",
  offline: "Offline",
  online: "Running",

  hostname: "Hostname",
  os: "OS",
  kernel: "Kernel",
  arch: "Architecture",
  uptime: "Uptime",
  load: "Load",

  cpuUsage: "CPU Usage",
  cores: "{count} cores",
  memoryUsage: "Memory Usage",
  diskUsage: "Disk Usage",
  noDiskData: "No disk data",
  networkThroughput: "Network Throughput",
  download: "Download",
  upload: "Upload",

  budgetTitle: "Resource Budget",
  openvzContainer: "OpenVZ container",
  memory: "Memory",
  disk: "Disk",
  natPorts: "NAT Ports",
  natReserved: "{reserved} / {total} reserved",
  natNotConfigured: "NAT port budget not configured",
  dockerUnavailable: "Docker is unavailable on this host — {reason}",
  missingKernelCapability: "Missing required kernel capability",

  cpuMemoryTrend: "CPU / Memory Trend",
  refreshHistory: "Refresh history",
  startTime: "Start time",
  endTime: "End time",
  chartLoading: "Loading chart…",

  installedApps: "Installed Apps",
  installedAppsDesc: "Apps deployed via this panel and their status",
  noAppsInstalled: "No apps installed yet",

  processSnapshot: "Process Snapshot",
  clickChartHint: "Click a point on the chart to see process usage at that moment",
  colProcess: "Process",
  colCpu: "CPU",
  colMemory: "Memory",
  notSelected: "Not selected",

  reportTitle: "Health Report",
  reportDesc: "The panel auto-generates a daily or weekly summary",
  daily: "Daily",
  weekly: "Weekly",
  generate: "Generate",
  noReportYet: "No report yet"
};
