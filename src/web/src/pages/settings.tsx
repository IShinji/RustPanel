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
import { formatDateTime, safeError } from "../lib/format";
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
import { useLocale } from "../lib/i18n/locale-provider";

export function ClusterAudit({ clients }: { clients: Clients }) {
  const { t, locale } = useLocale();
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
      setStatus(response.node ? t("settings.nodeSecretMsg", { secret: response.nodeSecret }) : t("settings.nodeJoined"));
      await load();
    } catch (err) {
      setStatus(safeError(err));
    }
  };

  const sendHeartbeat = async (node: ClusterNode) => {
    try {
      const nodeSecret = nodeSecrets[node.id];
      if (!nodeSecret) {
        setStatus(t("settings.noNodeSecret"));
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
          <h1>{t("settings.clusterAuditTitle")}</h1>
          <p>{status || t("settings.clusterAuditSummary", { nodes: nodes.length, events: events.length })}</p>
        </div>
        <IconButton label={t("settings.refresh")} icon={RefreshCw} onClick={() => void load()} />
      </header>

      <div className="panel">
        <div className="panel-title"><Server size={18} /><span>{t("settings.nodePairing")}</span></div>
        <Input label={t("settings.nodeName")} value={pairForm.name} onChange={(name) => setPairForm({ ...pairForm, name })} />
        <Input label="Endpoint" value={pairForm.endpoint} onChange={(endpoint) => setPairForm({ ...pairForm, endpoint })} />
        <Input label={t("settings.pairingSecret")} value={pairForm.pairingSecret} onChange={(pairingSecret) => setPairForm({ ...pairForm, pairingSecret })} />
        <button onClick={() => void pairNode()} type="button"><ShieldCheck size={15} />{t("settings.pair")}</button>
      </div>

      <div className="panel wide-panel">
        <div className="panel-title"><Server size={18} /><span>{t("settings.nodeList")}</span></div>
        <div className="table-list">
          {nodes.map((node) => (
            <div className="table-row" key={node.id}>
              <div>
                <strong>{node.name}</strong>
                <small>{node.endpoint} · heartbeat {formatDateTime(new Date(Number(node.lastHeartbeatSeconds) * 1000), locale)}</small>
              </div>
              <StatusPill label={node.status || t("settings.colStatus")} tone={node.status === "online" ? "good" : "muted"} />
              <IconButton label={t("settings.heartbeat")} icon={Activity} onClick={() => void sendHeartbeat(node)} />
            </div>
          ))}
          {!nodes.length && <div className="empty-state">{t("settings.noNodes")}</div>}
        </div>
      </div>

      <div className="panel">
        <div className="panel-title"><Upload size={18} /><span>{t("settings.unifiedDistribution")}</span></div>
        <Input label={t("settings.targetNodeId")} value={distributionForm.targetNodeId} onChange={(targetNodeId) => setDistributionForm({ ...distributionForm, targetNodeId })} />
        <Input label={t("settings.targetPath")} value={distributionForm.path} onChange={(path) => setDistributionForm({ ...distributionForm, path })} />
        <textarea
          className="pem-input code-input"
          value={distributionForm.content}
          onChange={(event) => setDistributionForm({ ...distributionForm, content: event.target.value })}
        />
        <button onClick={() => void distributeFile()} type="button"><FileUp size={15} />{t("settings.distribute")}</button>
      </div>

      <div className="panel wide-panel">
        <div className="panel-title"><FileText size={18} /><span>{t("settings.distributionRecords")}</span></div>
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
          {!records.length && <div className="empty-state">{t("settings.noDistributionRecords")}</div>}
        </div>
      </div>

      <div className="panel full-span">
        <div className="panel-title"><ShieldAlert size={18} /><span>{t("settings.auditBlackbox")}</span></div>
        <div className="toolbar backup-actions">
          <Input label={t("settings.keyword")} value={auditQuery} onChange={setAuditQuery} />
          <button onClick={() => void load()} type="button"><RefreshCw size={15} />{t("settings.search")}</button>
          <button onClick={() => void analyzeAudit()} type="button"><Activity size={15} />{t("settings.aiAnalysis")}</button>
          <button onClick={() => void clearAudit()} type="button"><Trash2 size={15} />{t("settings.clear")}</button>
        </div>
        <div className="table-list">
          {events.map((event) => (
            <div className="table-row audit-row" key={event.id}>
              <div>
                <strong>{event.module} · {event.action}</strong>
                <small>{event.description} · {event.sourceIp} · {formatDateTime(new Date(Number(event.timestampSeconds) * 1000), locale)}</small>
              </div>
              <StatusPill label={event.level || "info"} tone={event.level === "warning" ? "danger" : "muted"} />
            </div>
          ))}
          {!events.length && <div className="empty-state">{t("settings.noAuditEvents")}</div>}
        </div>
      </div>

      <div className="panel full-span">
        <div className="panel-title"><ShieldCheck size={18} /><span>{t("settings.logRiskAnalysis")}</span></div>
        <pre className="report-output">{analysis || t("settings.noAnalysisResult")}</pre>
      </div>
    </section>
  );
}

// ====== 面板设置 ======
export function SettingsPage({ clients, onLogout }: { clients: Clients; onLogout: () => void }) {
  const { t } = useLocale();
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
      setMessage(response.options?.lastApplyMessage || t("settings.optionsSaved"));
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  };

  const importCert = async () => {
    try {
      const response = await clients.ssl.importCertificate(importForm);
      setMessage(response.status?.message || t("settings.certImported"));
      setImportForm({ domain: "", group: "default", certificatePem: "", privateKeyPem: "" });
      void refresh();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const requestAcmeCert = async () => {
    if (!acmeForm.domain.trim() || !acmeForm.email.trim()) {
      setError(t("settings.domainEmailRequired"));
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
        setMessage(response.status?.message || t("settings.txtHintMsg"));
      } else {
        setAcmeChallengeHint(null);
        setMessage(t("settings.certIssued"));
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
        <h1 className="text-2xl font-semibold tracking-tight m-0">{t("settings.panelSettingsTitle")}</h1>
        <p className="text-sm text-muted-foreground m-0">{t("settings.panelSettingsSubtitle")}</p>
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
          <TabsTrigger value="basic">{t("settings.tabBasic")}</TabsTrigger>
          <TabsTrigger value="security">{t("settings.tabSecurity")}</TabsTrigger>
          <TabsTrigger value="ssl">SSL</TabsTrigger>
          <TabsTrigger value="modules">{t("settings.tabModules")}</TabsTrigger>
          <TabsTrigger value="about">{t("settings.tabAbout")}</TabsTrigger>
        </TabsList>

        <TabsContent value="modules" className="mt-4">
          <ModulesPanel clients={clients} />
        </TabsContent>

        <TabsContent value="basic" className="mt-4">
          <Card>
            <CardHeader>
              <CardTitle>{t("settings.entryTitle")}</CardTitle>
              <CardDescription>{t("settings.entryDesc")}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <div className="grid gap-2">
                <UILabel htmlFor="settings-listen">{t("settings.listenAddr")}</UILabel>
                <UIInput
                  id="settings-listen"
                  placeholder="0.0.0.0:8443"
                  value={options.panelListenAddr}
                  onChange={(event) =>
                    setOptions((prev) => ({ ...prev, panelListenAddr: event.target.value }))
                  }
                />
                <span className="text-xs text-muted-foreground">{t("settings.listenAddrHint")}</span>
              </div>
              <div className="grid gap-2">
                <UILabel htmlFor="settings-path">{t("settings.accessPath")}</UILabel>
                <UIInput
                  id="settings-path"
                  placeholder="/admin"
                  value={options.panelAccessPath}
                  onChange={(event) =>
                    setOptions((prev) => ({ ...prev, panelAccessPath: event.target.value }))
                  }
                />
                <span className="text-xs text-muted-foreground">{t("settings.accessPathHint")}</span>
              </div>
              <div className="flex justify-end">
                <UIButton onClick={() => void saveOptions()}>
                  <Save className="size-4" />
                  {t("settings.saveBasic")}
                </UIButton>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="security" className="mt-4 flex flex-col gap-4">
          <TwoFactorCard clients={clients} />
          <Card>
            <CardHeader>
              <CardTitle>{t("settings.loginProtectionTitle")}</CardTitle>
              <CardDescription>{t("settings.loginProtectionDesc")}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <div className="flex items-center justify-between rounded-md border border-border bg-card px-4 py-3">
                <div className="flex flex-col gap-0.5">
                  <span className="font-medium">{t("settings.force2fa")}</span>
                  <span className="text-xs text-muted-foreground">{t("settings.force2faDesc")}</span>
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
                  <span className="font-medium">{t("settings.disablePing")}</span>
                  <span className="text-xs text-muted-foreground">{t("settings.disablePingDesc")}</span>
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
                  <span className="font-medium">{t("settings.portScanProtection")}</span>
                  <span className="text-xs text-muted-foreground">
                    {t("settings.portScanProtectionDesc", {
                      burst: options.scanBurst,
                      window: options.scanWindowSeconds
                    })}
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
                  {t("settings.saveSecurity")}
                </UIButton>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="ssl" className="mt-4 flex flex-col gap-4">
          <AcmeSettingsCard clients={clients} onMessage={setMessage} onError={setError} />
          <Card>
            <CardHeader>
              <CardTitle>{t("settings.acmeCertTitle")}</CardTitle>
              <CardDescription>{t("settings.acmeCertDesc")}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <div className="grid gap-3 md:grid-cols-2">
                <div className="grid gap-2">
                  <UILabel htmlFor="acme-domain">{t("settings.domain")}</UILabel>
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
                  <UILabel htmlFor="acme-email">{t("settings.contactEmail")}</UILabel>
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
                  <UILabel htmlFor="acme-challenge">{t("settings.challengeType")}</UILabel>
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
                      <SelectItem value="dns01">{t("settings.challengeDns01")}</SelectItem>
                      <SelectItem value="http01">{t("settings.challengeHttp01")}</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
              </div>
              {acmeChallengeHint && (
                <div className="rounded-md border border-info/40 bg-info/10 px-3 py-3 text-sm flex flex-col gap-2">
                  <div className="font-medium text-info">{t("settings.needTxtRecord")}</div>
                  <div className="grid grid-cols-[80px_1fr] gap-x-3 gap-y-1 text-xs font-mono">
                    <span className="text-muted-foreground">RR Name</span>
                    <span>{acmeChallengeHint.name}</span>
                    <span className="text-muted-foreground">Type</span>
                    <span>TXT</span>
                    <span className="text-muted-foreground">Value</span>
                    <span className="break-all">{acmeChallengeHint.value}</span>
                  </div>
                  <div className="text-xs text-muted-foreground">
                    {t("settings.dnsVerifyBefore")}<code>dig +short TXT {acmeChallengeHint.name}</code>{t("settings.dnsVerifyAfter")}
                  </div>
                </div>
              )}
              <div className="flex justify-end">
                <UIButton onClick={() => void requestAcmeCert()}>
                  <ShieldCheck className="size-4" />
                  {t("settings.requestCert")}
                </UIButton>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>{t("settings.importExistingCertTitle")}</CardTitle>
              <CardDescription>{t("settings.importExistingCertDesc")}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <div className="grid gap-3 md:grid-cols-2">
                <div className="grid gap-2">
                  <UILabel htmlFor="ssl-domain">{t("settings.domain")}</UILabel>
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
                  <UILabel htmlFor="ssl-group">{t("settings.group")}</UILabel>
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
                <UILabel htmlFor="ssl-cert">{t("settings.certPem")}</UILabel>
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
                <UILabel htmlFor="ssl-key">{t("settings.privateKeyPem")}</UILabel>
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
                  {t("settings.importCert")}
                </UIButton>
              </div>

              <div className="mt-2">
                <h3 className="text-sm font-medium mb-2">{t("settings.managedCertsCount", { count: certs.length })}</h3>
                {certs.length === 0 ? (
                  <div className="empty-state text-sm">{t("settings.noCertsImported")}</div>
                ) : (
                  <Table>
                    <TableHeader>
                      <UITableRow>
                        <TableHead>{t("settings.colDomain")}</TableHead>
                        <TableHead>{t("settings.colGroup")}</TableHead>
                        <TableHead>{t("settings.colStatus")}</TableHead>
                      </UITableRow>
                    </TableHeader>
                    <TableBody>
                      {certs.map((cert) => (
                        <UITableRow key={`${cert.domain}-${cert.group}`}>
                          <TableCell className="font-medium">{cert.domain}</TableCell>
                          <TableCell>{cert.group || "default"}</TableCell>
                          <TableCell>
                            {cert.warningLevel === "self-signed-bootstrap" ? (
                              <Badge variant="warning" title={t("settings.placeholderPendingTitle")}>
                                {t("settings.placeholderPendingIssuance")}
                              </Badge>
                            ) : (
                              <Badge variant="muted">
                                {cert.warningLevel || t("settings.imported")}
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
              <CardTitle>{t("settings.aboutTitle")}</CardTitle>
              <CardDescription>{t("settings.aboutDesc")}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <div className="grid grid-cols-2 gap-4 text-sm">
                <div className="flex flex-col gap-0.5">
                  <span className="text-xs text-muted-foreground">{t("settings.hostname")}</span>
                  <span className="font-medium">{systemInfo.hostname}</span>
                </div>
                <div className="flex flex-col gap-0.5">
                  <span className="text-xs text-muted-foreground">{t("settings.operatingSystem")}</span>
                  <span className="font-medium">{systemInfo.os}</span>
                </div>
                <div className="flex flex-col gap-0.5">
                  <span className="text-xs text-muted-foreground">{t("settings.kernel")}</span>
                  <span className="font-medium">{systemInfo.kernel}</span>
                </div>
                <div className="flex flex-col gap-0.5">
                  <span className="text-xs text-muted-foreground">{t("settings.architecture")}</span>
                  <span className="font-medium">{systemInfo.arch}</span>
                </div>
              </div>
              <div className="flex flex-wrap gap-2">
                <UIButton variant="outline" asChild>
                  <a href="https://github.com/" target="_blank" rel="noopener noreferrer">
                    <Info className="size-4" />
                    {t("settings.viewRepo")}
                  </a>
                </UIButton>
                <UIButton variant="outline" onClick={onLogout}>
                  <LogOut className="size-4" />
                  {t("settings.logout")}
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
// 两步验证绑定:生成密钥 → 扫码 → 输入验证码确认才生效;关闭同样要验证码。
function TwoFactorCard({ clients }: { clients: Clients }) {
  const { t } = useLocale();
  const [enabled, setEnabled] = useState(false);
  const [source, setSource] = useState("");
  const [setup, setSetup] = useState<{ secret: string; qrSvg: string } | null>(null);
  const [code, setCode] = useState("");
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");

  const load = useCallback(async () => {
    try {
      const response = await clients.security.getTwoFactorStatus({});
      setEnabled(response.twoFactor?.enabled ?? false);
      setSource(response.twoFactor?.source ?? "");
    } catch (err) {
      setError(safeError(err));
    }
  }, [clients]);

  useEffect(() => {
    void load();
  }, [load]);

  const run = async (action: () => Promise<{ status?: { message: string } }>) => {
    setError("");
    setMessage("");
    try {
      const response = await action();
      setMessage(response.status?.message ?? "");
      setCode("");
      await load();
      return true;
    } catch (err) {
      setError(safeError(err));
      return false;
    }
  };

  const begin = async () => {
    setError("");
    try {
      const response = await clients.security.beginTwoFactorSetup({});
      setSetup({ secret: response.secret, qrSvg: response.qrSvg });
      setMessage(response.status?.message ?? "");
    } catch (err) {
      setError(safeError(err));
    }
  };

  const confirm = async () => {
    if (await run(() => clients.security.confirmTwoFactorSetup({ code }))) {
      setSetup(null);
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("settings.twoFactorTitle")}</CardTitle>
        <CardDescription>
          {enabled
            ? source === "env"
              ? t("settings.twoFactorEnabledEnv")
              : t("settings.twoFactorEnabledPanel")
            : t("settings.twoFactorDisabledHint")}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {message && !error && <p className="text-sm text-success">{message}</p>}
        {error && <p className="text-sm text-destructive">{error}</p>}
        {!enabled && !setup && (
          <div>
            <UIButton size="sm" onClick={() => void begin()}>
              <ShieldCheck className="size-4" />
              {t("settings.startBinding")}
            </UIButton>
          </div>
        )}
        {!enabled && setup && (
          <div className="flex flex-col gap-3">
            <p className="text-sm text-muted-foreground">{t("settings.scanQrHint")}</p>
            {/* SVG 由后端 qrcode crate 生成,只含矩形路径 */}
            <div
              className="w-[200px] rounded-md border border-border bg-white p-1 [&>svg]:h-auto [&>svg]:w-full"
              dangerouslySetInnerHTML={{ __html: setup.qrSvg }}
            />
            <p className="text-xs text-muted-foreground break-all">
              {t("settings.manualSecretBefore")}<span className="font-mono">{setup.secret}</span>
            </p>
            <div className="flex items-end gap-2">
              <div className="grid gap-1">
                <UILabel htmlFor="twofa-code">{t("settings.verificationCode")}</UILabel>
                <UIInput
                  id="twofa-code"
                  inputMode="numeric"
                  maxLength={6}
                  value={code}
                  onChange={(event) => setCode(event.target.value.trim())}
                />
              </div>
              <UIButton size="sm" onClick={() => void confirm()} disabled={code.length !== 6}>
                {t("settings.confirmEnable")}
              </UIButton>
              <UIButton size="sm" variant="outline" onClick={() => setSetup(null)}>
                {t("settings.cancel")}
              </UIButton>
            </div>
          </div>
        )}
        {enabled && source === "panel" && (
          <div className="flex items-end gap-2">
            <div className="grid gap-1">
              <UILabel htmlFor="twofa-disable-code">{t("settings.currentCode")}</UILabel>
              <UIInput
                id="twofa-disable-code"
                inputMode="numeric"
                maxLength={6}
                value={code}
                onChange={(event) => setCode(event.target.value.trim())}
              />
            </div>
            <UIButton
              size="sm"
              variant="destructive"
              disabled={code.length !== 6}
              onClick={() => void run(() => clients.security.disableTwoFactor({ code }))}
            >
              {t("settings.disable2fa")}
            </UIButton>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function AcmeSettingsCard({
  clients,
  onMessage,
  onError
}: {
  clients: Clients;
  onMessage: (text: string) => void;
  onError: (text: string) => void;
}) {
  const { t } = useLocale();
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
        t("settings.acmeSavedMsg", {
          mode: production ? t("settings.acmeModeProduction") : t("settings.acmeModeStaging")
        })
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
        <CardTitle>{t("settings.acmeSettingsTitle")}</CardTitle>
        <CardDescription>{t("settings.acmeSettingsDesc")}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <div className="grid gap-2">
          <UILabel htmlFor="acme-settings-email">{t("settings.contactEmail")}</UILabel>
          <UIInput
            id="acme-settings-email"
            type="email"
            placeholder="you@example.org"
            value={email}
            disabled={!loaded || saving}
            onChange={(event) => setEmail(event.target.value)}
          />
          <p className="text-xs text-muted-foreground m-0">
            {t("settings.acmeEmailHintBefore")}<code>example.com / .org / .net</code>{t("settings.acmeEmailHintAfter")}
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
              {t("settings.useProduction")}
            </UILabel>
            <p className="text-xs text-muted-foreground m-0">
              {t("settings.acmeProdHintOffBefore")}<code>STAGING</code>{t("settings.acmeProdHintOffAfter")}
              <br />
              {t("settings.acmeProdHintOn")}
            </p>
          </div>
        </div>
        <div>
          <UIButton onClick={save} disabled={!loaded || saving || !email.trim()}>
            {saving ? t("settings.saving") : t("settings.save")}
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
  const { t } = useLocale();
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
      setMessage(
        enabled
          ? t("settings.moduleEnabled", { name: module.name })
          : t("settings.moduleDisabled", { name: module.name })
      );
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
          {t("settings.modulesTitle")}
        </CardTitle>
        <CardDescription>
          {t("settings.modulesDesc")}
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
                  {m.required && <Badge variant="info">{t("settings.core")}</Badge>}
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
          {t("settings.modulesConfigBefore")}<code className="font-mono">/var/lib/rustpanel/runtime/modules.json</code>
          {t("settings.modulesConfigMiddle")}
          <code className="font-mono">RUSTPANEL_ENABLED_MODULES</code> /{" "}
          <code className="font-mono">RUSTPANEL_DISABLED_MODULES</code>
          {t("settings.modulesConfigEnd")}
        </p>
      </CardContent>
    </Card>
  );
}

export function FtpPage() {
  const { t } = useLocale();
  return (
    <section className="flex flex-col gap-5 max-w-4xl">
      <header className="flex flex-col gap-1">
        <h1 className="text-2xl font-semibold tracking-tight m-0">{t("settings.ftpTitle")}</h1>
        <p className="text-sm text-muted-foreground m-0">{t("settings.ftpSubtitle")}</p>
      </header>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <HardDrive className="size-5 text-muted-foreground" />
            {t("settings.notImplemented")}
          </CardTitle>
          <CardDescription>{t("settings.notImplementedDesc")}</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <div className="rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-sm text-warning-foreground">
            {t("settings.statusLabel")}<Badge variant="warning" className="ml-2">{t("settings.blockedBadge")}</Badge>
          </div>
          <p className="text-sm text-muted-foreground">
            {t("settings.alternativeHintBefore")}<Badge variant="muted">{t("settings.resourceFilesBadge")}</Badge>
            {t("settings.alternativeHintMiddle")}
            <Badge variant="muted" className="mx-1">{t("settings.toolsTerminalBadge")}</Badge>
            {t("settings.alternativeHintAfter")}
          </p>
        </CardContent>
      </Card>
    </section>
  );
}

// ====== 审计日志 ======
export function AuditPage({ clients }: { clients: Clients }) {
  const { t, locale } = useLocale();
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
          <h1 className="text-2xl font-semibold tracking-tight m-0">{t("settings.auditPageTitle")}</h1>
          <p className="text-sm text-muted-foreground m-0">{t("settings.auditPageSubtitle")}</p>
        </div>
        <UIButton variant="outline" size="sm" onClick={() => void load()}>
          <RefreshCw className="size-4" />
          {t("settings.refresh")}
        </UIButton>
      </header>

      <Card>
        <CardContent className="flex flex-col gap-4">
          <div className="flex flex-wrap items-center gap-2">
            <UIInput
              className="flex-1 min-w-[160px]"
              placeholder={t("settings.moduleFilterPlaceholder")}
              value={moduleFilter}
              onChange={(event) => setModuleFilter(event.target.value)}
            />
            <UIInput
              className="flex-1 min-w-[200px]"
              placeholder={t("settings.searchPlaceholder")}
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
            <UIButton size="sm" onClick={() => void load()}>
              <RefreshCw className="size-4" />
              {t("settings.query")}
            </UIButton>
          </div>

          {error && (
            <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
              {error}
            </div>
          )}

          {events.length === 0 ? (
            <div className="empty-state text-sm">{t("settings.noMatchingLogs")}</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>{t("settings.colTime")}</TableHead>
                  <TableHead>{t("settings.colUser")}</TableHead>
                  <TableHead>{t("settings.colModule")}</TableHead>
                  <TableHead>{t("settings.colAction")}</TableHead>
                  <TableHead>{t("settings.colLevel")}</TableHead>
                  <TableHead>{t("settings.colSourceIp")}</TableHead>
                  <TableHead>{t("settings.colDescription")}</TableHead>
                </UITableRow>
              </TableHeader>
              <TableBody>
                {events.map((event) => (
                  <UITableRow key={event.id}>
                    <TableCell className="text-xs whitespace-nowrap">
                      {formatDateTime(new Date(Number(event.timestampSeconds) * 1000), locale)}
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
