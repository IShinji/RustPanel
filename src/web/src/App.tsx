import {
  Activity,
  Archive,
  ArrowDownToLine,
  ArrowUpFromLine,
  BarChart3,
  Boxes,
  Clock,
  Cpu,
  Database,
  FileText,
  Folder,
  Globe,
  HardDrive,
  Info,
  LineChart as LineChartIcon,
  LogOut,
  Mail,
  MemoryStick,
  Network,
  Plus,
  Power,
  RefreshCw,
  ScrollText,
  Server,
  Settings as SettingsIcon,
  Shield,
  ShieldAlert,
  ShieldCheck,
  Store,
  TerminalSquare,
  Trash2,
  UserCircle2,
  Wifi,
  Bell
} from "lucide-react";
import { lazy, Suspense, useCallback, useEffect, useMemo, useState } from "react";

import {
  appStateVariant
} from "./lib/labels";
import { CapabilityRow } from "./components/software-store";
import type { ChartClickState } from "./components/monitor-chart";

// 首屏不需要的重依赖一律按需加载:图表(recharts+d3)、终端(xterm)、
// 编辑器(monaco)三块合计约占打包体积的一半,而它们分别只服务一个页面。
const MonitorChart = lazy(() => import("./components/monitor-chart"));
const WebTerminal = lazy(() => import("./components/web-terminal"));

import {
  InstalledApp
} from "./gen/rustpanel/v1/appstore_pb";
import {
  Capabilities,
  Ipv6Address,
  ReservedPort,
  ResourceBudget
} from "./gen/rustpanel/v1/capability_pb";
import { PendingRollbackAction } from "./gen/rustpanel/v1/rollback_pb";
import { ProcessResourceSnapshot, SystemStatus } from "./gen/rustpanel/v1/monitor_pb";
import { RuntimeModule } from "./gen/rustpanel/v1/system_pb";
import { Badge } from "./components/ui/badge";
import { Button as UIButton } from "./components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "./components/ui/card";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger
} from "./components/ui/dropdown-menu";
import { Input as UIInput } from "./components/ui/input";
import { Label as UILabel } from "./components/ui/label";
import { Progress } from "./components/ui/progress";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue
} from "./components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow as UITableRow
} from "./components/ui/table";
import { Tabs, TabsList, TabsTrigger } from "./components/ui/tabs";
import { ThemeToggle } from "./components/theme-toggle";
import { cn } from "./lib/utils";
import {
  clearAuthToken,
  clients,
  getAuthToken,
  onAuthChanged,
  setAuthToken,
  type Clients
} from "./lib/rpc";
import { formatBytes, formatDuration, formatPercent, safeError } from "./lib/format";
import {
  DockerApps,
  MicroPanel,
  SoftwareStorePage
} from "./pages/apps";
import {
  FileManager
} from "./pages/files";
import {
  SecurityPanel
} from "./pages/security";
import {
  SitesSsl
} from "./pages/sites";
import {
  AuditPage,
  ClusterAudit,
  FtpPage,
  SettingsPage
} from "./pages/settings";
import {
  CronPanel,
  DatabasePanel
} from "./pages/database";
import {
  BackupPage,
  NotificationPage,
  VsmtpAliasPage
} from "./pages/backup-notify";
import {
  AccessLogPage,
  DnsPage,
  ToolboxPage,
  UserPage
} from "./pages/ops";
import { useMonitorStore } from "./store/monitor";


type TabId =
  | "dashboard"
  | "sites"
  | "ftp"
  | "database"
  | "files"
  | "cron"
  | "appstore"
  | "vsmtp"
  | "docker"
  | "security"
  | "audit"
  | "cluster"
  | "terminal"
  | "micro"
  | "network"
  | "notifications"
  | "backup"
  | "users"
  | "toolbox"
  | "accesslog"
  | "dns"
  | "settings";
type NavGroup = "overview" | "host" | "resource" | "security" | "tools" | "system";
type NavTab = {
  id: TabId;
  label: string;
  icon: typeof Activity;
  group: NavGroup;
  modules?: string[];
};
type MonitorRange = "1h" | "24h" | "7d" | "custom";

const monitorRanges: Array<{ id: MonitorRange; label: string }> = [
  { id: "1h", label: "1h" },
  { id: "24h", label: "24h" },
  { id: "7d", label: "7d" },
  { id: "custom", label: "自定义" }
];
const tabs: NavTab[] = [
  { id: "dashboard", label: "仪表盘", icon: Activity, group: "overview" },
  { id: "sites", label: "网站", icon: Globe, group: "host", modules: ["sites", "ssl"] },
  { id: "ftp", label: "FTP", icon: HardDrive, group: "host" },
  { id: "database", label: "数据库", icon: Database, group: "host", modules: ["database"] },
  { id: "files", label: "文件", icon: Folder, group: "resource", modules: ["files"] },
  { id: "cron", label: "计划任务", icon: Clock, group: "resource", modules: ["cron"] },
  { id: "appstore", label: "软件商店", icon: Store, group: "resource", modules: ["appstore"] },
  { id: "vsmtp", label: "邮件别名", icon: Mail, group: "resource", modules: ["appstore"] },
  { id: "docker", label: "容器", icon: Boxes, group: "resource", modules: ["docker"] },
  { id: "security", label: "安全", icon: Shield, group: "security", modules: ["security"] },
  { id: "audit", label: "日志", icon: ScrollText, group: "security", modules: ["cluster"] },
  { id: "cluster", label: "集群", icon: Network, group: "security", modules: ["cluster"] },
  { id: "terminal", label: "终端", icon: TerminalSquare, group: "tools", modules: ["terminal"] },
  { id: "micro", label: "Micro", icon: Power, group: "tools", modules: ["static-sites", "workloads", "proxy"] },
  { id: "network", label: "网络与端口", icon: Wifi, group: "system" },
  { id: "notifications", label: "通知", icon: Bell, group: "system" },
  { id: "backup", label: "备份", icon: Archive, group: "tools" },
  { id: "toolbox", label: "工具箱", icon: HardDrive, group: "tools" },
  { id: "accesslog", label: "访问统计", icon: BarChart3, group: "tools" },
  { id: "dns", label: "DNS", icon: Globe, group: "tools" },
  { id: "users", label: "用户", icon: UserCircle2, group: "system" },
  { id: "settings", label: "面板设置", icon: SettingsIcon, group: "system" }
];

