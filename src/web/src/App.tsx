import Editor from "@monaco-editor/react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal as XTerm } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import {
  Activity,
  Archive,
  Ban,
  Boxes,
  Clock,
  Copy,
  Database,
  Download,
  FileDown,
  FileText,
  FileUp,
  Folder,
  FolderPlus,
  Globe,
  LogOut,
  Pause,
  Play,
  Plus,
  Power,
  RefreshCw,
  RotateCw,
  Save,
  Server,
  Shield,
  ShieldAlert,
  ShieldCheck,
  Square,
  Store,
  TerminalSquare,
  Trash2,
  Upload
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  CartesianGrid,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis
} from "recharts";

import { AppTemplate, InstalledApp } from "./gen/rustpanel/v1/appstore_pb";
import { AuditEvent } from "./gen/rustpanel/v1/audit_pb";
import { ClusterNode, DistributionRecord } from "./gen/rustpanel/v1/cluster_pb";
import { CronRunState, CronTask, CronTaskState } from "./gen/rustpanel/v1/cron_pb";
import { ComposeProject, ContainerItem, ImageItem } from "./gen/rustpanel/v1/docker_pb";
import { ArchiveFormat, FileItem, FileKind, RecycleBinItem, SearchMatch } from "./gen/rustpanel/v1/fs_pb";
import { ProcessResourceSnapshot, SystemStatus } from "./gen/rustpanel/v1/monitor_pb";
import { ProxyInstance, ProxyState, VpnCapability } from "./gen/rustpanel/v1/proxy_pb";
import {
  FirewallAction,
  FirewallBackend,
  FirewallDirection,
  FirewallProtocol,
  FirewallRule,
  SshKeyAlgorithm,
  SshKeyItem,
  SshLoginEvent,
  WafAttackEvent,
  WafRule,
  WafRuleKind
} from "./gen/rustpanel/v1/security_pb";
import { ReverseProxyRule, RewriteTemplate, SiteItem } from "./gen/rustpanel/v1/site_pb";
import { CertificateItem } from "./gen/rustpanel/v1/ssl_pb";
import { RuntimeModule } from "./gen/rustpanel/v1/system_pb";
import { WorkloadItem, WorkloadState } from "./gen/rustpanel/v1/workload_pb";
import { Button as UIButton } from "./components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "./components/ui/card";
import { Input as UIInput } from "./components/ui/input";
import { Label as UILabel } from "./components/ui/label";
import { cn } from "./lib/utils";
import {
  appendAuthQuery,
  authFetch,
  clearAuthToken,
  createRpcClients,
  getAuthToken,
  onAuthChanged,
  setAuthToken
} from "./lib/rpc";
import { formatBytes, formatDuration, formatPercent, safeError } from "./lib/format";
import { useMonitorStore } from "./store/monitor";

type Clients = ReturnType<typeof createRpcClients>;
type TabId = "dashboard" | "micro" | "security" | "terminal" | "files" | "docker" | "sites" | "database" | "cron" | "cluster";
type NavTab = { id: TabId; label: string; icon: typeof Activity; modules?: string[] };
type MonitorRange = "1h" | "24h" | "7d" | "custom";
type ChartPoint = {
  time: string;
  timestamp: number;
  cpu: number;
  memory: number;
};
type ChartClickState = {
  activePayload?: Array<{ payload?: ChartPoint }>;
};
type DockerQuotaForm = {
  containerId: string;
  cpuLimitCores: string;
  memoryLimitMb: string;
};
type ImagePullForm = {
  image: string;
  tag: string;
};
type ImageRollbackForm = {
  sourceImage: string;
  targetRepository: string;
  targetTag: string;
};
type ComposeForm = {
  name: string;
  composeYaml: string;
};
type WorkloadForm = {
  name: string;
  command: string;
  cwd: string;
  memoryLimitMb: string;
};
type MicroSiteForm = {
  name: string;
  root: string;
};
type ProxyForm = {
  name: string;
  listenPort: string;
  password: string;
};
type ClusterPairForm = {
  name: string;
  endpoint: string;
  pairingSecret: string;
};
type DistributionForm = {
  path: string;
  content: string;
  targetNodeId: string;
};
type FirewallForm = {
  id: string;
  name: string;
  protocol: FirewallProtocol;
  action: FirewallAction;
  direction: FirewallDirection;
  portStart: string;
  portEnd: string;
  source: string;
  destination: string;
  enabled: boolean;
  comment: string;
};
type SecurityOptionsForm = {
  disablePing: boolean;
  scanProtectionEnabled: boolean;
  scanBurst: number;
  scanWindowSeconds: number;
  backendPreference: FirewallBackend;
  lastApplyMessage: string;
  panelAccessPath: string;
  panelListenAddr: string;
  twoFactorRequired: boolean;
};
type WafSettingsForm = {
  enabled: boolean;
  ccProtectionEnabled: boolean;
  captchaChallengeEnabled: boolean;
  requestsPerMinute: number;
  burst: number;
  blockDurationSeconds: number;
  nginxConfigPath: string;
  challengePagePath: string;
  lastApplyMessage: string;
};
type WafRuleForm = {
  id: string;
  name: string;
  kind: WafRuleKind;
  pattern: string;
  enabled: boolean;
  scopeDomain: string;
  comment: string;
};
type SshSettingsForm = {
  serviceEnabled: boolean;
  port: number;
  passwordLoginDisabled: boolean;
  autoBanEnabled: boolean;
  failedAttemptLimit: number;
  failedAttemptWindowSeconds: number;
  configPath: string;
  lastApplyMessage: string;
};
type SshKeyForm = {
  name: string;
  algorithm: SshKeyAlgorithm;
};

const clients = createRpcClients();
const monitorRanges: Array<{ id: MonitorRange; label: string }> = [
  { id: "1h", label: "1h" },
  { id: "24h", label: "24h" },
  { id: "7d", label: "7d" },
  { id: "custom", label: "自定义" }
];
const defaultDockerQuotaForm: DockerQuotaForm = {
  containerId: "",
  cpuLimitCores: "1",
  memoryLimitMb: "512"
};
const defaultImagePullForm: ImagePullForm = {
  image: "nginx",
  tag: "latest"
};
const defaultImageRollbackForm: ImageRollbackForm = {
  sourceImage: "nginx:1.26-alpine",
  targetRepository: "nginx",
  targetTag: "stable"
};
const defaultComposeForm: ComposeForm = {
  name: "demo",
  composeYaml: `services:
  web:
    image: nginx:1.27-alpine
    container_name: rustpanel-demo-web
    ports:
      - "8080:80"
    restart: unless-stopped
`
};
const defaultWorkloadForm: WorkloadForm = {
  name: "rust-crawler",
  command: "./crawler",
  cwd: "/root",
  memoryLimitMb: "32"
};
const defaultMicroSiteForm: MicroSiteForm = {
  name: "site",
  root: "/var/www/site"
};
const defaultProxyForm: ProxyForm = {
  name: "ss-8388",
  listenPort: "8388",
  password: "change-me"
};
const defaultClusterPairForm: ClusterPairForm = {
  name: "node-1",
  endpoint: "local",
  pairingSecret: "rustpanel"
};
const defaultDistributionForm: DistributionForm = {
  path: "/tmp/rustpanel-distributed.conf",
  content: "managed_by=rustpanel\n",
  targetNodeId: ""
};
const defaultFirewallForm: FirewallForm = {
  id: "",
  name: "SSH 管理",
  protocol: FirewallProtocol.TCP,
  action: FirewallAction.ALLOW,
  direction: FirewallDirection.INBOUND,
  portStart: "22",
  portEnd: "",
  source: "",
  destination: "",
  enabled: true,
  comment: "面板安全入口"
};
const defaultSecurityOptions: SecurityOptionsForm = {
  disablePing: false,
  scanProtectionEnabled: false,
  scanBurst: 20,
  scanWindowSeconds: 60,
  backendPreference: FirewallBackend.UNSPECIFIED,
  lastApplyMessage: "",
  panelAccessPath: "/",
  panelListenAddr: "",
  twoFactorRequired: false
};
const defaultWafSettings: WafSettingsForm = {
  enabled: false,
  ccProtectionEnabled: true,
  captchaChallengeEnabled: true,
  requestsPerMinute: 120,
  burst: 30,
  blockDurationSeconds: 600,
  nginxConfigPath: "",
  challengePagePath: "",
  lastApplyMessage: ""
};
const defaultWafRuleForm: WafRuleForm = {
  id: "",
  name: "自定义关键词",
  kind: WafRuleKind.KEYWORD,
  pattern: "(badbot|malicious)",
  enabled: true,
  scopeDomain: "",
  comment: ""
};
const defaultSshSettings: SshSettingsForm = {
  serviceEnabled: true,
  port: 22,
  passwordLoginDisabled: false,
  autoBanEnabled: true,
  failedAttemptLimit: 5,
  failedAttemptWindowSeconds: 600,
  configPath: "",
  lastApplyMessage: ""
};
const defaultSshKeyForm: SshKeyForm = {
  name: "admin",
  algorithm: SshKeyAlgorithm.ED25519
};

const tabs: NavTab[] = [
  { id: "dashboard", label: "仪表盘", icon: Activity },
  { id: "micro", label: "Micro", icon: Power, modules: ["static-sites", "workloads", "proxy"] },
  { id: "security", label: "安全", icon: Shield, modules: ["security"] },
  { id: "terminal", label: "终端", icon: TerminalSquare, modules: ["terminal"] },
  { id: "files", label: "文件", icon: Folder, modules: ["files"] },
  { id: "docker", label: "容器", icon: Boxes, modules: ["docker", "appstore"] },
  { id: "sites", label: "站点", icon: Globe, modules: ["sites", "ssl"] },
  { id: "database", label: "数据库", icon: Database, modules: ["database"] },
  { id: "cron", label: "计划任务", icon: Clock, modules: ["cron"] },
  { id: "cluster", label: "集群/审计", icon: ShieldAlert, modules: ["cluster"] }
];

// 登录页:无 token 时唯一可访问的视图。提交后调用 AuthService.Login,成功则把 JWT 写入 sessionStorage
// 并触发 rustpanel:auth-changed 事件,App 会重新渲染主面板。
function LoginScreen({ onAuthenticated }: { onAuthenticated: () => void }) {
  const [username, setUsername] = useState("admin");
  const [password, setPassword] = useState("");
  const [totpCode, setTotpCode] = useState("");
  const [requiresTwoFactor, setRequiresTwoFactor] = useState(false);
  const [error, setError] = useState<string>("");
  const [submitting, setSubmitting] = useState(false);

  const handleSubmit = useCallback(
    async (event: React.FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      setError("");
      setSubmitting(true);
      try {
        const response = await clients.auth.login({
          username,
          password,
          totpCode
        });
        if (response.requiresTwoFactor && !response.accessToken) {
          setRequiresTwoFactor(true);
          setError("请输入两步验证码");
          return;
        }
        if (!response.accessToken) {
          setError(response.status?.message || "登录失败");
          return;
        }
        setAuthToken(response.accessToken);
        onAuthenticated();
      } catch (err) {
        setError(safeError(err));
      } finally {
        setSubmitting(false);
      }
    },
    [username, password, totpCode, onAuthenticated]
  );

  return (
    <div className="min-h-screen flex items-center justify-center bg-background p-6 bg-[radial-gradient(circle_at_top,oklch(0.25_0.04_260)_0%,oklch(0.16_0.01_250)_45%)]">
      <Card className="w-full max-w-sm border-border/60 shadow-2xl backdrop-blur supports-[backdrop-filter]:bg-card/80">
        <CardHeader className="gap-1.5">
          <div className="flex items-center gap-3">
            <div className="flex size-10 items-center justify-center rounded-lg bg-primary/15 text-primary ring-1 ring-primary/20">
              <Server className="size-5" />
            </div>
            <div className="flex flex-col">
              <CardTitle className="text-lg tracking-tight">RustPanel</CardTitle>
              <CardDescription>请使用管理员账户登录</CardDescription>
            </div>
          </div>
        </CardHeader>
        <CardContent>
          <form className="flex flex-col gap-4" onSubmit={handleSubmit}>
            <div className="flex flex-col gap-2">
              <UILabel htmlFor="login-username">用户名</UILabel>
              <UIInput
                id="login-username"
                autoComplete="username"
                disabled={submitting}
                onChange={(e) => setUsername(e.target.value)}
                required
                type="text"
                value={username}
              />
            </div>
            <div className="flex flex-col gap-2">
              <UILabel htmlFor="login-password">密码</UILabel>
              <UIInput
                id="login-password"
                autoComplete="current-password"
                disabled={submitting}
                onChange={(e) => setPassword(e.target.value)}
                required
                type="password"
                value={password}
              />
            </div>
            {requiresTwoFactor && (
              <div className="flex flex-col gap-2">
                <UILabel htmlFor="login-totp">两步验证码</UILabel>
                <UIInput
                  id="login-totp"
                  autoComplete="one-time-code"
                  disabled={submitting}
                  inputMode="numeric"
                  maxLength={6}
                  onChange={(e) => setTotpCode(e.target.value)}
                  pattern="[0-9]{6}"
                  required
                  type="text"
                  value={totpCode}
                />
              </div>
            )}
            {error && (
              <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                {error}
              </div>
            )}
            <UIButton className="w-full" disabled={submitting} type="submit">
              {submitting ? "登录中..." : "登录"}
            </UIButton>
            <p className="text-xs text-muted-foreground leading-relaxed">
              初始密码在安装时打印,也可在 <code className="rounded bg-muted px-1 py-0.5 font-mono text-[11px]">/www/wwwroot/rustpanel/.env</code> 里查 <code className="rounded bg-muted px-1 py-0.5 font-mono text-[11px]">RUSTPANEL_ADMIN_PASSWORD</code>。
            </p>
          </form>
        </CardContent>
      </Card>
    </div>
  );
}

