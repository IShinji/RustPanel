import { IconButton, Input, StatusPill } from "../components/form-controls";
import { Badge } from "../components/ui/badge";
import { Button as UIButton } from "../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../components/ui/card";
import { Input as UIInput } from "../components/ui/input";
import { Label as UILabel } from "../components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../components/ui/select";
import { Switch } from "../components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow as UITableRow } from "../components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../components/ui/tabs";
import { AuditEvent } from "../gen/rustpanel/v1/audit_pb";
import { ClusterNode, DistributionRecord } from "../gen/rustpanel/v1/cluster_pb";
import { AcmeChallengeType, CertificateItem } from "../gen/rustpanel/v1/ssl_pb";
import { RuntimeModule } from "../gen/rustpanel/v1/system_pb";
import { safeError } from "../lib/format";
import { auditLevelVariant } from "../lib/labels";
import { type Clients } from "../lib/rpc";
import { Activity, FileText, FileUp, HardDrive, Info, LogOut, RefreshCw, Save, Server, Settings as SettingsIcon, ShieldAlert, ShieldCheck, Trash2, Upload } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import {
  defaultClusterPairForm,
  defaultDistributionForm,
  defaultSecurityOptions,
  type ClusterPairForm,
  type DistributionForm,
  type SecurityOptionsForm
} from "../lib/forms";
import { useMountEffect } from "../lib/hooks";

export function ClusterAudit({ clients }: { clients: Clients }) {
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

  useMountEffect(() => load());

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
export function SettingsPage({ clients, onLogout }: { clients: Clients; onLogout: () => void }) {
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
        <div className="rounded-md border border-success/40 bg-success/10 px-3 py-2 text-sm text-success whitespace-pre-line">
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
          <AcmeSettingsCard clients={clients} onMessage={setMessage} onError={setError} />
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
                            {cert.warningLevel === "self-signed-bootstrap" ? (
                              <Badge variant="warning" title="占位自签证书,等待真 ACME 签发">
                                占位 · 待签发
                              </Badge>
                            ) : (
                              <Badge variant="muted">
                                {cert.warningLevel || "已导入"}
                              </Badge>
                            )}
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

// AcmeSettingsCard:面板级 ACME 偏好(联系邮箱 + staging↔production 开关)。
// 替代之前藏在 RUSTPANEL_ACME_PRODUCTION env var 里的 prod toggle ——
// 用户不用 ssh 改 .env 再 restart backend,直接 UI 里勾。
function AcmeSettingsCard({
  clients,
  onMessage,
  onError
}: {
  clients: Clients;
  onMessage: (text: string) => void;
  onError: (text: string) => void;
}) {
  const [email, setEmail] = useState("");
  const [production, setProduction] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const resp = await clients.ssl.getAcmeSettings({});
        if (cancelled) return;
        setEmail(resp.settings?.contactEmail ?? "");
        setProduction(resp.settings?.production ?? false);
      } catch (err) {
        if (!cancelled) onError(safeError(err));
      } finally {
        if (!cancelled) setLoaded(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [clients, onError]);

  const save = async () => {
    setSaving(true);
    try {
      const resp = await clients.ssl.updateAcmeSettings({
        settings: { contactEmail: email.trim(), production }
      });
      setEmail(resp.settings?.contactEmail ?? email);
      setProduction(resp.settings?.production ?? production);
      onMessage(
        `ACME 设置已保存 · 模式: ${production ? "production(真证书,浏览器认)" : "staging(测试,浏览器红色)"}`
      );
    } catch (err) {
      onError(safeError(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>ACME 设置</CardTitle>
        <CardDescription>
          面板级 Let&apos;s Encrypt 偏好,所有站点共用。改完点保存,下次申请 / 续签立即生效,无需重启后端。
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <div className="grid gap-2">
          <UILabel htmlFor="acme-settings-email">联系邮箱</UILabel>
          <UIInput
            id="acme-settings-email"
            type="email"
            placeholder="you@example.org"
            value={email}
            disabled={!loaded || saving}
            onChange={(event) => setEmail(event.target.value)}
          />
          <p className="text-xs text-muted-foreground m-0">
            LE 用这个邮箱发证书快过期提醒。<code>example.com / .org / .net</code> 会被服务端拒。
          </p>
        </div>
        <div className="flex items-start gap-3">
          <Switch
            id="acme-production"
            checked={production}
            disabled={!loaded || saving}
            onCheckedChange={(checked) => setProduction(checked)}
          />
          <div className="flex flex-col gap-1">
            <UILabel htmlFor="acme-production" className="cursor-pointer">
              使用 production(真证书)
            </UILabel>
            <p className="text-xs text-muted-foreground m-0">
              关闭 = staging 测试目录(Issuer 含 <code>STAGING</code>,浏览器不信任,但有 IP/域名速率限制宽松)。
              <br />
              开启 = LE production(Issuer 是 R10/R11/E5/E6 之类,浏览器认,**用前先确认 staging 全流程跑通**)。
            </p>
          </div>
        </div>
        <div>
          <UIButton onClick={save} disabled={!loaded || saving || !email.trim()}>
            {saving ? "保存中..." : "保存"}
          </UIButton>
        </div>
      </CardContent>
    </Card>
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

export function FtpPage() {
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
export function AuditPage({ clients }: { clients: Clients }) {
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