const navGroups: Array<{ id: NavGroup; label: string }> = [
  { id: "overview", label: "总览" },
  { id: "host", label: "主机" },
  { id: "resource", label: "资源" },
  { id: "security", label: "安全" },
  { id: "tools", label: "工具" },
  { id: "system", label: "系统" }
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
    <div className="login-shell">
      <Card className="w-full max-w-sm border-border/60 shadow-2xl backdrop-blur supports-[backdrop-filter]:bg-card/90">
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

/** 把 URL #hash 解析成 TabId。未知 hash 或空 → 返回 null,
 *  让 caller 决定回退到哪(通常是 dashboard)。 */
function tabIdFromHash(): TabId | null {
  const raw = window.location.hash.replace(/^#/, "").trim();
  if (!raw) return null;
  // 收紧:hash 必须正好是 TabId 之一,不接受 query string / 子路径
  const candidates: TabId[] = [
    "dashboard",
    "sites",
    "ftp",
    "database",
    "files",
    "cron",
    "appstore",
    "vsmtp",
    "docker",
    "security",
    "audit",
    "cluster",
    "terminal",
    "micro",
    "network",
    "settings"
  ];
  return (candidates as string[]).includes(raw) ? (raw as TabId) : null;
}

function AppShell({ onLogout }: { onLogout: () => void }) {
  // 路由:URL hash 是唯一真源,active 只是把它转成强类型 TabId。
  // 第一次渲染就读 hash,刷新页面能停在当前 Tab,深链接(/#sites)直达。
  const [active, setActiveState] = useState<TabId>(
    () => tabIdFromHash() ?? "dashboard"
  );
  const setActive = useCallback((next: TabId) => {
    setActiveState(next);
    // 只在和当前 hash 不同时写,避免 hashchange 自循环
    const currentHash = window.location.hash.replace(/^#/, "");
    if (currentHash !== next) {
      window.location.hash = next;
    }
  }, []);
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
    const refresh = () => {
      clients.system
        .listRuntimeModules({})
        .then((response) => setModules(response.modules))
        .catch(() => setModules([]));
    };
    refresh();
    // ModulesPanel toggle 后会 dispatch rustpanel:modules-changed,这里收到就
    // 重拉一次模块清单 → visibleTabs 重算 → 侧栏立刻反映新状态。
    const onChanged = () => refresh();
    window.addEventListener("rustpanel:modules-changed", onChanged);
    return () => window.removeEventListener("rustpanel:modules-changed", onChanged);
  }, []);

  // 路由 ↔ active 双向同步:浏览器前进 / 后退按钮(触发 hashchange)
  // 或用户手改 URL → setActiveState 跟进。setActive 那一侧写 hash
  // 已经做了同值短路,不会和这里形成循环。
  useEffect(() => {
    const onHashChange = () => {
      const next = tabIdFromHash();
      if (next) setActiveState(next);
    };
    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, []);

  useEffect(() => {
    if (!visibleTabs.some((tab) => tab.id === active)) {
      setActive("dashboard");
    }
  }, [active, visibleTabs, setActive]);

  const activeTab = visibleTabs.find((tab) => tab.id === active);
  const groupedTabs = useMemo(() => {
    return navGroups
      .map((group) => ({
        ...group,
        items: visibleTabs.filter((tab) => tab.group === group.id)
      }))
      .filter((group) => group.items.length > 0);
  }, [visibleTabs]);

  return (
    <div className="app-shell">
      <aside className="sidebar" aria-label="RustPanel navigation">
        <div className="brand">
          <div className="flex size-8 items-center justify-center rounded-md bg-primary/15 text-primary ring-1 ring-primary/20">
            <Server className="size-4" />
          </div>
          <div className="flex flex-col leading-tight">
            <span className="text-sm font-semibold tracking-tight">RustPanel</span>
            <span className="text-[11px] text-muted-foreground">控制面板</span>
          </div>
        </div>
        <nav className="nav-list flex flex-col">
          {groupedTabs.map((group) => (
            <div key={group.id} className="flex flex-col gap-0.5">
              {group.id !== "overview" && <div className="nav-group-title">{group.label}</div>}
              {group.items.map((tab) => {
                const Icon = tab.icon;
                const isActive = active === tab.id;
                return (
                  <button
                    key={tab.id}
                    onClick={() => setActive(tab.id)}
                    type="button"
                    className={cn("nav-item", isActive && "active")}
                  >
                    <Icon className="size-[18px] shrink-0" />
                    <span>{tab.label}</span>
                  </button>
                );
              })}
            </div>
          ))}
        </nav>
        <button className="nav-logout" onClick={onLogout} type="button">
          <LogOut className="size-[18px] shrink-0" />
          <span>退出登录</span>
        </button>
      </aside>

      <main className="min-w-0 flex flex-col overflow-hidden">
        <Topbar title={activeTab?.label ?? "仪表盘"} onLogout={onLogout} />
        <RollbackBanner clients={clients} />
        <div className="workspace flex-1 overflow-auto">
          {active === "dashboard" && <Dashboard clients={clients} />}
          {active === "sites" && <SitesSsl clients={clients} />}
          {active === "ftp" && <FtpPage />}
          {active === "database" && <DatabasePanel clients={clients} />}
          {active === "files" && (
            <FileManager
              clients={clients}
              openTerminal={(cwd) => {
                setTerminalCwd(cwd);
                setActive("terminal");
              }}
            />
          )}
          {active === "cron" && <CronPanel clients={clients} />}
          {active === "appstore" && <SoftwareStorePage clients={clients} />}
          {active === "vsmtp" && <VsmtpAliasPage clients={clients} />}
          {active === "docker" && <DockerApps clients={clients} />}
          {active === "security" && <SecurityPanel clients={clients} />}
          {active === "audit" && <AuditPage clients={clients} />}
          {active === "cluster" && <ClusterAudit clients={clients} />}
          {active === "terminal" && <TerminalPanel cwd={terminalCwd} />}
          {active === "micro" && <MicroPanel clients={clients} />}
          {active === "network" && <NetworkPage clients={clients} />}
          {active === "notifications" && <NotificationPage clients={clients} />}
          {active === "backup" && <BackupPage clients={clients} />}
          {active === "users" && <UserPage clients={clients} />}
          {active === "toolbox" && <ToolboxPage clients={clients} />}
          {active === "accesslog" && <AccessLogPage clients={clients} />}
          {active === "dns" && <DnsPage clients={clients} />}
          {active === "settings" && <SettingsPage clients={clients} onLogout={onLogout} />}
        </div>
      </main>
    </div>
  );
}

// Phase F: 30 秒自动回滚倒计时横幅。
// 任何调用了 rollback.scheduleRollback 的高风险动作(改 SSH 端口 / 防火墙
// 规则 / 面板端口)都会在这里产生倒计时,用户在到期前点"保留"才不会被
// 还原。空闲时不显示。
function RollbackBanner({ clients }: { clients: Clients }) {
  const [pending, setPending] = useState<PendingRollbackAction | null>(null);
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));

  useEffect(() => {
    let cancelled = false;
    const poll = async () => {
      try {
        const response = await clients.rollback.listPendingRollbacks({});
        if (cancelled) return;
        // 取最早过期的那个展示
        const sorted = [...response.actions].sort((a, b) =>
          Number(a.expiresAtSeconds - b.expiresAtSeconds)
        );
        setPending(sorted[0] ?? null);
      } catch {
        if (!cancelled) setPending(null);
      }
    };
    void poll();
    const interval = setInterval(poll, 3_000);
    const tick = setInterval(() => setNow(Math.floor(Date.now() / 1000)), 1_000);
    return () => {
      cancelled = true;
      clearInterval(interval);
      clearInterval(tick);
    };
  }, [clients]);

  if (!pending) return null;

  const remaining = Math.max(0, Number(pending.expiresAtSeconds) - now);
  const total = Math.max(
    1,
    Number(pending.expiresAtSeconds - pending.scheduledAtSeconds)
  );
  const percent = (remaining / total) * 100;

  const confirm = async () => {
    try {
      await clients.rollback.confirmRollback({ actionId: pending.actionId });
      setPending(null);
    } catch {
      // 容错:就算确认失败,下次轮询会更新
    }
  };

  return (
    <div
      role="alert"
      className="border-b border-warning/40 bg-warning/10 px-4 py-2 flex items-center gap-3 text-sm"
    >
      <ShieldAlert className="size-4 text-warning shrink-0" />
      <div className="flex flex-col flex-1 min-w-0">
        <span className="font-medium truncate">
          {pending.title} · 还剩 <span className="tabular-nums font-bold">{remaining}</span> 秒自动回滚
        </span>
        {pending.description && (
          <span className="text-xs text-muted-foreground truncate">
            {pending.description}
          </span>
        )}
        <div className="mt-1 h-1 w-full bg-warning/20 rounded">
          <div
            className="h-1 bg-warning rounded transition-all"
            style={{ width: `${percent}%` }}
          />
        </div>
      </div>
      <UIButton size="sm" onClick={() => void confirm()}>
        <ShieldCheck className="size-4" />
        保留(我能登录)
      </UIButton>
    </div>
  );
}