export default function App() {
  // 无 token 时显示登录页;rpc.ts 在 401 / Logout 时会清空 token 并广播 rustpanel:auth-changed
  const [authenticated, setAuthenticated] = useState<boolean>(() => getAuthToken() != null);
  useEffect(() => {
    return onAuthChanged(() => {
      setAuthenticated(getAuthToken() != null);
    });
  }, []);

  const handleLogout = useCallback(() => {
    void clients.auth.logout({}).catch(() => {});
    clearAuthToken();
  }, []);

  if (!authenticated) {
    return <LoginScreen onAuthenticated={() => setAuthenticated(true)} />;
  }

  return <AppShell onLogout={handleLogout} />;
}

function AppShell({ onLogout }: { onLogout: () => void }) {
  const [active, setActive] = useState<TabId>("dashboard");
  const [terminalCwd, setTerminalCwd] = useState("/");
  const [modules, setModules] = useState<RuntimeModule[]>([]);
  const enabledModules = useMemo(
    () => new Set(modules.filter((module) => module.enabled).map((module) => module.id)),
    [modules]
  );
  const visibleTabs = useMemo(
    () => tabs.filter((tab) => !tab.modules || !modules.length || tab.modules.some((module) => enabledModules.has(module))),
    [enabledModules, modules.length]
  );

  useEffect(() => {
    clients.system.listRuntimeModules({}).then((response) => {
      setModules(response.modules);
    }).catch(() => {
      setModules([]);
    });
  }, []);

  useEffect(() => {
    if (!visibleTabs.some((tab) => tab.id === active)) {
      setActive("dashboard");
    }
  }, [active, visibleTabs]);

  return (
    <div className="min-h-screen grid grid-cols-[232px_minmax(0,1fr)] bg-background text-foreground">
      <aside className="flex flex-col gap-1 border-r border-border bg-card/40 p-4" aria-label="RustPanel navigation">
        <div className="flex items-center gap-2.5 px-3 py-2 mb-2">
          <div className="flex size-8 items-center justify-center rounded-md bg-primary/15 text-primary ring-1 ring-primary/20">
            <Server className="size-4" />
          </div>
          <div className="flex flex-col leading-tight">
            <span className="text-sm font-semibold tracking-tight">RustPanel</span>
            <span className="text-[11px] text-muted-foreground">控制面板</span>
          </div>
        </div>
        <nav className="flex flex-col gap-0.5">
          {visibleTabs.map((tab) => {
            const Icon = tab.icon;
            const isActive = active === tab.id;
            return (
              <button
                key={tab.id}
                onClick={() => setActive(tab.id)}
                type="button"
                className={cn(
                  "flex items-center gap-2.5 rounded-md px-3 py-2 text-sm transition-colors",
                  isActive
                    ? "bg-accent text-accent-foreground font-medium"
                    : "text-muted-foreground hover:bg-accent/60 hover:text-foreground"
                )}
              >
                <Icon className="size-[18px] shrink-0" />
                <span>{tab.label}</span>
              </button>
            );
          })}
        </nav>
        <button
          className="mt-auto flex items-center gap-2.5 rounded-md px-3 py-2 text-sm text-muted-foreground transition-colors hover:bg-accent/60 hover:text-foreground"
          onClick={onLogout}
          type="button"
        >
          <LogOut className="size-[18px] shrink-0" />
          <span>退出登录</span>
        </button>
      </aside>

      <main className="min-w-0 p-6 overflow-auto">
        {active === "dashboard" && <Dashboard clients={clients} />}
        {active === "micro" && <MicroPanel clients={clients} />}
        {active === "security" && <SecurityPanel clients={clients} />}
        {active === "terminal" && <TerminalPanel cwd={terminalCwd} />}
        {active === "files" && <FileManager clients={clients} openTerminal={(cwd) => { setTerminalCwd(cwd); setActive("terminal"); }} />}
        {active === "docker" && <DockerApps clients={clients} />}
        {active === "sites" && <SitesSsl clients={clients} />}
        {active === "database" && <DatabasePanel clients={clients} />}
        {active === "cron" && <CronPanel clients={clients} />}
        {active === "cluster" && <ClusterAudit clients={clients} />}
      </main>
    </div>
  );
}

