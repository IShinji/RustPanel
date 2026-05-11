import Editor from "@monaco-editor/react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal as XTerm } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import {
  Activity,
  Archive,
  ArrowLeftRight,
  ArrowDownToLine,
  ArrowUpFromLine,
  Ban,
  Boxes,
  ChevronDown,
  Clock,
  Copy,
  Cpu,
  Database,
  Download,
  FileDown,
  FileText,
  ExternalLink,
  FileUp,
  Folder,
  FolderPlus,
  Globe,
  HardDrive,
  Info,
  LineChart as LineChartIcon,
  LogOut,
  Mail,
  MemoryStick,
  Network,
  Pause,
  Play,
  Plus,
  Power,
  RefreshCw,
  RotateCw,
  Save,
  ScrollText,
  Server,
  Settings as SettingsIcon,
  Shield,
  ShieldAlert,
  ShieldCheck,
  Square,
  Store,
  TerminalSquare,
  Trash2,
  Upload,
  UserCircle2,
  Wifi
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

import {
  AppCategory,
  AppTemplate,
  CompatibilityStatus,
  InstallMethod,
  InstalledApp
} from "./gen/rustpanel/v1/appstore_pb";
import { VsmtpAlias } from "./gen/rustpanel/v1/vsmtp_pb";
import { AuditEvent } from "./gen/rustpanel/v1/audit_pb";
import {
  Capabilities,
  Ipv6Address,
  ReservedPort,
  ResourceBudget
} from "./gen/rustpanel/v1/capability_pb";
import { RedisInfo, SqliteFile } from "./gen/rustpanel/v1/db_pb";
import { PendingRollbackAction } from "./gen/rustpanel/v1/rollback_pb";
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
import {
  ReverseProxyRule,
  RewriteTemplate,
  SiteBindKind,
  SiteItem,
  SiteKind,
  SiteTlsStrategy
} from "./gen/rustpanel/v1/site_pb";
import {
  AcmeChallengeType,
  CertificateItem,
  RequestCertificateResponse
} from "./gen/rustpanel/v1/ssl_pb";
import { RuntimeModule } from "./gen/rustpanel/v1/system_pb";
import { WorkloadItem, WorkloadState } from "./gen/rustpanel/v1/workload_pb";
import { Badge } from "./components/ui/badge";
import { Button as UIButton } from "./components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "./components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger
} from "./components/ui/dialog";
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
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle
} from "./components/ui/sheet";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue
} from "./components/ui/select";
import { Switch } from "./components/ui/switch";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow as UITableRow
} from "./components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "./components/ui/tabs";
import { ThemeToggle } from "./components/theme-toggle";
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
            <ResponsiveContainer width="100%" height={260}>
              <LineChart data={chartData} onClick={(state) => handleChartClick(state as ChartClickState)}>
                <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
                <XAxis dataKey="time" minTickGap={24} stroke="var(--muted-foreground)" fontSize={12} />
                <YAxis
                  domain={[0, 100]}
                  stroke="var(--muted-foreground)"
                  fontSize={12}
                  tickFormatter={(value) => `${Math.round(Number(value))}%`}
                  width={40}
                />
                <Tooltip
                  contentStyle={{
                    background: "var(--popover)",
                    border: "1px solid var(--border)",
                    borderRadius: 8,
                    color: "var(--popover-foreground)"
                  }}
                  formatter={(value) => {
                    const num = typeof value === "number" ? value : Number(value);
                    return Number.isFinite(num) ? `${num.toFixed(1)}%` : String(value);
                  }}
                />
                <Line dataKey="cpu" dot={false} stroke="var(--chart-1)" strokeWidth={2} name="CPU" />
                <Line dataKey="memory" dot={false} stroke="var(--chart-2)" strokeWidth={2} name="内存" />
              </LineChart>
            </ResponsiveContainer>
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