function Topbar({ title, onLogout }: { title: string; onLogout: () => void }) {
  return (
    <header className="flex h-14 items-center justify-between gap-4 border-b border-border bg-card/60 px-6 backdrop-blur">
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <span>RustPanel</span>
        <span className="text-border">/</span>
        <span className="font-medium text-foreground">{title}</span>
      </div>
      <div className="flex items-center gap-2">
        <ThemeToggle />
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <UIButton variant="ghost" size="icon" aria-label="账户">
              <UserCircle2 className="size-5" />
            </UIButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuLabel>账户</DropdownMenuLabel>
            <DropdownMenuSeparator />
            <DropdownMenuItem variant="destructive" onClick={onLogout}>
              <LogOut className="size-4" />
              <span>退出登录</span>
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </header>
  );
}

function Dashboard({ clients }: { clients: Clients }) {
  const current = useMonitorStore((state) => state.current);
  const history = useMonitorStore((state) => state.history);
  const setCurrent = useMonitorStore((state) => state.setCurrent);
  const [system, setSystem] = useState({ hostname: "-", os: "-", kernel: "-", arch: "-" });
  const [installedApps, setInstalledApps] = useState<InstalledApp[]>([]);
  const [budget, setBudget] = useState<ResourceBudget | undefined>(undefined);
  const [capabilities, setCapabilities] = useState<Capabilities | undefined>(undefined);
  const [error, setError] = useState("");
  const [range, setRange] = useState<MonitorRange>("1h");
  const [customStart, setCustomStart] = useState(() => toLocalInputValue(Date.now() - 60 * 60 * 1000));
  const [customEnd, setCustomEnd] = useState(() => toLocalInputValue(Date.now()));
  const [metricSamples, setMetricSamples] = useState<SystemStatus[]>([]);
  const [selectedTimestamp, setSelectedTimestamp] = useState<number>();
  const [processes, setProcesses] = useState<ProcessResourceSnapshot[]>([]);
  const [reportPeriod, setReportPeriod] = useState<"daily" | "weekly">("daily");
  const [healthReport, setHealthReport] = useState("");

  useEffect(() => {
    clients.appStore
      .listInstalledApps({})
      .then((response) => setInstalledApps(response.apps))
      .catch(() => setInstalledApps([]));
  }, [clients]);

  const loadBudget = useCallback(async () => {
    try {
      const response = await clients.capability.getResourceBudget({});
      setBudget(response.budget);
    } catch {
      // 后端未启用 / OpenVZ 沙箱场景下静默降级,Dashboard 其他卡片继续工作
    }
  }, [clients]);

  useEffect(() => {
    void loadBudget();
    void clients.capability
      .getCapabilities({})
      .then((response) => setCapabilities(response.capabilities))
      .catch(() => undefined);
    const interval = setInterval(() => void loadBudget(), 15_000);
    return () => clearInterval(interval);
  }, [clients, loadBudget]);

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

  const memoryPercent =
    current?.memory && current.memory.totalBytes > 0n
      ? (Number(current.memory.usedBytes) / Number(current.memory.totalBytes)) * 100
      : 0;
  const rootDisk = current?.disks?.find((disk) => disk.mountPoint === "/") ?? current?.disks?.[0];
  const diskUsed = rootDisk
    ? Number(rootDisk.totalSpaceBytes) - Number(rootDisk.availableSpaceBytes)
    : 0;
  const diskTotal = rootDisk ? Number(rootDisk.totalSpaceBytes) : 0;
  const diskPercent = diskTotal > 0 ? (diskUsed / diskTotal) * 100 : 0;
  const primaryNetwork = current?.networks?.find(
    (net) => net.interfaceName !== "lo" && !net.interfaceName.startsWith("docker")
  ) ?? current?.networks?.[0];
  const netRx = primaryNetwork ? Number(primaryNetwork.receivedBytes) : 0;
  const netTx = primaryNetwork ? Number(primaryNetwork.transmittedBytes) : 0;

  return (
    <section className="flex flex-col gap-5">
      <header className="flex items-start justify-between gap-4 flex-wrap">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-semibold tracking-tight m-0">仪表盘</h1>
          <p className="text-sm text-muted-foreground m-0">服务器实时状态总览</p>
        </div>
        <Badge variant={error ? "destructive" : "success"}>
          {error ? "离线" : "运行中"}
        </Badge>
      </header>

      <BudgetBars budget={budget} capabilities={capabilities} />

      <Card>
        <CardContent>
          <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-6 gap-x-6 gap-y-3 text-sm">
            <ServerInfoCell label="主机名" value={system.hostname} />
            <ServerInfoCell label="操作系统" value={system.os} />
            <ServerInfoCell label="内核" value={system.kernel} />
            <ServerInfoCell label="架构" value={system.arch} />
            <ServerInfoCell label="运行时间" value={formatDuration(current?.uptimeSeconds ?? 0)} />
            <ServerInfoCell
              label="负载"
              value={`${(current?.loadAverage?.oneMinute ?? 0).toFixed(2)} / ${(current?.loadAverage?.fiveMinutes ?? 0).toFixed(2)} / ${(current?.loadAverage?.fifteenMinutes ?? 0).toFixed(2)}`}
            />
          </div>
        </CardContent>
      </Card>

      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
        <MetricCard
          icon={Cpu}
          label="CPU 使用率"
          value={formatPercent(current?.cpuUsagePercent ?? 0)}
          detail={`${current?.cpuCores.length ?? 0} 核心`}
          percent={current?.cpuUsagePercent ?? 0}
        />
        <MetricCard
          icon={MemoryStick}
          label="内存使用"
          value={`${memoryPercent.toFixed(1)}%`}
          detail={`${formatBytes(current?.memory?.usedBytes ?? 0)} / ${formatBytes(current?.memory?.totalBytes ?? 0)}`}
          percent={memoryPercent}
        />
        <MetricCard
          icon={HardDrive}
          label="磁盘使用"
          value={diskTotal > 0 ? `${diskPercent.toFixed(1)}%` : "-"}
          detail={diskTotal > 0 ? `${formatBytes(BigInt(diskUsed))} / ${formatBytes(BigInt(diskTotal))}` : "无磁盘数据"}
          percent={diskPercent}
        />
        <NetworkMetricCard
          interfaceName={primaryNetwork?.interfaceName ?? "-"}
          receivedBytes={netRx}
          transmittedBytes={netTx}
        />
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
        <Card className="lg:col-span-2">
          <CardHeader className="border-b border-border [.border-b]:pb-3">
            <div className="flex items-center justify-between gap-3 flex-wrap">
              <div className="flex items-center gap-2">
                <LineChartIcon className="size-4 text-primary" />
                <CardTitle className="text-base">CPU / 内存趋势</CardTitle>
              </div>
              <div className="flex items-center gap-2">
                <Tabs value={range} onValueChange={(value) => setRange(value as MonitorRange)}>
                  <TabsList className="h-8">
                    {monitorRanges.map((item) => (
                      <TabsTrigger key={item.id} value={item.id} className="h-6 text-xs">
                        {item.label}
                      </TabsTrigger>
                    ))}
                  </TabsList>
                </Tabs>
                <UIButton
                  variant="ghost"
                  size="icon"
                  className="size-8"
                  aria-label="刷新历史"
                  onClick={() => void loadMetricHistory()}
                >
                  <RefreshCw className="size-4" />
                </UIButton>
              </div>
            </div>
          </CardHeader>
          <CardContent className="pt-4">
            {range === "custom" && (
              <div className="flex flex-wrap items-center gap-2 mb-3">
                <UIInput
                  aria-label="开始时间"
                  className="h-8 w-auto text-xs"
                  onChange={(event) => setCustomStart(event.target.value)}
                  type="datetime-local"
                  value={customStart}
                />
                <UIInput
                  aria-label="结束时间"
                  className="h-8 w-auto text-xs"
                  onChange={(event) => setCustomEnd(event.target.value)}
                  type="datetime-local"
                  value={customEnd}
                />
              </div>
            )}
            <Suspense
              fallback={<div className="h-[260px] flex items-center justify-center text-xs text-muted-foreground">图表加载中…</div>}
            >
              <MonitorChart data={chartData} onPointClick={handleChartClick} />
            </Suspense>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="pb-3 [.border-b]:pb-3 border-b border-border">
            <div className="flex items-center gap-2">
              <Boxes className="size-4 text-primary" />
              <CardTitle className="text-base">已安装软件</CardTitle>
            </div>
            <CardDescription>当前面板部署的应用与运行状态</CardDescription>
          </CardHeader>
          <CardContent className="pt-4">
            {installedApps.length === 0 ? (
              <div className="empty-state">尚未安装任何应用</div>
            ) : (
              <ul className="flex flex-col gap-2">
                {installedApps.slice(0, 8).map((app) => (
                  <li
                    key={app.slug}
                    className="flex items-center justify-between gap-2 rounded-md border border-border bg-card px-3 py-2"
                  >
                    <div className="flex flex-col min-w-0">
                      <span className="text-sm font-medium truncate">{app.appName}</span>
                      <span className="text-xs text-muted-foreground truncate">
                        {app.image} · {app.version || "-"}
                      </span>
                    </div>
                    <Badge variant={appStateVariant(app.state)}>{app.state || "unknown"}</Badge>
                  </li>
                ))}
              </ul>
            )}
          </CardContent>
        </Card>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
        <Card className="lg:col-span-2">
          <CardHeader className="border-b border-border [.border-b]:pb-3">
            <div className="flex items-center justify-between gap-2 flex-wrap">
              <div className="flex items-center gap-2">
                <Server className="size-4 text-primary" />
                <CardTitle className="text-base">异常时刻进程</CardTitle>
              </div>
              <span className="text-xs text-muted-foreground">{selectedLabel}</span>
            </div>
          </CardHeader>
          <CardContent className="pt-4">
            {processes.length === 0 ? (
              <div className="empty-state">点击趋势图查看该时刻进程资源</div>
            ) : (
              <Table>
                <TableHeader>
                  <UITableRow>
                    <TableHead>进程</TableHead>
                    <TableHead className="text-right">CPU</TableHead>
                    <TableHead className="text-right">内存</TableHead>
                  </UITableRow>
                </TableHeader>
                <TableBody>
                  {processes.map((process) => (
                    <UITableRow key={`${process.pid}-${process.name}`}>
                      <TableCell>
                        <div className="flex flex-col">
                          <span className="font-medium">{process.name || process.pid}</span>
                          <span className="text-xs text-muted-foreground">PID {process.pid}</span>
                        </div>
                      </TableCell>
                      <TableCell className="text-right">{formatPercent(process.cpuUsagePercent)}</TableCell>
                      <TableCell className="text-right">{formatBytes(process.memoryBytes)}</TableCell>
                    </UITableRow>
                  ))}
                </TableBody>
              </Table>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="pb-3 [.border-b]:pb-3 border-b border-border">
            <div className="flex items-center gap-2">
              <FileText className="size-4 text-primary" />
              <CardTitle className="text-base">运行报告</CardTitle>
            </div>
            <CardDescription>面板自动汇总日报或周报</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-3 pt-4">
            <div className="flex gap-2">
              <Select value={reportPeriod} onValueChange={(value) => setReportPeriod(value as "daily" | "weekly")}>
                <SelectTrigger className="h-8 flex-1">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="daily">日报</SelectItem>
                  <SelectItem value="weekly">周报</SelectItem>
                </SelectContent>
              </Select>
              <UIButton size="sm" onClick={() => void generateReport()}>
                <RefreshCw className="size-3.5" />
                生成
              </UIButton>
            </div>
            <pre className="report-output text-xs">{healthReport || "暂无报告"}</pre>
          </CardContent>
        </Card>
      </div>
    </section>
  );
}

function ServerInfoCell({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col gap-0.5 min-w-0">
      <span className="text-xs text-muted-foreground">{label}</span>
      <span className="font-medium text-foreground truncate" title={value}>
        {value}
      </span>
    </div>
  );
}

function MetricCard({
  icon: Icon,
  label,
  value,
  detail,
  percent
}: {
  icon: typeof Cpu;
  label: string;
  value: string;
  detail: string;
  percent: number;
}) {
  const clamped = Math.max(0, Math.min(100, percent));
  return (
    <Card>
      <CardContent>
        <div className="flex items-center justify-between gap-2 mb-2">
          <span className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
            {label}
          </span>
          <Icon className="size-4 text-primary" />
        </div>
        <div className="text-2xl font-semibold tracking-tight text-foreground">{value}</div>
        <div className="text-xs text-muted-foreground mt-1 mb-3 truncate">{detail}</div>
        <Progress value={clamped} className="h-1.5" />
      </CardContent>
    </Card>
  );
}

function NetworkMetricCard({
  interfaceName,
  receivedBytes,
  transmittedBytes
}: {
  interfaceName: string;
  receivedBytes: number;
  transmittedBytes: number;
}) {
  return (
    <Card>
      <CardContent>
        <div className="flex items-center justify-between gap-2 mb-2">
          <span className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
            网络吞吐
          </span>
          <Wifi className="size-4 text-primary" />
        </div>
        <div className="text-sm text-muted-foreground truncate" title={interfaceName}>
          {interfaceName}
        </div>
        <div className="flex flex-col gap-1 mt-2">
          <div className="flex items-center justify-between text-sm">
            <div className="flex items-center gap-1.5 text-muted-foreground">
              <ArrowDownToLine className="size-3.5 text-info" />
              <span>下行</span>
            </div>
            <span className="font-medium text-foreground">{formatBytes(BigInt(receivedBytes))}</span>
          </div>
          <div className="flex items-center justify-between text-sm">
            <div className="flex items-center gap-1.5 text-muted-foreground">
              <ArrowUpFromLine className="size-3.5 text-warning" />
              <span>上行</span>
            </div>
            <span className="font-medium text-foreground">{formatBytes(BigInt(transmittedBytes))}</span>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

// ====== Phase A: 资源预算条 + 主机能力提示 ======
function BudgetBars({
  budget,
  capabilities
}: {
  budget?: ResourceBudget;
  capabilities?: Capabilities;
}) {
  const memory = budget?.memory;
  const memoryPercent =
    memory && memory.totalBytes > 0n
      ? (Number(memory.usedBytes) / Number(memory.totalBytes)) * 100
      : 0;
  const rootDisk =
    budget?.disks?.find((disk) => disk.mountPoint === "/") ?? budget?.disks?.[0];
  const diskPercent =
    rootDisk && rootDisk.totalBytes > 0n
      ? (Number(rootDisk.usedBytes) / Number(rootDisk.totalBytes)) * 100
      : 0;
  const ports = budget?.ports;
  const portTotal = ports?.total ?? 0;
  const portReserved = ports?.reserved ?? 0;
  const portPercent = portTotal > 0 ? (portReserved / portTotal) * 100 : 0;

  return (
    <Card>
      <CardContent className="flex flex-col gap-3">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Activity className="size-4 text-primary" />
            <span className="text-sm font-medium">资源预算</span>
          </div>
          {capabilities?.isOpenvz && (
            <Badge variant="warning" title={capabilities.dockerBlockReason || "OpenVZ 容器"}>
              OpenVZ
            </Badge>
          )}
        </div>

        <BudgetRow
          label="内存"
          icon={MemoryStick}
          percent={memoryPercent}
          detail={
            memory
              ? `${formatBytes(memory.usedBytes)} / ${formatBytes(memory.totalBytes)}`
              : "-"
          }
          warnAt={75}
          dangerAt={90}
        />
        <BudgetRow
          label="磁盘"
          icon={HardDrive}
          percent={diskPercent}
          detail={
            rootDisk
              ? `${formatBytes(rootDisk.usedBytes)} / ${formatBytes(rootDisk.totalBytes)}  ·  ${rootDisk.mountPoint}`
              : "-"
          }
          warnAt={80}
          dangerAt={92}
        />
        <BudgetRow
          label="NAT 端口"
          icon={Wifi}
          percent={portPercent}
          detail={portTotal > 0 ? `${portReserved} / ${portTotal} 已预留` : "未配置 NAT 端口预算"}
          warnAt={70}
          dangerAt={90}
        />

        {capabilities && !capabilities.canRunDocker && (
          <div className="rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-xs text-warning-foreground flex items-center gap-2">
            <Info className="size-3.5" />
            <span>
              Docker 在本机不可用 —— {capabilities.dockerBlockReason || "缺少必要内核能力"}
            </span>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function BudgetRow({
  label,
  icon: Icon,
  percent,
  detail,
  warnAt,
  dangerAt
}: {
  label: string;
  icon: typeof Activity;
  percent: number;
  detail: string;
  warnAt: number;
  dangerAt: number;
}) {
  const clamped = Math.max(0, Math.min(100, percent));
  const tone =
    clamped >= dangerAt ? "danger" : clamped >= warnAt ? "warn" : "ok";
  const indicatorClass =
    tone === "danger"
      ? "[&>[data-slot=progress-indicator]]:bg-destructive"
      : tone === "warn"
        ? "[&>[data-slot=progress-indicator]]:bg-warning"
        : "[&>[data-slot=progress-indicator]]:bg-primary";

  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center justify-between text-xs">
        <div className="flex items-center gap-1.5 text-muted-foreground">
          <Icon className="size-3.5" />
          <span>{label}</span>
        </div>
        <div className="flex items-center gap-2 tabular-nums">
          <span className="font-medium text-foreground">{clamped.toFixed(1)}%</span>
          <span className="text-muted-foreground">{detail}</span>
        </div>
      </div>
      <Progress value={clamped} className={cn("h-1.5", indicatorClass)} />
    </div>
  );
}

// ====== Phase A: 网络与端口管理页 ======

function NetworkPage({ clients }: { clients: Clients }) {
  const [budget, setBudget] = useState<ResourceBudget | undefined>(undefined);
  const [capabilities, setCapabilities] = useState<Capabilities | undefined>(undefined);
  const [reservedPorts, setReservedPorts] = useState<ReservedPort[]>([]);
  const [ipv6Addresses, setIpv6Addresses] = useState<Ipv6Address[]>([]);
  const [ipv6Prefixes, setIpv6Prefixes] = useState<string[]>([]);
  const [reserveForm, setReserveForm] = useState({
    port: "",
    owner: "",
    description: "",
    protocol: "tcp"
  });
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");

  const refresh = useCallback(async () => {
    try {
      const [budgetResp, capResp, portsResp, ipv6Resp] = await Promise.all([
        clients.capability.getResourceBudget({}),
        clients.capability.getCapabilities({}),
        clients.capability.listReservedPorts({}),
        clients.capability.listIpv6Addresses({})
      ]);
      setBudget(budgetResp.budget);
      setCapabilities(capResp.capabilities);
      setReservedPorts(portsResp.ports);
      setIpv6Addresses(ipv6Resp.addresses);
      setIpv6Prefixes(ipv6Resp.prefixes);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  }, [clients]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const reservePort = async () => {
    const port = Number.parseInt(reserveForm.port, 10);
    if (!Number.isFinite(port) || port < 1 || port > 65535) {
      setError("端口号需为 1-65535");
      return;
    }
    if (!reserveForm.owner.trim()) {
      setError("请填写预留方");
      return;
    }
    try {
      await clients.capability.reservePort({
        port,
        owner: reserveForm.owner,
        description: reserveForm.description,
        protocol: reserveForm.protocol
      });
      setReserveForm({ port: "", owner: "", description: "", protocol: "tcp" });
      setMessage(`端口 ${port} 已预留`);
      setError("");
      void refresh();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const releasePort = async (port: number) => {
    try {
      await clients.capability.releasePort({ port });
      setMessage(`端口 ${port} 已释放`);
      void refresh();
    } catch (err) {
      setError(safeError(err));
    }
  };

  return (
    <section className="flex flex-col gap-5">
      <header className="flex items-start justify-between gap-4 flex-wrap">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-semibold tracking-tight m-0">网络与端口</h1>
          <p className="text-sm text-muted-foreground m-0">
            管理 NAT VPS 的 20 个公网端口预算 + 公网 IPv6 地址池
          </p>
        </div>
        <UIButton variant="outline" size="sm" onClick={() => void refresh()}>
          <RefreshCw className="size-4" />
          刷新
        </UIButton>
      </header>

      <BudgetBars budget={budget} capabilities={capabilities} />

      {error && (
        <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
          {error}
        </div>
      )}
      {message && !error && (
        <div className="rounded-md border border-success/40 bg-success/10 px-3 py-2 text-sm text-success whitespace-pre-line">
          {message}
        </div>
      )}

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Wifi className="size-4 text-primary" />
            NAT 端口预算
          </CardTitle>
          <CardDescription>
            登记每个端口给了谁用,避免装新软件时撞端口。建议把面板/SSH/已上线服务都登记一遍。
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <div className="grid gap-3 md:grid-cols-[120px_1fr_1fr_120px_auto] md:items-end">
            <div className="grid gap-1">
              <UILabel htmlFor="port-num">端口</UILabel>
              <UIInput
                id="port-num"
                type="number"
                min={1}
                max={65535}
                value={reserveForm.port}
                onChange={(event) =>
                  setReserveForm((prev) => ({ ...prev, port: event.target.value }))
                }
              />
            </div>
            <div className="grid gap-1">
              <UILabel htmlFor="port-owner">预留方</UILabel>
              <UIInput
                id="port-owner"
                placeholder="例如 panel / site:my-blog"
                value={reserveForm.owner}
                onChange={(event) =>
                  setReserveForm((prev) => ({ ...prev, owner: event.target.value }))
                }
              />
            </div>
            <div className="grid gap-1">
              <UILabel htmlFor="port-desc">说明</UILabel>
              <UIInput
                id="port-desc"
                placeholder="可选"
                value={reserveForm.description}
                onChange={(event) =>
                  setReserveForm((prev) => ({ ...prev, description: event.target.value }))
                }
              />
            </div>
            <div className="grid gap-1">
              <UILabel htmlFor="port-proto">协议</UILabel>
              <Select
                value={reserveForm.protocol}
                onValueChange={(value) =>
                  setReserveForm((prev) => ({ ...prev, protocol: value }))
                }
              >
                <SelectTrigger id="port-proto">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="tcp">TCP</SelectItem>
                  <SelectItem value="udp">UDP</SelectItem>
                  <SelectItem value="both">TCP + UDP</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <UIButton onClick={() => void reservePort()}>
              <Plus className="size-4" />
              预留
            </UIButton>
          </div>

          {reservedPorts.length === 0 ? (
            <div className="empty-state text-sm">尚未登记任何端口</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>端口</TableHead>
                  <TableHead>协议</TableHead>
                  <TableHead>预留方</TableHead>
                  <TableHead>说明</TableHead>
                  <TableHead>登记时间</TableHead>
                  <TableHead className="text-right">操作</TableHead>
                </UITableRow>
              </TableHeader>
              <TableBody>
                {reservedPorts.map((port) => (
                  <UITableRow key={port.port}>
                    <TableCell className="font-mono">{port.port}</TableCell>
                    <TableCell>
                      <Badge variant="muted">{port.protocol || "tcp"}</Badge>
                    </TableCell>
                    <TableCell className="font-medium">{port.owner}</TableCell>
                    <TableCell className="text-muted-foreground">{port.description || "-"}</TableCell>
                    <TableCell className="text-xs text-muted-foreground">
                      {port.reservedAtSeconds > 0n
                        ? new Date(Number(port.reservedAtSeconds) * 1000).toLocaleString()
                        : "-"}
                    </TableCell>
                    <TableCell className="text-right">
                      <UIButton
                        variant="ghost"
                        size="sm"
                        onClick={() => void releasePort(port.port)}
                      >
                        <Trash2 className="size-3.5" />
                        释放
                      </UIButton>
                    </TableCell>
                  </UITableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Globe className="size-4 text-primary" />
            公网 IPv6 地址池
          </CardTitle>
          <CardDescription>
            NAT VPS 上 IPv6 是绕过 20 端口约束的关键 —— 每个站点直接绑一个 v6,无需占用 NAT 端口。
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          {ipv6Prefixes.length > 0 && (
            <div className="rounded-md border border-info/40 bg-info/10 px-3 py-2 text-sm">
              <div className="font-medium text-info mb-1">检测到的公网前缀</div>
              <div className="flex flex-wrap gap-2">
                {ipv6Prefixes.map((prefix) => (
                  <Badge key={prefix} variant="info" className="font-mono">
                    {prefix}
                  </Badge>
                ))}
              </div>
            </div>
          )}
          {ipv6Addresses.length === 0 ? (
            <div className="empty-state text-sm">未检测到公网 IPv6 地址(可能未开启 IPv6 或处于 link-local 模式)</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>地址</TableHead>
                  <TableHead>前缀</TableHead>
                  <TableHead>接口</TableHead>
                  <TableHead>类型</TableHead>
                </UITableRow>
              </TableHeader>
              <TableBody>
                {ipv6Addresses.map((addr) => (
                  <UITableRow key={`${addr.address}-${addr.interfaceName}`}>
                    <TableCell className="font-mono text-xs">{addr.address}</TableCell>
                    <TableCell className="font-mono">/{addr.prefixLength}</TableCell>
                    <TableCell>{addr.interfaceName}</TableCell>
                    <TableCell>
                      <Badge variant={addr.isGlobal ? "success" : "muted"}>
                        {addr.isGlobal ? "公网" : "本地"}
                      </Badge>
                    </TableCell>
                  </UITableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>

      {capabilities && (
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Info className="size-4 text-primary" />
              主机能力探测
            </CardTitle>
            <CardDescription>开机探测一次,1 小时刷新一次</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-x-6 gap-y-3 text-sm">
              <CapabilityRow label="OpenVZ 容器" value={capabilities.isOpenvz} />
              <CapabilityRow label="Docker / LXC 内" value={capabilities.isContainer} />
              <CapabilityRow label="Docker 守护进程" value={capabilities.dockerRunning} />
              <CapabilityRow label="Docker 可用" value={capabilities.canRunDocker} />
              <CapabilityRow label="overlay2 文件系统" value={capabilities.hasOverlay2} />
              <CapabilityRow label="FUSE" value={capabilities.hasFuse} />
              <CapabilityRow label="iptables 二进制" value={capabilities.hasIptables} />
              <CapabilityRow label="nf_nat 模块" value={capabilities.hasNfNat} />
              <CapabilityRow label="Swap 分区" value={capabilities.hasSwap} />
              <CapabilityRow label="BBR 拥塞控制" value={capabilities.hasBbr} />
              <CapabilityRow label="cgroups v2" value={capabilities.hasCgroupsV2} />
              <CapabilityRow label="user namespaces" value={capabilities.hasUserNamespaces} />
            </div>
            {capabilities.kernelVersion && (
              <div className="mt-4 text-xs text-muted-foreground">
                内核:<span className="font-mono">{capabilities.kernelVersion}</span>
              </div>
            )}
          </CardContent>
        </Card>
      )}
    </section>
  );
}

// ====== Phase B: 软件商店四组分类 ======

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

function TerminalPanel({ cwd }: { cwd: string }) {
  return (
    <Suspense
      fallback={<section className="page-grid terminal-layout"><p className="text-xs text-muted-foreground">终端加载中…</p></section>}
    >
      <WebTerminal cwd={cwd} />
    </Suspense>
  );
}