function Dashboard({ clients }: { clients: Clients }) {
  const current = useMonitorStore((state) => state.current);
  const history = useMonitorStore((state) => state.history);
  const setCurrent = useMonitorStore((state) => state.setCurrent);
  const [system, setSystem] = useState({ hostname: "-", os: "-", kernel: "-", arch: "-" });
  const [error, setError] = useState("");
  const [range, setRange] = useState<MonitorRange>("1h");
  const [customStart, setCustomStart] = useState(() => toLocalInputValue(Date.now() - 60 * 60 * 1000));
  const [customEnd, setCustomEnd] = useState(() => toLocalInputValue(Date.now()));
  const [metricSamples, setMetricSamples] = useState<SystemStatus[]>([]);
  const [selectedTimestamp, setSelectedTimestamp] = useState<number>();
  const [processes, setProcesses] = useState<ProcessResourceSnapshot[]>([]);
  const [reportPeriod, setReportPeriod] = useState<"daily" | "weekly">("daily");
  const [healthReport, setHealthReport] = useState("");

  const loadMetricHistory = useCallback(async () => {
    const window = resolveHistoryWindow(range, customStart, customEnd);
    const response = await clients.monitor.getMetricHistory({
      startSeconds: BigInt(window.startSeconds),
      endSeconds: BigInt(window.endSeconds)
    });
    setMetricSamples(response.samples);
  }, [clients, customEnd, customStart, range]);

  useEffect(() => {
    const controller = new AbortController();
    clients.system
      .getSystemInfo({})
      .then((info) =>
        setSystem({
          hostname: info.hostname || "-",
          os: info.operatingSystem || "-",
          kernel: info.kernelVersion || "-",
          arch: info.architecture || "-"
        })
      )
      .catch((err: unknown) => setError(safeError(err)));

    clients.monitor
      .getSystemStatus({})
      .then((response) => {
        if (response.systemStatus) {
          setCurrent(response.systemStatus);
        }
      })
      .catch((err: unknown) => setError(safeError(err)));
    void (async () => {
      try {
        for await (const event of clients.monitor.watchSystemStatus(
          { intervalSeconds: 1 },
          { signal: controller.signal }
        )) {
          if (event.systemStatus) {
            setCurrent(event.systemStatus);
          }
        }
      } catch (err) {
        if (!controller.signal.aborted) {
          setError(safeError(err));
        }
      }
    })();

    return () => controller.abort();
  }, [clients, setCurrent]);

  useEffect(() => {
    loadMetricHistory().catch((err: unknown) => setError(safeError(err)));
  }, [loadMetricHistory]);

  // 把 SSE 流过来的实时点拼到历史采样末尾,保证图表每秒都有新数据点
  // (否则 metricSamples 一旦载入,chart 就停在那一刻不再刷新)
  const chartSource = useMemo(() => {
    if (metricSamples.length === 0) {
      return history;
    }
    const lastSampleAt = Number(metricSamples[metricSamples.length - 1].timestampSeconds);
    const trailingLive = history.filter(
      (sample) => Number(sample.timestampSeconds) > lastSampleAt
    );
    return trailingLive.length === 0 ? metricSamples : [...metricSamples, ...trailingLive];
  }, [metricSamples, history]);
  const chartData = useMemo(
    () =>
      chartSource.map((sample) => {
        const memory = sample.memory;
        const memoryPercent =
          memory && memory.totalBytes > 0n
            ? (Number(memory.usedBytes) / Number(memory.totalBytes)) * 100
            : 0;

        return {
          time: formatChartTimestamp(sample.timestampSeconds, range),
          timestamp: Number(sample.timestampSeconds),
          cpu: sample.cpuUsagePercent,
          memory: memoryPercent
        };
      }),
    [chartSource, range]
  );
  const selectedLabel = selectedTimestamp
    ? new Date(selectedTimestamp * 1000).toLocaleString()
    : "未选择";

  const loadProcessSnapshot = async (timestamp: number) => {
    try {
      const response = await clients.monitor.getProcessSnapshot({
        timestampSeconds: BigInt(timestamp),
        limit: 8
      });
      setSelectedTimestamp(timestamp);
      setProcesses(response.processes);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  };

  const handleChartClick = (state: ChartClickState) => {
    const point = state.activePayload?.[0]?.payload;
    if (point) {
      void loadProcessSnapshot(point.timestamp);
    }
  };

  const generateReport = async () => {
    try {
      const response = await clients.monitor.generateHealthReport({ period: reportPeriod });
      setHealthReport(response.report);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  };

  return (
    <section className="page-grid">
      <header className="full-span flex items-start justify-between gap-4 flex-wrap pb-2 border-b border-border/60 mb-2">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-semibold tracking-tight m-0">资源监控</h1>
          <p className="text-sm text-muted-foreground m-0">
            {system.hostname} · {system.os} · 内核 {system.kernel} · {system.arch}
          </p>
        </div>
        <StatusPill label={error ? "离线" : "运行中"} tone={error ? "danger" : "good"} />
      </header>

      <Metric label="CPU" value={formatPercent(current?.cpuUsagePercent ?? 0)} detail={`${current?.cpuCores.length ?? 0} 核心`} />
      <Metric
        label="内存"
        value={formatBytes(current?.memory?.usedBytes ?? 0)}
        detail={`总计 ${formatBytes(current?.memory?.totalBytes ?? 0)}`}
      />
      <Metric
        label="负载"
        value={(current?.loadAverage?.oneMinute ?? 0).toFixed(2)}
        detail={`${(current?.loadAverage?.fiveMinutes ?? 0).toFixed(2)} / ${(current?.loadAverage?.fifteenMinutes ?? 0).toFixed(2)}`}
      />
      <Metric label="运行时间" value={formatDuration(current?.uptimeSeconds ?? 0)} detail="守护进程状态" />

      <div className="panel chart-panel full-span">
        <div className="panel-title">
          <Activity size={18} />
          <span>CPU / 内存趋势</span>
          <div className="range-tabs">
            {monitorRanges.map((item) => (
              <button
                className={range === item.id ? "range-tab active" : "range-tab"}
                key={item.id}
                onClick={() => setRange(item.id)}
                type="button"
              >
                {item.label}
              </button>
            ))}
            <IconButton label="刷新历史" icon={RefreshCw} onClick={() => void loadMetricHistory()} />
          </div>
        </div>
        {range === "custom" && (
          <div className="custom-range">
            <input
              aria-label="开始时间"
              onChange={(event) => setCustomStart(event.target.value)}
              type="datetime-local"
              value={customStart}
            />
            <input
              aria-label="结束时间"
              onChange={(event) => setCustomEnd(event.target.value)}
              type="datetime-local"
              value={customEnd}
            />
          </div>
        )}
        <ResponsiveContainer width="100%" height={280}>
          <LineChart data={chartData} onClick={(state) => handleChartClick(state as ChartClickState)}>
            <CartesianGrid strokeDasharray="3 3" />
            <XAxis dataKey="time" minTickGap={24} />
            <YAxis domain={[0, 100]} />
            <Tooltip />
            <Line dataKey="cpu" dot={false} stroke="#136f63" strokeWidth={2} />
            <Line dataKey="memory" dot={false} stroke="#ba5a31" strokeWidth={2} />
          </LineChart>
        </ResponsiveContainer>
      </div>

      <div className="panel wide-panel">
        <div className="panel-title">
          <Server size={18} />
          <span>异常时刻进程</span>
          <small>{selectedLabel}</small>
        </div>
        <div className="table-list">
          {processes.length === 0 && <div className="empty-state">点击趋势图查看该时刻进程资源</div>}
          {processes.map((process) => (
            <div className="table-row process-row" key={`${process.pid}-${process.name}`}>
              <div>
                <strong>{process.name || process.pid}</strong>
                <small>PID {process.pid}</small>
              </div>
              <span>{formatPercent(process.cpuUsagePercent)}</span>
              <small>{formatBytes(process.memoryBytes)}</small>
            </div>
          ))}
        </div>
      </div>

      <div className="panel report-panel">
        <div className="panel-title">
          <FileText size={18} />
          <span>运行报告</span>
        </div>
        <div className="report-actions">
          <select value={reportPeriod} onChange={(event) => setReportPeriod(event.target.value as "daily" | "weekly")}>
            <option value="daily">日报</option>
            <option value="weekly">周报</option>
          </select>
          <button onClick={() => void generateReport()} type="button">
            <RefreshCw size={15} />
            生成
          </button>
        </div>
        <pre className="report-output">{healthReport || "暂无报告"}</pre>
      </div>
    </section>
  );
}

function resolveHistoryWindow(range: MonitorRange, customStart: string, customEnd: string) {
  const nowSeconds = Math.floor(Date.now() / 1000);
  if (range === "custom") {
    const startSeconds = localInputToSeconds(customStart) || nowSeconds - 60 * 60;
    const endSeconds = localInputToSeconds(customEnd) || nowSeconds;
    return {
      startSeconds: Math.min(startSeconds, endSeconds),
      endSeconds: Math.max(startSeconds, endSeconds)
    };
  }

  const seconds = range === "7d" ? 7 * 24 * 60 * 60 : range === "24h" ? 24 * 60 * 60 : 60 * 60;
  return {
    startSeconds: nowSeconds - seconds,
    endSeconds: nowSeconds
  };
}

function toLocalInputValue(timestampMs: number) {
  const date = new Date(timestampMs);
  const offsetMs = date.getTimezoneOffset() * 60 * 1000;
  return new Date(timestampMs - offsetMs).toISOString().slice(0, 16);
}

function localInputToSeconds(value: string) {
  const timestamp = new Date(value).getTime();
  return Number.isFinite(timestamp) ? Math.floor(timestamp / 1000) : 0;
}

function formatChartTimestamp(timestampSeconds: bigint, range: MonitorRange) {
  const date = new Date(Number(timestampSeconds) * 1000);
  if (range === "7d") {
    return date.toLocaleDateString(undefined, { month: "2-digit", day: "2-digit" });
  }
  return date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

function SecurityPanel({ clients }: { clients: Clients }) {
  const [rules, setRules] = useState<FirewallRule[]>([]);
  const [ruleForm, setRuleForm] = useState<FirewallForm>(defaultFirewallForm);
  const [options, setOptions] = useState<SecurityOptionsForm>(defaultSecurityOptions);
  const [wafSettings, setWafSettings] = useState<WafSettingsForm>(defaultWafSettings);
  const [wafRules, setWafRules] = useState<WafRule[]>([]);
  const [wafRuleForm, setWafRuleForm] = useState<WafRuleForm>(defaultWafRuleForm);
  const [wafEvents, setWafEvents] = useState<WafAttackEvent[]>([]);
  const [sshSettings, setSshSettings] = useState<SshSettingsForm>(defaultSshSettings);
  const [sshKeys, setSshKeys] = useState<SshKeyItem[]>([]);
  const [sshKeyForm, setSshKeyForm] = useState<SshKeyForm>(defaultSshKeyForm);
  const [sshEvents, setSshEvents] = useState<SshLoginEvent[]>([]);
  const [backupJson, setBackupJson] = useState("");
  const [status, setStatus] = useState("");

  const load = async () => {
    try {
      const [firewallResponse, wafResponse, wafEventResponse, sshResponse, sshEventResponse] = await Promise.all([
        clients.security.listFirewallRules({}),
        clients.security.getWafSettings({}),
        clients.security.listWafAttackEvents({ limit: 100 }),
        clients.security.getSshSettings({}),
        clients.security.listSshLoginEvents({ limit: 100 })
      ]);
      setRules(firewallResponse.rules);
      if (firewallResponse.options) {
        setOptions({ ...defaultSecurityOptions, ...firewallResponse.options });
      }
      if (wafResponse.settings) {
        setWafSettings({ ...defaultWafSettings, ...wafResponse.settings });
      }
      setWafRules(wafResponse.rules);
      setWafEvents(wafEventResponse.events);
      if (sshResponse.settings) {
        setSshSettings({ ...defaultSshSettings, ...sshResponse.settings });
      }
      setSshKeys(sshResponse.keys);
      setSshEvents(sshEventResponse.events);
      setStatus("");
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const saveRule = async () => {
    try {
      const isIcmp = ruleForm.protocol === FirewallProtocol.ICMP;
      await clients.security.upsertFirewallRule({
        rule: {
          id: ruleForm.id,
          name: ruleForm.name,
          protocol: ruleForm.protocol,
          action: ruleForm.action,
          direction: ruleForm.direction,
          portStart: isIcmp ? 0 : Number(ruleForm.portStart || 0),
          portEnd: isIcmp || !ruleForm.portEnd ? 0 : Number(ruleForm.portEnd),
          source: ruleForm.source,
          destination: ruleForm.destination,
          enabled: ruleForm.enabled,
          comment: ruleForm.comment,
          createdAtSeconds: 0n,
          updatedAtSeconds: 0n
        }
      });
      setRuleForm(defaultFirewallForm);
      await load();
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const editRule = (rule: FirewallRule) => {
    setRuleForm({
      id: rule.id,
      name: rule.name,
      protocol: rule.protocol,
      action: rule.action,
      direction: rule.direction,
      portStart: rule.portStart ? String(rule.portStart) : "",
      portEnd: rule.portEnd ? String(rule.portEnd) : "",
      source: rule.source,
      destination: rule.destination,
      enabled: rule.enabled,
      comment: rule.comment
    });
  };

  const deleteRule = async (rule: FirewallRule) => {
    try {
      await clients.security.deleteFirewallRule({ id: rule.id });
      await load();
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const toggleRule = async (rule: FirewallRule) => {
    try {
      await clients.security.setFirewallRuleEnabled({ id: rule.id, enabled: !rule.enabled });
      await load();
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const saveOptions = async () => {
    try {
      const response = await clients.security.updateSecurityOptions({ options });
      if (response.options) {
        setOptions({ ...defaultSecurityOptions, ...response.options });
      }
      setStatus(response.options?.lastApplyMessage ?? "安全选项已保存");
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const exportRules = async () => {
    try {
      const response = await clients.security.exportFirewallRules({});
      setBackupJson(response.backupJson);
      setStatus("规则备份已生成");
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const importRules = async () => {
    try {
      const response = await clients.security.importFirewallRules({
        backupJson,
        replaceExisting: true
      });
      setRules(response.rules);
      if (response.options) {
        setOptions({ ...defaultSecurityOptions, ...response.options });
      }
      setStatus("规则备份已导入");
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const saveWafSettings = async () => {
    try {
      const response = await clients.security.updateWafSettings({ settings: wafSettings });
      if (response.settings) {
        setWafSettings({ ...defaultWafSettings, ...response.settings });
        setStatus(response.settings.lastApplyMessage || "WAF 配置已保存");
      }
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const saveWafRule = async () => {
    try {
      await clients.security.upsertWafRule({
        rule: {
          id: wafRuleForm.id,
          name: wafRuleForm.name,
          kind: wafRuleForm.kind,
          pattern: wafRuleForm.pattern,
          enabled: wafRuleForm.enabled,
          scopeDomain: wafRuleForm.scopeDomain,
          comment: wafRuleForm.comment,
          createdAtSeconds: 0n,
          updatedAtSeconds: 0n
        }
      });
      setWafRuleForm(defaultWafRuleForm);
      await load();
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const editWafRule = (rule: WafRule) => {
    setWafRuleForm({
      id: rule.id,
      name: rule.name,
      kind: rule.kind,
      pattern: rule.pattern,
      enabled: rule.enabled,
      scopeDomain: rule.scopeDomain,
      comment: rule.comment
    });
  };

  const deleteWafRule = async (rule: WafRule) => {
    try {
      await clients.security.deleteWafRule({ id: rule.id });
      await load();
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const saveSshSettings = async () => {
    try {
      const response = await clients.security.updateSshSettings({ settings: sshSettings });
      if (response.settings) {
        setSshSettings({ ...defaultSshSettings, ...response.settings });
        setStatus(response.settings.lastApplyMessage || "SSH 配置已保存");
      }
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const generateSshKey = async () => {
    try {
      await clients.security.generateSshKey({
        name: sshKeyForm.name,
        algorithm: sshKeyForm.algorithm
      });
      await load();
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const wafIpRanking = wafEvents.reduce<Array<{ ip: string; count: number; country: string }>>((ranking, event) => {
    const existing = ranking.find((item) => item.ip === event.sourceIp);
    if (existing) {
      existing.count += 1;
      return ranking;
    }
    ranking.push({ ip: event.sourceIp || "-", count: 1, country: event.countryName || event.countryCode || "-" });
    return ranking;
  }, []).sort((left, right) => right.count - left.count);

  const wafCountryRanking = wafEvents.reduce<Array<{ code: string; name: string; count: number }>>((ranking, event) => {
    const code = event.countryCode || "UN";
    const existing = ranking.find((item) => item.code === code);
    if (existing) {
      existing.count += 1;
      return ranking;
    }
    ranking.push({ code, name: event.countryName || code, count: 1 });
    return ranking;
  }, []).sort((left, right) => right.count - left.count);

  return (
    <section className="page-grid security-layout">
      <header className="section-header full-span">
        <div>
          <h1>安全管理</h1>
          <p>{status || options.lastApplyMessage || `${rules.length} 条防火墙规则`}</p>
        </div>
        <div className="toolbar">
          <IconButton label="刷新" icon={RefreshCw} onClick={() => void load()} />
          <IconButton label="新建规则" icon={Plus} onClick={() => setRuleForm(defaultFirewallForm)} />
        </div>
      </header>

      <div className="panel security-options">
        <div className="panel-title"><ShieldAlert size={18} /><span>入口防护</span></div>
        <Input
          label="访问路径"
          value={options.panelAccessPath}
          onChange={(panelAccessPath) => setOptions({ ...options, panelAccessPath })}
        />
        <Input
          label="监听地址"
          value={options.panelListenAddr}
          onChange={(panelListenAddr) => setOptions({ ...options, panelListenAddr })}
        />
        <ToggleRow
          label="2FA 登录"
          checked={options.twoFactorRequired}
          onChange={(twoFactorRequired) => setOptions({ ...options, twoFactorRequired })}
        />
        <ToggleRow label="禁 Ping" checked={options.disablePing} onChange={(disablePing) => setOptions({ ...options, disablePing })} />
        <ToggleRow
          label="防扫描"
          checked={options.scanProtectionEnabled}
          onChange={(scanProtectionEnabled) => setOptions({ ...options, scanProtectionEnabled })}
        />
        <NumberInput label="触发次数" value={options.scanBurst} onChange={(scanBurst) => setOptions({ ...options, scanBurst })} />
        <NumberInput
          label="窗口秒数"
          value={options.scanWindowSeconds}
          onChange={(scanWindowSeconds) => setOptions({ ...options, scanWindowSeconds })}
        />
        <SelectRow
          label="后端"
          value={options.backendPreference}
          onChange={(backendPreference) => setOptions({ ...options, backendPreference: Number(backendPreference) as FirewallBackend })}
          options={[
            [FirewallBackend.UNSPECIFIED, "自动检测"],
            [FirewallBackend.UFW, "UFW"],
            [FirewallBackend.FIREWALLD, "Firewalld"],
            [FirewallBackend.IPTABLES, "Iptables"]
          ]}
        />
        <button onClick={() => void saveOptions()} type="button"><Save size={15} />保存开关</button>
      </div>

      <div className="panel rule-form">
        <div className="panel-title"><ShieldCheck size={18} /><span>{ruleForm.id ? "编辑规则" : "新建规则"}</span></div>
        <Input label="名称" value={ruleForm.name} onChange={(name) => setRuleForm({ ...ruleForm, name })} />
        <SelectRow
          label="协议"
          value={ruleForm.protocol}
          onChange={(protocol) => setRuleForm({ ...ruleForm, protocol: Number(protocol) as FirewallProtocol })}
          options={[
            [FirewallProtocol.TCP, "TCP"],
            [FirewallProtocol.UDP, "UDP"],
            [FirewallProtocol.ICMP, "ICMP"]
          ]}
        />
        <SelectRow
          label="动作"
          value={ruleForm.action}
          onChange={(action) => setRuleForm({ ...ruleForm, action: Number(action) as FirewallAction })}
          options={[
            [FirewallAction.ALLOW, "放行"],
            [FirewallAction.DENY, "屏蔽"],
            [FirewallAction.REJECT, "拒绝"]
          ]}
        />
        <SelectRow
          label="方向"
          value={ruleForm.direction}
          onChange={(direction) => setRuleForm({ ...ruleForm, direction: Number(direction) as FirewallDirection })}
          options={[
            [FirewallDirection.INBOUND, "入站"],
            [FirewallDirection.OUTBOUND, "出站"]
          ]}
        />
        {ruleForm.protocol !== FirewallProtocol.ICMP && (
          <div className="inline-grid">
            <Input label="起始端口" type="number" value={ruleForm.portStart} onChange={(portStart) => setRuleForm({ ...ruleForm, portStart })} />
            <Input label="结束端口" type="number" value={ruleForm.portEnd} onChange={(portEnd) => setRuleForm({ ...ruleForm, portEnd })} />
          </div>
        )}
        <Input label="来源 IP/CIDR" value={ruleForm.source} onChange={(source) => setRuleForm({ ...ruleForm, source })} />
        <Input label="目标 IP/CIDR" value={ruleForm.destination} onChange={(destination) => setRuleForm({ ...ruleForm, destination })} />
        <Input label="备注" value={ruleForm.comment} onChange={(comment) => setRuleForm({ ...ruleForm, comment })} />
        <ToggleRow label="启用" checked={ruleForm.enabled} onChange={(enabled) => setRuleForm({ ...ruleForm, enabled })} />
        <button onClick={() => void saveRule()} type="button"><Save size={15} />保存规则</button>
      </div>

      <div className="panel wide-panel firewall-list">
        <div className="panel-title"><Shield size={18} /><span>防火墙规则</span></div>
        <div className="table-list">
          {rules.map((rule) => (
            <div className="table-row firewall-row" key={rule.id}>
              <div>
                <strong>{rule.name}</strong>
                <small>
                  {firewallProtocolLabel(rule.protocol)} · {firewallActionLabel(rule.action)} · {firewallDirectionLabel(rule.direction)}
                  {rule.protocol !== FirewallProtocol.ICMP ? ` · ${rule.portStart}${rule.portEnd && rule.portEnd !== rule.portStart ? `-${rule.portEnd}` : ""}` : ""}
                  {rule.source ? ` · ${rule.source}` : ""}
                </small>
              </div>
              <StatusPill label={rule.enabled ? "启用" : "停用"} tone={rule.enabled ? "good" : "muted"} />
              <div className="row-actions">
                <IconButton label={rule.enabled ? "停用" : "启用"} icon={Power} onClick={() => void toggleRule(rule)} />
                <IconButton label="编辑" icon={Copy} onClick={() => editRule(rule)} />
                <IconButton label="删除" icon={Ban} onClick={() => void deleteRule(rule)} />
              </div>
            </div>
          ))}
          {!rules.length && <div className="empty-state">暂无规则</div>}
        </div>
      </div>

      <div className="panel waf-settings">
        <div className="panel-title"><ShieldAlert size={18} /><span>WAF 防护</span></div>
        <ToggleRow label="WAF 总开关" checked={wafSettings.enabled} onChange={(enabled) => setWafSettings({ ...wafSettings, enabled })} />
        <ToggleRow
          label="抗 CC"
          checked={wafSettings.ccProtectionEnabled}
          onChange={(ccProtectionEnabled) => setWafSettings({ ...wafSettings, ccProtectionEnabled })}
        />
        <ToggleRow
          label="验证码挑战"
          checked={wafSettings.captchaChallengeEnabled}
          onChange={(captchaChallengeEnabled) => setWafSettings({ ...wafSettings, captchaChallengeEnabled })}
        />
        <NumberInput label="每分钟请求" value={wafSettings.requestsPerMinute} onChange={(requestsPerMinute) => setWafSettings({ ...wafSettings, requestsPerMinute })} />
        <NumberInput label="突发请求" value={wafSettings.burst} onChange={(burst) => setWafSettings({ ...wafSettings, burst })} />
        <NumberInput
          label="封禁秒数"
          value={wafSettings.blockDurationSeconds}
          onChange={(blockDurationSeconds) => setWafSettings({ ...wafSettings, blockDurationSeconds })}
        />
        <Input label="Nginx 片段" value={wafSettings.nginxConfigPath} onChange={(nginxConfigPath) => setWafSettings({ ...wafSettings, nginxConfigPath })} />
        <Input label="挑战页" value={wafSettings.challengePagePath} onChange={(challengePagePath) => setWafSettings({ ...wafSettings, challengePagePath })} />
        <button onClick={() => void saveWafSettings()} type="button"><Save size={15} />保存 WAF</button>
      </div>

      <div className="panel waf-rule-form">
        <div className="panel-title"><ShieldCheck size={18} /><span>{wafRuleForm.id ? "编辑 WAF 规则" : "新建 WAF 规则"}</span></div>
        <Input label="名称" value={wafRuleForm.name} onChange={(name) => setWafRuleForm({ ...wafRuleForm, name })} />
        <SelectRow
          label="类型"
          value={wafRuleForm.kind}
          onChange={(kind) => setWafRuleForm({ ...wafRuleForm, kind: Number(kind) as WafRuleKind })}
          options={[
            [WafRuleKind.SQL_INJECTION, "SQL 注入"],
            [WafRuleKind.XSS, "XSS"],
            [WafRuleKind.KEYWORD, "关键词"],
            [WafRuleKind.SCANNER, "扫描器"],
            [WafRuleKind.CC, "CC"]
          ]}
        />
        <Input label="匹配规则" value={wafRuleForm.pattern} onChange={(pattern) => setWafRuleForm({ ...wafRuleForm, pattern })} />
        <Input label="站点域名" value={wafRuleForm.scopeDomain} onChange={(scopeDomain) => setWafRuleForm({ ...wafRuleForm, scopeDomain })} />
        <Input label="备注" value={wafRuleForm.comment} onChange={(comment) => setWafRuleForm({ ...wafRuleForm, comment })} />
        <ToggleRow label="启用" checked={wafRuleForm.enabled} onChange={(enabled) => setWafRuleForm({ ...wafRuleForm, enabled })} />
        <button onClick={() => void saveWafRule()} type="button"><Save size={15} />保存规则</button>
      </div>

      <div className="panel wide-panel waf-rule-list">
        <div className="panel-title"><Shield size={18} /><span>WAF 规则库</span></div>
        <div className="table-list">
          {wafRules.map((rule) => (
            <div className="table-row firewall-row" key={rule.id}>
              <div>
                <strong>{rule.name}</strong>
                <small>{wafKindLabel(rule.kind)} · {rule.pattern}{rule.scopeDomain ? ` · ${rule.scopeDomain}` : ""}</small>
              </div>
              <StatusPill label={rule.enabled ? "启用" : "停用"} tone={rule.enabled ? "good" : "muted"} />
              <div className="row-actions">
                <IconButton label="编辑" icon={Copy} onClick={() => editWafRule(rule)} />
                <IconButton label="删除" icon={Ban} onClick={() => void deleteWafRule(rule)} />
              </div>
            </div>
          ))}
        </div>
      </div>

      <div className="panel waf-map">
        <div className="panel-title"><Globe size={18} /><span>攻击来源</span></div>
        <div className="world-map" aria-label="WAF attack source map">
          {wafCountryRanking.slice(0, 8).map((country, index) => (
            <span className={`map-point point-${index + 1}`} key={country.code} title={`${country.name}: ${country.count}`}>
              {country.code}
            </span>
          ))}
        </div>
      </div>

      <div className="panel waf-ranking">
        <div className="panel-title"><ShieldAlert size={18} /><span>攻击 IP 排名</span></div>
        <div className="table-list">
          {wafIpRanking.slice(0, 8).map((item) => (
            <div className="rank-row" key={item.ip}>
              <strong>{item.ip}</strong>
              <small>{item.country}</small>
              <StatusPill label={String(item.count)} tone="danger" />
            </div>
          ))}
          {!wafIpRanking.length && <div className="empty-state">暂无拦截记录</div>}
        </div>
      </div>

      <div className="panel ssh-settings">
        <div className="panel-title"><TerminalSquare size={18} /><span>SSH 加固</span></div>
        <ToggleRow label="服务启用" checked={sshSettings.serviceEnabled} onChange={(serviceEnabled) => setSshSettings({ ...sshSettings, serviceEnabled })} />
        <NumberInput label="SSH 端口" value={sshSettings.port} onChange={(port) => setSshSettings({ ...sshSettings, port })} />
        <ToggleRow
          label="禁用密码"
          checked={sshSettings.passwordLoginDisabled}
          onChange={(passwordLoginDisabled) => setSshSettings({ ...sshSettings, passwordLoginDisabled })}
        />
        <ToggleRow label="自动封禁" checked={sshSettings.autoBanEnabled} onChange={(autoBanEnabled) => setSshSettings({ ...sshSettings, autoBanEnabled })} />
        <NumberInput
          label="失败阈值"
          value={sshSettings.failedAttemptLimit}
          onChange={(failedAttemptLimit) => setSshSettings({ ...sshSettings, failedAttemptLimit })}
        />
        <NumberInput
          label="窗口秒数"
          value={sshSettings.failedAttemptWindowSeconds}
          onChange={(failedAttemptWindowSeconds) => setSshSettings({ ...sshSettings, failedAttemptWindowSeconds })}
        />
        <Input label="配置文件" value={sshSettings.configPath} onChange={(configPath) => setSshSettings({ ...sshSettings, configPath })} />
        <button onClick={() => void saveSshSettings()} type="button"><Save size={15} />保存 SSH</button>
      </div>

      <div className="panel ssh-keys">
        <div className="panel-title"><ShieldCheck size={18} /><span>SSH 密钥</span></div>
        <Input label="名称" value={sshKeyForm.name} onChange={(name) => setSshKeyForm({ ...sshKeyForm, name })} />
        <SelectRow
          label="算法"
          value={sshKeyForm.algorithm}
          onChange={(algorithm) => setSshKeyForm({ ...sshKeyForm, algorithm: Number(algorithm) as SshKeyAlgorithm })}
          options={[
            [SshKeyAlgorithm.ED25519, "Ed25519"],
            [SshKeyAlgorithm.RSA, "RSA 4096"]
          ]}
        />
        <button onClick={() => void generateSshKey()} type="button"><Plus size={15} />生成</button>
        <div className="table-list compact-list">
          {sshKeys.map((key) => (
            <div className="key-row" key={key.id}>
              <strong>{key.name}</strong>
              <small>{sshAlgorithmLabel(key.algorithm)} · {key.privateKeyPath}</small>
            </div>
          ))}
          {!sshKeys.length && <div className="empty-state">暂无密钥</div>}
        </div>
      </div>

      <div className="panel full-span ssh-audit">
        <div className="panel-title"><FileText size={18} /><span>SSH 登录审计</span></div>
        <div className="table-list">
          {sshEvents.slice(0, 12).map((event) => (
            <div className="table-row firewall-row" key={event.id}>
              <div>
                <strong>{event.username} · {event.sourceIp || "-"}</strong>
                <small>{event.message || new Date(Number(event.occurredAtSeconds) * 1000).toLocaleString()}</small>
              </div>
              <StatusPill label={event.successful ? "成功" : "失败"} tone={event.successful ? "good" : "danger"} />
              <StatusPill label={event.autoBanned ? "已封禁" : "未封禁"} tone={event.autoBanned ? "danger" : "muted"} />
            </div>
          ))}
          {!sshEvents.length && <div className="empty-state">暂无审计记录</div>}
        </div>
      </div>

      <div className="panel backup-panel full-span">
        <div className="panel-title"><FileDown size={18} /><span>规则备份</span></div>
        <div className="toolbar backup-actions">
          <button onClick={() => void exportRules()} type="button"><FileDown size={15} />导出</button>
          <button onClick={() => void importRules()} type="button"><FileUp size={15} />导入覆盖</button>
        </div>
        <textarea
          onChange={(event) => setBackupJson(event.target.value)}
          spellCheck={false}
          value={backupJson}
        />
      </div>
    </section>
  );
}

function TerminalPanel({ cwd }: { cwd: string }) {
  const terminalRef = useRef<HTMLDivElement | null>(null);
  const socketRef = useRef<WebSocket | null>(null);

  useEffect(() => {
    const terminal = new XTerm({
      cursorBlink: true,
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
      fontSize: 13,
      // 没显式设置 allowProposedApi 时 fit 用默认窗口大小;让 xterm 跑成 256 色 + 终端响应
      allowProposedApi: true,
      convertEol: false,
      scrollback: 5000,
      theme: {
        background: "#101418",
        foreground: "#eef2f3",
        cursor: "#eef2f3"
      }
    });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(terminalRef.current as HTMLDivElement);
    fit.fit();

    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    // 浏览器无法给 WebSocket 设置自定义 header,只能用 ?token= 把 JWT 一起发出去
    const wsUrl = appendAuthQuery(`/api/terminal/ws?cwd=${encodeURIComponent(cwd)}`);
    const socket = new WebSocket(`${protocol}//${window.location.host}${wsUrl}`);
    socket.binaryType = "arraybuffer";
    socketRef.current = socket;

    // PTY 初始尺寸是 120x30,fit 算完真实尺寸后必须显式同步给后端,
    // 否则 top/htop 这种全屏 TUI 会按 PTY 默认尺寸渲染,不匹配可视区
    const sendResize = (cols: number, rows: number) => {
      if (socket.readyState === WebSocket.OPEN) {
        socket.send(JSON.stringify({ type: "resize", cols, rows }));
      }
    };
    socket.onopen = () => {
      sendResize(terminal.cols, terminal.rows);
      terminal.focus();
    };
    socket.onmessage = (event) => {
      const text =
        typeof event.data === "string"
          ? event.data
          : new TextDecoder().decode(new Uint8Array(event.data));
      terminal.write(text);
    };
    terminal.onData((data) => socket.readyState === WebSocket.OPEN && socket.send(data));
    terminal.onResize((size) => sendResize(size.cols, size.rows));
    const resize = () => fit.fit();
    window.addEventListener("resize", resize);

    return () => {
      window.removeEventListener("resize", resize);
      socket.close();
      terminal.dispose();
    };
  }, [cwd]);

  return (
    <section className="page-grid terminal-layout">
      <header className="section-header full-span">
        <div>
          <h1>Web 终端</h1>
          <p>{cwd} · PTY 会话</p>
        </div>
      </header>
      <div className="terminal-surface full-span" ref={terminalRef} />
    </section>
  );
}

function FileManager({ clients, openTerminal }: { clients: Clients; openTerminal: (cwd: string) => void }) {
  const [path, setPath] = useState("/");
  const [items, setItems] = useState<FileItem[]>([]);
  const [selected, setSelected] = useState<FileItem | undefined>();
  const [editorValue, setEditorValue] = useState("");
  const [menu, setMenu] = useState<{ x: number; y: number; item: FileItem } | undefined>();
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<SearchMatch[]>([]);
  const [recycleItems, setRecycleItems] = useState<RecycleBinItem[]>([]);
  const inputRef = useRef<HTMLInputElement | null>(null);

  const load = async (nextPath = path) => {
    const response = await clients.files.listDirectory({ path: nextPath, recursive: false });
    const recycle = await clients.files.listRecycleBin({});
    setPath(nextPath);
    setItems(response.items);
    setRecycleItems(recycle.items);
  };

  useEffect(() => {
    void load("/");
  }, []);

  const openItem = async (item: FileItem) => {
    setSelected(item);
    if (item.kind === FileKind.DIRECTORY) {
      await load(item.path);
      return;
    }
    const response = await clients.files.readFile({ path: item.path });
    setEditorValue(new TextDecoder().decode(response.content));
  };

  const saveFile = async () => {
    if (!selected) return;
    await clients.files.saveFile({ path: selected.path, content: new TextEncoder().encode(editorValue) });
    await load(path);
  };

  const upload = async (files: FileList | null) => {
    if (!files?.length) return;
    for (const file of files) {
      if (file.size > 5 * 1024 * 1024) {
        await uploadInChunks(file);
      } else {
        const form = new FormData();
        form.append("file", file);
        await authFetch(`/api/fs/upload?path=${encodeURIComponent(path)}`, {
          method: "POST",
          body: form
        });
      }
    }
    await load(path);
  };

  const uploadInChunks = async (file: File) => {
    const chunkSize = 1024 * 1024;
    const totalChunks = Math.ceil(file.size / chunkSize);
    const uploadId = `${Date.now()}-${file.name}`;
    for (let chunkIndex = 0; chunkIndex < totalChunks; chunkIndex += 1) {
      const chunk = file.slice(chunkIndex * chunkSize, Math.min(file.size, (chunkIndex + 1) * chunkSize));
      await authFetch(`/api/fs/upload/chunk?path=${encodeURIComponent(path)}&upload_id=${encodeURIComponent(uploadId)}&chunk_index=${chunkIndex}&total_chunks=${totalChunks}&file_name=${encodeURIComponent(file.name)}`, {
        method: "POST",
        body: chunk
      });
    }
  };

  const deleteItem = async (item: FileItem) => {
    await clients.files.deletePath({ path: item.path, recursive: item.kind === FileKind.DIRECTORY });
    setMenu(undefined);
    await load(path);
  };

  const search = async () => {
    const response = await clients.files.searchFiles({
      rootPath: path,
      query: searchQuery,
      regex: true,
      maxResults: 100
    });
    setSearchResults(response.matches);
  };

  const restoreRecycleItem = async (item: RecycleBinItem) => {
    await clients.files.restoreRecycleItem({ id: item.id });
    await load(path);
  };

  const archiveItem = async (item: FileItem) => {
    await clients.files.createArchive({
      sourcePaths: [item.path],
      archivePath: `${path.replace(/\/$/, "")}/${item.name}.zip`,
      format: ArchiveFormat.ZIP
    });
    setMenu(undefined);
    await load(path);
  };

  return (
    <section className="file-grid">
      <header className="section-header full-span">
        <div>
          <h1>文件管理器</h1>
          <p>{path}</p>
        </div>
        <div className="toolbar">
          <IconButton label="刷新" icon={RefreshCw} onClick={() => void load(path)} />
          <IconButton label="新建目录" icon={FolderPlus} onClick={() => void clients.files.createDirectory({ path: `${path.replace(/\/$/, "")}/new-folder` }).then(() => load(path))} />
          <IconButton label="上传" icon={Upload} onClick={() => inputRef.current?.click()} />
          <IconButton label="终端" icon={TerminalSquare} onClick={() => openTerminal(path)} />
          <input className="toolbar-input" onChange={(event) => setSearchQuery(event.target.value)} placeholder="搜索" value={searchQuery} />
          <IconButton label="搜索" icon={FileText} onClick={() => void search()} />
          <input hidden multiple onChange={(event) => void upload(event.target.files)} ref={inputRef} type="file" />
        </div>
      </header>

      <div
        className="panel file-list"
        onDragOver={(event) => event.preventDefault()}
        onDrop={(event) => {
          event.preventDefault();
          void upload(event.dataTransfer.files);
        }}
      >
        <button className="breadcrumb" onClick={() => void load(parentPath(path))} type="button">
          ../
        </button>
        {items.map((item) => (
          <button
            className={selected?.path === item.path ? "file-row active" : "file-row"}
            key={item.path}
            onClick={() => void openItem(item)}
            onContextMenu={(event) => {
              event.preventDefault();
              setMenu({ x: event.clientX, y: event.clientY, item });
            }}
            type="button"
          >
            {item.kind === FileKind.DIRECTORY ? <Folder size={16} /> : <FileText size={16} />}
            <span>{item.name}</span>
            <small>{formatBytes(item.sizeBytes)}</small>
          </button>
        ))}
      </div>

      <div className="panel editor-panel">
        <div className="panel-title">
          <FileText size={18} />
          <span>{selected?.name ?? "未选择文件"}</span>
          {selected && selected.kind !== FileKind.DIRECTORY && (
            <>
              <IconButton label="保存" icon={Save} onClick={() => void saveFile()} />
              <IconButton
                label="下载"
                icon={Download}
                onClick={() => {
                  // 浏览器跳转无法带 Authorization header,所以把 token 拼到 query 里
                  window.location.href = appendAuthQuery(`/api/fs/download?path=${encodeURIComponent(selected.path)}`);
                }}
              />
            </>
          )}
        </div>
        <Editor
          height="520px"
          language={languageForPath(selected?.name ?? "")}
          onChange={(value) => setEditorValue(value ?? "")}
          options={{ minimap: { enabled: false }, fontSize: 13 }}
          value={editorValue}
        />
      </div>

      <div className="panel full-span">
        <div className="panel-title"><Trash2 size={18} /><span>回收站</span></div>
        <div className="table-list">
          {recycleItems.slice(0, 8).map((item) => (
            <div className="table-row" key={item.id}>
              <div>
                <strong>{item.originalPath}</strong>
                <small>{item.recyclePath}</small>
              </div>
              <IconButton label="还原" icon={RotateCw} onClick={() => void restoreRecycleItem(item)} />
            </div>
          ))}
          {!recycleItems.length && <div className="empty-state">回收站为空</div>}
        </div>
      </div>

      <div className="panel full-span">
        <div className="panel-title"><FileText size={18} /><span>全文检索</span></div>
        <div className="table-list">
          {searchResults.map((match) => (
            <div className="table-row" key={`${match.path}-${match.lineNumber}`}>
              <div>
                <strong>{match.path}:{match.lineNumber}</strong>
                <small>{match.line}</small>
              </div>
            </div>
          ))}
          {!searchResults.length && <div className="empty-state">暂无结果</div>}
        </div>
      </div>

      {menu && (
        <div className="context-menu" style={{ left: menu.x, top: menu.y }}>
          <button onClick={() => void archiveItem(menu.item)} type="button"><Archive size={15} />打包</button>
          <button onClick={() => void deleteItem(menu.item)} type="button"><Trash2 size={15} />删除</button>
        </div>
      )}
    </section>
  );
}

function DockerApps({ clients }: { clients: Clients }) {
  const [containers, setContainers] = useState<ContainerItem[]>([]);
  const [images, setImages] = useState<ImageItem[]>([]);
  const [composeProjects, setComposeProjects] = useState<ComposeProject[]>([]);
  const [templates, setTemplates] = useState<AppTemplate[]>([]);
  const [installedApps, setInstalledApps] = useState<InstalledApp[]>([]);
  const [quotaForm, setQuotaForm] = useState<DockerQuotaForm>(defaultDockerQuotaForm);
  const [pullForm, setPullForm] = useState<ImagePullForm>(defaultImagePullForm);
  const [rollbackForm, setRollbackForm] = useState<ImageRollbackForm>(defaultImageRollbackForm);
  const [composeForm, setComposeForm] = useState<ComposeForm>(defaultComposeForm);
  const [selectedVersions, setSelectedVersions] = useState<Record<string, string>>({});
  const [logLines, setLogLines] = useState<string[]>([]);
  const [pullLines, setPullLines] = useState<string[]>([]);
  const [error, setError] = useState("");

  const load = async () => {
    try {
      const [containerResponse, imageResponse, composeResponse, templateResponse, installedResponse] = await Promise.all([
        clients.docker.listContainers({ all: true }),
        clients.docker.listImages({ all: true }),
        clients.docker.listComposeProjects({}),
        clients.appStore.listAppTemplates({}),
        clients.appStore.listInstalledApps({})
      ]);
      setContainers(containerResponse.containers);
      setImages(imageResponse.images);
      setComposeProjects(composeResponse.projects);
      setTemplates(templateResponse.templates);
      setInstalledApps(installedResponse.apps);
      setSelectedVersions((current) => ({
        ...Object.fromEntries(templateResponse.templates.map((template) => [template.slug, template.defaultVersion])),
        ...current
      }));
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const action = async (containerId: string, kind: "start" | "stop" | "restart" | "pause" | "remove") => {
    const payload = { containerId };
    if (kind === "start") await clients.docker.startContainer(payload);
    if (kind === "stop") await clients.docker.stopContainer(payload);
    if (kind === "restart") await clients.docker.restartContainer(payload);
    if (kind === "pause") await clients.docker.pauseContainer(payload);
    if (kind === "remove") await clients.docker.removeContainer(payload);
    await load();
  };

  const saveQuota = async () => {
    try {
      await clients.docker.setContainerResources({
        containerId: quotaForm.containerId,
        cpuLimitCores: Number(quotaForm.cpuLimitCores || 0),
        memoryLimitBytes: BigInt(Math.max(0, Number(quotaForm.memoryLimitMb || 0)) * 1024 * 1024)
      });
      await load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const pullImage = async () => {
    setPullLines([]);
    try {
      for await (const event of clients.docker.watchImagePull({ image: pullForm.image, tag: pullForm.tag })) {
        const progress = event.progress ? ` · ${event.progress}` : "";
        setPullLines((lines) => [...lines.slice(-80), `${event.statusText}${progress}\n`]);
      }
      await load();
    } catch (err) {
      setPullLines((lines) => [...lines, `${safeError(err)}\n`]);
    }
  };

  const pruneResources = async () => {
    try {
      const response = await clients.docker.pruneDockerResources({
        images: true,
        containers: true,
        volumes: true,
        networks: true,
        allImages: false
      });
      setPullLines((lines) => [
        ...lines,
        `清理 ${response.deletedCount} 项，释放 ${formatBytes(response.spaceReclaimedBytes)} · ${response.summary}\n`
      ]);
      await load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const rollbackImage = async () => {
    try {
      await clients.docker.rollbackImageTag(rollbackForm);
      await load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const saveCompose = async () => {
    try {
      const response = await clients.docker.upsertComposeProject(composeForm);
      if (response.project) {
        setComposeForm({ name: response.project.name, composeYaml: response.project.composeYaml });
      }
      await load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const deployCompose = async (name = composeForm.name) => {
    try {
      await clients.docker.deployComposeProject({ name });
      await load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const removeCompose = async (name: string) => {
    try {
      await clients.docker.removeComposeProject({ name, deleteFiles: true });
      await load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const deployTemplate = async (template: AppTemplate) => {
    try {
      const version = selectedVersions[template.slug] || template.defaultVersion;
      await clients.appStore.deployApp({
        slug: template.slug,
        appName: `${template.slug}-${version}`.replaceAll(".", "-"),
        version
      });
      await load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const updateInstalledApp = async (app: InstalledApp) => {
    try {
      const template = templates.find((item) => item.slug === app.slug);
      const version = template?.defaultVersion || app.version;
      await clients.appStore.updateApp({ appName: app.appName, version });
      await load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const uninstallApp = async (app: InstalledApp) => {
    try {
      await clients.appStore.uninstallApp({ appName: app.appName });
      await load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const watchLogs = async (container: ContainerItem) => {
    setLogLines([]);
    try {
      for await (const line of clients.docker.watchContainerLogs({ containerId: container.id, tailLines: 200 })) {
        setLogLines((lines) => [...lines.slice(-300), line.line]);
      }
    } catch (err) {
      setLogLines((lines) => [...lines, safeError(err)]);
    }
  };

  return (
    <section className="page-grid">
      <header className="section-header full-span">
        <div>
          <h1>容器与应用商店</h1>
          <p>{error || `${containers.length} 个容器 · ${images.length} 个镜像 · ${templates.length} 个模板`}</p>
        </div>
        <IconButton label="刷新" icon={RefreshCw} onClick={() => void load()} />
      </header>

      <div className="panel wide-panel">
        <div className="panel-title"><Boxes size={18} /><span>容器列表</span></div>
        <div className="table-list">
          {containers.map((container) => (
            <div className="table-row" key={container.id}>
              <div>
                <strong>{container.name || container.id.slice(0, 12)}</strong>
                <small>
                  {container.image} · {container.statusText} · CPU {container.cpuLimitCores ? container.cpuLimitCores.toFixed(2) : "不限"} · 内存 {container.memoryLimitBytes ? formatBytes(container.memoryLimitBytes) : "不限"}
                </small>
              </div>
              <StatusPill label={container.state || "unknown"} tone={container.state === "running" ? "good" : "muted"} />
              <div className="row-actions">
                <IconButton label="启动" icon={Play} onClick={() => void action(container.id, "start")} />
                <IconButton label="停止" icon={Square} onClick={() => void action(container.id, "stop")} />
                <IconButton label="重启" icon={RotateCw} onClick={() => void action(container.id, "restart")} />
                <IconButton label="暂停" icon={Pause} onClick={() => void action(container.id, "pause")} />
                <IconButton label="日志" icon={TerminalSquare} onClick={() => void watchLogs(container)} />
              </div>
            </div>
          ))}
        </div>
      </div>

      <div className="panel">
        <div className="panel-title"><Boxes size={18} /><span>资源配额</span></div>
        <Input label="容器 ID" value={quotaForm.containerId} onChange={(containerId) => setQuotaForm({ ...quotaForm, containerId })} />
        <Input label="CPU 核数" value={quotaForm.cpuLimitCores} onChange={(cpuLimitCores) => setQuotaForm({ ...quotaForm, cpuLimitCores })} />
        <Input label="内存 MB" value={quotaForm.memoryLimitMb} onChange={(memoryLimitMb) => setQuotaForm({ ...quotaForm, memoryLimitMb })} />
        <button onClick={() => void saveQuota()} type="button"><Save size={15} />应用配额</button>
      </div>

      <div className="panel wide-panel">
        <div className="panel-title"><Download size={18} /><span>镜像管理</span></div>
        <div className="inline-grid">
          <Input label="镜像" value={pullForm.image} onChange={(image) => setPullForm({ ...pullForm, image })} />
          <Input label="标签" value={pullForm.tag} onChange={(tag) => setPullForm({ ...pullForm, tag })} />
        </div>
        <div className="row-actions backup-actions">
          <button onClick={() => void pullImage()} type="button"><Download size={15} />拉取</button>
          <button onClick={() => void pruneResources()} type="button"><Trash2 size={15} />清理残留</button>
        </div>
        <div className="inline-grid">
          <Input label="回滚来源" value={rollbackForm.sourceImage} onChange={(sourceImage) => setRollbackForm({ ...rollbackForm, sourceImage })} />
          <Input label="目标仓库" value={rollbackForm.targetRepository} onChange={(targetRepository) => setRollbackForm({ ...rollbackForm, targetRepository })} />
          <Input label="目标标签" value={rollbackForm.targetTag} onChange={(targetTag) => setRollbackForm({ ...rollbackForm, targetTag })} />
        </div>
        <button onClick={() => void rollbackImage()} type="button"><RotateCw size={15} />回滚标签</button>
        <pre className="report-output compact-output">{pullLines.join("") || "暂无镜像任务"}</pre>
      </div>

      <div className="panel">
        <div className="panel-title"><Archive size={18} /><span>本地镜像</span></div>
        <div className="table-list compact-list">
          {images.slice(0, 8).map((image) => (
            <div className="key-row" key={image.id}>
              <strong>{image.repoTags[0] || image.id.slice(0, 18)}</strong>
              <small>{formatBytes(image.sizeBytes)} · containers {image.containers}</small>
            </div>
          ))}
          {!images.length && <div className="empty-state">暂无镜像</div>}
        </div>
      </div>

      <div className="panel wide-panel">
        <div className="panel-title"><FileText size={18} /><span>Compose 编排</span></div>
        <Input label="项目名" value={composeForm.name} onChange={(name) => setComposeForm({ ...composeForm, name })} />
        <textarea
          className="pem-input code-input"
          value={composeForm.composeYaml}
          onChange={(event) => setComposeForm({ ...composeForm, composeYaml: event.target.value })}
        />
        <div className="row-actions backup-actions">
          <button onClick={() => void saveCompose()} type="button"><Save size={15} />保存</button>
          <button onClick={() => void deployCompose()} type="button"><Play size={15} />部署</button>
        </div>
        <div className="table-list compact-list">
          {composeProjects.map((project) => (
            <div className="table-row" key={project.name}>
              <div>
                <strong>{project.name}</strong>
                <small>{project.serviceNames.join(", ") || project.composePath}</small>
              </div>
              <StatusPill label={project.statusText || "saved"} tone="muted" />
              <div className="row-actions">
                <IconButton label="编辑" icon={FileText} onClick={() => setComposeForm({ name: project.name, composeYaml: project.composeYaml })} />
                <IconButton label="部署" icon={Play} onClick={() => void deployCompose(project.name)} />
                <IconButton label="删除" icon={Trash2} onClick={() => void removeCompose(project.name)} />
              </div>
            </div>
          ))}
          {!composeProjects.length && <div className="empty-state">暂无 Compose 项目</div>}
        </div>
      </div>

      <div className="panel">
        <div className="panel-title"><Store size={18} /><span>应用模板</span></div>
        <div className="template-grid">
          {templates.map((template) => (
            <div className="item-card" key={template.slug}>
              <strong>{template.name}</strong>
              <small>{template.runtimeKind} · {template.image}</small>
              <select
                value={selectedVersions[template.slug] || template.defaultVersion}
                onChange={(event) => setSelectedVersions({ ...selectedVersions, [template.slug]: event.target.value })}
              >
                {template.versions.map((version) => (
                  <option key={version.version} value={version.version}>{version.version}</option>
                ))}
              </select>
              <button onClick={() => void deployTemplate(template)} type="button">
                <Play size={15} />安装
              </button>
            </div>
          ))}
        </div>
      </div>

      <div className="panel full-span">
        <div className="panel-title"><Store size={18} /><span>已安装运行环境</span></div>
        <div className="table-list">
          {installedApps.map((app) => (
            <div className="table-row" key={app.appName}>
              <div>
                <strong>{app.appName}</strong>
                <small>{app.slug} {app.version} · {app.image}</small>
              </div>
              <StatusPill label={app.state || "installed"} tone="good" />
              <div className="row-actions">
                <IconButton label="更新" icon={RefreshCw} onClick={() => void updateInstalledApp(app)} />
                <IconButton label="卸载" icon={Trash2} onClick={() => void uninstallApp(app)} />
              </div>
            </div>
          ))}
          {!installedApps.length && <div className="empty-state">暂无已安装应用</div>}
        </div>
      </div>

      <div className="panel log-panel">
        <div className="panel-title"><TerminalSquare size={18} /><span>容器日志</span></div>
        <pre>{logLines.join("")}</pre>
      </div>
    </section>
  );
}

function MicroPanel({ clients }: { clients: Clients }) {
  const [sites, setSites] = useState<SiteItem[]>([]);
  const [workloads, setWorkloads] = useState<WorkloadItem[]>([]);
  const [proxies, setProxies] = useState<ProxyInstance[]>([]);
  const [vpnCapabilities, setVpnCapabilities] = useState<VpnCapability[]>([]);
  const [siteForm, setSiteForm] = useState<MicroSiteForm>(defaultMicroSiteForm);
  const [workloadForm, setWorkloadForm] = useState<WorkloadForm>(defaultWorkloadForm);
  const [proxyForm, setProxyForm] = useState<ProxyForm>(defaultProxyForm);
  const [log, setLog] = useState("");
  const [status, setStatus] = useState("");

  const load = async () => {
    try {
      const [siteResponse, workloadResponse, proxyResponse, vpnResponse] = await Promise.all([
        clients.site.listSites({}),
        clients.workload.listWorkloads({}),
        clients.proxy.listProxyInstances({}),
        clients.proxy.detectVpnCapabilities({})
      ]);
      setSites(siteResponse.sites.filter((site) => site.engine === "builtin"));
      setWorkloads(workloadResponse.workloads);
      setProxies(proxyResponse.instances);
      setVpnCapabilities(vpnResponse.capabilities);
      setStatus(vpnResponse.summary);
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const createSite = async () => {
    try {
      await clients.site.createSite({
        name: siteForm.name,
        domains: [],
        root: siteForm.root,
        proxyTarget: "",
        sslEnabled: false,
        engine: "builtin",
        listenAddr: ""
      });
      await load();
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const saveWorkload = async () => {
    try {
      await clients.workload.upsertWorkload({
        workload: {
          id: "",
          name: workloadForm.name,
          command: workloadForm.command,
          cwd: workloadForm.cwd,
          env: [],
          autostart: true,
          memoryLimitMb: BigInt(Math.max(1, Number(workloadForm.memoryLimitMb || 32))),
          logLimitBytes: 5n * 1024n * 1024n,
          restartLimit: 3,
          scheduleCron: "",
          state: WorkloadState.STOPPED,
          pid: 0,
          logPath: "",
          lastMessage: "",
          updatedAtSeconds: 0n
        }
      });
      await load();
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const startWorkload = async (workload: WorkloadItem) => {
    await clients.workload.startWorkload({ id: workload.id });
    await load();
  };

  const stopWorkload = async (workload: WorkloadItem) => {
    await clients.workload.stopWorkload({ id: workload.id });
    await load();
  };

  const readWorkloadLog = async (workload: WorkloadItem) => {
    const response = await clients.workload.getWorkloadLog({ id: workload.id, maxBytes: 64n * 1024n });
    setLog(response.content);
  };

  const saveProxy = async () => {
    try {
      await clients.proxy.upsertProxyInstance({
        instance: {
          id: "",
          name: proxyForm.name,
          templateId: "shadowsocks-rust",
          listenHost: "0.0.0.0",
          listenPort: Math.max(1, Number(proxyForm.listenPort || 8388)),
          method: "",
          password: proxyForm.password,
          state: ProxyState.STOPPED,
          pid: 0,
          logPath: "",
          lastMessage: "",
          updatedAtSeconds: 0n
        }
      });
      await load();
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const startProxy = async (proxy: ProxyInstance) => {
    try {
      await clients.proxy.startProxyInstance({ id: proxy.id });
      await load();
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const stopProxy = async (proxy: ProxyInstance) => {
    await clients.proxy.stopProxyInstance({ id: proxy.id });
    await load();
  };

  const readProxyLog = async (proxy: ProxyInstance) => {
    const response = await clients.proxy.getProxyLog({ id: proxy.id, maxBytes: 64n * 1024n });
    setLog(response.content);
  };

  return (
    <section className="page-grid">
      <header className="section-header full-span">
        <div>
          <h1>Micro 极限模式</h1>
          <p>{status || `${sites.length} 个静态站点 · ${workloads.length} 个任务 · ${proxies.length} 个代理`}</p>
        </div>
        <IconButton label="刷新" icon={RefreshCw} onClick={() => void load()} />
      </header>

      <div className="panel">
        <div className="panel-title"><Globe size={18} /><span>内置静态托管</span></div>
        <Input label="名称" value={siteForm.name} onChange={(name) => setSiteForm({ ...siteForm, name })} />
        <Input label="目录" value={siteForm.root} onChange={(root) => setSiteForm({ ...siteForm, root })} />
        <button onClick={() => void createSite()} type="button"><Save size={15} />创建</button>
      </div>

      <div className="panel wide-panel">
        <div className="panel-title"><FileText size={18} /><span>静态站点</span></div>
        <div className="table-list compact-list">
          {sites.map((site) => (
            <div className="table-row" key={site.name}>
              <div>
                <strong>{site.name}</strong>
                <small>{site.root} · {site.publicPath}</small>
              </div>
              <StatusPill label="builtin" tone="good" />
            </div>
          ))}
          {!sites.length && <div className="empty-state">暂无内置静态站点</div>}
        </div>
      </div>

      <div className="panel">
        <div className="panel-title"><TerminalSquare size={18} /><span>Rust 爬虫/进程</span></div>
        <Input label="名称" value={workloadForm.name} onChange={(name) => setWorkloadForm({ ...workloadForm, name })} />
        <Input label="命令" value={workloadForm.command} onChange={(command) => setWorkloadForm({ ...workloadForm, command })} />
        <Input label="目录" value={workloadForm.cwd} onChange={(cwd) => setWorkloadForm({ ...workloadForm, cwd })} />
        <Input label="内存 MB" value={workloadForm.memoryLimitMb} onChange={(memoryLimitMb) => setWorkloadForm({ ...workloadForm, memoryLimitMb })} />
        <button onClick={() => void saveWorkload()} type="button"><Save size={15} />保存</button>
      </div>

      <div className="panel wide-panel">
        <div className="panel-title"><Power size={18} /><span>托管任务</span></div>
        <div className="table-list">
          {workloads.map((workload) => (
            <div className="table-row" key={workload.id}>
              <div>
                <strong>{workload.name}</strong>
                <small>PID {workload.pid || "-"} · {workload.command} · {Number(workload.memoryLimitMb)}MB</small>
              </div>
              <StatusPill label={WorkloadState[workload.state]} tone={workload.state === WorkloadState.RUNNING ? "good" : "muted"} />
              <div className="row-actions">
                <IconButton label="启动" icon={Play} onClick={() => void startWorkload(workload)} />
                <IconButton label="停止" icon={Square} onClick={() => void stopWorkload(workload)} />
                <IconButton label="日志" icon={TerminalSquare} onClick={() => void readWorkloadLog(workload)} />
              </div>
            </div>
          ))}
          {!workloads.length && <div className="empty-state">暂无托管任务</div>}
        </div>
      </div>

      <div className="panel">
        <div className="panel-title"><ShieldCheck size={18} /><span>shadowsocks-rust</span></div>
        <Input label="名称" value={proxyForm.name} onChange={(name) => setProxyForm({ ...proxyForm, name })} />
        <Input label="端口" value={proxyForm.listenPort} onChange={(listenPort) => setProxyForm({ ...proxyForm, listenPort })} />
        <Input label="密码" value={proxyForm.password} onChange={(password) => setProxyForm({ ...proxyForm, password })} />
        <button onClick={() => void saveProxy()} type="button"><Save size={15} />保存</button>
      </div>

      <div className="panel wide-panel">
        <div className="panel-title"><ShieldCheck size={18} /><span>代理实例</span></div>
        <div className="table-list">
          {proxies.map((proxy) => (
            <div className="table-row" key={proxy.id}>
              <div>
                <strong>{proxy.name}</strong>
                <small>{proxy.templateId} · {proxy.listenHost}:{proxy.listenPort} · PID {proxy.pid || "-"}</small>
              </div>
              <StatusPill label={ProxyState[proxy.state]} tone={proxy.state === ProxyState.RUNNING ? "good" : "muted"} />
              <div className="row-actions">
                <IconButton label="启动" icon={Play} onClick={() => void startProxy(proxy)} />
                <IconButton label="停止" icon={Square} onClick={() => void stopProxy(proxy)} />
                <IconButton label="日志" icon={TerminalSquare} onClick={() => void readProxyLog(proxy)} />
              </div>
            </div>
          ))}
          {!proxies.length && <div className="empty-state">暂无代理实例</div>}
        </div>
      </div>

      <div className="panel">
        <div className="panel-title"><ShieldAlert size={18} /><span>VPN 能力探测</span></div>
        <div className="table-list compact-list">
          {vpnCapabilities.map((capability) => (
            <div className="key-row" key={capability.id}>
              <strong>{capability.name}</strong>
              <small>{capability.reason}</small>
              <StatusPill label={capability.available ? "可用" : "不可用"} tone={capability.available ? "good" : "danger"} />
            </div>
          ))}
        </div>
      </div>

      <div className="panel log-panel">
        <div className="panel-title"><TerminalSquare size={18} /><span>Micro 日志</span></div>
        <pre>{log || "暂无日志"}</pre>
      </div>
    </section>
  );
}

function SitesSsl({ clients }: { clients: Clients }) {
  const [sites, setSites] = useState<SiteItem[]>([]);
  const [certificates, setCertificates] = useState<CertificateItem[]>([]);
  const [rewriteTemplates, setRewriteTemplates] = useState<RewriteTemplate[]>([]);
  const [rewriteContent, setRewriteContent] = useState("");
  const [proxyRules, setProxyRules] = useState<ReverseProxyRule[]>([]);
  const [form, setForm] = useState({ name: "demo", domains: "example.com", root: "/var/www/html", proxyTarget: "" });
  const [sslDomain, setSslDomain] = useState("example.com");
  const [importForm, setImportForm] = useState({ domain: "example.com", group: "default", certificatePem: "", privateKeyPem: "" });
  const [proxyForm, setProxyForm] = useState({
    name: "api",
    domain: "example.com",
    pathPrefix: "/api/",
    targets: "http://127.0.0.1:3000,http://127.0.0.1:3001",
    method: "least_conn",
    cacheEnabled: false,
    rateLimit: 120
  });
  const [status, setStatus] = useState("");

  const load = async () => {
    const [siteResponse, certResponse, templateResponse, proxyResponse] = await Promise.all([
      clients.site.listSites({}),
      clients.ssl.listCertificates({}),
      clients.site.listRewriteTemplates({}),
      clients.site.listReverseProxyRules({})
    ]);
    setSites(siteResponse.sites);
    setCertificates(certResponse.certificates);
    setRewriteTemplates(templateResponse.templates);
    if (!rewriteContent && templateResponse.templates[0]) {
      setRewriteContent(templateResponse.templates[0].content);
    }
    setProxyRules(proxyResponse.rules);
  };

  useEffect(() => {
    void load();
  }, []);

  const createSite = async () => {
    await clients.site.createSite({
      name: form.name,
      domains: form.domains.split(",").map((domain) => domain.trim()).filter(Boolean),
      root: form.root,
      proxyTarget: form.proxyTarget,
      sslEnabled: false
    });
    await load();
  };

  const requestSsl = async () => {
    const response = await clients.ssl.requestCertificate({ domain: sslDomain, email: "admin@example.com" });
    setStatus(response.certificate ? `${response.certificate.domain} 已签发` : "证书申请已提交");
    await load();
  };

  const importCertificate = async () => {
    const response = await clients.ssl.importCertificate(importForm);
    setStatus(response.certificate ? `${response.certificate.domain} 已导入` : "证书已导入");
    await load();
  };

  const renewCertificate = async (certificate: CertificateItem) => {
    const response = await clients.ssl.renewCertificate({ domain: certificate.domain });
    setStatus(response.output || `${certificate.domain} 已续签`);
    await load();
  };

  const saveReverseProxy = async () => {
    const targets = proxyForm.targets
      .split(",")
      .map((target) => target.trim())
      .filter(Boolean)
      .map((url) => ({ url, weight: 1, healthy: true }));
    await clients.site.upsertReverseProxyRule({
      rule: {
        id: "",
        name: proxyForm.name,
        domain: proxyForm.domain,
        pathPrefix: proxyForm.pathPrefix,
        targets,
        loadBalanceMethod: proxyForm.method,
        cacheEnabled: proxyForm.cacheEnabled,
        rateLimitPerMinute: proxyForm.rateLimit,
        enabled: true,
        configPath: "",
        createdAtSeconds: 0n,
        updatedAtSeconds: 0n
      }
    });
    await load();
  };

  const deleteReverseProxy = async (rule: ReverseProxyRule) => {
    await clients.site.deleteReverseProxyRule({ id: rule.id });
    await load();
  };

  return (
    <section className="page-grid">
      <header className="section-header full-span">
        <div>
          <h1>站点与 SSL</h1>
          <p>{sites.length} 个 Nginx 配置</p>
        </div>
      </header>

      <div className="panel">
        <div className="panel-title"><Globe size={18} /><span>建站向导</span></div>
        <Input label="名称" value={form.name} onChange={(name) => setForm({ ...form, name })} />
        <Input label="域名" value={form.domains} onChange={(domains) => setForm({ ...form, domains })} />
        <Input label="目录" value={form.root} onChange={(root) => setForm({ ...form, root })} />
        <Input label="反代" value={form.proxyTarget} onChange={(proxyTarget) => setForm({ ...form, proxyTarget })} />
        <button onClick={() => void createSite()} type="button"><Save size={15} />创建</button>
      </div>

      <div className="panel">
        <div className="panel-title"><ShieldCheck size={18} /><span>SSL 自动化</span></div>
        <Input label="域名" value={sslDomain} onChange={setSslDomain} />
        <button onClick={() => void requestSsl()} type="button"><ShieldCheck size={15} />申请</button>
        <small>{status}</small>
      </div>

      <div className="panel">
        <div className="panel-title"><FileUp size={18} /><span>手动导入</span></div>
        <Input label="域名" value={importForm.domain} onChange={(domain) => setImportForm({ ...importForm, domain })} />
        <Input label="分组" value={importForm.group} onChange={(group) => setImportForm({ ...importForm, group })} />
        <textarea
          className="pem-input"
          onChange={(event) => setImportForm({ ...importForm, certificatePem: event.target.value })}
          placeholder="-----BEGIN CERTIFICATE-----"
          value={importForm.certificatePem}
        />
        <textarea
          className="pem-input"
          onChange={(event) => setImportForm({ ...importForm, privateKeyPem: event.target.value })}
          placeholder="-----BEGIN PRIVATE KEY-----"
          value={importForm.privateKeyPem}
        />
        <button onClick={() => void importCertificate()} type="button"><FileUp size={15} />导入</button>
      </div>

      <div className="panel">
        <div className="panel-title"><FileText size={18} /><span>伪静态模板</span></div>
        <SelectRow
          label="模板"
          value={0}
          onChange={(index) => setRewriteContent(rewriteTemplates[Number(index)]?.content ?? "")}
          options={rewriteTemplates.map((template, index) => [index, template.name])}
        />
        <textarea
          className="pem-input code-input"
          onChange={(event) => setRewriteContent(event.target.value)}
          value={rewriteContent}
        />
      </div>

      <div className="panel">
        <div className="panel-title"><Globe size={18} /><span>反向代理</span></div>
        <Input label="名称" value={proxyForm.name} onChange={(name) => setProxyForm({ ...proxyForm, name })} />
        <Input label="域名" value={proxyForm.domain} onChange={(domain) => setProxyForm({ ...proxyForm, domain })} />
        <Input label="路径" value={proxyForm.pathPrefix} onChange={(pathPrefix) => setProxyForm({ ...proxyForm, pathPrefix })} />
        <Input label="目标" value={proxyForm.targets} onChange={(targets) => setProxyForm({ ...proxyForm, targets })} />
        <SelectRow
          label="均衡"
          value={["round_robin", "least_conn", "ip_hash"].indexOf(proxyForm.method)}
          onChange={(index) => setProxyForm({ ...proxyForm, method: ["round_robin", "least_conn", "ip_hash"][Number(index)] })}
          options={[
            [0, "轮询"],
            [1, "最少连接"],
            [2, "IP Hash"]
          ]}
        />
        <ToggleRow label="缓存" checked={proxyForm.cacheEnabled} onChange={(cacheEnabled) => setProxyForm({ ...proxyForm, cacheEnabled })} />
        <NumberInput label="限速/分钟" value={proxyForm.rateLimit} onChange={(rateLimit) => setProxyForm({ ...proxyForm, rateLimit })} />
        <button onClick={() => void saveReverseProxy()} type="button"><Save size={15} />保存反代</button>
      </div>

      <div className="panel wide-panel">
        <div className="panel-title"><Server size={18} /><span>站点列表</span></div>
        {sites.map((site) => (
          <div className="table-row" key={site.configPath || site.name}>
            <div>
              <strong>{site.name}</strong>
              <small>{site.domains.join(", ") || site.configPath}</small>
            </div>
            <StatusPill label={site.sslEnabled ? "SSL" : "HTTP"} tone={site.sslEnabled ? "good" : "muted"} />
          </div>
        ))}
      </div>

      <div className="panel full-span">
        <div className="panel-title"><ShieldCheck size={18} /><span>证书统一视图</span></div>
        <div className="table-list">
          {certificates.map((certificate) => (
            <div className="table-row" key={certificate.domain}>
              <div>
                <strong>{certificate.domain}</strong>
                <small>{certificate.group || "default"} · {certificate.certificatePath}</small>
              </div>
              <StatusPill label={`${certificate.daysUntilExpiry} 天`} tone={certificate.warningLevel === "ok" ? "good" : "danger"} />
              <IconButton label="续签" icon={RotateCw} onClick={() => void renewCertificate(certificate)} />
            </div>
          ))}
          {!certificates.length && <div className="empty-state">暂无证书</div>}
        </div>
      </div>

      <div className="panel full-span">
        <div className="panel-title"><Globe size={18} /><span>反代规则</span></div>
        <div className="table-list">
          {proxyRules.map((rule) => (
            <div className="table-row" key={rule.id}>
              <div>
                <strong>{rule.name} · {rule.domain}{rule.pathPrefix}</strong>
                <small>{rule.loadBalanceMethod || "round_robin"} · {rule.targets.map((target) => target.url).join(", ")}</small>
              </div>
              <StatusPill label={rule.enabled ? "启用" : "停用"} tone={rule.enabled ? "good" : "muted"} />
              <IconButton label="删除" icon={Trash2} onClick={() => void deleteReverseProxy(rule)} />
            </div>
          ))}
          {!proxyRules.length && <div className="empty-state">暂无反代规则</div>}
        </div>
      </div>
    </section>
  );
}

function DatabasePanel({ clients }: { clients: Clients }) {
  const [dsn, setDsn] = useState("sqlite::memory:");
  const [sql, setSql] = useState("select 1 as value");
  const [columns, setColumns] = useState<string[]>([]);
  const [rows, setRows] = useState<string[][]>([]);
  const [databases, setDatabases] = useState<string[]>([]);

  const listDatabases = async () => {
    const response = await clients.database.listDatabases({ dsn });
    setDatabases(response.databases.map((database) => database.name));
  };

  const execute = async () => {
    const response = await clients.database.executeSql({ dsn, sql, maxRows: 100 });
    setColumns(response.columns);
    setRows(response.rows.map((row) => row.values));
  };

  return (
    <section className="split-page">
      <div className="panel">
        <div className="panel-title"><Database size={18} /><span>连接</span></div>
        <Input label="DSN" value={dsn} onChange={setDsn} />
        <button onClick={() => void listDatabases()} type="button"><RefreshCw size={15} />连接</button>
        <div className="tree-list">
          {databases.map((database) => <span key={database}>{database}</span>)}
        </div>
      </div>
      <div className="panel sql-panel">
        <div className="panel-title"><FileText size={18} /><span>SQL 查询台</span></div>
        <Editor height="260px" language="sql" onChange={(value) => setSql(value ?? "")} value={sql} />
        <button onClick={() => void execute()} type="button"><Play size={15} />执行</button>
        <table className="result-table">
          <thead><tr>{columns.map((column) => <th key={column}>{column}</th>)}</tr></thead>
          <tbody>{rows.map((row, index) => <tr key={index}>{row.map((value, cell) => <td key={cell}>{value}</td>)}</tr>)}</tbody>
        </table>
      </div>
    </section>
  );
}

function CronPanel({ clients }: { clients: Clients }) {
  const [tasks, setTasks] = useState<CronTask[]>([]);
  const [form, setForm] = useState({ name: "daily-backup", cron: "0 0 2 * * *", command: "echo ok" });
  const [log, setLog] = useState("");

  const load = async () => {
    const response = await clients.cron.listCronTasks({});
    setTasks(response.tasks);
  };

  useEffect(() => {
    void load();
  }, []);

  const createTask = async () => {
    await clients.cron.createCronTask({
      task: {
        id: "",
        name: form.name,
        cronExpression: form.cron,
        command: form.command,
        state: CronTaskState.ENABLED,
        timeoutSeconds: 300n,
        nextRunAt: ""
      }
    });
    await load();
  };

  const runTask = async (task: CronTask) => {
    const run = await clients.cron.runCronTask({ taskId: task.id });
    setLog(`${task.name}: ${CronRunState[run.run?.state ?? CronRunState.UNSPECIFIED]}`);
    const logResponse = await clients.cron.getCronTaskLog({ taskId: task.id });
    setLog(logResponse.content);
  };

  return (
    <section className="page-grid">
      <header className="section-header full-span">
        <div>
          <h1>计划任务</h1>
          <p>{tasks.length} 个任务</p>
        </div>
      </header>

      <div className="panel">
        <div className="panel-title"><Clock size={18} /><span>创建任务</span></div>
        <Input label="名称" value={form.name} onChange={(name) => setForm({ ...form, name })} />
        <Input label="Cron" value={form.cron} onChange={(cron) => setForm({ ...form, cron })} />
        <Input label="脚本" value={form.command} onChange={(command) => setForm({ ...form, command })} />
        <button onClick={() => void createTask()} type="button"><Save size={15} />保存</button>
      </div>

      <div className="panel wide-panel">
        <div className="panel-title"><Clock size={18} /><span>任务列表</span></div>
        {tasks.map((task) => (
          <div className="table-row" key={task.id}>
            <div>
              <strong>{task.name}</strong>
              <small>{task.cronExpression} · {task.command}</small>
            </div>
            <StatusPill label={task.state === CronTaskState.ENABLED ? "启用" : "暂停"} tone={task.state === CronTaskState.ENABLED ? "good" : "muted"} />
            <IconButton label="运行" icon={Play} onClick={() => void runTask(task)} />
          </div>
        ))}
      </div>

      <div className="panel log-panel">
        <div className="panel-title"><FileText size={18} /><span>执行日志</span></div>
        <pre>{log}</pre>
      </div>
    </section>
  );
}

function ClusterAudit({ clients }: { clients: Clients }) {
  const [nodes, setNodes] = useState<ClusterNode[]>([]);
  const [records, setRecords] = useState<DistributionRecord[]>([]);
  const [events, setEvents] = useState<AuditEvent[]>([]);
  const [pairForm, setPairForm] = useState<ClusterPairForm>(defaultClusterPairForm);
  const [distributionForm, setDistributionForm] = useState<DistributionForm>(defaultDistributionForm);
  const [nodeSecrets, setNodeSecrets] = useState<Record<string, string>>({});
  const [auditQuery, setAuditQuery] = useState("");
  const [analysis, setAnalysis] = useState("");
  const [status, setStatus] = useState("");

  const load = async () => {
    try {
      const [nodeResponse, recordResponse, auditResponse] = await Promise.all([
        clients.cluster.listClusterNodes({}),
        clients.cluster.listDistributionRecords({ limit: 100 }),
        clients.audit.listAuditEvents({ query: auditQuery, limit: 100 })
      ]);
      setNodes(nodeResponse.nodes);
      setRecords(recordResponse.records);
      setEvents(auditResponse.events);
      setStatus("");
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const pairNode = async () => {
    try {
      const response = await clients.cluster.pairClusterNode(pairForm);
      if (response.node) {
        setNodeSecrets((current) => ({ ...current, [response.node!.id]: response.nodeSecret }));
      }
      setStatus(response.node ? `节点密钥 ${response.nodeSecret}` : "节点已接入");
      await load();
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const sendHeartbeat = async (node: ClusterNode) => {
    try {
      const nodeSecret = nodeSecrets[node.id];
      if (!nodeSecret) {
        setStatus("当前会话没有该节点密钥，请重新接入后发送心跳");
        return;
      }
      await clients.cluster.heartbeatClusterNode({
        nodeId: node.id,
        nodeSecret,
        loadAverage: 0
      });
      await load();
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const distributeFile = async () => {
    try {
      await clients.cluster.distributeFile({
        targetNodeIds: distributionForm.targetNodeId ? [distributionForm.targetNodeId] : [],
        path: distributionForm.path,
        content: new TextEncoder().encode(distributionForm.content),
        mode: 0o644
      });
      await load();
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const analyzeAudit = async () => {
    try {
      const response = await clients.audit.analyzeAuditEvents({ query: auditQuery, limit: 200 });
      setAnalysis([response.summary, ...response.riskFindings].join("\n"));
      await load();
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const clearAudit = async () => {
    try {
      await clients.audit.clearAuditEvents({});
      await load();
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  return (
    <section className="page-grid">
      <header className="section-header full-span">
        <div>
          <h1>集群与审计</h1>
          <p>{status || `${nodes.length} 个节点 · ${events.length} 条审计事件`}</p>
        </div>
        <IconButton label="刷新" icon={RefreshCw} onClick={() => void load()} />
      </header>

      <div className="panel">
        <div className="panel-title"><Server size={18} /><span>节点接入</span></div>
        <Input label="节点名" value={pairForm.name} onChange={(name) => setPairForm({ ...pairForm, name })} />
        <Input label="Endpoint" value={pairForm.endpoint} onChange={(endpoint) => setPairForm({ ...pairForm, endpoint })} />
        <Input label="配对密钥" value={pairForm.pairingSecret} onChange={(pairingSecret) => setPairForm({ ...pairForm, pairingSecret })} />
        <button onClick={() => void pairNode()} type="button"><ShieldCheck size={15} />接入</button>
      </div>

      <div className="panel wide-panel">
        <div className="panel-title"><Server size={18} /><span>节点列表</span></div>
        <div className="table-list">
          {nodes.map((node) => (
            <div className="table-row" key={node.id}>
              <div>
                <strong>{node.name}</strong>
                <small>{node.endpoint} · heartbeat {new Date(Number(node.lastHeartbeatSeconds) * 1000).toLocaleString()}</small>
              </div>
              <StatusPill label={node.status || "unknown"} tone={node.status === "online" ? "good" : "muted"} />
              <IconButton label="心跳" icon={Activity} onClick={() => void sendHeartbeat(node)} />
            </div>
          ))}
          {!nodes.length && <div className="empty-state">暂无节点</div>}
        </div>
      </div>

      <div className="panel">
        <div className="panel-title"><Upload size={18} /><span>统一分发</span></div>
        <Input label="目标节点 ID" value={distributionForm.targetNodeId} onChange={(targetNodeId) => setDistributionForm({ ...distributionForm, targetNodeId })} />
        <Input label="目标路径" value={distributionForm.path} onChange={(path) => setDistributionForm({ ...distributionForm, path })} />
        <textarea
          className="pem-input code-input"
          value={distributionForm.content}
          onChange={(event) => setDistributionForm({ ...distributionForm, content: event.target.value })}
        />
        <button onClick={() => void distributeFile()} type="button"><FileUp size={15} />分发</button>
      </div>

      <div className="panel wide-panel">
        <div className="panel-title"><FileText size={18} /><span>分发记录</span></div>
        <div className="table-list">
          {records.map((record) => (
            <div className="table-row" key={record.id}>
              <div>
                <strong>{record.path}</strong>
                <small>{record.nodeId} · {record.message}</small>
              </div>
              <StatusPill label={record.status} tone={record.status === "delivered" ? "good" : "muted"} />
            </div>
          ))}
          {!records.length && <div className="empty-state">暂无分发记录</div>}
        </div>
      </div>

      <div className="panel full-span">
        <div className="panel-title"><ShieldAlert size={18} /><span>操作黑匣子</span></div>
        <div className="toolbar backup-actions">
          <Input label="关键词" value={auditQuery} onChange={setAuditQuery} />
          <button onClick={() => void load()} type="button"><RefreshCw size={15} />检索</button>
          <button onClick={() => void analyzeAudit()} type="button"><Activity size={15} />AI 分析</button>
          <button onClick={() => void clearAudit()} type="button"><Trash2 size={15} />清空</button>
        </div>
        <div className="table-list">
          {events.map((event) => (
            <div className="table-row audit-row" key={event.id}>
              <div>
                <strong>{event.module} · {event.action}</strong>
                <small>{event.description} · {event.sourceIp} · {new Date(Number(event.timestampSeconds) * 1000).toLocaleString()}</small>
              </div>
              <StatusPill label={event.level || "info"} tone={event.level === "warning" ? "danger" : "muted"} />
            </div>
          ))}
          {!events.length && <div className="empty-state">暂无审计事件</div>}
        </div>
      </div>

      <div className="panel full-span">
        <div className="panel-title"><ShieldCheck size={18} /><span>日志风险分析</span></div>
        <pre className="report-output">{analysis || "暂无分析结果"}</pre>
      </div>
    </section>
  );
}

function Metric({ label, value, detail }: { label: string; value: string; detail: string }) {
  return (
    <div className="rounded-xl border border-border/60 bg-card p-5 flex flex-col gap-1.5 shadow-sm transition-colors hover:border-border">
      <span className="text-xs uppercase tracking-wider text-muted-foreground font-medium">{label}</span>
      <span className="text-2xl font-semibold tracking-tight tabular-nums">{value}</span>
      <span className="text-xs text-muted-foreground">{detail}</span>
    </div>
  );
}

function StatusPill({ label, tone }: { label: string; tone: "good" | "danger" | "muted" }) {
  return <span className={`status-pill ${tone}`}>{label}</span>;
}

function IconButton({
  icon: Icon,
  label,
  onClick
}: {
  icon: typeof Activity;
  label: string;
  onClick: () => void;
}) {
  return (
    <button className="icon-button" onClick={onClick} title={label} type="button">
      <Icon size={16} />
    </button>
  );
}

function ToggleRow({
  checked,
  label,
  onChange
}: {
  checked: boolean;
  label: string;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="toggle-row">
      <span>{label}</span>
      <input checked={checked} onChange={(event) => onChange(event.target.checked)} type="checkbox" />
    </label>
  );
}

function SelectRow({
  label,
  onChange,
  options,
  value
}: {
  label: string;
  onChange: (value: string) => void;
  options: Array<[number, string]>;
  value: number;
}) {
  return (
    <label className="input-row">
      <span>{label}</span>
      <select onChange={(event) => onChange(event.target.value)} value={value}>
        {options.map(([optionValue, optionLabel]) => (
          <option key={optionValue} value={optionValue}>{optionLabel}</option>
        ))}
      </select>
    </label>
  );
}

function NumberInput({
  label,
  value,
  onChange
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
}) {
  return (
    <Input
      label={label}
      onChange={(nextValue) => onChange(Number(nextValue || 0))}
      type="number"
      value={String(value)}
    />
  );
}

function Input({
  label,
  type = "text",
  value,
  onChange
}: {
  label: string;
  type?: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <label className="input-row">
      <span>{label}</span>
      <input onChange={(event) => onChange(event.target.value)} type={type} value={value} />
    </label>
  );
}

function firewallProtocolLabel(protocol: FirewallProtocol): string {
  if (protocol === FirewallProtocol.TCP) return "TCP";
  if (protocol === FirewallProtocol.UDP) return "UDP";
  if (protocol === FirewallProtocol.ICMP) return "ICMP";
  return "-";
}

function firewallActionLabel(action: FirewallAction): string {
  if (action === FirewallAction.ALLOW) return "放行";
  if (action === FirewallAction.DENY) return "屏蔽";
  if (action === FirewallAction.REJECT) return "拒绝";
  return "-";
}

function firewallDirectionLabel(direction: FirewallDirection): string {
  if (direction === FirewallDirection.INBOUND) return "入站";
  if (direction === FirewallDirection.OUTBOUND) return "出站";
  return "-";
}

function wafKindLabel(kind: WafRuleKind): string {
  if (kind === WafRuleKind.CC) return "CC";
  if (kind === WafRuleKind.SQL_INJECTION) return "SQL 注入";
  if (kind === WafRuleKind.XSS) return "XSS";
  if (kind === WafRuleKind.KEYWORD) return "关键词";
  if (kind === WafRuleKind.SCANNER) return "扫描器";
  return "-";
}

function sshAlgorithmLabel(algorithm: SshKeyAlgorithm): string {
  if (algorithm === SshKeyAlgorithm.ED25519) return "Ed25519";
  if (algorithm === SshKeyAlgorithm.RSA) return "RSA";
  return "-";
}

function languageForPath(name: string): string {
  const extension = name.split(".").pop()?.toLowerCase();
  if (extension === "ts" || extension === "tsx") return "typescript";
  if (extension === "js" || extension === "jsx") return "javascript";
  if (extension === "rs") return "rust";
  if (extension === "go") return "go";
  if (extension === "py") return "python";
  if (extension === "php") return "php";
  if (extension === "json") return "json";
  if (extension === "css") return "css";
  if (extension === "html") return "html";
  if (extension === "sql") return "sql";
  if (extension === "md") return "markdown";
  if (extension === "yml" || extension === "yaml") return "yaml";
  return "plaintext";
}

function parentPath(path: string): string {
  if (path === "/") return "/";
  const parts = path.split("/").filter(Boolean);
  parts.pop();
  return `/${parts.join("/")}`;
}
