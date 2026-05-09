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
import { useEffect, useMemo, useRef, useState } from "react";
import {
  CartesianGrid,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis
} from "recharts";

import { AppTemplate } from "./gen/rustpanel/v1/appstore_pb";
import { CronRunState, CronTask, CronTaskState } from "./gen/rustpanel/v1/cron_pb";
import { ContainerItem } from "./gen/rustpanel/v1/docker_pb";
import { ArchiveFormat, FileItem, FileKind } from "./gen/rustpanel/v1/fs_pb";
import { SystemStatus } from "./gen/rustpanel/v1/monitor_pb";
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
import { SiteItem } from "./gen/rustpanel/v1/site_pb";
import { CertificateItem } from "./gen/rustpanel/v1/ssl_pb";
import { createRpcClients } from "./lib/rpc";
import { formatBytes, formatDuration, formatPercent, safeError } from "./lib/format";
import { useMonitorStore } from "./store/monitor";

type Clients = ReturnType<typeof createRpcClients>;
type TabId = "dashboard" | "security" | "terminal" | "files" | "docker" | "sites" | "database" | "cron";
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

const tabs: Array<{ id: TabId; label: string; icon: typeof Activity }> = [
  { id: "dashboard", label: "仪表盘", icon: Activity },
  { id: "security", label: "安全", icon: Shield },
  { id: "terminal", label: "终端", icon: TerminalSquare },
  { id: "files", label: "文件", icon: Folder },
  { id: "docker", label: "容器", icon: Boxes },
  { id: "sites", label: "站点", icon: Globe },
  { id: "database", label: "数据库", icon: Database },
  { id: "cron", label: "计划任务", icon: Clock }
];