function appStateVariant(
  state: string
): "success" | "destructive" | "warning" | "muted" {
  const normalized = state.toLowerCase();
  if (normalized.includes("running") || normalized === "up" || normalized === "active") return "success";
  if (normalized.includes("error") || normalized.includes("fail") || normalized === "dead") return "destructive";
  if (normalized.includes("paused") || normalized.includes("restart") || normalized.includes("starting")) return "warning";
  return "muted";
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
        <div className="rounded-md border border-success/40 bg-success/10 px-3 py-2 text-sm text-success">
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

function SoftwareStore({
  templates,
  selectedVersions,
  onVersionChange,
  onDeploy
}: {
  templates: AppTemplate[];
  selectedVersions: Record<string, string>;
  onVersionChange: (slug: string, version: string) => void;
  onDeploy: (template: AppTemplate) => void;
}) {
  const groups: Array<{
    key: CompatibilityStatus;
    title: string;
    description: string;
    tone: "ok" | "warn" | "muted";
    defaultOpen: boolean;
  }> = [
    {
      key: CompatibilityStatus.COMPATIBLE,
      title: "可用",
      description: "硬件和内核都满足,可以直接安装",
      tone: "ok",
      defaultOpen: true
    },
    {
      key: CompatibilityStatus.RESOURCE_SHORT,
      title: "资源不足",
      description: "RAM 或磁盘不够,装上也很可能跑不起来",
      tone: "warn",
      defaultOpen: false
    },
    {
      key: CompatibilityStatus.KERNEL_UNSUPPORTED,
      title: "内核不支持",
      description: "OpenVZ 等受限内核缺必要能力,无法工作",
      tone: "muted",
      defaultOpen: false
    },
    {
      key: CompatibilityStatus.NEEDS_DOCKER,
      title: "需要 Docker",
      description: "走容器路线,本机 Docker 不可用时折叠",
      tone: "muted",
      defaultOpen: false
    }
  ];

  return (
    <div className="panel full-span flex flex-col gap-4">
      <div className="flex items-center gap-2">
        <Store className="size-4 text-primary" />
        <span className="font-semibold">软件商店</span>
        <span className="text-xs text-muted-foreground">
          按当前主机能力分组 · 共 {templates.length} 个模板
        </span>
      </div>

      {groups.map((group) => {
        const items = templates.filter((tpl) => tpl.compatibility === group.key);
        if (items.length === 0) return null;
        return (
          <SoftwareGroup
            key={group.key}
            title={group.title}
            description={group.description}
            tone={group.tone}
            count={items.length}
            defaultOpen={group.defaultOpen}
            templates={items}
            selectedVersions={selectedVersions}
            onVersionChange={onVersionChange}
            onDeploy={onDeploy}
          />
        );
      })}
    </div>
  );
}

function SoftwareGroup({
  title,
  description,
  tone,
  count,
  defaultOpen,
  templates,
  selectedVersions,
  onVersionChange,
  onDeploy
}: {
  title: string;
  description: string;
  tone: "ok" | "warn" | "muted";
  count: number;
  defaultOpen: boolean;
  templates: AppTemplate[];
  selectedVersions: Record<string, string>;
  onVersionChange: (slug: string, version: string) => void;
  onDeploy: (template: AppTemplate) => void;
}) {
  const [open, setOpen] = useState(defaultOpen);
  const badgeVariant: "success" | "warning" | "muted" =
    tone === "ok" ? "success" : tone === "warn" ? "warning" : "muted";

  return (
    <div className="rounded-lg border border-border">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        className="flex w-full items-center justify-between gap-3 px-4 py-3 text-left hover:bg-accent/50 transition-colors"
      >
        <div className="flex items-center gap-2">
          <ChevronDown
            className={cn(
              "size-4 text-muted-foreground transition-transform",
              !open && "-rotate-90"
            )}
          />
          <span className="font-medium">{title}</span>
          <Badge variant={badgeVariant}>{count}</Badge>
        </div>
        <span className="text-xs text-muted-foreground hidden sm:block">{description}</span>
      </button>
      {open && (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3 p-4 border-t border-border">
          {templates.map((template) => (
            <SoftwareCard
              key={template.slug}
              template={template}
              version={selectedVersions[template.slug] || template.defaultVersion}
              onVersionChange={(version) => onVersionChange(template.slug, version)}
              onDeploy={() => onDeploy(template)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function SoftwareCard({
  template,
  version,
  onVersionChange,
  onDeploy
}: {
  template: AppTemplate;
  version: string;
  onVersionChange: (version: string) => void;
  onDeploy: () => void;
}) {
  const isCompatible = template.compatibility === CompatibilityStatus.COMPATIBLE;
  return (
    <div className="flex flex-col gap-2 rounded-md border border-border bg-card p-3">
      <div className="flex items-start justify-between gap-2">
        <div className="flex flex-col min-w-0 gap-0.5">
          <div className="flex items-center gap-1.5 flex-wrap">
            <span className="font-medium truncate">{template.name}</span>
            {template.recommended && <Badge variant="info">推荐</Badge>}
          </div>
          <span className="text-xs text-muted-foreground truncate">
            {appCategoryLabel(template.category)} · {installMethodLabel(template.installMethod)}
          </span>
        </div>
        {template.homepage && (
          <a
            href={template.homepage}
            target="_blank"
            rel="noopener noreferrer"
            className="text-muted-foreground hover:text-foreground transition-colors shrink-0"
            title={`官网: ${template.homepage}`}
          >
            <ExternalLink className="size-3.5" />
          </a>
        )}
      </div>
      <p className="text-xs text-muted-foreground line-clamp-2">{template.description}</p>
      <div className="flex flex-wrap gap-1 text-[11px] text-muted-foreground">
        {template.minRamMb > 0 && <Badge variant="muted">RAM ≥ {template.minRamMb} MB</Badge>}
        {template.minDiskMb > 0 && <Badge variant="muted">Disk ≥ {template.minDiskMb} MB</Badge>}
        {template.expectedRuntimeRamMb > 0 && (
          <Badge variant="outline">运行 ~{template.expectedRuntimeRamMb} MB</Badge>
        )}
      </div>
      {!isCompatible && template.compatibilityReason && (
        <div className="text-xs text-warning bg-warning/10 rounded px-2 py-1">
          {template.compatibilityReason}
        </div>
      )}
      {template.versions.length > 0 && (
        <Select value={version} onValueChange={onVersionChange}>
          <SelectTrigger className="h-8">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {template.versions.map((v) => (
              <SelectItem key={v.version} value={v.version}>
                {v.version}
                {v.recommended && " (推荐)"}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}
      <UIButton
        size="sm"
        disabled={!isCompatible}
        onClick={onDeploy}
        variant={isCompatible ? "default" : "outline"}
      >
        {isCompatible ? (
          <>
            <Play className="size-3.5" />
            安装
          </>
        ) : (
          "暂不可用"
        )}
      </UIButton>
    </div>
  );
}

function appCategoryLabel(category: AppCategory): string {
  switch (category) {
    case AppCategory.WEB_SERVER:
      return "Web 服务";
    case AppCategory.DATABASE:
      return "数据库";
    case AppCategory.RUNTIME:
      return "运行时";
    case AppCategory.TOOL:
      return "工具";
    case AppCategory.VPN:
      return "VPN";
    case AppCategory.MONITOR:
      return "监控";
    default:
      return "其他";
  }
}

function installMethodLabel(method: InstallMethod): string {
  switch (method) {
    case InstallMethod.NATIVE_PACKAGE:
      return "apt 包";
    case InstallMethod.BINARY_DOWNLOAD:
      return "二进制";
    case InstallMethod.CARGO_INSTALL:
      return "cargo install";
    case InstallMethod.DOCKER_COMPOSE:
      return "Docker";
    default:
      return "未指定";
  }
}

function CapabilityRow({ label, value }: { label: string; value: boolean }) {
  return (
    <div className="flex items-center justify-between gap-2">
      <span className="text-muted-foreground">{label}</span>
      <Badge variant={value ? "success" : "muted"}>{value ? "可用" : "不可用"}</Badge>
    </div>
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

// Phase B 后续修补:把"软件商店"从 DockerApps 抽成独立页,不再跟着
// Docker daemon 一起挂掉。OpenVZ 上 docker.* 全部 reject 也能正常使用。
function SoftwareStorePage({ clients }: { clients: Clients }) {
  const [templates, setTemplates] = useState<AppTemplate[]>([]);
  const [installedApps, setInstalledApps] = useState<InstalledApp[]>([]);
  const [selectedVersions, setSelectedVersions] = useState<Record<string, string>>({});
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  // BinaryDownload 安装时后端在 composeYaml 字段塞的人话计划摘要,
  // 单独存一份用 <pre> 渲染,避免和单行 message 抢空间。
  const [installSummary, setInstallSummary] = useState("");

  const load = useCallback(async () => {
    try {
      const [tplResp, installedResp] = await Promise.all([
        clients.appStore.listAppTemplates({}),
        clients.appStore.listInstalledApps({}).catch(() => ({ apps: [] as InstalledApp[] }))
      ]);
      setTemplates(tplResp.templates);
      setInstalledApps(installedResp.apps);
      setSelectedVersions((current) => ({
        ...Object.fromEntries(
          tplResp.templates.map((tpl) => [tpl.slug, tpl.defaultVersion])
        ),
        ...current
      }));
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  }, [clients]);

  useEffect(() => {
    void load();
  }, [load]);

  const deployTemplate = async (template: AppTemplate) => {
    try {
      const version = selectedVersions[template.slug] || template.defaultVersion;
      const response = await clients.appStore.deployApp({
        slug: template.slug,
        appName: `${template.slug}-${version || "default"}`.replaceAll(".", "-"),
        version
      });
      setMessage(`${template.name} 已开始部署`);
      // BinaryDownload 路径下 composeYaml 是"上游/版本/asset/装到哪/下一步"
      // 的人话摘要;Docker 路径下它是真的 compose yaml,也可以让用户看一眼。
      setInstallSummary(response.composeYaml || "");
      void load();
    } catch (err) {
      setError(safeError(err));
      setInstallSummary("");
    }
  };

  const uninstallApp = async (app: InstalledApp) => {
    try {
      await clients.appStore.uninstallApp({ appName: app.appName });
      setMessage(`${app.appName} 已卸载`);
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  return (
    <section className="flex flex-col gap-5">
      <header className="flex items-start justify-between gap-4 flex-wrap">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-semibold tracking-tight m-0">软件商店</h1>
          <p className="text-sm text-muted-foreground m-0">
            按当前主机能力分组(可用 / 资源不足 / 内核不支持 / 需要 Docker)。
            轻量包优先,docker 路线在 OpenVZ 上自动折叠。
          </p>
        </div>
        <UIButton variant="outline" size="sm" onClick={() => void load()}>
          <RefreshCw className="size-4" />
          刷新
        </UIButton>
      </header>

      {error && (
        <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
          {error}
        </div>
      )}
      {message && !error && (
        <div className="rounded-md border border-success/40 bg-success/10 px-3 py-2 text-sm text-success">
          {message}
        </div>
      )}
      {installSummary && !error && (
        <pre className="rounded-md border border-border bg-muted/40 px-3 py-2 text-xs text-muted-foreground whitespace-pre-wrap font-mono overflow-x-auto m-0">
          {installSummary}
        </pre>
      )}

      <SoftwareStore
        templates={templates}
        selectedVersions={selectedVersions}
        onVersionChange={(slug, version) =>
          setSelectedVersions((prev) => ({ ...prev, [slug]: version }))
        }
        onDeploy={(template) => void deployTemplate(template)}
      />

      <Card>
        <CardHeader>
          <CardTitle>已安装运行环境</CardTitle>
          <CardDescription>面板托管的应用与状态</CardDescription>
        </CardHeader>
        <CardContent>
          {installedApps.length === 0 ? (
            <div className="empty-state text-sm">尚未安装任何应用</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>名称</TableHead>
                  <TableHead>镜像 / 版本</TableHead>
                  <TableHead>状态</TableHead>
                  <TableHead className="text-right">操作</TableHead>
                </UITableRow>
              </TableHeader>
              <TableBody>
                {installedApps.map((app) => (
                  <UITableRow key={app.appName}>
                    <TableCell className="font-medium">{app.appName}</TableCell>
                    <TableCell className="text-xs text-muted-foreground">
                      {app.image} · {app.version || "-"}
                    </TableCell>
                    <TableCell>
                      <Badge variant={appStateVariant(app.state)}>{app.state || "unknown"}</Badge>
                    </TableCell>
                    <TableCell className="text-right">
                      <UIButton variant="ghost" size="sm" onClick={() => void uninstallApp(app)}>
                        <Trash2 className="size-3.5" />
                        卸载
                      </UIButton>
                    </TableCell>
                  </UITableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>
    </section>
  );
}

function VsmtpAliasPage({ clients }: { clients: Clients }) {
  const [aliases, setAliases] = useState<VsmtpAlias[]>([]);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const [form, setForm] = useState({ alias: "", forwardTo: "", note: "" });

  const load = useCallback(async () => {
    try {
      const response = await clients.vsmtpAlias.listVsmtpAliases({});
      setAliases(response.aliases);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  }, [clients]);

  useEffect(() => {
    void load();
  }, [load]);

  const upsert = async () => {
    if (!form.alias.trim() || !form.forwardTo.trim()) {
      setError("alias 与转发邮箱都不能为空");
      return;
    }
    try {
      await clients.vsmtpAlias.upsertVsmtpAlias({
        alias: {
          alias: form.alias.trim(),
          forwardTo: form.forwardTo.trim(),
          note: form.note.trim(),
          createdAtSeconds: 0n,
          updatedAtSeconds: 0n
        }
      });
      setMessage(`${form.alias} 已保存`);
      setForm({ alias: "", forwardTo: "", note: "" });
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const remove = async (alias: string) => {
    try {
      await clients.vsmtpAlias.deleteVsmtpAlias({ alias });
      setMessage(`${alias} 已删除`);
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  return (
    <section className="flex flex-col gap-5">
      <header className="flex items-start justify-between gap-4 flex-wrap">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-semibold tracking-tight m-0">邮件别名(vSMTP)</h1>
          <p className="text-sm text-muted-foreground m-0">
            外部发到 alias@你的域名 的邮件会按下方规则转发到 forward_to。
            出站必须在 vSMTP 配置里走 SMTP relay(Resend / SES / Postmark),
            **绝不直连 25 端口**。
          </p>
        </div>
        <UIButton variant="outline" size="sm" onClick={() => void load()}>
          <RefreshCw className="size-4" />
          刷新
        </UIButton>
      </header>

      {error && (
        <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
          {error}
        </div>
      )}
      {message && !error && (
        <div className="rounded-md border border-success/40 bg-success/10 px-3 py-2 text-sm text-success">
          {message}
        </div>
      )}

      <Card>
        <CardHeader>
          <CardTitle>添加 / 更新 alias</CardTitle>
          <CardDescription>同名 alias 会被覆盖,创建时间保留</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3 sm:grid-cols-[1fr_1fr_1fr_auto] sm:items-end">
            <Input
              label="alias(小写 / 数字 / . _ -)"
              value={form.alias}
              onChange={(alias) => setForm((prev) => ({ ...prev, alias }))}
            />
            <Input
              label="forward_to"
              value={form.forwardTo}
              onChange={(forwardTo) => setForm((prev) => ({ ...prev, forwardTo }))}
            />
            <Input
              label="备注(可选)"
              value={form.note}
              onChange={(note) => setForm((prev) => ({ ...prev, note }))}
            />
            <UIButton size="sm" onClick={() => void upsert()}>
              <Plus className="size-3.5" />
              保存
            </UIButton>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>已有 alias</CardTitle>
          <CardDescription>共 {aliases.length} 条</CardDescription>
        </CardHeader>
        <CardContent>
          {aliases.length === 0 ? (
            <div className="empty-state text-sm">尚未配置任何 alias</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>alias</TableHead>
                  <TableHead>转发到</TableHead>
                  <TableHead>备注</TableHead>
                  <TableHead className="text-right">操作</TableHead>
                </UITableRow>
              </TableHeader>
              <TableBody>
                {aliases.map((item) => (
                  <UITableRow key={item.alias}>
                    <TableCell className="font-mono text-xs">{item.alias}</TableCell>
                    <TableCell className="font-mono text-xs">{item.forwardTo}</TableCell>
                    <TableCell className="text-muted-foreground">
                      {item.note || "-"}
                    </TableCell>
                    <TableCell className="text-right">
                      <UIButton
                        size="sm"
                        variant="outline"
                        onClick={() => void remove(item.alias)}
                      >
                        <Trash2 className="size-3.5" />
                        删除
                      </UIButton>
                    </TableCell>
                  </UITableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>
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
    // Promise.allSettled 而不是 Promise.all:OpenVZ 上 Docker 不通也不能让
    // 同页面的 SoftwareStore / 已装应用列表跟着挂掉。每个调用各自吞错。
    const [containerR, imageR, composeR, templateR, installedR] = await Promise.allSettled([
      clients.docker.listContainers({ all: true }),
      clients.docker.listImages({ all: true }),
      clients.docker.listComposeProjects({}),
      clients.appStore.listAppTemplates({}),
      clients.appStore.listInstalledApps({})
    ]);
    if (containerR.status === "fulfilled") setContainers(containerR.value.containers);
    if (imageR.status === "fulfilled") setImages(imageR.value.images);
    if (composeR.status === "fulfilled") setComposeProjects(composeR.value.projects);
    if (templateR.status === "fulfilled") {
      setTemplates(templateR.value.templates);
      setSelectedVersions((current) => ({
        ...Object.fromEntries(
          templateR.value.templates.map((template) => [template.slug, template.defaultVersion])
        ),
        ...current
      }));
    }
    if (installedR.status === "fulfilled") setInstalledApps(installedR.value.apps);
    const failures = [containerR, imageR, composeR, templateR, installedR]
      .filter((r): r is PromiseRejectedResult => r.status === "rejected")
      .map((r) => safeError(r.reason));
    setError(failures.length ? failures.join("; ") : "");
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

      <SoftwareStore
        templates={templates}
        selectedVersions={selectedVersions}
        onVersionChange={(slug, version) =>
          setSelectedVersions({ ...selectedVersions, [slug]: version })
        }
        onDeploy={(template) => void deployTemplate(template)}
      />

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

type SiteSheetMode = "new" | "edit" | null;

function siteKindLabel(kind: number): string {
  switch (kind) {
    case SiteKind.STATIC:
      return "静态";
    case SiteKind.RUST_BINARY:
      return "Rust 二进制";
    case SiteKind.REVERSE_PROXY:
      return "反向代理";
    default:
      return "默认";
  }
}

// 与后端 site.rs::safe_name 对齐的 slug 规则:小写 / 数字 / 连字符,
// 其他字符全部替换成 '-';首尾连字符清掉。供前端"根据站点名自动派生
// 根目录"和"撞库检查"复用。
function safeName(input: string): string {
  return input
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

function defaultRootFor(name: string): string {
  const safe = safeName(name);
  return safe ? `/var/www/${safe}` : "";
}

function defaultBinaryPathFor(name: string): string {
  const safe = safeName(name);
  return safe ? `/usr/local/bin/${safe}-server` : "";
}

// 站点类型在 SmartSiteForm 里以"卡片"形式让用户选,每张卡都有
// 图标 / 一句话用途 / 举例。比之前裸 Select 的"nginx serve root"这
// 种术语对小白友好得多。
const SITE_KIND_OPTIONS = [
  {
    value: "static" as const,
    icon: Folder,
    label: "静态站",
    description: "只有 HTML / CSS / JS / 图片这类文件,无后端进程",
    useCase: "博客 / 文档站 / Hugo / Zola / React 编译产物"
  },
  {
    value: "rust-binary" as const,
    icon: Server,
    label: "Rust 二进制",
    description: "你写的程序,面板帮你启动并反代",
    useCase: "自写的 API / Telegram bot / Web 服务后端"
  },
  {
    value: "reverse-proxy" as const,
    icon: ArrowLeftRight,
    label: "反向代理",
    description: "已经在跑的服务,只给它套上域名 / HTTPS",
    useCase: "把 localhost:3000 暴露到公网域名"
  }
];

function SitesSsl({ clients }: { clients: Clients }) {
  // === 数据 ===
  const [sites, setSites] = useState<SiteItem[]>([]);
  const [certificates, setCertificates] = useState<CertificateItem[]>([]);
  const [rewriteTemplates, setRewriteTemplates] = useState<RewriteTemplate[]>([]);
  const [proxyRules, setProxyRules] = useState<ReverseProxyRule[]>([]);
  const [reservedPorts, setReservedPorts] = useState<ReservedPort[]>([]);
  const [ipv6Pool, setIpv6Pool] = useState<Ipv6Address[]>([]);
  const [capabilities, setCapabilities] = useState<Capabilities | null>(null);
  // === 反馈 ===
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  // === Sheet 抽屉状态 ===
  const [sheetMode, setSheetMode] = useState<SiteSheetMode>(null);
  const [selectedSite, setSelectedSite] = useState<SiteItem | null>(null);
  // 删除确认 Dialog 的目标站点 / 证书。null = 不显示。
  const [siteToDelete, setSiteToDelete] = useState<SiteItem | null>(null);
  const [certToRevoke, setCertToRevoke] = useState<CertificateItem | null>(null);

  const load = useCallback(async () => {
    try {
      const [siteRes, certRes, tplRes, proxyRes, portsRes, v6Res] = await Promise.all([
        clients.site.listSites({}),
        clients.ssl.listCertificates({}),
        clients.site.listRewriteTemplates({}),
        clients.site.listReverseProxyRules({}),
        clients.capability
          .listReservedPorts({})
          .catch(() => ({ ports: [] as ReservedPort[] })),
        clients.capability
          .listIpv6Addresses({})
          .catch(() => ({ addresses: [] as Ipv6Address[], prefixes: [] as string[] }))
      ]);
      setSites(siteRes.sites);
      setCertificates(certRes.certificates);
      setRewriteTemplates(tplRes.templates);
      setProxyRules(proxyRes.rules);
      setReservedPorts(portsRes.ports);
      setIpv6Pool(v6Res.addresses);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  }, [clients]);

  // 主机能力(capabilities)只在挂载时探一次,影响"绑定方式"字段的展开
  useEffect(() => {
    void clients.capability
      .getCapabilities({})
      .then((response) => setCapabilities(response.capabilities ?? null))
      .catch(() => setCapabilities(null));
  }, [clients]);

  useEffect(() => {
    void load();
  }, [load]);

  // 路由:/#sites/<name> 或 /#sites/new 直接打开对应抽屉。
  // 主路由(a4bd433)只识别一级 TabId,这里负责解析第二段并按 sites 列
  // 表匹配。`sites` 还没拉到时挂起;拉到后这个 effect 会重跑一次。
  useEffect(() => {
    const parse = () => {
      const raw = window.location.hash.replace(/^#/, "");
      const parts = raw.split("/");
      if (parts[0] !== "sites") return;
      if (parts[1] === "new") {
        setSheetMode("new");
        setSelectedSite(null);
      } else if (parts[1]) {
        const target = sites.find((site) => site.name === parts[1]);
        if (target) {
          setSheetMode("edit");
          setSelectedSite(target);
        }
      } else {
        setSheetMode(null);
        setSelectedSite(null);
      }
    };
    parse();
    window.addEventListener("hashchange", parse);
    return () => window.removeEventListener("hashchange", parse);
  }, [sites]);

  const openCreate = () => {
    setSheetMode("new");
    setSelectedSite(null);
    if (window.location.hash !== "#sites/new") {
      window.history.replaceState(null, "", "#sites/new");
    }
  };

  const openEdit = (site: SiteItem) => {
    setSheetMode("edit");
    setSelectedSite(site);
    const target = `#sites/${site.name}`;
    if (window.location.hash !== target) {
      window.history.replaceState(null, "", target);
    }
  };

  const closeSheet = () => {
    setSheetMode(null);
    setSelectedSite(null);
    if (window.location.hash !== "#sites") {
      window.history.replaceState(null, "", "#sites");
    }
  };

  const renewCertificate = async (certificate: CertificateItem) => {
    try {
      const response = await clients.ssl.renewCertificate({ domain: certificate.domain });
      setMessage(response.output || `${certificate.domain} 已续签`);
      await load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const deleteReverseProxy = async (rule: ReverseProxyRule) => {
    try {
      await clients.site.deleteReverseProxyRule({ id: rule.id });
      setMessage(`${rule.name} 已删除`);
      await load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const performRevokeCert = async () => {
    if (!certToRevoke) return;
    const target = certToRevoke;
    setCertToRevoke(null);
    try {
      await clients.ssl.revokeCertificate({ domain: target.domain });
      setMessage(`${target.domain} 证书已撤销 · 清除本地 fullchain.pem / privkey.pem`);
      await load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const performDeleteSite = async () => {
    if (!siteToDelete) return;
    const target = siteToDelete;
    setSiteToDelete(null);
    try {
      const response = await clients.site.deleteSite({ name: target.name });
      const cleaned = response.cleanedPaths?.length ?? 0;
      setMessage(
        cleaned > 0
          ? `${target.name} 已删除 · 清理 ${cleaned} 项`
          : `${target.name} 已删除`
      );
      // 释放预留的 NAT 端口预算(本来就是 owner=site:<name> reserve 的)
      const port = target.binding?.natPort;
      if (port && port > 0) {
        await clients.capability.releasePort({ port }).catch(() => undefined);
      }
      // 抽屉里如果是这个站点就关掉
      if (selectedSite?.name === target.name) {
        closeSheet();
      }
      await load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  return (
    <section className="flex flex-col gap-5">
      <header className="flex items-start justify-between gap-4 flex-wrap">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-semibold tracking-tight m-0">网站</h1>
          <p className="text-sm text-muted-foreground m-0">
            站点 {sites.length} 个 · 证书 {certificates.length} 张 · 反代规则 {proxyRules.length} 条
          </p>
        </div>
        <div className="flex gap-2">
          <UIButton variant="outline" size="sm" onClick={() => void load()}>
            <RefreshCw className="size-4" />
            刷新
          </UIButton>
          <UIButton size="sm" onClick={openCreate}>
            <Plus className="size-4" />
            新建站点
          </UIButton>
        </div>
      </header>

      {error && (
        <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
          {error}
        </div>
      )}
      {message && !error && (
        <div className="rounded-md border border-success/40 bg-success/10 px-3 py-2 text-sm text-success">
          {message}
        </div>
      )}

      <Card>
        <CardHeader>
          <CardTitle>站点</CardTitle>
          <CardDescription>点行打开详情(基础 / SSL / 反代 / 伪静态)</CardDescription>
        </CardHeader>
        <CardContent>
          {sites.length === 0 ? (
            <div className="empty-state text-sm">尚无站点 — 点右上"+ 新建站点"开始</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>名称</TableHead>
                  <TableHead>域名</TableHead>
                  <TableHead>类型</TableHead>
                  <TableHead>SSL</TableHead>
                  <TableHead className="text-right">操作</TableHead>
                </UITableRow>
              </TableHeader>
              <TableBody>
                {sites.map((site) => (
                  <UITableRow
                    key={site.configPath || site.name}
                    className="cursor-pointer hover:bg-accent/50"
                    onClick={() => openEdit(site)}
                  >
                    <TableCell className="font-medium">{site.name}</TableCell>
                    <TableCell className="font-mono text-xs">
                      {site.domains.join(", ") || "—"}
                    </TableCell>
                    <TableCell>{siteKindLabel(site.kind)}</TableCell>
                    <TableCell>
                      <Badge variant={site.sslEnabled ? "success" : "muted"}>
                        {site.sslEnabled ? "SSL" : "HTTP"}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-right">
                      <div className="flex justify-end gap-1">
                        <UIButton
                          size="sm"
                          variant="ghost"
                          onClick={(event) => {
                            event.stopPropagation();
                            openEdit(site);
                          }}
                        >
                          编辑
                        </UIButton>
                        <UIButton
                          size="sm"
                          variant="ghost"
                          title="删除站点"
                          onClick={(event) => {
                            event.stopPropagation();
                            setSiteToDelete(site);
                          }}
                        >
                          <Trash2 className="size-3.5 text-destructive" />
                        </UIButton>
                      </div>
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
          <CardTitle>证书统一视图</CardTitle>
          <CardDescription>跨站点的所有证书;续签也可在抽屉 SSL Tab 触发。</CardDescription>
        </CardHeader>
        <CardContent>
          {certificates.length === 0 ? (
            <div className="empty-state text-sm">暂无证书</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>域名</TableHead>
                  <TableHead>分组</TableHead>
                  <TableHead>剩余</TableHead>
                  <TableHead className="text-right">操作</TableHead>
                </UITableRow>
              </TableHeader>
              <TableBody>
                {certificates.map((cert) => (
                  <UITableRow key={cert.domain}>
                    <TableCell className="font-medium">{cert.domain}</TableCell>
                    <TableCell className="text-muted-foreground">
                      {cert.group || "default"}
                    </TableCell>
                    <TableCell>
                      <Badge variant={cert.warningLevel === "ok" ? "success" : "destructive"}>
                        {cert.daysUntilExpiry} 天
                      </Badge>
                    </TableCell>
                    <TableCell className="text-right">
                      <div className="flex justify-end gap-1">
                        <UIButton
                          size="sm"
                          variant="outline"
                          onClick={() => void renewCertificate(cert)}
                        >
                          <RotateCw className="size-3.5" />
                          续签
                        </UIButton>
                        <UIButton
                          size="sm"
                          variant="ghost"
                          title="撤销证书"
                          onClick={() => setCertToRevoke(cert)}
                        >
                          <Trash2 className="size-3.5 text-destructive" />
                        </UIButton>
                      </div>
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
          <CardTitle>反向代理规则</CardTitle>
          <CardDescription>跨站点的全局规则;单站点反代请在抽屉的"反向代理" Tab 管。</CardDescription>
        </CardHeader>
        <CardContent>
          {proxyRules.length === 0 ? (
            <div className="empty-state text-sm">暂无反代规则</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>名称</TableHead>
                  <TableHead>路径</TableHead>
                  <TableHead>目标</TableHead>
                  <TableHead>状态</TableHead>
                  <TableHead className="text-right">操作</TableHead>
                </UITableRow>
              </TableHeader>
              <TableBody>
                {proxyRules.map((rule) => (
                  <UITableRow key={rule.id}>
                    <TableCell className="font-medium">{rule.name}</TableCell>
                    <TableCell className="font-mono text-xs">
                      {rule.domain}
                      {rule.pathPrefix}
                    </TableCell>
                    <TableCell className="font-mono text-xs">
                      {rule.targets.map((target) => target.url).join(", ")}
                    </TableCell>
                    <TableCell>
                      <Badge variant={rule.enabled ? "success" : "muted"}>
                        {rule.enabled ? "启用" : "停用"}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-right">
                      <UIButton
                        size="sm"
                        variant="outline"
                        onClick={() => void deleteReverseProxy(rule)}
                      >
                        <Trash2 className="size-3.5" />
                        删除
                      </UIButton>
                    </TableCell>
                  </UITableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>

      <SiteDetailSheet
        mode={sheetMode}
        site={selectedSite}
        sites={sites}
        capabilities={capabilities}
        reservedPorts={reservedPorts}
        ipv6Pool={ipv6Pool}
        certificates={certificates}
        proxyRules={proxyRules}
        rewriteTemplates={rewriteTemplates}
        clients={clients}
        onClose={closeSheet}
        onChanged={() => void load()}
        onMessage={setMessage}
        onError={setError}
        onRequestDelete={(site) => setSiteToDelete(site)}
        onRequestRevokeCert={(cert) => setCertToRevoke(cert)}
      />

      <Dialog
        open={siteToDelete !== null}
        onOpenChange={(open) => !open && setSiteToDelete(null)}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>删除站点 "{siteToDelete?.name}"?</DialogTitle>
            <DialogDescription>
              会清理:nginx vhost / 元数据 sidecar / rpxy 站点片段 /
              sws@实例(如果有)。config 模板与用户上传的网站文件 **不动**;
              NAT 端口预算会自动释放,可重复使用。
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <UIButton variant="outline" onClick={() => setSiteToDelete(null)}>
              取消
            </UIButton>
            <UIButton variant="destructive" onClick={() => void performDeleteSite()}>
              <Trash2 className="size-4" />
              确认删除
            </UIButton>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={certToRevoke !== null}
        onOpenChange={(open) => !open && setCertToRevoke(null)}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>撤销证书 "{certToRevoke?.domain}"?</DialogTitle>
            <DialogDescription>
              清除本地 fullchain.pem / privkey.pem 文件。绑定该域名的 nginx
              vhost 会立刻找不到证书,**网站 HTTPS 立即不可用**,直到你
              重新申请或导入新证书。这一步不会通知 Let's Encrypt 真正
              revoke(那需要 ACME revokeCert 请求,本面板暂未实现);
              只是本地清空文件。
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <UIButton variant="outline" onClick={() => setCertToRevoke(null)}>
              取消
            </UIButton>
            <UIButton variant="destructive" onClick={() => void performRevokeCert()}>
              <Trash2 className="size-4" />
              确认撤销
            </UIButton>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  );
}

type SiteSheetProps = {
  mode: SiteSheetMode;
  site: SiteItem | null;
  sites: SiteItem[];
  capabilities: Capabilities | null;
  reservedPorts: ReservedPort[];
  ipv6Pool: Ipv6Address[];
  certificates: CertificateItem[];
  proxyRules: ReverseProxyRule[];
  rewriteTemplates: RewriteTemplate[];
  clients: Clients;
  onClose: () => void;
  onChanged: () => void;
  onMessage: (message: string) => void;
  onError: (error: string) => void;
  onRequestDelete?: (site: SiteItem) => void;
  onRequestRevokeCert?: (cert: CertificateItem) => void;
};

function SiteDetailSheet(props: SiteSheetProps) {
  const { mode, site, onClose } = props;
  return (
    <Sheet open={mode !== null} onOpenChange={(open) => !open && onClose()}>
      <SheetContent>
        <SheetHeader>
          <SheetTitle>
            {mode === "new" ? "新建站点" : site?.name || "站点详情"}
          </SheetTitle>
          <SheetDescription>
            {mode === "new"
              ? "保存后回到此抽屉继续配置 SSL / 反代 / 伪静态"
              : site?.domains.join(", ") || ""}
          </SheetDescription>
        </SheetHeader>
        <div className="flex-1 overflow-y-auto px-5 pb-3">
          <Tabs defaultValue="basic">
            <TabsList>
              <TabsTrigger value="basic">基础</TabsTrigger>
              <TabsTrigger value="ssl" disabled={mode === "new"}>
                SSL
              </TabsTrigger>
              <TabsTrigger value="rp" disabled={mode === "new"}>
                反向代理
              </TabsTrigger>
              <TabsTrigger value="rewrite">伪静态</TabsTrigger>
            </TabsList>
            <TabsContent value="basic" className="pt-3">
              <SmartSiteForm {...props} />
            </TabsContent>
            <TabsContent value="ssl" className="pt-3">
              {mode === "edit" && site ? (
                <SslPanel {...props} site={site} />
              ) : (
                <p className="text-sm text-muted-foreground">保存站点后可配置 SSL</p>
              )}
            </TabsContent>
            <TabsContent value="rp" className="pt-3">
              {mode === "edit" && site ? (
                <PerSiteReverseProxyPanel {...props} site={site} />
              ) : (
                <p className="text-sm text-muted-foreground">保存站点后可配置反向代理</p>
              )}
            </TabsContent>
            <TabsContent value="rewrite" className="pt-3">
              <RewritePanel templates={props.rewriteTemplates} />
            </TabsContent>
          </Tabs>
        </div>
        <SheetFooter>
          {mode === "edit" && site && props.onRequestDelete && (
            <UIButton
              variant="destructive"
              size="sm"
              onClick={() => props.onRequestDelete!(site)}
            >
              <Trash2 className="size-3.5" />
              删除站点
            </UIButton>
          )}
          <UIButton variant="outline" onClick={onClose}>
            关闭
          </UIButton>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}

function SmartSiteForm({
  mode,
  site,
  sites,
  capabilities,
  reservedPorts,
  ipv6Pool,
  clients,
  onChanged,
  onMessage,
  onError,
  onClose
}: SiteSheetProps) {
  // 智能表单合并了 Phase C 高级版 + 经典向导 —— 字段始终是同一套,
  // "绑定方式 / NAT 端口 / IPv6 地址"按 capabilities 揭示。
  const isEdit = mode === "edit";
  const [name, setName] = useState(site?.name ?? "");
  const [domain, setDomain] = useState(site?.domains.join(", ") ?? "");
  const [kind, setKind] = useState<"static" | "rust-binary" | "reverse-proxy">(() => {
    if (!site) return "static";
    if (site.kind === SiteKind.RUST_BINARY) return "rust-binary";
    if (site.kind === SiteKind.REVERSE_PROXY) return "reverse-proxy";
    return "static";
  });
  // 用户没手动改 root 之前,跟着 name 自动派生为 /var/www/<safe-name>;
  // 一旦用户在 root 框里敲过任何字符,rootTouched 翻 true,不再追。
  // 编辑模式直接展示后端给的 root,默认视作"已 touched"。
  const [root, setRoot] = useState(site?.root ?? "");
  const [rootTouched, setRootTouched] = useState(Boolean(site?.root));
  const [proxyTarget, setProxyTarget] = useState(site?.proxyTarget ?? "");
  const [binaryPath, setBinaryPath] = useState("");
  const [binaryPathTouched, setBinaryPathTouched] = useState(false);
  const [bindKind, setBindKind] = useState<"nat-port" | "ipv6">(() =>
    ipv6Pool.length > 0 ? "ipv6" : "nat-port"
  );
  const [natPort, setNatPort] = useState(site?.binding?.natPort?.toString() ?? "");
  const [ipv6Address, setIpv6Address] = useState(site?.binding?.ipv6Address ?? "");
  const [tls, setTls] = useState<"none" | "dns01" | "imported">(() => {
    if (!site) return "dns01";
    if (site.tlsStrategy === SiteTlsStrategy.LETSENCRYPT_DNS01) return "dns01";
    if (site.tlsStrategy === SiteTlsStrategy.IMPORTED) return "imported";
    return site.sslEnabled ? "dns01" : "none";
  });

  // name 变化时,如果用户没手动改 root / binaryPath,自动派生新值。
  // 同时只在新建模式下生效;编辑模式既已 disabled,也不需要再追。
  useEffect(() => {
    if (isEdit) return;
    if (!rootTouched) setRoot(defaultRootFor(name));
    if (!binaryPathTouched) setBinaryPath(defaultBinaryPathFor(name));
  }, [name, rootTouched, binaryPathTouched, isEdit]);

  // 撞库实时提示:name / root / 域名任意一项与现有站点冲突就在按钮上方给警告。
  // 提交时再做一次硬校验阻断;预览只是提早告诉用户。
  const otherSites = isEdit ? sites.filter((item) => item.name !== site?.name) : sites;
  const nameConflict = useMemo(() => {
    const safe = safeName(name);
    if (!safe) return null;
    return otherSites.find((item) => safeName(item.name) === safe) ?? null;
  }, [name, otherSites]);
  const rootConflict = useMemo(() => {
    if (kind !== "static" || !root) return null;
    return otherSites.find((item) => item.root === root) ?? null;
  }, [kind, root, otherSites]);
  const domainConflict = useMemo(() => {
    const wants = domain
      .split(/[\s,]+/)
      .map((value) => value.trim().toLowerCase())
      .filter(Boolean);
    if (!wants.length) return null;
    for (const other of otherSites) {
      const have = other.domains.map((d) => d.toLowerCase());
      const collide = wants.find((value) => have.includes(value));
      if (collide) return { domain: collide, site: other };
    }
    return null;
  }, [domain, otherSites]);

  // IPv6 共享是合法的(nginx SNI 路由),NAT 端口共享是非法的(一个端口
  // 只能挂一个 listener)。下拉里给用户标出每个地址 / 端口的占用情况,
  // 让"复用同一 v6 给多个域名"这件事看得见。
  const ipv6UsageMap = useMemo(() => {
    const map = new Map<string, string[]>();
    for (const other of otherSites) {
      const addr = other.binding?.ipv6Address;
      if (!addr) continue;
      const list = map.get(addr) ?? [];
      list.push(other.name);
      map.set(addr, list);
    }
    return map;
  }, [otherSites]);
  const natPortUsageMap = useMemo(() => {
    const map = new Map<number, string>();
    for (const other of otherSites) {
      const port = other.binding?.natPort;
      if (!port) continue;
      map.set(port, other.name);
    }
    return map;
  }, [otherSites]);
  // NAT 端口冲突:与"另外某个站点"占了同一端口 → 拒。reservedPorts 那条
  // 检查是"端口预算总池",这里专门拦的是站点之间撞车。
  const natPortConflict = useMemo(() => {
    if (bindKind !== "nat-port") return null;
    const port = Number.parseInt(natPort, 10);
    if (!port) return null;
    const owner = natPortUsageMap.get(port);
    return owner ? { port, owner } : null;
  }, [bindKind, natPort, natPortUsageMap]);

  // NAT VPS 总是有 NAT 端口预算,所以绑定段一直显示;有 IPv6 池时多
  // 出"IPv6 直连"那一项。capabilities 字段不直接给端口数,具体可用端口
  // 由 listReservedPorts 反查。
  void capabilities;
  const showBinding = true;

  const submit = async () => {
    if (isEdit) {
      onError("当前后端无 UpdateSite RPC,暂不支持编辑已有站点。删了重建可绕过。");
      return;
    }
    // 硬校验:撞名 / 撞 root / 撞域名一律拒
    if (!safeName(name)) {
      onError("站点名不能为空,且需含字母 / 数字 / 连字符");
      return;
    }
    if (nameConflict) {
      onError(`站点名 "${name}" 已存在,请换一个`);
      return;
    }
    if (rootConflict) {
      onError(
        `根目录 "${root}" 已被站点 "${rootConflict.name}" 使用 — 改个站点名让它自动派生新路径`
      );
      return;
    }
    if (domainConflict) {
      onError(
        `域名 "${domainConflict.domain}" 已被站点 "${domainConflict.site.name}" 占用`
      );
      return;
    }
    if (natPortConflict) {
      onError(
        `NAT 端口 ${natPortConflict.port} 已被站点 "${natPortConflict.owner}" 占用 — 一个端口只能挂一个监听`
      );
      return;
    }
    const protoKind =
      kind === "static"
        ? SiteKind.STATIC
        : kind === "rust-binary"
          ? SiteKind.RUST_BINARY
          : SiteKind.REVERSE_PROXY;
    const protoTls =
      tls === "dns01"
        ? SiteTlsStrategy.LETSENCRYPT_DNS01
        : tls === "imported"
          ? SiteTlsStrategy.IMPORTED
          : SiteTlsStrategy.NONE;
    const binding = {
      kind:
        bindKind === "nat-port" ? SiteBindKind.NAT_PORT : SiteBindKind.IPV6_ADDRESS,
      natPort:
        bindKind === "nat-port" ? Number.parseInt(natPort, 10) || 0 : 0,
      ipv6Address: bindKind === "ipv6" ? ipv6Address : ""
    };
    try {
      const response = await clients.site.createSite({
        name,
        domains: domain
          .split(/[\s,]+/)
          .map((value) => value.trim())
          .filter(Boolean),
        root,
        proxyTarget,
        sslEnabled: protoTls !== SiteTlsStrategy.NONE,
        engine: "nginx",
        listenAddr: "",
        kind: protoKind,
        binding,
        tlsStrategy: protoTls,
        binaryPath
      });
      if (binding.kind === SiteBindKind.NAT_PORT && binding.natPort > 0) {
        await clients.capability
          .reservePort({
            port: binding.natPort,
            owner: `site:${name}`,
            description: domain,
            protocol: "tcp"
          })
          .catch(() => undefined);
      }
      if (response.site) {
        onMessage(`${response.site.name} 已创建`);
      }
      onChanged();
      onClose();
    } catch (err) {
      onError(safeError(err));
    }
  };

  return (
    <div className="flex flex-col gap-3">
      <div className="grid gap-3 md:grid-cols-2">
        <div className="grid gap-1">
          <UILabel htmlFor="site-name">站点名</UILabel>
          <UIInput
            id="site-name"
            value={name}
            disabled={isEdit}
            onChange={(event) => setName(event.target.value)}
          />
        </div>
        <div className="grid gap-1">
          <UILabel htmlFor="site-domain">域名(空格或逗号分隔)</UILabel>
          <UIInput
            id="site-domain"
            value={domain}
            disabled={isEdit}
            onChange={(event) => setDomain(event.target.value)}
          />
        </div>
        <div className="grid gap-1.5 md:col-span-2">
          <UILabel>站点类型</UILabel>
          <div className="grid gap-2 md:grid-cols-3">
            {SITE_KIND_OPTIONS.map((option) => {
              const Icon = option.icon;
              const selected = kind === option.value;
              return (
                <button
                  key={option.value}
                  type="button"
                  disabled={isEdit}
                  onClick={() => setKind(option.value)}
                  className={cn(
                    "flex flex-col items-start gap-1 rounded-lg border p-3 text-left transition",
                    selected
                      ? "border-primary bg-primary/5 ring-1 ring-primary/30"
                      : "border-border hover:border-primary/40 hover:bg-accent/30",
                    isEdit && "opacity-60 cursor-not-allowed"
                  )}
                >
                  <div className="flex items-center gap-1.5">
                    <Icon className="size-4 text-primary" />
                    <span className="font-medium text-sm">{option.label}</span>
                  </div>
                  <span className="text-xs text-muted-foreground">
                    {option.description}
                  </span>
                  <span className="text-[11px] text-muted-foreground/80">
                    适合:{option.useCase}
                  </span>
                </button>
              );
            })}
          </div>
        </div>
        {kind === "static" && (
          <div className="grid gap-1 md:col-span-2">
            <UILabel htmlFor="site-root">网站根目录</UILabel>
            <UIInput
              id="site-root"
              placeholder={defaultRootFor(name) || "/var/www/<填好站点名后自动生成>"}
              value={root}
              disabled={isEdit}
              onChange={(event) => {
                setRootTouched(true);
                setRoot(event.target.value);
              }}
            />
            <span className="text-xs text-muted-foreground">
              {rootTouched
                ? "已自定义。放 index.html 的目录;nginx 会直接把里头的文件喂给浏览器。"
                : `跟着站点名自动派生 ${defaultRootFor(name) || "(待站点名输入)"};可手动覆盖。`}
            </span>
          </div>
        )}
        {kind === "rust-binary" && (
          <div className="grid gap-1 md:col-span-2">
            <UILabel htmlFor="site-bin">Rust 程序的二进制路径</UILabel>
            <UIInput
              id="site-bin"
              placeholder={defaultBinaryPathFor(name) || "/usr/local/bin/<填好站点名后自动生成>"}
              value={binaryPath}
              disabled={isEdit}
              onChange={(event) => {
                setBinaryPathTouched(true);
                setBinaryPath(event.target.value);
              }}
            />
            <span className="text-xs text-muted-foreground">
              你 `cargo build --release` 出来的可执行文件路径。面板会自动写
              systemd 服务把它拉起来(internal 127.0.0.1:9100+),再让 nginx 反代给它。
            </span>
          </div>
        )}
        {kind === "reverse-proxy" && (
          <div className="grid gap-1 md:col-span-2">
            <UILabel htmlFor="site-upstream">目标服务地址</UILabel>
            <UIInput
              id="site-upstream"
              placeholder="http://127.0.0.1:3000"
              value={proxyTarget}
              disabled={isEdit}
              onChange={(event) => setProxyTarget(event.target.value)}
            />
            <span className="text-xs text-muted-foreground">
              已经在你机器上跑着的服务地址。把它"包装"成 https://你的域名 暴露出去。
            </span>
          </div>
        )}
        {showBinding && (
          <>
            <div className="grid gap-1 md:col-span-2">
              <UILabel>怎么对外</UILabel>
              <Select
                value={bindKind}
                onValueChange={(value) => setBindKind(value as typeof bindKind)}
                disabled={isEdit}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {ipv6Pool.length > 0 && (
                    <SelectItem value="ipv6">用 IPv6 地址(推荐 · 不占端口)</SelectItem>
                  )}
                  <SelectItem value="nat-port">用 NAT 端口(占一个公网端口)</SelectItem>
                </SelectContent>
              </Select>
              <span className="text-xs text-muted-foreground">
                NAT VPS 总共 20 个公网端口预算;有 IPv6 公网地址就用 v6,
                不占端口、不限数量。
              </span>
            </div>
            {bindKind === "ipv6" ? (
              <div className="grid gap-1 md:col-span-2">
                <UILabel htmlFor="site-v6">公网 IPv6 地址</UILabel>
                <Select
                  value={ipv6Address}
                  onValueChange={setIpv6Address}
                  disabled={isEdit}
                >
                  <SelectTrigger id="site-v6">
                    <SelectValue placeholder="选择一个 v6 地址" />
                  </SelectTrigger>
                  <SelectContent>
                    {ipv6Pool.length === 0 && (
                      <SelectItem value="__none__" disabled>
                        未检测到公网 IPv6
                      </SelectItem>
                    )}
                    {ipv6Pool.map((addr) => {
                      const users = ipv6UsageMap.get(addr.address) ?? [];
                      return (
                        <SelectItem key={addr.address} value={addr.address}>
                          {addr.address}/{addr.prefixLength} ({addr.interfaceName})
                          {users.length > 0 && ` · 已用于 ${users.join(", ")}`}
                        </SelectItem>
                      );
                    })}
                  </SelectContent>
                </Select>
                <span className="text-xs text-muted-foreground">
                  {ipv6Address && (ipv6UsageMap.get(ipv6Address)?.length ?? 0) > 0
                    ? `当前 IPv6 已用于 ${ipv6UsageMap.get(ipv6Address)!.join(", ")};可继续共享 — nginx 按域名(SNI)分发,只要本站域名不与已有站点重复就行。`
                    : "同一 IPv6 可被多个站点共享 —— nginx 根据浏览器请求的域名(SNI)路由到对应项目,不占额外端口预算。"}
                </span>
              </div>
            ) : (
              <div className="grid gap-1 md:col-span-2">
                <UILabel htmlFor="site-natport">
                  NAT 公网端口(预算池已占:
                  {reservedPorts.map((p) => p.port).join(", ") || "无"})
                </UILabel>
                <UIInput
                  id="site-natport"
                  type="number"
                  placeholder="例如 8443"
                  value={natPort}
                  disabled={isEdit}
                  onChange={(event) => setNatPort(event.target.value)}
                />
                <span className="text-xs text-muted-foreground">
                  一个端口只能挂一个监听,**不能多站共享** — 同一端口的两个
                  站点必有一个起不来。需要多域名共享请用上方"IPv6 直连"。
                </span>
              </div>
            )}
          </>
        )}
        <div className="grid gap-1 md:col-span-2">
          <UILabel>HTTPS 证书</UILabel>
          <Select
            value={tls}
            onValueChange={(value) => setTls(value as typeof tls)}
            disabled={isEdit}
          >
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="dns01">自动申请免费证书(推荐 · Let's Encrypt)</SelectItem>
              <SelectItem value="imported">我已有证书(到时手动粘贴 PEM)</SelectItem>
              <SelectItem value="none">不启用 HTTPS</SelectItem>
            </SelectContent>
          </Select>
          <span className="text-xs text-muted-foreground">
            自动模式走 DNS-01 验证,无需 80/443 公网端口,适合 NAT VPS。
          </span>
        </div>
      </div>
      {!isEdit && (nameConflict || rootConflict || domainConflict || natPortConflict) && (
        <div className="rounded border border-warning/40 bg-warning/10 px-3 py-2 text-xs text-warning space-y-0.5">
          {nameConflict && (
            <div>· 站点名 "{name}" 已被使用 — 改一个</div>
          )}
          {rootConflict && (
            <div>
              · 根目录 "{root}" 已被站点 "{rootConflict.name}" 占用 — 改站点名能自动派生新路径
            </div>
          )}
          {domainConflict && (
            <div>
              · 域名 "{domainConflict.domain}" 已被站点 "{domainConflict.site.name}" 占用
            </div>
          )}
          {natPortConflict && (
            <div>
              · NAT 端口 {natPortConflict.port} 已被站点 "{natPortConflict.owner}" 占用 — 一个端口只能挂一个监听
            </div>
          )}
        </div>
      )}
      <div className="flex justify-end gap-2">
        <UIButton onClick={() => void submit()} disabled={isEdit}>
          <Save className="size-4" />
          {isEdit ? "暂不支持编辑(UpdateSite RPC 待落地)" : "创建站点"}
        </UIButton>
      </div>
      {isEdit && site && (
        <div className="rounded border border-border bg-muted/40 p-3 text-xs space-y-1">
          <div>
            <span className="text-muted-foreground">配置路径:</span>{" "}
            <span className="font-mono">{site.configPath}</span>
          </div>
          {site.systemdUnit && (
            <div>
              <span className="text-muted-foreground">systemd unit:</span>{" "}
              <span className="font-mono">{site.systemdUnit}</span>
            </div>
          )}
          {site.internalPort > 0 && (
            <div>
              <span className="text-muted-foreground">内部端口:</span>{" "}
              127.0.0.1:{site.internalPort}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function SslPanel({
  site,
  certificates,
  clients,
  onChanged,
  onMessage,
  onError,
  onRequestRevokeCert
}: SiteSheetProps & { site: SiteItem }) {
  const primaryDomain = site.domains[0] || "";
  const [importForm, setImportForm] = useState({
    domain: primaryDomain,
    group: "default",
    certificatePem: "",
    privateKeyPem: ""
  });
  const [pendingChallenge, setPendingChallenge] = useState<RequestCertificateResponse | null>(null);
  const cert = certificates.find((c) => c.domain === primaryDomain);

  const requestSsl = async () => {
    if (!primaryDomain) {
      onError("本站点没有绑定域名,无法申请证书 — 在'基础'Tab 填上 domains 后再来");
      return;
    }
    try {
      // 显式 DNS-01:NAT VPS 上 80 端口拿不到,HTTP-01 走不通;并且
      // 不传 challenge_type 后端会 fall through 到一个**自签**回退路径,
      // "一键成功"但浏览器不信任 —— 等于骗用户。这里硬走 DNS-01。
      const response = await clients.ssl.requestCertificate({
        domain: primaryDomain,
        email: "admin@example.com",
        challengeType: AcmeChallengeType.DNS_01
      });
      if (response.dnsRecordName || response.dnsRecordValue) {
        // 第一次调用:ACME 返回 TXT 挑战,要用户去 DNS 服务商添加
        setPendingChallenge(response);
        onMessage(
          response.status?.message ??
            "请按下方提示添加 TXT 记录,DNS 生效后再点一次申请完成签发"
        );
      } else if (response.certificate) {
        // 第二次调用(TXT 生效后):真正拿到证书
        setPendingChallenge(null);
        onMessage(`${response.certificate.domain} 已签发`);
      } else {
        onMessage(response.status?.message ?? "申请已提交,请按提示继续");
      }
      onChanged();
    } catch (err) {
      onError(safeError(err));
    }
  };
  const importCertificate = async () => {
    try {
      await clients.ssl.importCertificate(importForm);
      onMessage(`${importForm.domain} 已导入`);
      onChanged();
    } catch (err) {
      onError(safeError(err));
    }
  };
  const renewCert = async () => {
    if (!cert) return;
    try {
      const response = await clients.ssl.renewCertificate({ domain: cert.domain });
      onMessage(response.output || `${cert.domain} 已续签`);
      onChanged();
    } catch (err) {
      onError(safeError(err));
    }
  };

  return (
    <div className="flex flex-col gap-4">
      {cert ? (
        <div className="rounded border border-success/30 bg-success/5 p-3 text-sm flex items-center justify-between gap-3">
          <div>
            <div className="font-medium">{cert.domain}</div>
            <div className="text-xs text-muted-foreground">
              剩余 {cert.daysUntilExpiry} 天 · 分组 {cert.group || "default"}
            </div>
          </div>
          <div className="flex gap-1">
            <UIButton size="sm" variant="outline" onClick={() => void renewCert()}>
              <RotateCw className="size-3.5" />
              续签
            </UIButton>
            {onRequestRevokeCert && (
              <UIButton
                size="sm"
                variant="ghost"
                title="撤销证书"
                onClick={() => onRequestRevokeCert(cert)}
              >
                <Trash2 className="size-3.5 text-destructive" />
              </UIButton>
            )}
          </div>
        </div>
      ) : (
        <div className="rounded border border-warning/30 bg-warning/5 p-3 text-sm text-muted-foreground">
          {primaryDomain || "(无域名)"} 当前无证书,可走自动签发或手动导入。
        </div>
      )}

      <div className="space-y-2">
        <div className="text-sm font-medium">自动签发(Let's Encrypt DNS-01)</div>
        <div className="text-xs text-muted-foreground">
          <strong>两步</strong>:第一次点 → 后端给出 TXT 记录 → 到你的 DNS 服务商加上 →
          DNS 生效后(可用 `dig TXT _acme-challenge.&lt;域名&gt;` 验证)再点一次完成签发。
        </div>
        <UIButton
          size="sm"
          onClick={() => void requestSsl()}
          disabled={!primaryDomain}
        >
          <ShieldCheck className="size-3.5" />
          {primaryDomain
            ? pendingChallenge
              ? `已添加 TXT,继续签发 ${primaryDomain}`
              : `开始申请 ${primaryDomain}`
            : "需先在'基础'Tab 绑定域名"}
        </UIButton>
        {pendingChallenge && (pendingChallenge.dnsRecordName || pendingChallenge.dnsRecordValue) && (
          <div className="rounded border border-info/30 bg-info/5 p-3 text-xs space-y-2">
            <div className="font-medium">需要在 DNS 服务商添加这条 TXT 记录:</div>
            <div className="grid gap-1 sm:grid-cols-[80px_1fr]">
              <span className="text-muted-foreground">记录类型</span>
              <span className="font-mono">TXT</span>
              <span className="text-muted-foreground">主机记录</span>
              <span className="font-mono break-all">{pendingChallenge.dnsRecordName || "(待返回)"}</span>
              <span className="text-muted-foreground">记录值</span>
              <span className="font-mono break-all">{pendingChallenge.dnsRecordValue || "(待返回)"}</span>
            </div>
            <div className="text-muted-foreground">
              加完后等几分钟 DNS 全球生效,再点上面按钮完成签发。
            </div>
          </div>
        )}
      </div>

      <div className="space-y-2">
        <div className="text-sm font-medium">手动导入证书</div>
        <div className="grid gap-2 sm:grid-cols-2">
          <div className="grid gap-1">
            <UILabel htmlFor="ssl-domain">域名</UILabel>
            <UIInput
              id="ssl-domain"
              value={importForm.domain}
              onChange={(event) =>
                setImportForm({ ...importForm, domain: event.target.value })
              }
            />
          </div>
          <div className="grid gap-1">
            <UILabel htmlFor="ssl-group">分组</UILabel>
            <UIInput
              id="ssl-group"
              value={importForm.group}
              onChange={(event) =>
                setImportForm({ ...importForm, group: event.target.value })
              }
            />
          </div>
        </div>
        <textarea
          className="pem-input w-full"
          rows={6}
          onChange={(event) =>
            setImportForm({ ...importForm, certificatePem: event.target.value })
          }
          placeholder="-----BEGIN CERTIFICATE-----"
          value={importForm.certificatePem}
        />
        <textarea
          className="pem-input w-full"
          rows={6}
          onChange={(event) =>
            setImportForm({ ...importForm, privateKeyPem: event.target.value })
          }
          placeholder="-----BEGIN PRIVATE KEY-----"
          value={importForm.privateKeyPem}
        />
        <UIButton size="sm" variant="outline" onClick={() => void importCertificate()}>
          <FileUp className="size-3.5" />
          导入
        </UIButton>
      </div>
    </div>
  );
}

function PerSiteReverseProxyPanel({
  site,
  proxyRules,
  clients,
  onChanged,
  onMessage,
  onError
}: SiteSheetProps & { site: SiteItem }) {
  const primaryDomain = site.domains[0] || "";
  const [form, setForm] = useState({
    name: site.name,
    pathPrefix: "/api/",
    targets: "http://127.0.0.1:3000",
    method: "least_conn" as "round_robin" | "least_conn" | "ip_hash",
    cacheEnabled: false,
    rateLimit: 120
  });
  const ownRules = proxyRules.filter((rule) => rule.domain === primaryDomain);

  const submit = async () => {
    const targets = form.targets
      .split(",")
      .map((target) => target.trim())
      .filter(Boolean)
      .map((url) => ({ url, weight: 1, healthy: true }));
    try {
      await clients.site.upsertReverseProxyRule({
        rule: {
          id: "",
          name: form.name,
          domain: primaryDomain,
          pathPrefix: form.pathPrefix,
          targets,
          loadBalanceMethod: form.method,
          cacheEnabled: form.cacheEnabled,
          rateLimitPerMinute: form.rateLimit,
          enabled: true,
          configPath: "",
          createdAtSeconds: 0n,
          updatedAtSeconds: 0n
        }
      });
      onMessage(`${form.name} 已保存`);
      onChanged();
    } catch (err) {
      onError(safeError(err));
    }
  };
  const remove = async (rule: ReverseProxyRule) => {
    try {
      await clients.site.deleteReverseProxyRule({ id: rule.id });
      onMessage(`${rule.name} 已删除`);
      onChanged();
    } catch (err) {
      onError(safeError(err));
    }
  };

  return (
    <div className="flex flex-col gap-3">
      <div className="text-sm font-medium">当前规则({ownRules.length})</div>
      {ownRules.length === 0 ? (
        <div className="empty-state text-xs">{primaryDomain} 暂无反代规则</div>
      ) : (
        <div className="flex flex-col gap-2">
          {ownRules.map((rule) => (
            <div
              key={rule.id}
              className="rounded border border-border p-2 text-xs flex items-center justify-between gap-2"
            >
              <div>
                <div className="font-medium">
                  {rule.name} · {rule.domain}
                  {rule.pathPrefix}
                </div>
                <div className="text-muted-foreground font-mono">
                  {rule.loadBalanceMethod || "round_robin"} ·{" "}
                  {rule.targets.map((target) => target.url).join(", ")}
                </div>
              </div>
              <UIButton size="sm" variant="outline" onClick={() => void remove(rule)}>
                <Trash2 className="size-3.5" />
                删除
              </UIButton>
            </div>
          ))}
        </div>
      )}

      <div className="text-sm font-medium pt-2">新增规则</div>
      <div className="grid gap-2 sm:grid-cols-2">
        <div className="grid gap-1">
          <UILabel htmlFor="rp-name">名称</UILabel>
          <UIInput
            id="rp-name"
            value={form.name}
            onChange={(event) => setForm({ ...form, name: event.target.value })}
          />
        </div>
        <div className="grid gap-1">
          <UILabel htmlFor="rp-path">路径前缀</UILabel>
          <UIInput
            id="rp-path"
            value={form.pathPrefix}
            onChange={(event) => setForm({ ...form, pathPrefix: event.target.value })}
          />
        </div>
        <div className="grid gap-1 sm:col-span-2">
          <UILabel htmlFor="rp-targets">目标(逗号分隔)</UILabel>
          <UIInput
            id="rp-targets"
            value={form.targets}
            onChange={(event) => setForm({ ...form, targets: event.target.value })}
          />
        </div>
        <div className="grid gap-1">
          <UILabel>负载均衡</UILabel>
          <Select
            value={form.method}
            onValueChange={(value) =>
              setForm({ ...form, method: value as typeof form.method })
            }
          >
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="round_robin">轮询</SelectItem>
              <SelectItem value="least_conn">最少连接</SelectItem>
              <SelectItem value="ip_hash">IP Hash</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <div className="grid gap-1">
          <UILabel htmlFor="rp-rate">限速 / 分钟</UILabel>
          <UIInput
            id="rp-rate"
            type="number"
            value={form.rateLimit}
            onChange={(event) =>
              setForm({ ...form, rateLimit: Number.parseInt(event.target.value, 10) || 0 })
            }
          />
        </div>
        <div className="flex items-center gap-2 sm:col-span-2">
          <Switch
            checked={form.cacheEnabled}
            onCheckedChange={(checked) => setForm({ ...form, cacheEnabled: checked })}
          />
          <UILabel>启用缓存</UILabel>
        </div>
      </div>
      <div className="flex justify-end">
        <UIButton size="sm" onClick={() => void submit()}>
          <Plus className="size-3.5" />
          新增规则
        </UIButton>
      </div>
    </div>
  );
}

function RewritePanel({ templates }: { templates: RewriteTemplate[] }) {
  const [selectedId, setSelectedId] = useState(templates[0]?.id ?? "");
  const [content, setContent] = useState(templates[0]?.content ?? "");

  useEffect(() => {
    if (!selectedId && templates[0]) {
      setSelectedId(templates[0].id);
      setContent(templates[0].content);
    }
  }, [templates, selectedId]);

  return (
    <div className="flex flex-col gap-3">
      <div className="grid gap-1">
        <UILabel>模板</UILabel>
        <Select
          value={selectedId}
          onValueChange={(id) => {
            setSelectedId(id);
            const template = templates.find((tpl) => tpl.id === id);
            if (template) setContent(template.content);
          }}
        >
          <SelectTrigger>
            <SelectValue placeholder="选择模板" />
          </SelectTrigger>
          <SelectContent>
            {templates.map((template) => (
              <SelectItem key={template.id} value={template.id}>
                {template.name}
                {template.stack ? ` · ${template.stack}` : ""}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <textarea
        className="pem-input code-input w-full"
        rows={12}
        value={content}
        onChange={(event) => setContent(event.target.value)}
      />
      <p className="text-xs text-muted-foreground">
        伪静态目前只在前端预览;站点 vhost 写入由 site.rs render_site_config 决定,后续在 UpdateSite RPC 落地时把这段配置打通持久化。
      </p>
    </div>
  );
}

function DatabasePanel({ clients }: { clients: Clients }) {
  const [dsn, setDsn] = useState("sqlite::memory:");
  const [sql, setSql] = useState("select 1 as value");
  const [columns, setColumns] = useState<string[]>([]);
  const [rows, setRows] = useState<string[][]>([]);
  const [databases, setDatabases] = useState<string[]>([]);
  const [newDbName, setNewDbName] = useState("");
  const [userForm, setUserForm] = useState({
    username: "app",
    password: "",
    database: ""
  });
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  // ====== Phase D: SQLite 文件管理 ======
  const [sqliteFiles, setSqliteFiles] = useState<SqliteFile[]>([]);
  const [scanDirs, setScanDirs] = useState("");
  const [newSqlitePath, setNewSqlitePath] = useState("");
  // ====== Phase D: Redis 监控 ======
  const [redisUrl, setRedisUrl] = useState("redis://127.0.0.1:6379");
  const [redisInfo, setRedisInfo] = useState<RedisInfo | undefined>(undefined);

  const refreshSqlite = useCallback(async () => {
    try {
      const response = await clients.database.listSqliteFiles({
        scanDirs: scanDirs
          .split(/[\s,]+/)
          .map((s) => s.trim())
          .filter(Boolean)
      });
      setSqliteFiles(response.files);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  }, [clients, scanDirs]);

  const createSqliteFile = async () => {
    if (!newSqlitePath.trim()) {
      setError("请填写 SQLite 文件路径");
      return;
    }
    try {
      await clients.database.createSqliteFile({ path: newSqlitePath.trim() });
      setMessage(`已创建 ${newSqlitePath}`);
      setNewSqlitePath("");
      void refreshSqlite();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const vacuumSqliteFile = async (path: string) => {
    try {
      const response = await clients.database.vacuumSqlite({ path });
      const saved =
        Number(response.sizeBeforeBytes) - Number(response.sizeAfterBytes);
      setMessage(
        `VACUUM 完成,${saved > 0 ? `节省 ${formatBytes(BigInt(saved))}` : "无空间可压缩"}`
      );
      void refreshSqlite();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const refreshRedis = useCallback(async () => {
    try {
      const response = await clients.database.getRedisInfo({ url: redisUrl });
      setRedisInfo(response.info);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  }, [clients, redisUrl]);

  useEffect(() => {
    void refreshSqlite();
    void refreshRedis();
  }, [refreshSqlite, refreshRedis]);

  const listDatabases = useCallback(async () => {
    try {
      const response = await clients.database.listDatabases({ dsn });
      setDatabases(response.databases.map((database) => database.name));
      setMessage(`已连接,共 ${response.databases.length} 个数据库`);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  }, [clients, dsn]);

  const createDatabase = async () => {
    if (!newDbName.trim()) return;
    try {
      await clients.database.createDatabase({ dsn, name: newDbName.trim() });
      setMessage(`数据库 ${newDbName} 已创建`);
      setNewDbName("");
      void listDatabases();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const backupDatabase = async (database: string) => {
    try {
      const response = await clients.database.backupDatabase({ dsn, database });
      setMessage(`备份完成:${response.downloadUrl || "下载链接已生成"}`);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  };

  const createDatabaseUser = async () => {
    try {
      await clients.database.createDatabaseUser({
        dsn,
        username: userForm.username,
        password: userForm.password,
        database: userForm.database
      });
      setMessage(`用户 ${userForm.username} 已创建并授权 ${userForm.database}`);
      setUserForm((prev) => ({ ...prev, password: "" }));
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  };

  const execute = async () => {
    try {
      const response = await clients.database.executeSql({ dsn, sql, maxRows: 200 });
      setColumns(response.columns);
      setRows(response.rows.map((row) => row.values));
      setMessage(`返回 ${response.rows.length} 行,影响 ${response.rowsAffected} 行`);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  };

  return (
    <section className="flex flex-col gap-5">
      <header className="flex flex-col gap-1">
        <h1 className="text-2xl font-semibold tracking-tight m-0">数据库</h1>
        <p className="text-sm text-muted-foreground m-0">
          轻量优先:SQLite 单文件 → Redis(可选)→ 通用 DSN(MySQL / PostgreSQL,需 ≥ 256MB RAM)
        </p>
      </header>

      {error && (
        <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
          {error}
        </div>
      )}
      {message && !error && (
        <div className="rounded-md border border-success/40 bg-success/10 px-3 py-2 text-sm text-success">
          {message}
        </div>
      )}

      <Tabs defaultValue="sqlite">
        <TabsList>
          <TabsTrigger value="sqlite">SQLite</TabsTrigger>
          <TabsTrigger value="redis">Redis</TabsTrigger>
          <TabsTrigger value="dsn">通用 DSN</TabsTrigger>
        </TabsList>

        <TabsContent value="sqlite" className="mt-4">
          <Card>
            <CardHeader>
              <CardTitle>SQLite 文件</CardTitle>
              <CardDescription>
                嵌入式数据库,无需常驻进程,RustPanel 在低配 VPS 上的默认推荐。每个站点
                一个 .db 文件即可。
              </CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <div className="grid gap-2">
                <UILabel htmlFor="sqlite-dirs">扫描目录(可选,空格或逗号分隔)</UILabel>
                <div className="flex gap-2">
                  <UIInput
                    id="sqlite-dirs"
                    placeholder="留空使用默认 /var/lib/rustpanel/sqlite, /srv/sqlite ..."
                    value={scanDirs}
                    onChange={(event) => setScanDirs(event.target.value)}
                  />
                  <UIButton variant="outline" onClick={() => void refreshSqlite()}>
                    <RefreshCw className="size-4" />
                    扫描
                  </UIButton>
                </div>
              </div>
              <div className="flex gap-2">
                <UIInput
                  className="flex-1"
                  placeholder="新建文件,如 /var/lib/rustpanel/sqlite/blog.db"
                  value={newSqlitePath}
                  onChange={(event) => setNewSqlitePath(event.target.value)}
                />
                <UIButton onClick={() => void createSqliteFile()}>
                  <Plus className="size-4" />
                  创建
                </UIButton>
              </div>
              {sqliteFiles.length === 0 ? (
                <div className="empty-state text-sm">
                  未发现 SQLite 文件 —— 检查扫描目录是否存在,或先创建一个
                </div>
              ) : (
                <Table>
                  <TableHeader>
                    <UITableRow>
                      <TableHead>路径</TableHead>
                      <TableHead className="text-right">大小</TableHead>
                      <TableHead>修改时间</TableHead>
                      <TableHead className="text-right">操作</TableHead>
                    </UITableRow>
                  </TableHeader>
                  <TableBody>
                    {sqliteFiles.map((file) => (
                      <UITableRow key={file.path}>
                        <TableCell className="font-mono text-xs">{file.path}</TableCell>
                        <TableCell className="text-right tabular-nums">
                          {formatBytes(file.sizeBytes)}
                        </TableCell>
                        <TableCell className="text-xs text-muted-foreground">
                          {file.modifiedAtSeconds > 0n
                            ? new Date(Number(file.modifiedAtSeconds) * 1000).toLocaleString()
                            : "-"}
                        </TableCell>
                        <TableCell className="text-right">
                          <UIButton
                            variant="outline"
                            size="sm"
                            onClick={() => void vacuumSqliteFile(file.path)}
                          >
                            <RotateCw className="size-3.5" />
                            VACUUM
                          </UIButton>
                        </TableCell>
                      </UITableRow>
                    ))}
                  </TableBody>
                </Table>
              )}
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="redis" className="mt-4">
          <Card>
            <CardHeader>
              <CardTitle>Redis 连接监控</CardTitle>
              <CardDescription>
                小内存机器推荐安装 redis-tuned(maxmemory 30MB + LRU)。这里读 INFO 命令展示
                关键指标。
              </CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <div className="grid gap-2">
                <UILabel htmlFor="redis-url">Redis URL</UILabel>
                <div className="flex gap-2">
                  <UIInput
                    id="redis-url"
                    placeholder="redis://127.0.0.1:6379 或 rediss://user:pass@host:6380/0"
                    value={redisUrl}
                    onChange={(event) => setRedisUrl(event.target.value)}
                  />
                  <UIButton onClick={() => void refreshRedis()}>
                    <RefreshCw className="size-4" />
                    连接
                  </UIButton>
                </div>
              </div>

              {redisInfo && (
                <RedisInfoView info={redisInfo} />
              )}
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="dsn" className="mt-4">
          <Card>
            <CardContent className="flex flex-col gap-3">
              <div className="grid gap-2">
                <UILabel htmlFor="db-dsn">数据库连接 DSN</UILabel>
                <div className="flex gap-2">
                  <UIInput
                    id="db-dsn"
                    placeholder="mysql://user:pass@host/db 或 postgres://user:pass@host/db"
                    value={dsn}
                    onChange={(event) => setDsn(event.target.value)}
                  />
                  <UIButton onClick={() => void listDatabases()}>
                    <RefreshCw className="size-4" />
                    连接
                  </UIButton>
                </div>
                <span className="text-xs text-muted-foreground">
                  提示:MySQL/PostgreSQL 容器运行时建议 ≥ 256MB RAM。低配机器请优先使用
                  SQLite Tab。
                </span>
              </div>
            </CardContent>
          </Card>

          <Tabs defaultValue="databases" className="mt-4">
            <TabsList>
              <TabsTrigger value="databases">数据库</TabsTrigger>
              <TabsTrigger value="users">用户</TabsTrigger>
              <TabsTrigger value="sql">SQL 控制台</TabsTrigger>
            </TabsList>

        <TabsContent value="databases" className="mt-4">
          <Card>
            <CardHeader>
              <CardTitle>数据库列表</CardTitle>
              <CardDescription>新建数据库,或对已有库一键备份</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-3">
              <div className="flex flex-wrap gap-2">
                <UIInput
                  className="flex-1 min-w-[200px]"
                  placeholder="新数据库名"
                  value={newDbName}
                  onChange={(event) => setNewDbName(event.target.value)}
                />
                <UIButton onClick={() => void createDatabase()}>
                  <Plus className="size-4" />
                  创建数据库
                </UIButton>
              </div>
              {databases.length === 0 ? (
                <div className="empty-state text-sm">先连接 DSN 以加载数据库列表</div>
              ) : (
                <Table>
                  <TableHeader>
                    <UITableRow>
                      <TableHead>名称</TableHead>
                      <TableHead className="text-right">操作</TableHead>
                    </UITableRow>
                  </TableHeader>
                  <TableBody>
                    {databases.map((database) => (
                      <UITableRow key={database}>
                        <TableCell className="font-medium">{database}</TableCell>
                        <TableCell className="text-right">
                          <UIButton
                            variant="outline"
                            size="sm"
                            onClick={() => void backupDatabase(database)}
                          >
                            <Download className="size-3.5" />
                            备份
                          </UIButton>
                        </TableCell>
                      </UITableRow>
                    ))}
                  </TableBody>
                </Table>
              )}
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="users" className="mt-4">
          <Card>
            <CardHeader>
              <CardTitle>创建数据库用户</CardTitle>
              <CardDescription>为指定数据库创建独立账号并授权</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-3">
              <div className="grid gap-3 md:grid-cols-3">
                <div className="grid gap-2">
                  <UILabel htmlFor="db-user">用户名</UILabel>
                  <UIInput
                    id="db-user"
                    value={userForm.username}
                    onChange={(event) => setUserForm((prev) => ({ ...prev, username: event.target.value }))}
                  />
                </div>
                <div className="grid gap-2">
                  <UILabel htmlFor="db-pass">密码</UILabel>
                  <UIInput
                    id="db-pass"
                    type="password"
                    value={userForm.password}
                    onChange={(event) => setUserForm((prev) => ({ ...prev, password: event.target.value }))}
                  />
                </div>
                <div className="grid gap-2">
                  <UILabel htmlFor="db-target">授权数据库</UILabel>
                  <UIInput
                    id="db-target"
                    value={userForm.database}
                    onChange={(event) => setUserForm((prev) => ({ ...prev, database: event.target.value }))}
                  />
                </div>
              </div>
              <div className="flex justify-end">
                <UIButton onClick={() => void createDatabaseUser()}>
                  <Plus className="size-4" />
                  创建并授权
                </UIButton>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="sql" className="mt-4">
          <Card>
            <CardHeader>
              <CardTitle>SQL 控制台</CardTitle>
              <CardDescription>仅返回最多 200 行结果</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-3">
              <div className="border border-border rounded-md overflow-hidden">
                <Editor
                  height="240px"
                  language="sql"
                  onChange={(value) => setSql(value ?? "")}
                  value={sql}
                  options={{ minimap: { enabled: false }, fontSize: 13 }}
                />
              </div>
              <div className="flex justify-end">
                <UIButton onClick={() => void execute()}>
                  <Play className="size-4" />
                  执行
                </UIButton>
              </div>
              {columns.length > 0 && (
                <div className="overflow-x-auto">
                  <table className="result-table">
                    <thead>
                      <tr>
                        {columns.map((column) => (
                          <th key={column}>{column}</th>
                        ))}
                      </tr>
                    </thead>
                    <tbody>
                      {rows.map((row, index) => (
                        <tr key={index}>
                          {row.map((value, cell) => (
                            <td key={cell}>{value}</td>
                          ))}
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </CardContent>
          </Card>
        </TabsContent>
          </Tabs>
        </TabsContent>
      </Tabs>
    </section>
  );
}

function RedisInfoView({ info }: { info: RedisInfo }) {
  if (!info.reachable) {
    return (
      <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
        无法连接 Redis:{info.error || "未知错误"}
      </div>
    );
  }
  const hits = Number(info.keyspaceHits);
  const misses = Number(info.keyspaceMisses);
  const hitRate = hits + misses > 0 ? (hits / (hits + misses)) * 100 : 0;
  const memoryPercent =
    info.maxMemoryBytes > 0n
      ? (Number(info.usedMemoryBytes) / Number(info.maxMemoryBytes)) * 100
      : 0;

  return (
    <div className="grid grid-cols-2 md:grid-cols-3 gap-3">
      <RedisStat label="版本" value={info.version || "-"} />
      <RedisStat label="模式" value={info.mode || "-"} />
      <RedisStat label="客户端" value={String(info.connectedClients)} />
      <RedisStat
        label="已用内存"
        value={formatBytes(info.usedMemoryBytes)}
        detail={
          info.maxMemoryBytes > 0n
            ? `${memoryPercent.toFixed(1)}% / ${formatBytes(info.maxMemoryBytes)}`
            : "无 maxmemory 限制"
        }
      />
      <RedisStat label="淘汰策略" value={info.maxMemoryPolicy || "noeviction"} />
      <RedisStat
        label="命中率"
        value={`${hitRate.toFixed(1)}%`}
        detail={`${hits} 命中 / ${misses} 未命中`}
      />
      <RedisStat label="累计命令" value={String(info.totalCommandsProcessed)} />
      <RedisStat label="运行时长" value={formatDuration(info.uptimeSeconds)} />
    </div>
  );
}

function RedisStat({
  label,
  value,
  detail
}: {
  label: string;
  value: string;
  detail?: string;
}) {
  return (
    <div className="flex flex-col gap-0.5 rounded-md border border-border bg-card p-3">
      <span className="text-xs text-muted-foreground">{label}</span>
      <span className="font-medium tabular-nums">{value}</span>
      {detail && <span className="text-xs text-muted-foreground">{detail}</span>}
    </div>
  );
}

// Phase E:常见 Cron 任务预设,适合微型 VPS 上的日常维护。
const CRON_PRESETS: Array<{ id: string; label: string; cron: string; command: string }> = [
  {
    id: "sqlite-daily-backup",
    label: "每日 SQLite 备份",
    cron: "0 0 3 * * *",
    command: "tar czf /var/backups/sqlite-$(date +%F).tgz /var/lib/rustpanel/sqlite/"
  },
  {
    id: "restic-weekly",
    label: "每周 restic 增量备份",
    cron: "0 0 4 * * 0",
    command: "restic -r $RESTIC_REPO backup /var/lib /etc --tag weekly"
  },
  {
    id: "logrotate-monthly",
    label: "每月清理日志",
    cron: "0 0 5 1 * *",
    command: "find /var/log -name '*.log' -mtime +30 -delete"
  },
  {
    id: "disk-alert",
    label: "磁盘 80% 告警",
    cron: "0 */15 * * * *",
    command: "df / | awk 'NR==2 && $5+0>80 {print \"disk \"$5}' | logger -t rustpanel"
  },
  {
    id: "ssl-renew-check",
    label: "SSL 续期检查(每天 02:00)",
    cron: "0 0 2 * * *",
    command: "rustpanel-backend ssl renew-due"
  },
  {
    id: "fail2ban-status",
    label: "fail2ban 状态汇报",
    cron: "0 0 */6 * * *",
    command: "fail2ban-client status | logger -t rustpanel"
  }
];

function CronPanel({ clients }: { clients: Clients }) {
  const [tasks, setTasks] = useState<CronTask[]>([]);
  const [form, setForm] = useState({ name: "daily-backup", cron: "0 0 2 * * *", command: "echo ok" });
  const [presetId, setPresetId] = useState("custom");
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
        <div className="input-row">
          <span>常用模板</span>
          <Select
            value={presetId}
            onValueChange={(value) => {
              setPresetId(value);
              const preset = CRON_PRESETS.find((p) => p.id === value);
              if (preset) {
                setForm({ name: preset.id, cron: preset.cron, command: preset.command });
              }
            }}
          >
            <SelectTrigger>
              <SelectValue placeholder="自定义" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="custom">自定义</SelectItem>
              {CRON_PRESETS.map((preset) => (
                <SelectItem key={preset.id} value={preset.id}>
                  {preset.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
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

// ====== 面板设置 ======
function SettingsPage({ clients, onLogout }: { clients: Clients; onLogout: () => void }) {
  const [options, setOptions] = useState<SecurityOptionsForm>(defaultSecurityOptions);
  const [systemInfo, setSystemInfo] = useState({ hostname: "-", os: "-", kernel: "-", arch: "-" });
  const [certs, setCerts] = useState<CertificateItem[]>([]);
  const [importForm, setImportForm] = useState({
    domain: "",
    group: "default",
    certificatePem: "",
    privateKeyPem: ""
  });
  const [acmeForm, setAcmeForm] = useState({
    domain: "",
    email: "",
    challenge: "dns01" as "http01" | "dns01"
  });
  const [acmeChallengeHint, setAcmeChallengeHint] = useState<{
    name: string;
    value: string;
  } | null>(null);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");

  const refresh = useCallback(async () => {
    try {
      const [security, info, certResponse] = await Promise.all([
        clients.security.listFirewallRules({}),
        clients.system.getSystemInfo({}),
        clients.ssl.listCertificates({})
      ]);
      if (security.options) {
        setOptions({
          disablePing: security.options.disablePing,
          scanProtectionEnabled: security.options.scanProtectionEnabled,
          scanBurst: security.options.scanBurst,
          scanWindowSeconds: security.options.scanWindowSeconds,
          backendPreference: security.options.backendPreference,
          lastApplyMessage: security.options.lastApplyMessage,
          panelAccessPath: security.options.panelAccessPath,
          panelListenAddr: security.options.panelListenAddr,
          twoFactorRequired: security.options.twoFactorRequired
        });
      }
      setSystemInfo({
        hostname: info.hostname || "-",
        os: info.operatingSystem || "-",
        kernel: info.kernelVersion || "-",
        arch: info.architecture || "-"
      });
      setCerts(certResponse.certificates);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  }, [clients]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const saveOptions = async () => {
    try {
      const response = await clients.security.updateSecurityOptions({ options });
      if (response.options) {
        setOptions({ ...defaultSecurityOptions, ...response.options });
      }
      setMessage(response.options?.lastApplyMessage || "设置已保存");
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  };

  const importCert = async () => {
    try {
      await clients.ssl.importCertificate(importForm);
      setMessage("证书已导入");
      setImportForm({ domain: "", group: "default", certificatePem: "", privateKeyPem: "" });
      void refresh();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const requestAcmeCert = async () => {
    if (!acmeForm.domain.trim() || !acmeForm.email.trim()) {
      setError("域名和邮箱必填");
      return;
    }
    try {
      const response = await clients.ssl.requestCertificate({
        domain: acmeForm.domain.trim(),
        email: acmeForm.email.trim(),
        challengeType:
          acmeForm.challenge === "dns01"
            ? AcmeChallengeType.DNS_01
            : AcmeChallengeType.HTTP_01,
        dnsProvider: acmeForm.challenge === "dns01" ? "manual" : "",
        dnsCredentials: ""
      });
      if (response.dnsRecordName) {
        setAcmeChallengeHint({
          name: response.dnsRecordName,
          value: response.dnsRecordValue
        });
        setMessage(response.status?.message || "请添加下方 TXT 记录后再点一次申请");
      } else {
        setAcmeChallengeHint(null);
        setMessage("证书已签发");
        void refresh();
      }
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  };

  return (
    <section className="flex flex-col gap-5 max-w-4xl">
      <header className="flex flex-col gap-1">
        <h1 className="text-2xl font-semibold tracking-tight m-0">面板设置</h1>
        <p className="text-sm text-muted-foreground m-0">控制面板访问入口、安全策略与 SSL 证书</p>
      </header>

      {error && (
        <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
          {error}
        </div>
      )}
      {message && !error && (
        <div className="rounded-md border border-success/40 bg-success/10 px-3 py-2 text-sm text-success">
          {message}
        </div>
      )}

      <Tabs defaultValue="basic">
        <TabsList>
          <TabsTrigger value="basic">基础</TabsTrigger>
          <TabsTrigger value="security">安全</TabsTrigger>
          <TabsTrigger value="ssl">SSL</TabsTrigger>
          <TabsTrigger value="modules">模块</TabsTrigger>
          <TabsTrigger value="about">关于</TabsTrigger>
        </TabsList>

        <TabsContent value="modules" className="mt-4">
          <ModulesPanel clients={clients} />
        </TabsContent>

        <TabsContent value="basic" className="mt-4">
          <Card>
            <CardHeader>
              <CardTitle>访问入口</CardTitle>
              <CardDescription>修改面板监听地址与访问路径以增强安全</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <div className="grid gap-2">
                <UILabel htmlFor="settings-listen">监听地址</UILabel>
                <UIInput
                  id="settings-listen"
                  placeholder="0.0.0.0:8443"
                  value={options.panelListenAddr}
                  onChange={(event) =>
                    setOptions((prev) => ({ ...prev, panelListenAddr: event.target.value }))
                  }
                />
                <span className="text-xs text-muted-foreground">
                  修改监听地址需要重启面板服务才能生效
                </span>
              </div>
              <div className="grid gap-2">
                <UILabel htmlFor="settings-path">访问路径</UILabel>
                <UIInput
                  id="settings-path"
                  placeholder="/admin"
                  value={options.panelAccessPath}
                  onChange={(event) =>
                    setOptions((prev) => ({ ...prev, panelAccessPath: event.target.value }))
                  }
                />
                <span className="text-xs text-muted-foreground">
                  设置自定义路径可降低被扫描器探测到的概率
                </span>
              </div>
              <div className="flex justify-end">
                <UIButton onClick={() => void saveOptions()}>
                  <Save className="size-4" />
                  保存基础设置
                </UIButton>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="security" className="mt-4">
          <Card>
            <CardHeader>
              <CardTitle>登录与请求保护</CardTitle>
              <CardDescription>两步验证、ICMP 与端口扫描防护</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <div className="flex items-center justify-between rounded-md border border-border bg-card px-4 py-3">
                <div className="flex flex-col gap-0.5">
                  <span className="font-medium">强制两步验证</span>
                  <span className="text-xs text-muted-foreground">
                    所有面板用户登录时必须输入 TOTP 验证码
                  </span>
                </div>
                <Switch
                  checked={options.twoFactorRequired}
                  onCheckedChange={(checked) =>
                    setOptions((prev) => ({ ...prev, twoFactorRequired: checked }))
                  }
                />
              </div>
              <div className="flex items-center justify-between rounded-md border border-border bg-card px-4 py-3">
                <div className="flex flex-col gap-0.5">
                  <span className="font-medium">禁用 ICMP Ping</span>
                  <span className="text-xs text-muted-foreground">
                    阻止外部使用 ping 探测主机存活
                  </span>
                </div>
                <Switch
                  checked={options.disablePing}
                  onCheckedChange={(checked) =>
                    setOptions((prev) => ({ ...prev, disablePing: checked }))
                  }
                />
              </div>
              <div className="flex items-center justify-between rounded-md border border-border bg-card px-4 py-3">
                <div className="flex flex-col gap-0.5">
                  <span className="font-medium">端口扫描防护</span>
                  <span className="text-xs text-muted-foreground">
                    阈值 {options.scanBurst} 次 / {options.scanWindowSeconds} 秒
                  </span>
                </div>
                <Switch
                  checked={options.scanProtectionEnabled}
                  onCheckedChange={(checked) =>
                    setOptions((prev) => ({ ...prev, scanProtectionEnabled: checked }))
                  }
                />
              </div>
              <div className="flex justify-end">
                <UIButton onClick={() => void saveOptions()}>
                  <Save className="size-4" />
                  保存安全设置
                </UIButton>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="ssl" className="mt-4 flex flex-col gap-4">
          <Card>
            <CardHeader>
              <CardTitle>申请 Let's Encrypt 证书</CardTitle>
              <CardDescription>
                NAT VPS 拿不到公网 80,默认 DNS-01 挑战:面板返回 TXT 记录,你加到 DNS 后再点一次申请。
              </CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <div className="grid gap-3 md:grid-cols-2">
                <div className="grid gap-2">
                  <UILabel htmlFor="acme-domain">域名</UILabel>
                  <UIInput
                    id="acme-domain"
                    placeholder="example.com"
                    value={acmeForm.domain}
                    onChange={(event) =>
                      setAcmeForm((prev) => ({ ...prev, domain: event.target.value }))
                    }
                  />
                </div>
                <div className="grid gap-2">
                  <UILabel htmlFor="acme-email">联系邮箱</UILabel>
                  <UIInput
                    id="acme-email"
                    type="email"
                    placeholder="admin@example.com"
                    value={acmeForm.email}
                    onChange={(event) =>
                      setAcmeForm((prev) => ({ ...prev, email: event.target.value }))
                    }
                  />
                </div>
                <div className="grid gap-2">
                  <UILabel htmlFor="acme-challenge">挑战方式</UILabel>
                  <Select
                    value={acmeForm.challenge}
                    onValueChange={(value) =>
                      setAcmeForm((prev) => ({
                        ...prev,
                        challenge: value as "http01" | "dns01"
                      }))
                    }
                  >
                    <SelectTrigger id="acme-challenge">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="dns01">DNS-01(NAT VPS 推荐)</SelectItem>
                      <SelectItem value="http01">HTTP-01(需开放 80 端口)</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
              </div>
              {acmeChallengeHint && (
                <div className="rounded-md border border-info/40 bg-info/10 px-3 py-3 text-sm flex flex-col gap-2">
                  <div className="font-medium text-info">需要添加 DNS TXT 记录</div>
                  <div className="grid grid-cols-[80px_1fr] gap-x-3 gap-y-1 text-xs font-mono">
                    <span className="text-muted-foreground">RR Name</span>
                    <span>{acmeChallengeHint.name}</span>
                    <span className="text-muted-foreground">Type</span>
                    <span>TXT</span>
                    <span className="text-muted-foreground">Value</span>
                    <span className="break-all">{acmeChallengeHint.value}</span>
                  </div>
                  <div className="text-xs text-muted-foreground">
                    DNS 生效后(可用 <code>dig +short TXT {acmeChallengeHint.name}</code> 验证),再点击下方"申请证书"完成签发。
                  </div>
                </div>
              )}
              <div className="flex justify-end">
                <UIButton onClick={() => void requestAcmeCert()}>
                  <ShieldCheck className="size-4" />
                  申请证书
                </UIButton>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>导入已有证书</CardTitle>
              <CardDescription>手工签发或商用 SSL 证书直接粘贴 PEM</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <div className="grid gap-3 md:grid-cols-2">
                <div className="grid gap-2">
                  <UILabel htmlFor="ssl-domain">域名</UILabel>
                  <UIInput
                    id="ssl-domain"
                    placeholder="example.com"
                    value={importForm.domain}
                    onChange={(event) =>
                      setImportForm((prev) => ({ ...prev, domain: event.target.value }))
                    }
                  />
                </div>
                <div className="grid gap-2">
                  <UILabel htmlFor="ssl-group">分组</UILabel>
                  <UIInput
                    id="ssl-group"
                    placeholder="default"
                    value={importForm.group}
                    onChange={(event) =>
                      setImportForm((prev) => ({ ...prev, group: event.target.value }))
                    }
                  />
                </div>
              </div>
              <div className="grid gap-2">
                <UILabel htmlFor="ssl-cert">证书 PEM</UILabel>
                <textarea
                  id="ssl-cert"
                  className="pem-input"
                  placeholder="-----BEGIN CERTIFICATE-----"
                  value={importForm.certificatePem}
                  onChange={(event) =>
                    setImportForm((prev) => ({ ...prev, certificatePem: event.target.value }))
                  }
                />
              </div>
              <div className="grid gap-2">
                <UILabel htmlFor="ssl-key">私钥 PEM</UILabel>
                <textarea
                  id="ssl-key"
                  className="pem-input"
                  placeholder="-----BEGIN PRIVATE KEY-----"
                  value={importForm.privateKeyPem}
                  onChange={(event) =>
                    setImportForm((prev) => ({ ...prev, privateKeyPem: event.target.value }))
                  }
                />
              </div>
              <div className="flex justify-end">
                <UIButton onClick={() => void importCert()}>
                  <Upload className="size-4" />
                  导入证书
                </UIButton>
              </div>

              <div className="mt-2">
                <h3 className="text-sm font-medium mb-2">已托管证书 ({certs.length})</h3>
                {certs.length === 0 ? (
                  <div className="empty-state text-sm">尚未导入任何证书</div>
                ) : (
                  <Table>
                    <TableHeader>
                      <UITableRow>
                        <TableHead>域名</TableHead>
                        <TableHead>分组</TableHead>
                        <TableHead>状态</TableHead>
                      </UITableRow>
                    </TableHeader>
                    <TableBody>
                      {certs.map((cert) => (
                        <UITableRow key={`${cert.domain}-${cert.group}`}>
                          <TableCell className="font-medium">{cert.domain}</TableCell>
                          <TableCell>{cert.group || "default"}</TableCell>
                          <TableCell>
                            <Badge variant="muted">{cert.warningLevel || "已导入"}</Badge>
                          </TableCell>
                        </UITableRow>
                      ))}
                    </TableBody>
                  </Table>
                )}
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="about" className="mt-4">
          <Card>
            <CardHeader>
              <CardTitle>关于</CardTitle>
              <CardDescription>面板与系统信息</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <div className="grid grid-cols-2 gap-4 text-sm">
                <div className="flex flex-col gap-0.5">
                  <span className="text-xs text-muted-foreground">主机名</span>
                  <span className="font-medium">{systemInfo.hostname}</span>
                </div>
                <div className="flex flex-col gap-0.5">
                  <span className="text-xs text-muted-foreground">操作系统</span>
                  <span className="font-medium">{systemInfo.os}</span>
                </div>
                <div className="flex flex-col gap-0.5">
                  <span className="text-xs text-muted-foreground">内核</span>
                  <span className="font-medium">{systemInfo.kernel}</span>
                </div>
                <div className="flex flex-col gap-0.5">
                  <span className="text-xs text-muted-foreground">架构</span>
                  <span className="font-medium">{systemInfo.arch}</span>
                </div>
              </div>
              <div className="flex flex-wrap gap-2">
                <UIButton variant="outline" asChild>
                  <a href="https://github.com/" target="_blank" rel="noopener noreferrer">
                    <Info className="size-4" />
                    查看项目仓库
                  </a>
                </UIButton>
                <UIButton variant="outline" onClick={onLogout}>
                  <LogOut className="size-4" />
                  退出登录
                </UIButton>
              </div>
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>
    </section>
  );
}

// ====== FTP 占位 ======
// ModulesPanel:面板里直接 toggle 启用/禁用功能模块。
// 切换后立即生效(后端写 modules.json override),不需要改 .env / 重启。
// 同时 dispatch 自定义事件 rustpanel:modules-changed,让侧栏刷新可见 Tab。
function ModulesPanel({ clients }: { clients: Clients }) {
  const [modules, setModules] = useState<RuntimeModule[]>([]);
  const [profile, setProfile] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");

  const load = useCallback(async () => {
    try {
      const response = await clients.system.listRuntimeModules({});
      setModules(response.modules);
      setProfile(response.profile);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  }, [clients]);

  useEffect(() => {
    void load();
  }, [load]);

  const toggle = async (module: RuntimeModule, enabled: boolean) => {
    if (module.required) return;
    setBusy(module.id);
    try {
      const response = await clients.system.setModuleEnabled({
        moduleId: module.id,
        enabled
      });
      setModules(response.modules);
      setProfile(response.profile);
      setMessage(`${module.name} 已${enabled ? "启用" : "禁用"}`);
      setError("");
      // 通知 AppShell 重新拉模块清单刷新侧栏
      window.dispatchEvent(new CustomEvent("rustpanel:modules-changed"));
    } catch (err) {
      setError(safeError(err));
    } finally {
      setBusy(null);
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <SettingsIcon className="size-4 text-primary" />
          功能模块开关
        </CardTitle>
        <CardDescription>
          启用/禁用立即生效,不需要重启面板。当前 profile:
          <Badge variant="muted" className="ml-2">{profile || "custom"}</Badge>
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {error && (
          <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {error}
          </div>
        )}
        {message && !error && (
          <div className="rounded-md border border-success/40 bg-success/10 px-3 py-2 text-sm text-success">
            {message}
          </div>
        )}
        <div className="flex flex-col divide-y divide-border rounded-md border border-border">
          {modules.map((m) => (
            <div
              key={m.id}
              className="flex items-center justify-between gap-3 px-4 py-3"
            >
              <div className="flex flex-col gap-0.5 min-w-0">
                <div className="flex items-center gap-2 flex-wrap">
                  <span className="font-medium">{m.name}</span>
                  <Badge variant="outline" className="font-mono text-[10px]">
                    {m.id}
                  </Badge>
                  {m.required && <Badge variant="info">核心</Badge>}
                </div>
                <span className="text-xs text-muted-foreground truncate">
                  {m.reason}
                </span>
              </div>
              <Switch
                checked={m.enabled}
                disabled={m.required || busy === m.id}
                onCheckedChange={(checked) => void toggle(m, checked)}
              />
            </div>
          ))}
        </div>
        <p className="text-xs text-muted-foreground">
          配置写到 <code className="font-mono">/var/lib/rustpanel/runtime/modules.json</code>,优先级高于 .env 中
          的 <code className="font-mono">RUSTPANEL_ENABLED_MODULES</code> /
          <code className="font-mono">RUSTPANEL_DISABLED_MODULES</code>。
        </p>
      </CardContent>
    </Card>
  );
}

function FtpPage() {
  return (
    <section className="flex flex-col gap-5 max-w-4xl">
      <header className="flex flex-col gap-1">
        <h1 className="text-2xl font-semibold tracking-tight m-0">FTP</h1>
        <p className="text-sm text-muted-foreground m-0">FTP 用户与共享目录管理</p>
      </header>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <HardDrive className="size-5 text-muted-foreground" />
            尚未实现
          </CardTitle>
          <CardDescription>
            FTP 服务后端尚未实现,该功能已计入 v3.x 任务规划。当前可使用文件管理器或 SFTP/Web 终端替代。
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <div className="rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-sm text-warning-foreground">
            状态:<Badge variant="warning" className="ml-2">BLOCKED · 后端待实现</Badge>
          </div>
          <p className="text-sm text-muted-foreground">
            如需提前传输文件,可访问 <Badge variant="muted">资源 → 文件</Badge> 模块进行上传/下载;或在
            <Badge variant="muted" className="mx-1">工具 → 终端</Badge> 中通过 sftp/scp 命令操作。
          </p>
        </CardContent>
      </Card>
    </section>
  );
}

// ====== 审计日志 ======
function AuditPage({ clients }: { clients: Clients }) {
  const [events, setEvents] = useState<AuditEvent[]>([]);
  const [moduleFilter, setModuleFilter] = useState("");
  const [query, setQuery] = useState("");
  const [error, setError] = useState("");

  const load = useCallback(async () => {
    try {
      const response = await clients.audit.listAuditEvents({
        module: moduleFilter,
        query,
        limit: 200
      });
      setEvents(response.events);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  }, [clients, moduleFilter, query]);

  useEffect(() => {
    void load();
  }, [load]);

  return (
    <section className="flex flex-col gap-5">
      <header className="flex items-start justify-between gap-4 flex-wrap">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-semibold tracking-tight m-0">操作日志</h1>
          <p className="text-sm text-muted-foreground m-0">面板内所有操作的审计追踪</p>
        </div>
        <UIButton variant="outline" size="sm" onClick={() => void load()}>
          <RefreshCw className="size-4" />
          刷新
        </UIButton>
      </header>

      <Card>
        <CardContent className="flex flex-col gap-4">
          <div className="flex flex-wrap items-center gap-2">
            <UIInput
              className="flex-1 min-w-[160px]"
              placeholder="模块,如 security / files"
              value={moduleFilter}
              onChange={(event) => setModuleFilter(event.target.value)}
            />
            <UIInput
              className="flex-1 min-w-[200px]"
              placeholder="搜索关键字"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
            <UIButton size="sm" onClick={() => void load()}>
              <RefreshCw className="size-4" />
              查询
            </UIButton>
          </div>

          {error && (
            <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
              {error}
            </div>
          )}

          {events.length === 0 ? (
            <div className="empty-state text-sm">暂无符合条件的日志</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>时间</TableHead>
                  <TableHead>用户</TableHead>
                  <TableHead>模块</TableHead>
                  <TableHead>动作</TableHead>
                  <TableHead>级别</TableHead>
                  <TableHead>来源 IP</TableHead>
                  <TableHead>说明</TableHead>
                </UITableRow>
              </TableHeader>
              <TableBody>
                {events.map((event) => (
                  <UITableRow key={event.id}>
                    <TableCell className="text-xs whitespace-nowrap">
                      {new Date(Number(event.timestampSeconds) * 1000).toLocaleString()}
                    </TableCell>
                    <TableCell className="font-medium">{event.user || "-"}</TableCell>
                    <TableCell>{event.module}</TableCell>
                    <TableCell>{event.action}</TableCell>
                    <TableCell>
                      <Badge variant={auditLevelVariant(event.level)}>{event.level || "info"}</Badge>
                    </TableCell>
                    <TableCell className="text-xs">{event.sourceIp || "-"}</TableCell>
                    <TableCell className="max-w-[320px] truncate" title={event.description}>
                      {event.description}
                    </TableCell>
                  </UITableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>
    </section>
  );
}

function auditLevelVariant(level: string): "destructive" | "warning" | "info" | "muted" {
  const normalized = level.toLowerCase();
  if (normalized === "error" || normalized === "critical" || normalized === "alert") return "destructive";
  if (normalized === "warn" || normalized === "warning") return "warning";
  if (normalized === "info" || normalized === "notice") return "info";
  return "muted";
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