export default function App() {
  const [active, setActive] = useState<TabId>("dashboard");

  return (
    <div className="app-shell">
      <aside className="sidebar" aria-label="RustPanel navigation">
        <div className="brand">
          <Server size={22} />
          <span>RustPanel</span>
        </div>
        <nav className="nav-list">
          {tabs.map((tab) => {
            const Icon = tab.icon;
            return (
              <button
                className={active === tab.id ? "nav-item active" : "nav-item"}
                key={tab.id}
                onClick={() => setActive(tab.id)}
                type="button"
              >
                <Icon size={18} />
                <span>{tab.label}</span>
              </button>
            );
          })}
        </nav>
      </aside>

      <main className="workspace">
        {active === "dashboard" && <Dashboard clients={clients} />}
        {active === "security" && <SecurityPanel clients={clients} />}
        {active === "terminal" && <TerminalPanel />}
        {active === "files" && <FileManager clients={clients} />}
        {active === "docker" && <DockerApps clients={clients} />}
        {active === "sites" && <SitesSsl clients={clients} />}
        {active === "database" && <DatabasePanel clients={clients} />}
        {active === "cron" && <CronPanel clients={clients} />}
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

  const chartData = history.map((sample) => {
    const memory = sample.memory;
    const memoryPercent =
      memory && memory.totalBytes > 0n
        ? (Number(memory.usedBytes) / Number(memory.totalBytes)) * 100
        : 0;

    return {
      time: new Date(Number(sample.timestampSeconds) * 1000).toLocaleTimeString(),
      cpu: sample.cpuUsagePercent,
      memory: memoryPercent
    };
  });

  return (
    <section className="page-grid">
      <header className="section-header full-span">
        <div>
          <h1>资源监控</h1>
          <p>{system.hostname} · {system.os} · {system.kernel} · {system.arch}</p>
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
        </div>
        <ResponsiveContainer width="100%" height={280}>
          <LineChart data={chartData}>
            <CartesianGrid strokeDasharray="3 3" />
            <XAxis dataKey="time" minTickGap={24} />
            <YAxis domain={[0, 100]} />
            <Tooltip />
            <Line dataKey="cpu" dot={false} stroke="#136f63" strokeWidth={2} />
            <Line dataKey="memory" dot={false} stroke="#ba5a31" strokeWidth={2} />
          </LineChart>
        </ResponsiveContainer>
      </div>
    </section>
  );
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

function TerminalPanel() {
  const terminalRef = useRef<HTMLDivElement | null>(null);
  const socketRef = useRef<WebSocket | null>(null);

  useEffect(() => {
    const terminal = new XTerm({
      cursorBlink: true,
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
      fontSize: 13,
      theme: {
        background: "#101418",
        foreground: "#eef2f3"
      }
    });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(terminalRef.current as HTMLDivElement);
    fit.fit();

    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    const socket = new WebSocket(`${protocol}//${window.location.host}/api/terminal/ws`);
    socket.binaryType = "arraybuffer";
    socketRef.current = socket;
    socket.onmessage = (event) => {
      const text =
        typeof event.data === "string"
          ? event.data
          : new TextDecoder().decode(new Uint8Array(event.data));
      terminal.write(text);
    };
    terminal.onData((data) => socket.readyState === WebSocket.OPEN && socket.send(data));
    terminal.onResize((size) => {
      if (socket.readyState === WebSocket.OPEN) {
        socket.send(JSON.stringify({ type: "resize", cols: size.cols, rows: size.rows }));
      }
    });
    const resize = () => fit.fit();
    window.addEventListener("resize", resize);

    return () => {
      window.removeEventListener("resize", resize);
      socket.close();
      terminal.dispose();
    };
  }, []);

  return (
    <section className="page-grid terminal-layout">
      <header className="section-header full-span">
        <div>
          <h1>Web 终端</h1>
          <p>PTY 会话 · Zsh/Bash 自动检测</p>
        </div>
      </header>
      <div className="terminal-surface full-span" ref={terminalRef} />
    </section>
  );
}

function FileManager({ clients }: { clients: Clients }) {
  const [path, setPath] = useState("/");
  const [items, setItems] = useState<FileItem[]>([]);
  const [selected, setSelected] = useState<FileItem | undefined>();
  const [editorValue, setEditorValue] = useState("");
  const [menu, setMenu] = useState<{ x: number; y: number; item: FileItem } | undefined>();
  const inputRef = useRef<HTMLInputElement | null>(null);

  const load = async (nextPath = path) => {
    const response = await clients.files.listDirectory({ path: nextPath, recursive: false });
    setPath(nextPath);
    setItems(response.items);
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
    const form = new FormData();
    for (const file of files) {
      form.append("file", file);
    }
    await fetch(`/api/fs/upload?path=${encodeURIComponent(path)}`, {
      method: "POST",
      body: form
    });
    await load(path);
  };

  const deleteItem = async (item: FileItem) => {
    await clients.files.deletePath({ path: item.path, recursive: item.kind === FileKind.DIRECTORY });
    setMenu(undefined);
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
          <input hidden multiple onChange={(event) => void upload(event.target.files)} ref={inputRef} type="file" />
        </div>
      </header>

      <div className="panel file-list">
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
                  window.location.href = `/api/fs/download?path=${encodeURIComponent(selected.path)}`;
                }}
              />
            </>
          )}
        </div>
        <Editor
          height="520px"
          language="plaintext"
          onChange={(value) => setEditorValue(value ?? "")}
          options={{ minimap: { enabled: false }, fontSize: 13 }}
          value={editorValue}
        />
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
  const [templates, setTemplates] = useState<AppTemplate[]>([]);
  const [logLines, setLogLines] = useState<string[]>([]);
  const [error, setError] = useState("");

  const load = async () => {
    try {
      const [containerResponse, templateResponse] = await Promise.all([
        clients.docker.listContainers({ all: true }),
        clients.appStore.listAppTemplates({})
      ]);
      setContainers(containerResponse.containers);
      setTemplates(templateResponse.templates);
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
          <p>{error || `${containers.length} 个容器 · ${templates.length} 个模板`}</p>
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
                <small>{container.image} · {container.statusText}</small>
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
        <div className="panel-title"><Store size={18} /><span>应用模板</span></div>
        <div className="template-grid">
          {templates.map((template) => (
            <div className="item-card" key={template.slug}>
              <strong>{template.name}</strong>
              <small>{template.image}</small>
              <button onClick={() => void clients.appStore.deployApp({ slug: template.slug, appName: template.slug }).then(load)} type="button">
                <Play size={15} />安装
              </button>
            </div>
          ))}
        </div>
      </div>

      <div className="panel log-panel">
        <div className="panel-title"><TerminalSquare size={18} /><span>容器日志</span></div>
        <pre>{logLines.join("")}</pre>
      </div>
    </section>
  );
}

function SitesSsl({ clients }: { clients: Clients }) {
  const [sites, setSites] = useState<SiteItem[]>([]);
  const [certificates, setCertificates] = useState<CertificateItem[]>([]);
  const [form, setForm] = useState({ name: "demo", domains: "example.com", root: "/var/www/html", proxyTarget: "" });
  const [sslDomain, setSslDomain] = useState("example.com");
  const [importForm, setImportForm] = useState({ domain: "example.com", group: "default", certificatePem: "", privateKeyPem: "" });
  const [status, setStatus] = useState("");

  const load = async () => {
    const [siteResponse, certResponse] = await Promise.all([
      clients.site.listSites({}),
      clients.ssl.listCertificates({})
    ]);
    setSites(siteResponse.sites);
    setCertificates(certResponse.certificates);
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

function Metric({ label, value, detail }: { label: string; value: string; detail: string }) {
  return (
    <div className="metric-card">
      <small>{label}</small>
      <strong>{value}</strong>
      <span>{detail}</span>
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

function parentPath(path: string): string {
  if (path === "/") return "/";
  const parts = path.split("/").filter(Boolean);
  parts.pop();
  return `/${parts.join("/")}`;
}
