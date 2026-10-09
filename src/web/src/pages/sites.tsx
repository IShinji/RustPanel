import { Badge } from "../components/ui/badge";
import { Button as UIButton } from "../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../components/ui/dialog";
import { Input as UIInput } from "../components/ui/input";
import { Label as UILabel } from "../components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../components/ui/select";
import { Sheet, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle } from "../components/ui/sheet";
import { Switch } from "../components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow as UITableRow } from "../components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../components/ui/tabs";
import { InstalledApp } from "../gen/rustpanel/v1/appstore_pb";
import { Capabilities, Ipv6Address, ReservedPort, ResourceBudget } from "../gen/rustpanel/v1/capability_pb";
import { ReverseProxyRule, RewriteTemplate, SiteArchive, SiteBindKind, SiteItem, SiteKind, SiteServiceAction, SiteServiceStatus, SiteTlsStrategy } from "../gen/rustpanel/v1/site_pb";
import { AcmeChallengeType, CertificateItem, RequestCertificateResponse } from "../gen/rustpanel/v1/ssl_pb";
import { formatBytes, formatDateTime, safeError } from "../lib/format";
import { appendAuthQuery, type Clients } from "../lib/rpc";
import { cn } from "../lib/utils";
import { ArrowLeftRight, FileUp, Folder, Loader2, Plus, RefreshCw, RotateCw, Save, Server, ShieldCheck, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ensureAcmeEmail } from "../lib/acme";
import { useLocale } from "../lib/i18n/locale-provider";
import { tGlobal, type MessageKey, type TFn } from "../lib/i18n/translate";

type SiteSheetMode = "new" | "edit" | null;

function siteKindLabel(kind: number, t: TFn = tGlobal): string {
  switch (kind) {
    case SiteKind.STATIC:
      return t("sites.kindStatic");
    case SiteKind.RUST_BINARY:
      return t("sites.kindRustBinary");
    case SiteKind.REVERSE_PROXY:
      return t("sites.kindReverseProxy");
    default:
      return t("sites.kindDefault");
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
const SITE_KIND_OPTIONS: Array<{
  value: "static" | "rust-binary" | "reverse-proxy";
  icon: typeof Folder;
  labelKey: MessageKey;
  descriptionKey: MessageKey;
  useCaseKey: MessageKey;
}> = [
  {
    value: "static",
    icon: Folder,
    labelKey: "sites.siteTypeStaticLabel",
    descriptionKey: "sites.siteTypeStaticDesc",
    useCaseKey: "sites.siteTypeStaticUseCase"
  },
  {
    value: "rust-binary",
    icon: Server,
    labelKey: "sites.siteTypeRustBinaryLabel",
    descriptionKey: "sites.siteTypeRustBinaryDesc",
    useCaseKey: "sites.siteTypeRustBinaryUseCase"
  },
  {
    value: "reverse-proxy",
    icon: ArrowLeftRight,
    labelKey: "sites.siteTypeReverseProxyLabel",
    descriptionKey: "sites.siteTypeReverseProxyDesc",
    useCaseKey: "sites.siteTypeReverseProxyUseCase"
  }
];

export function SitesSsl({ clients }: { clients: Clients }) {
  const { t } = useLocale();
  // === 数据 ===
  const [sites, setSites] = useState<SiteItem[]>([]);
  const [certificates, setCertificates] = useState<CertificateItem[]>([]);
  const [rewriteTemplates, setRewriteTemplates] = useState<RewriteTemplate[]>([]);
  const [proxyRules, setProxyRules] = useState<ReverseProxyRule[]>([]);
  const [reservedPorts, setReservedPorts] = useState<ReservedPort[]>([]);
  const [ipv6Pool, setIpv6Pool] = useState<Ipv6Address[]>([]);
  const [capabilities, setCapabilities] = useState<Capabilities | null>(null);
  const [budget, setSiteBudget] = useState<ResourceBudget | null>(null);
  const [installedApps, setInstalledApps] = useState<InstalledApp[]>([]);
  // === 反馈 ===
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  // === Sheet 抽屉状态 ===
  const [sheetMode, setSheetMode] = useState<SiteSheetMode>(null);
  const [selectedSite, setSelectedSite] = useState<SiteItem | null>(null);
  // 删除确认 Dialog 的目标站点 / 证书。null = 不显示。
  const [siteToDelete, setSiteToDelete] = useState<SiteItem | null>(null);
  const [certToRevoke, setCertToRevoke] = useState<CertificateItem | null>(null);
  // 创建站点选 DNS-01 时,自动发起 ACME 拿到的 TXT 挑战;非空就在
  // 主页面顶部渲染一块大横条引导用户加 DNS。
  const [pendingAcme, setPendingAcme] = useState<{
    domain: string;
    recordName: string;
    recordValue: string;
  } | null>(null);

  const load = useCallback(async () => {
    try {
      const [siteRes, certRes, tplRes, proxyRes, portsRes, v6Res, budgetRes, installedRes] =
        await Promise.all([
          clients.site.listSites({}),
          clients.ssl.listCertificates({}),
          clients.site.listRewriteTemplates({}),
          clients.site.listReverseProxyRules({}),
          clients.capability
            .listReservedPorts({})
            .catch(() => ({ ports: [] as ReservedPort[] })),
          clients.capability
            .listIpv6Addresses({})
            .catch(() => ({ addresses: [] as Ipv6Address[], prefixes: [] as string[] })),
          clients.capability
            .getResourceBudget({})
            .catch(() => ({ budget: null as ResourceBudget | null })),
          clients.appStore
            .listInstalledApps({})
            .catch(() => ({ apps: [] as InstalledApp[] }))
        ]);
      setSites(siteRes.sites);
      setCertificates(certRes.certificates);
      setRewriteTemplates(tplRes.templates);
      setProxyRules(proxyRes.rules);
      setReservedPorts(portsRes.ports);
      setIpv6Pool(v6Res.addresses);
      setSiteBudget(budgetRes.budget ?? null);
      setInstalledApps(installedRes.apps);
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
      const head = response.status?.message ?? t("sites.certRenewed", { domain: certificate.domain });
      // 续签是两步:第一次后端返 Challenge,dns_record_name 非空 → 提示用户加
      // TXT,信息单独显示;第二次后端返 Issued,output 是 reload 的辅助信息
      // (典型 "rpxy reloaded"),作为副 message 显示但不阻断成功状态。
      if (response.dnsRecordName) {
        const tipLines = [
          head,
          "",
          t("sites.txtRecordNameLine", { name: response.dnsRecordName }),
          t("sites.txtRecordValueLine", { value: response.dnsRecordValue }),
          "",
          t("sites.txtAddHint")
        ];
        setMessage(tipLines.join("\n"));
      } else {
        const detail = response.output;
        setMessage(detail ? t("sites.renewDetailSuffix", { head, detail }) : head);
      }
      await load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const deleteReverseProxy = async (rule: ReverseProxyRule) => {
    try {
      await clients.site.deleteReverseProxyRule({ id: rule.id });
      setMessage(t("sites.rpDeleted", { name: rule.name }));
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
      setMessage(t("sites.certRevoked", { domain: target.domain }));
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
          ? t("sites.siteDeletedWithCleanup", { name: target.name, count: cleaned })
          : t("sites.siteDeleted", { name: target.name })
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
          <h1 className="text-2xl font-semibold tracking-tight m-0">{t("sites.title")}</h1>
          <p className="text-sm text-muted-foreground m-0">
            {t("sites.subtitle", { sites: sites.length, certs: certificates.length, rules: proxyRules.length })}
          </p>
        </div>
        <div className="flex gap-2">
          <UIButton variant="outline" size="sm" onClick={() => void load()}>
            <RefreshCw className="size-4" />
            {t("sites.refresh")}
          </UIButton>
          <UIButton size="sm" onClick={openCreate}>
            <Plus className="size-4" />
            {t("sites.newSite")}
          </UIButton>
        </div>
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
      {pendingAcme && (
        <div className="rounded-md border border-info/40 bg-info/5 px-4 py-3 text-sm space-y-2">
          <div className="flex items-center justify-between">
            <div className="font-medium">
              {t("sites.pendingAcmeTitle", { domain: pendingAcme.domain })}
            </div>
            <UIButton
              size="sm"
              variant="ghost"
              onClick={() => setPendingAcme(null)}
              title={t("sites.closeHint")}
            >
              ×
            </UIButton>
          </div>
          <div className="text-xs text-muted-foreground">{t("sites.pendingAcmeDesc")}</div>
          <div className="grid gap-1 sm:grid-cols-[80px_1fr] text-xs">
            <span className="text-muted-foreground">{t("sites.recordType")}</span>
            <span className="font-mono">TXT</span>
            <span className="text-muted-foreground">{t("sites.hostRecord")}</span>
            <span className="font-mono break-all">{pendingAcme.recordName || t("sites.pendingPlaceholder")}</span>
            <span className="text-muted-foreground">{t("sites.recordValue")}</span>
            <span className="font-mono break-all">{pendingAcme.recordValue || t("sites.pendingPlaceholder")}</span>
          </div>
          <div className="text-xs text-muted-foreground">
            {t("sites.digHintBefore")}<span className="font-mono">dig TXT _acme-challenge.{pendingAcme.domain}</span>{t("sites.digHintAfter")}
          </div>
        </div>
      )}

      <Card>
        <CardHeader>
          <CardTitle>{t("sites.sitesCardTitle")}</CardTitle>
          <CardDescription>{t("sites.sitesCardDesc")}</CardDescription>
        </CardHeader>
        <CardContent>
          {sites.length === 0 ? (
            <div className="empty-state text-sm">{t("sites.noSitesYet")}</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>{t("sites.colName")}</TableHead>
                  <TableHead>{t("sites.colDomain")}</TableHead>
                  <TableHead>{t("sites.colType")}</TableHead>
                  <TableHead>{t("sites.colSsl")}</TableHead>
                  <TableHead>{t("sites.colUsage")}</TableHead>
                  <TableHead className="text-right">{t("sites.colActions")}</TableHead>
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
                    <TableCell>{siteKindLabel(site.kind, t)}</TableCell>
                    <TableCell>
                      <Badge variant={site.sslEnabled ? "success" : "muted"}>
                        {site.sslEnabled ? "SSL" : "HTTP"}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-xs text-muted-foreground">
                      {site.diskBytes > 0n ? formatBytes(site.diskBytes) : "—"}
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
                          {t("sites.edit")}
                        </UIButton>
                        <UIButton
                          size="sm"
                          variant="ghost"
                          title={t("sites.deleteSite")}
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
          <CardTitle>{t("sites.certsCardTitle")}</CardTitle>
          <CardDescription>{t("sites.certsCardDesc")}</CardDescription>
        </CardHeader>
        <CardContent>
          {certificates.length === 0 ? (
            <div className="empty-state text-sm">{t("sites.noCerts")}</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>{t("sites.colDomain")}</TableHead>
                  <TableHead>{t("sites.colGroup")}</TableHead>
                  <TableHead>{t("sites.colRemaining")}</TableHead>
                  <TableHead className="text-right">{t("sites.colActions")}</TableHead>
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
                      {cert.warningLevel === "self-signed-bootstrap" ? (
                        <Badge variant="warning" title={t("sites.placeholderPendingTitle")}>
                          {t("sites.placeholderPendingIssuance")}
                        </Badge>
                      ) : (
                        <Badge variant={cert.warningLevel === "ok" ? "success" : "destructive"}>
                          {t("sites.daysSuffix", { days: cert.daysUntilExpiry })}
                        </Badge>
                      )}
                    </TableCell>
                    <TableCell className="text-right">
                      <div className="flex justify-end gap-1">
                        <UIButton
                          size="sm"
                          variant="outline"
                          onClick={() => void renewCertificate(cert)}
                        >
                          <RotateCw className="size-3.5" />
                          {t("sites.renew")}
                        </UIButton>
                        <UIButton
                          size="sm"
                          variant="ghost"
                          title={t("sites.revokeCertTitle")}
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
          <CardTitle>{t("sites.rpCardTitle")}</CardTitle>
          <CardDescription>{t("sites.rpCardDesc")}</CardDescription>
        </CardHeader>
        <CardContent>
          {proxyRules.length === 0 ? (
            <div className="empty-state text-sm">{t("sites.noRpRules")}</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>{t("sites.colName")}</TableHead>
                  <TableHead>{t("sites.colPath")}</TableHead>
                  <TableHead>{t("sites.colTarget")}</TableHead>
                  <TableHead>{t("sites.colStatus")}</TableHead>
                  <TableHead className="text-right">{t("sites.colActions")}</TableHead>
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
                        {rule.enabled ? t("sites.enabled") : t("sites.disabled")}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-right">
                      <UIButton
                        size="sm"
                        variant="outline"
                        onClick={() => void deleteReverseProxy(rule)}
                      >
                        <Trash2 className="size-3.5" />
                        {t("sites.delete")}
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
        budget={budget}
        installedApps={installedApps}
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
        onAcmeChallenge={(domain, recordName, recordValue) =>
          setPendingAcme({ domain, recordName, recordValue })
        }
      />

      <Dialog
        open={siteToDelete !== null}
        onOpenChange={(open) => !open && setSiteToDelete(null)}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("sites.deleteSiteDialogTitle", { name: siteToDelete?.name ?? "" })}</DialogTitle>
            <DialogDescription>{t("sites.deleteSiteDialogDesc")}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <UIButton variant="outline" onClick={() => setSiteToDelete(null)}>
              {t("sites.cancel")}
            </UIButton>
            <UIButton variant="destructive" onClick={() => void performDeleteSite()}>
              <Trash2 className="size-4" />
              {t("sites.confirmDelete")}
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
            <DialogTitle>{t("sites.revokeCertDialogTitle", { domain: certToRevoke?.domain ?? "" })}</DialogTitle>
            <DialogDescription>{t("sites.revokeCertDialogDesc")}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <UIButton variant="outline" onClick={() => setCertToRevoke(null)}>
              {t("sites.cancel")}
            </UIButton>
            <UIButton variant="destructive" onClick={() => void performRevokeCert()}>
              <Trash2 className="size-4" />
              {t("sites.confirmRevoke")}
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
  budget: ResourceBudget | null;
  installedApps: InstalledApp[];
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
  // 新建站点 + DNS-01 时:SmartSiteForm 自动发起 ACME,拿到 TXT 后
  // 把记录推到顶部 banner 让用户尽快去 DNS 服务商添加。
  onAcmeChallenge?: (domain: string, recordName: string, recordValue: string) => void;
};

// 网站引擎选项:小白可选,后端按选择装 + 配置 vhost
type SiteEngineChoice = "nginx" | "rpxy";

type EngineRecommendation = {
  choice: SiteEngineChoice;
  reason: string;
};

/// 根据主机能力 + 已安装包推荐 backend。
/// - 已装 nginx-mainline 或 rpxy 之一:用已装的
/// - OpenVZ 或 RAM < 256MB:推 rpxy(单文件 Rust,绕开 apt fork 上限)
/// - 否则:推 nginx-mainline(C 写的,单进程多站省 RAM)
function recommendEngine(
  capabilities: Capabilities | null,
  budget: ResourceBudget | null,
  installedApps: InstalledApp[],
  t: TFn = tGlobal
): EngineRecommendation {
  const installedSlugs = new Set(installedApps.map((app) => app.slug));
  if (installedSlugs.has("nginx-mainline") || installedSlugs.has("nginx-light")) {
    return { choice: "nginx", reason: t("sites.engineReasonNginxDetected") };
  }
  if (installedSlugs.has("rpxy")) {
    return { choice: "rpxy", reason: t("sites.engineReasonRpxyDetected") };
  }
  if (capabilities?.isOpenvz) {
    return {
      choice: "rpxy",
      reason: t("sites.engineReasonOpenvz")
    };
  }
  const totalRamMb = budget?.memory ? Number(budget.memory.totalBytes / 1024n / 1024n) : 0;
  if (totalRamMb > 0 && totalRamMb < 256) {
    return {
      choice: "rpxy",
      reason: t("sites.engineReasonLowRam", { ram: totalRamMb })
    };
  }
  return {
    choice: "nginx",
    reason: t("sites.engineReasonDefault")
  };
}

function SiteDetailSheet(props: SiteSheetProps) {
  const { mode, site, onClose } = props;
  const { t } = useLocale();
  return (
    <Sheet open={mode !== null} onOpenChange={(open) => !open && onClose()}>
      <SheetContent>
        <SheetHeader>
          <SheetTitle>
            {mode === "new" ? t("sites.newSiteTitle") : site?.name || t("sites.siteDetailsTitle")}
          </SheetTitle>
          <SheetDescription>
            {mode === "new"
              ? t("sites.newSiteDesc")
              : site?.domains.join(", ") || ""}
          </SheetDescription>
        </SheetHeader>
        <div className="flex-1 overflow-y-auto px-5 pb-3">
          <Tabs defaultValue="basic">
            <TabsList>
              <TabsTrigger value="basic">{t("sites.tabBasic")}</TabsTrigger>
              <TabsTrigger value="deploy" disabled={mode === "new"}>
                {t("sites.tabDeploy")}
              </TabsTrigger>
              <TabsTrigger value="ssl" disabled={mode === "new"}>
                SSL
              </TabsTrigger>
              <TabsTrigger value="rp" disabled={mode === "new"}>
                {t("sites.tabRp")}
              </TabsTrigger>
              <TabsTrigger value="services" disabled={mode === "new"}>
                {t("sites.tabServices")}
              </TabsTrigger>
              <TabsTrigger value="rewrite">{t("sites.tabRewrite")}</TabsTrigger>
            </TabsList>
            <TabsContent value="basic" className="pt-3">
              <SmartSiteForm {...props} />
            </TabsContent>
            <TabsContent value="deploy" className="pt-3">
              {mode === "edit" && site ? (
                <DeployPanel {...props} site={site} />
              ) : (
                <p className="text-sm text-muted-foreground">{t("sites.saveSiteFirstDeploy")}</p>
              )}
            </TabsContent>
            <TabsContent value="ssl" className="pt-3">
              {mode === "edit" && site ? (
                <SslPanel {...props} site={site} />
              ) : (
                <p className="text-sm text-muted-foreground">{t("sites.saveSiteFirstSsl")}</p>
              )}
            </TabsContent>
            <TabsContent value="rp" className="pt-3">
              {mode === "edit" && site ? (
                <PerSiteReverseProxyPanel {...props} site={site} />
              ) : (
                <p className="text-sm text-muted-foreground">{t("sites.saveSiteFirstRp")}</p>
              )}
            </TabsContent>
            <TabsContent value="services" className="pt-3">
              {mode === "edit" && site ? (
                <SiteServicesPanel {...props} site={site} />
              ) : (
                <p className="text-sm text-muted-foreground">{t("sites.saveSiteFirstServices")}</p>
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
              {t("sites.deleteSite")}
            </UIButton>
          )}
          <UIButton variant="outline" onClick={onClose}>
            {t("sites.close")}
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
  budget,
  installedApps,
  reservedPorts,
  ipv6Pool,
  clients,
  onChanged,
  onMessage,
  onError,
  onClose,
  onAcmeChallenge
}: SiteSheetProps) {
  const { t } = useLocale();
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

  // 提交进度:后端要装 nginx + 申请 ACME,链路 10-60 秒,得把"正在干啥"
  // 实时透出去,免得用户以为页面假死。submitStep 不为空就显示进度条。
  const [submitting, setSubmitting] = useState(false);
  const [submitStep, setSubmitStep] = useState("");

  // 引擎推荐 + 用户选择。useMemo 避免每渲染重算;但选择改了的话保持用户选择。
  const recommendation = useMemo(
    () => recommendEngine(capabilities, budget, installedApps, t),
    [capabilities, budget, installedApps, t]
  );
  const [engineChoice, setEngineChoice] = useState<SiteEngineChoice>(
    recommendation.choice
  );
  // 推荐变了(数据刚拉到)且用户没动过选择,跟着推荐走
  const [engineUserTouched, setEngineUserTouched] = useState(false);
  useEffect(() => {
    if (!engineUserTouched) setEngineChoice(recommendation.choice);
  }, [recommendation.choice, engineUserTouched]);

  const installedSlugs = useMemo(
    () => new Set(installedApps.map((app) => app.slug)),
    [installedApps]
  );

  const submitEdit = async () => {
    if (!site) return;
    const domains = domain
      .split(/[\s,]+/)
      .map((item) => item.trim())
      .filter(Boolean);
    if (domains.length === 0) {
      onError(t("sites.atLeastOneDomain"));
      return;
    }
    if (domainConflict) {
      onError(t("sites.domainInUseError"));
      return;
    }
    const newName = name.trim();
    const renaming = newName !== "" && newName !== site.name;
    if (renaming) {
      if (!safeName(newName)) {
        onError(t("sites.siteNameRequired"));
        return;
      }
      if (nameConflict) {
        onError(t("sites.siteNameTaken", { name: newName }));
        return;
      }
    }
    setSubmitting(true);
    try {
      await clients.site.updateSite({
        name: site.name,
        newName: renaming ? newName : "",
        domains,
        proxyTarget: kind === "reverse-proxy" ? proxyTarget.trim() : "",
        tlsStrategy:
          tls === "none"
            ? SiteTlsStrategy.NONE
            : tls === "imported"
              ? SiteTlsStrategy.IMPORTED
              : SiteTlsStrategy.LETSENCRYPT_DNS01
      });
      onMessage(t("sites.siteUpdated", { name: renaming ? newName : site.name }));
      if (renaming) {
        // NAT 端口预留的 owner 带站点名,跟着改名换一次(失败不影响改名本身)
        const natPort = site.binding?.kind === SiteBindKind.NAT_PORT ? site.binding.natPort : 0;
        if (natPort > 0) {
          await clients.capability.releasePort({ port: natPort }).catch(() => undefined);
          await clients.capability
            .reservePort({
              port: natPort,
              owner: `site:${newName}`,
              description: domains[0] ?? "",
              protocol: "tcp"
            })
            .catch(() => undefined);
        }
        // 列表刷新后路由 effect 会按新名重新选中站点
        window.history.replaceState(null, "", `#sites/${newName}`);
      }
      onChanged();
    } catch (err) {
      onError(safeError(err));
    } finally {
      setSubmitting(false);
    }
  };

  const submit = async () => {
    if (isEdit) {
      await submitEdit();
      return;
    }
    // 硬校验:撞名 / 撞 root / 撞域名一律拒
    if (!safeName(name)) {
      onError(t("sites.siteNameEmpty"));
      return;
    }
    if (nameConflict) {
      onError(t("sites.siteNameTaken", { name }));
      return;
    }
    if (rootConflict) {
      onError(t("sites.rootTaken", { root, owner: rootConflict.name }));
      return;
    }
    if (domainConflict) {
      onError(t("sites.domainTaken", { domain: domainConflict.domain, owner: domainConflict.site.name }));
      return;
    }
    if (natPortConflict) {
      onError(t("sites.natPortTaken", { port: natPortConflict.port, owner: natPortConflict.owner }));
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
    setSubmitting(true);
    try {
      // 用户选 rpxy:先确保 rpxy(以及静态站需要的 SWS)已经装上,
      // 后端 createSite 看到 engine=rpxy 就会跳过 nginx 自动安装。
      //
      // **关键阻断点**:任一前置依赖装失败,直接 throw 一个带明确"站点
      // 未创建"语境的错误,catch 收住后 onError + 跳过 createSite。
      // 这一段如果硬走下去,后端会写出一份 vhost 但没人 serve,反而更
      // 误导用户。
      if (engineChoice === "rpxy") {
        if (!installedSlugs.has("rpxy")) {
          setSubmitStep(t("sites.rpxyInstallStep"));
          try {
            await clients.appStore.deployApp({
              slug: "rpxy",
              appName: "rpxy",
              version: "latest"
            });
          } catch (installErr) {
            throw new Error(
              t("sites.rpxyInstallFailed", { error: safeError(installErr) }),
              { cause: installErr }
            );
          }
        }
        if (kind === "static" && !installedSlugs.has("static-web-server")) {
          setSubmitStep(t("sites.swsInstallStep"));
          try {
            await clients.appStore.deployApp({
              slug: "static-web-server",
              appName: "static-web-server",
              version: "latest"
            });
          } catch (installErr) {
            throw new Error(
              t("sites.swsInstallFailed", { error: safeError(installErr) }),
              { cause: installErr }
            );
          }
        }
      }
      setSubmitStep(
        engineChoice === "rpxy"
          ? t("sites.writingRpxyStep")
          : t("sites.writingNginxStep")
      );
      const response = await clients.site.createSite({
        name,
        domains: domain
          .split(/[\s,]+/)
          .map((value) => value.trim())
          .filter(Boolean),
        root,
        proxyTarget,
        sslEnabled: protoTls !== SiteTlsStrategy.NONE,
        engine: engineChoice,
        listenAddr: "",
        kind: protoKind,
        binding,
        tlsStrategy: protoTls,
        binaryPath
      });
      if (binding.kind === SiteBindKind.NAT_PORT && binding.natPort > 0) {
        setSubmitStep(t("sites.reservingPortStep"));
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
        // status.message 现在带 bootstrap notes(nginx 装了 / 装失败的原因),
        // 优先用它;没附加信息时退回简单的"已创建"。
        const richMsg = response.status?.message;
        onMessage(
          richMsg && richMsg.length > 0
            ? richMsg
            : t("sites.siteCreatedDefault", { name: response.site.name })
        );
      }
      // 选了 DNS-01 自动签发:立即发起 ACME 拿 TXT,通过 onAcmeChallenge
      // 推到主页面顶部 banner。即使失败也只 log,不阻塞站点创建已经
      // 跑通的事实。
      if (tls === "dns01" && onAcmeChallenge) {
        const primaryDomain = domain
          .split(/[\s,]+/)
          .map((value) => value.trim())
          .filter(Boolean)[0];
        const acmeEmail = primaryDomain ? await ensureAcmeEmail() : null;
        if (primaryDomain && acmeEmail) {
          setSubmitStep(t("sites.requestingAcmeStep"));
          try {
            const acmeResp = await clients.ssl.requestCertificate({
              domain: primaryDomain,
              email: acmeEmail,
              challengeType: AcmeChallengeType.DNS_01
            });
            if (acmeResp.dnsRecordName || acmeResp.dnsRecordValue) {
              onAcmeChallenge(
                primaryDomain,
                acmeResp.dnsRecordName,
                acmeResp.dnsRecordValue
              );
            }
          } catch (acmeErr) {
            console.warn("auto ACME request failed:", acmeErr);
            // 不报错给用户 —— 站点创建已成功,SSL 可以稍后在抽屉里手动触发
          }
        }
      }
      onChanged();
      onClose();
    } catch (err) {
      onError(safeError(err));
    } finally {
      setSubmitting(false);
      setSubmitStep("");
    }
  };

  return (
    <div className="flex flex-col gap-3">
      <div className="grid gap-3 md:grid-cols-2">
        <div className="grid gap-1">
          <UILabel htmlFor="site-name">{t("sites.siteNameLabel")}</UILabel>
          <UIInput
            id="site-name"
            value={name}
            disabled={isEdit && site?.kind === SiteKind.RUST_BINARY}
            onChange={(event) => setName(event.target.value)}
          />
          {isEdit && site?.kind === SiteKind.RUST_BINARY && (
            <p className="text-xs text-muted-foreground">{t("sites.renameUnsupportedRustBinary")}</p>
          )}
        </div>
        <div className="grid gap-1">
          <UILabel htmlFor="site-domain">{t("sites.domainFieldLabel")}</UILabel>
          <UIInput
            id="site-domain"
            value={domain}
            onChange={(event) => setDomain(event.target.value)}
          />
        </div>
        {!isEdit && (
          <div className="grid gap-1.5 md:col-span-2">
            <UILabel>{t("sites.engineLabel")}</UILabel>
            <div className="rounded border border-info/30 bg-info/5 px-3 py-2 text-xs text-muted-foreground">
              <span className="font-medium text-foreground">{t("sites.recommendedPrefix")}</span>{" "}
              {recommendation.choice === "nginx" ? "nginx-mainline" : "rpxy + SWS"} —— {recommendation.reason}
            </div>
            <div className="grid gap-2 md:grid-cols-2">
              {(
                [
                  {
                    value: "nginx" as const,
                    icon: Server,
                    label: "nginx mainline",
                    description: t("sites.nginxDescription"),
                    note: installedSlugs.has("nginx-mainline")
                      ? t("sites.nginxNoteInstalled")
                      : installedSlugs.has("nginx-light")
                        ? t("sites.nginxNoteInstalledLight")
                        : t("sites.nginxNoteNotInstalled")
                  },
                  {
                    value: "rpxy" as const,
                    icon: ArrowLeftRight,
                    label: "rpxy + SWS",
                    description: t("sites.rpxyDescription"),
                    note: installedSlugs.has("rpxy")
                      ? t("sites.rpxyNoteInstalled") + (installedSlugs.has("static-web-server") ? t("sites.rpxyNoteSwsInstalled") : t("sites.rpxyNoteSwsAuto"))
                      : t("sites.rpxyNoteNotInstalled")
                  }
                ] as Array<{
                  value: SiteEngineChoice;
                  icon: typeof Server;
                  label: string;
                  description: string;
                  note: string;
                }>
              ).map((option) => {
                const Icon = option.icon;
                const selected = engineChoice === option.value;
                const isRecommended = recommendation.choice === option.value;
                return (
                  <button
                    key={option.value}
                    type="button"
                    onClick={() => {
                      setEngineChoice(option.value);
                      setEngineUserTouched(true);
                    }}
                    className={cn(
                      "flex flex-col items-start gap-1 rounded-lg border p-3 text-left transition",
                      selected
                        ? "border-primary bg-primary/5 ring-1 ring-primary/30"
                        : "border-border hover:border-primary/40 hover:bg-accent/30"
                    )}
                  >
                    <div className="flex items-center gap-1.5">
                      <Icon className="size-4 text-primary" />
                      <span className="font-medium text-sm">{option.label}</span>
                      {isRecommended && <Badge variant="info">{t("sites.recommended")}</Badge>}
                    </div>
                    <span className="text-xs text-muted-foreground">
                      {option.description}
                    </span>
                    <span className="text-[11px] text-muted-foreground/80">
                      {option.note}
                    </span>
                  </button>
                );
              })}
            </div>
          </div>
        )}

        <div className="grid gap-1.5 md:col-span-2">
          <UILabel>{t("sites.siteTypeLabel")}</UILabel>
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
                    <span className="font-medium text-sm">{t(option.labelKey)}</span>
                  </div>
                  <span className="text-xs text-muted-foreground">
                    {t(option.descriptionKey)}
                  </span>
                  <span className="text-[11px] text-muted-foreground/80">
                    {t("sites.suitableForPrefix")}{t(option.useCaseKey)}
                  </span>
                </button>
              );
            })}
          </div>
        </div>
        {kind === "static" && (
          <div className="grid gap-1 md:col-span-2">
            <UILabel htmlFor="site-root">{t("sites.siteRootLabel")}</UILabel>
            <UIInput
              id="site-root"
              placeholder={defaultRootFor(name) || t("sites.siteRootPlaceholder")}
              value={root}
              disabled={isEdit}
              onChange={(event) => {
                setRootTouched(true);
                setRoot(event.target.value);
              }}
            />
            <span className="text-xs text-muted-foreground">
              {rootTouched
                ? t("sites.siteRootCustomHint")
                : t("sites.siteRootAutoHint", { path: defaultRootFor(name) || t("sites.siteRootAutoHintPending") })}
            </span>
          </div>
        )}
        {kind === "rust-binary" && (
          <div className="grid gap-1 md:col-span-2">
            <UILabel htmlFor="site-bin">{t("sites.binaryPathLabel")}</UILabel>
            <UIInput
              id="site-bin"
              placeholder={defaultBinaryPathFor(name) || t("sites.binaryPathPlaceholder")}
              value={binaryPath}
              disabled={isEdit}
              onChange={(event) => {
                setBinaryPathTouched(true);
                setBinaryPath(event.target.value);
              }}
            />
            <span className="text-xs text-muted-foreground">{t("sites.binaryPathHint")}</span>
          </div>
        )}
        {kind === "reverse-proxy" && (
          <div className="grid gap-1 md:col-span-2">
            <UILabel htmlFor="site-upstream">{t("sites.upstreamLabel")}</UILabel>
            <UIInput
              id="site-upstream"
              placeholder="http://127.0.0.1:3000"
              value={proxyTarget}
              onChange={(event) => setProxyTarget(event.target.value)}
            />
            <span className="text-xs text-muted-foreground">{t("sites.upstreamHint")}</span>
          </div>
        )}
        {showBinding && (
          <>
            <div className="grid gap-1 md:col-span-2">
              <UILabel>{t("sites.bindingLabel")}</UILabel>
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
                    <SelectItem value="ipv6">{t("sites.bindIpv6Option")}</SelectItem>
                  )}
                  <SelectItem value="nat-port">{t("sites.bindNatPortOption")}</SelectItem>
                </SelectContent>
              </Select>
              <span className="text-xs text-muted-foreground">{t("sites.bindingHint")}</span>
            </div>
            {bindKind === "ipv6" ? (
              <div className="grid gap-1 md:col-span-2">
                <UILabel htmlFor="site-v6">{t("sites.ipv6AddressLabel")}</UILabel>
                <Select
                  value={ipv6Address}
                  onValueChange={setIpv6Address}
                  disabled={isEdit}
                >
                  <SelectTrigger id="site-v6">
                    <SelectValue placeholder={t("sites.ipv6SelectPlaceholder")} />
                  </SelectTrigger>
                  <SelectContent>
                    {ipv6Pool.length === 0 && (
                      <SelectItem value="__none__" disabled>
                        {t("sites.ipv6NoneDetected")}
                      </SelectItem>
                    )}
                    {ipv6Pool.map((addr) => {
                      const users = ipv6UsageMap.get(addr.address) ?? [];
                      return (
                        <SelectItem key={addr.address} value={addr.address}>
                          {addr.address}/{addr.prefixLength} ({addr.interfaceName})
                          {users.length > 0 && t("sites.ipv6UsedBySuffix", { names: users.join(", ") })}
                        </SelectItem>
                      );
                    })}
                  </SelectContent>
                </Select>
                <span className="text-xs text-muted-foreground">
                  {ipv6Address && (ipv6UsageMap.get(ipv6Address)?.length ?? 0) > 0
                    ? t("sites.ipv6SharedHintUsed", { names: ipv6UsageMap.get(ipv6Address)!.join(", ") })
                    : t("sites.ipv6SharedHintUnused")}
                </span>
              </div>
            ) : (
              <div className="grid gap-1 md:col-span-2">
                <UILabel htmlFor="site-natport">
                  {t("sites.natPortLabel", { ports: reservedPorts.map((p) => p.port).join(", ") || t("sites.natPortNone") })}
                </UILabel>
                <UIInput
                  id="site-natport"
                  type="number"
                  placeholder={t("sites.natPortPlaceholder")}
                  value={natPort}
                  disabled={isEdit}
                  onChange={(event) => setNatPort(event.target.value)}
                />
                <span className="text-xs text-muted-foreground">{t("sites.natPortHint")}</span>
              </div>
            )}
          </>
        )}
        <div className="grid gap-1 md:col-span-2">
          <UILabel>{t("sites.tlsLabel")}</UILabel>
          <Select
            value={tls}
            onValueChange={(value) => setTls(value as typeof tls)}
          >
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="dns01">{t("sites.tlsAutoOption")}</SelectItem>
              <SelectItem value="imported">{t("sites.tlsImportedOption")}</SelectItem>
              <SelectItem value="none">{t("sites.tlsNoneOption")}</SelectItem>
            </SelectContent>
          </Select>
          <span className="text-xs text-muted-foreground">{t("sites.tlsHint")}</span>
        </div>
      </div>
      {!isEdit && (nameConflict || rootConflict || domainConflict || natPortConflict) && (
        <div className="rounded border border-warning/40 bg-warning/10 px-3 py-2 text-xs text-warning space-y-0.5">
          {nameConflict && (
            <div>{t("sites.nameConflictWarning", { name })}</div>
          )}
          {rootConflict && (
            <div>
              {t("sites.rootConflictWarning", { root, owner: rootConflict.name })}
            </div>
          )}
          {domainConflict && (
            <div>
              {t("sites.domainConflictWarning", { domain: domainConflict.domain, owner: domainConflict.site.name })}
            </div>
          )}
          {natPortConflict && (
            <div>
              {t("sites.natPortConflictWarning", { port: natPortConflict.port, owner: natPortConflict.owner })}
            </div>
          )}
        </div>
      )}
      {submitting && (
        <div className="rounded border border-info/40 bg-info/5 px-3 py-2 text-xs flex items-center gap-2">
          <Loader2 className="size-3.5 animate-spin shrink-0 text-info" />
          <span className="text-muted-foreground break-all">
            {submitStep || t("sites.processingDefault")}
          </span>
        </div>
      )}
      <div className="flex justify-end gap-2">
        <UIButton onClick={() => void submit()} disabled={submitting}>
          {submitting ? (
            <>
              <Loader2 className="size-4 animate-spin" />
              {isEdit ? t("sites.savingInProgress") : t("sites.creatingInProgress")}
            </>
          ) : (
            <>
              <Save className="size-4" />
              {isEdit ? t("sites.saveChanges") : t("sites.createSite")}
            </>
          )}
        </UIButton>
      </div>
      {isEdit && site && (
        <div className="rounded border border-border bg-muted/40 p-3 text-xs space-y-1">
          <div>
            <span className="text-muted-foreground">{t("sites.configPathLabel")}</span>{" "}
            <span className="font-mono">{site.configPath}</span>
          </div>
          {site.systemdUnit && (
            <div>
              <span className="text-muted-foreground">{t("sites.systemdUnitLabel")}</span>{" "}
              <span className="font-mono">{site.systemdUnit}</span>
            </div>
          )}
          {site.internalPort > 0 && (
            <div>
              <span className="text-muted-foreground">{t("sites.internalPortLabel")}</span>{" "}
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
  const { t } = useLocale();
  const primaryDomain = site.domains[0] || "";
  const [importForm, setImportForm] = useState({
    domain: primaryDomain,
    group: "default",
    certificatePem: "",
    privateKeyPem: ""
  });
  const [pendingChallenge, setPendingChallenge] = useState<RequestCertificateResponse | null>(null);
  // ACME challenge mode 由 capability 推荐 + 站点能力共同决定,**不让用户选**:
  // - 后端 probe_capabilities 判 OpenVZ / NAT / 公网 IP 给出 recommended
  // - 静态站(有 webroot)且推荐 HTTP-01 → HTTP-01
  // - 反代 / RustBinary 站没 webroot → 强制 DNS-01
  // 用户想 override 走"高级 · 手动切换"展开 radio。
  const supportsHttp01 = (site.root?.trim().length ?? 0) > 0;
  const [recommended, setRecommended] = useState<{
    mode: "http01" | "dns01";
    reason: string;
  }>({ mode: "dns01", reason: t("sites.probingHint") });
  const [challengeMode, setChallengeMode] = useState<"http01" | "dns01">("dns01");
  const [advancedOpen, setAdvancedOpen] = useState(false);
  // 把推荐值映射成实际可用值:不支持 webroot 时强制 DNS-01
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const resp = await clients.capability.getCapabilities({});
        if (cancelled) return;
        const raw = resp.capabilities?.recommendedAcmeChallenge?.trim().toLowerCase();
        const mode: "http01" | "dns01" = raw === "http01" ? "http01" : "dns01";
        const reason =
          resp.capabilities?.acmeChallengeReason?.trim() ||
          (mode === "http01" ? t("sites.publicRecommendHttp01") : t("sites.restrictedRecommendDns01"));
        // 站点不支持 HTTP-01 时,推荐值再好也只能落到 DNS-01
        const eff = mode === "http01" && !supportsHttp01 ? "dns01" : mode;
        setRecommended({ mode, reason });
        setChallengeMode(eff);
      } catch {
        // capability 拿不到不致命,保持 dns01 fallback
        setRecommended({
          mode: "dns01",
          reason: t("sites.probeFailedFallback")
        });
      }
    })();
    return () => {
      cancelled = true;
    };
    // supportsHttp01 是 site.root 的纯函数,不会乱重跑
  }, [clients, supportsHttp01, t]);
  // DNS-01 自动轮询状态:加完 TXT 后前端 10 秒一次问 1.1.1.1 DoH,看到
  // 期望值就**自动**再调一次 requestCertificate 完成签发,用户不用手动
  // 再点。"checking"/"propagated"/"failed" 描述实时状态。
  const [autoPollState, setAutoPollState] = useState<
    "idle" | "checking" | "propagated" | "failed"
  >("idle");
  const cert = certificates.find((c) => c.domain === primaryDomain);

  // DNS-01 自动轮询副作用:pendingChallenge 一出现就启动 10 秒 / 次的
  // DoH 查询(走 Cloudflare 1.1.1.1,绕开本机系统 resolver 的缓存),
  // 看到期望 TXT 值就**自动调一次 requestCertificate** 完成签发。
  // 用户不用再手动点第二次。
  // 5 分钟还没看到就停轮询(typical DNS propagation 1-2 min,5 min 没到
  // 大概率 TXT 没写对 / proxy 没关 / 域名解析有问题)。
  useEffect(() => {
    if (!pendingChallenge?.dnsRecordName || !pendingChallenge?.dnsRecordValue) {
      setAutoPollState("idle");
      return;
    }
    if (!primaryDomain) return;

    let cancelled = false;
    let attempts = 0;
    setAutoPollState("checking");

    const checkOnce = async () => {
      if (cancelled) return;
      attempts += 1;
      try {
        const url = `https://1.1.1.1/dns-query?name=${encodeURIComponent(
          pendingChallenge.dnsRecordName
        )}&type=TXT`;
        const resp = await fetch(url, {
          headers: { accept: "application/dns-json" }
        });
        const data: { Answer?: Array<{ data?: string }> } = await resp.json();
        const txts = (data.Answer ?? [])
          .map((a) => (a.data ?? "").replace(/^"|"$/g, ""));
        const seen = txts.includes(pendingChallenge.dnsRecordValue);
        if (cancelled) return;
        if (seen) {
          setAutoPollState("propagated");
          // TXT 已传播,自动再发一次申请走完 finalize 流程
          try {
            const acmeEmail = await ensureAcmeEmail();
            if (!acmeEmail || cancelled) return;
            const finalResp = await clients.ssl.requestCertificate({
              domain: primaryDomain,
              email: acmeEmail,
              challengeType: AcmeChallengeType.DNS_01
            });
            if (cancelled) return;
            if (finalResp.certificate) {
              setPendingChallenge(null);
              setAutoPollState("idle");
              onMessage(t("sites.certIssuedAutoFor", { domain: primaryDomain }));
              onChanged();
            } else if (finalResp.dnsRecordName) {
              // 后端还说要 TXT?说明上一份 pending 已过期,新生成了一份
              setPendingChallenge(finalResp);
              setAutoPollState("checking");
              attempts = 0;
            }
          } catch (err) {
            if (!cancelled) {
              setAutoPollState("failed");
              onError(safeError(err));
            }
          }
          return;
        }
        if (attempts >= 30) {
          // 30 次 × 10s = 5 分钟,放弃,让用户检查 DNS 配置
          setAutoPollState("failed");
        }
      } catch {
        // DoH 网络错误不致命,继续重试
        if (attempts >= 30) setAutoPollState("failed");
      }
    };

    void checkOnce();
    const interval = window.setInterval(() => {
      void checkOnce();
    }, 10_000);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [pendingChallenge, primaryDomain, clients, onChanged, onMessage, onError, t]);

  const requestSsl = async () => {
    if (!primaryDomain) {
      onError(t("sites.domainRequiredError"));
      return;
    }
    const acmeEmail = await ensureAcmeEmail();
    if (!acmeEmail) {
      onError(t("sites.acmeCancelledError"));
      return;
    }
    try {
      const response = await clients.ssl.requestCertificate({
        domain: primaryDomain,
        email: acmeEmail,
        challengeType:
          challengeMode === "http01"
            ? AcmeChallengeType.HTTP_01
            : AcmeChallengeType.DNS_01
      });
      if (response.dnsRecordName || response.dnsRecordValue) {
        // DNS-01 第一次调用:ACME 返回 TXT 挑战
        setPendingChallenge(response);
        onMessage(
          response.status?.message ?? t("sites.addTxtHintMsg")
        );
      } else if (response.certificate) {
        // 拿到证书(HTTP-01 一步到位,或 DNS-01 第二次调用)
        setPendingChallenge(null);
        onMessage(t("sites.certIssuedFor", { domain: response.certificate.domain }));
      } else {
        onMessage(response.status?.message ?? t("sites.requestSubmittedDefault"));
      }
      onChanged();
    } catch (err) {
      onError(safeError(err));
    }
  };
  const importCertificate = async () => {
    try {
      const response = await clients.ssl.importCertificate(importForm);
      onMessage(response.status?.message || t("sites.certImported", { domain: importForm.domain }));
      onChanged();
    } catch (err) {
      onError(safeError(err));
    }
  };
  const renewCert = async () => {
    if (!cert) return;
    try {
      const response = await clients.ssl.renewCertificate({ domain: cert.domain });
      const head = response.status?.message ?? t("sites.certRenewed", { domain: cert.domain });
      if (response.dnsRecordName) {
        const tipLines = [
          head,
          "",
          t("sites.txtRecordNameLine", { name: response.dnsRecordName }),
          t("sites.txtRecordValueLine", { value: response.dnsRecordValue }),
          "",
          t("sites.txtAddHint")
        ];
        onMessage(tipLines.join("\n"));
      } else {
        const detail = response.output;
        onMessage(detail ? t("sites.renewDetailSuffix", { head, detail }) : head);
      }
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
              {t("sites.certRemainingDetail", { days: cert.daysUntilExpiry, group: cert.group || "default" })}
            </div>
          </div>
          <div className="flex gap-1">
            <UIButton size="sm" variant="outline" onClick={() => void renewCert()}>
              <RotateCw className="size-3.5" />
              {t("sites.renew")}
            </UIButton>
            {onRequestRevokeCert && (
              <UIButton
                size="sm"
                variant="ghost"
                title={t("sites.revokeCertTitle")}
                onClick={() => onRequestRevokeCert(cert)}
              >
                <Trash2 className="size-3.5 text-destructive" />
              </UIButton>
            )}
          </div>
        </div>
      ) : (
        <div className="rounded border border-warning/30 bg-warning/5 p-3 text-sm text-muted-foreground">
          {t("sites.noCertYet", { domain: primaryDomain || t("sites.noDomainPlaceholder") })}
        </div>
      )}

      <div className="space-y-2">
        <div className="text-sm font-medium">{t("sites.autoIssueTitle")}</div>
        <div className="rounded border border-info/30 bg-info/5 p-3 text-xs space-y-1">
          <div>
            <span className="font-medium">{t("sites.panelChosePrefix")}</span>
            {challengeMode === "http01"
              ? t("sites.http01Chosen")
              : t("sites.dns01Chosen")}
          </div>
          <div className="text-muted-foreground">{recommended.reason}</div>
          {!supportsHttp01 && recommended.mode === "http01" && (
            <div className="text-muted-foreground">{t("sites.noWebrootAutoSwitch")}</div>
          )}
        </div>
        <UIButton
          size="sm"
          onClick={() => void requestSsl()}
          disabled={!primaryDomain}
        >
          <ShieldCheck className="size-3.5" />
          {primaryDomain
            ? pendingChallenge
              ? t("sites.continueAfterTxt", { domain: primaryDomain })
              : challengeMode === "http01"
                ? t("sites.oneClickRequest", { domain: primaryDomain })
                : t("sites.startRequest", { domain: primaryDomain })
            : t("sites.needBindDomainFirst")}
        </UIButton>
        <button
          type="button"
          className="text-xs text-muted-foreground hover:text-foreground underline-offset-2 hover:underline self-start"
          onClick={() => setAdvancedOpen((v) => !v)}
        >
          {advancedOpen ? t("sites.collapseAdvanced") : t("sites.expandAdvanced")}
        </button>
        {advancedOpen && (
          <div className="flex flex-col gap-1.5 rounded border border-border bg-muted/30 p-3 text-xs">
            <label className="flex items-start gap-2 cursor-pointer">
              <input
                type="radio"
                className="mt-0.5"
                checked={challengeMode === "http01"}
                disabled={!supportsHttp01}
                onChange={() => setChallengeMode("http01")}
              />
              <div>
                <div className="font-medium">
                  HTTP-01
                  {!supportsHttp01 && (
                    <span className="ml-1 text-muted-foreground">{t("sites.http01NoWebrootSuffix")}</span>
                  )}
                </div>
                <div className="text-muted-foreground">
                  {t("sites.http01DescBefore")}<code>.well-known/acme-challenge/</code>{t("sites.http01DescAfter")}
                </div>
              </div>
            </label>
            <label className="flex items-start gap-2 cursor-pointer">
              <input
                type="radio"
                className="mt-0.5"
                checked={challengeMode === "dns01"}
                onChange={() => setChallengeMode("dns01")}
              />
              <div>
                <div className="font-medium">DNS-01</div>
                <div className="text-muted-foreground">{t("sites.dns01Desc")}</div>
              </div>
            </label>
          </div>
        )}
        {pendingChallenge && (pendingChallenge.dnsRecordName || pendingChallenge.dnsRecordValue) && (
          <div className="rounded border border-info/30 bg-info/5 p-3 text-xs space-y-2">
            <div className="font-medium">{t("sites.needTxtRecordHint")}</div>
            <div className="grid gap-1 sm:grid-cols-[80px_1fr]">
              <span className="text-muted-foreground">{t("sites.recordType")}</span>
              <span className="font-mono">TXT</span>
              <span className="text-muted-foreground">{t("sites.hostRecord")}</span>
              <span className="font-mono break-all">{pendingChallenge.dnsRecordName || t("sites.pendingPlaceholder")}</span>
              <span className="text-muted-foreground">{t("sites.recordValue")}</span>
              <span className="font-mono break-all">{pendingChallenge.dnsRecordValue || t("sites.pendingPlaceholder")}</span>
            </div>
            {autoPollState === "checking" && (
              <div className="flex items-center gap-2 text-muted-foreground">
                <Loader2 className="size-3 animate-spin" />
                {t("sites.pollingCheckingHint")}
              </div>
            )}
            {autoPollState === "propagated" && (
              <div className="text-success">{t("sites.pollingPropagatedHint")}</div>
            )}
            {autoPollState === "failed" && (
              <div className="text-destructive">{t("sites.pollingFailedHint")}</div>
            )}
          </div>
        )}
      </div>

      <div className="space-y-2">
        <div className="text-sm font-medium">{t("sites.manualImportTitle")}</div>
        <div className="grid gap-2 sm:grid-cols-2">
          <div className="grid gap-1">
            <UILabel htmlFor="ssl-domain">{t("sites.domain")}</UILabel>
            <UIInput
              id="ssl-domain"
              value={importForm.domain}
              onChange={(event) =>
                setImportForm({ ...importForm, domain: event.target.value })
              }
            />
          </div>
          <div className="grid gap-1">
            <UILabel htmlFor="ssl-group">{t("sites.group")}</UILabel>
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
          {t("sites.import")}
        </UIButton>
      </div>
    </div>
  );
}

/// DeployPanel:站点详情抽屉的"部署" Tab。
///
/// 流程:
/// 1. 用户拖 .zip → 前端切 1MB 片
/// 2. 顺序 POST /api/site/deploy/chunk;每片完成更新上传进度条
/// 3. 最后一片返回 job_id → 开 WS /api/site/deploy/progress?job_id=X
/// 4. WS 收 stage 事件:extracting / swapping / done / error
/// 5. done → 显示部署结果 + 备份路径;error → 提示
///
/// 之前是单次 multipart POST + XHR upload progress,大 zip(>50MB)容易
/// 被反代超时;extract+swap 阶段前端只看 spinner 不知道在做啥。
/// 切片 + WS 既能扛大文件,又能让用户看到每一阶段真实进度。
function DeployPanel({
  site,
  clients,
  onChanged,
  onMessage,
  onError
}: SiteSheetProps & { site: SiteItem }) {
  const { t, locale } = useLocale();
  const [phase, setPhase] = useState<"idle" | "uploading" | "extracting" | "swapping" | "done">(
    "idle"
  );
  const [progressPct, setProgressPct] = useState(0);
  const [extractInfo, setExtractInfo] = useState<{ bytes?: number; files?: number }>({});
  const [lastDeploy, setLastDeploy] = useState<{
    deployedPath: string;
    previousBackupPath: string;
    bytesReceived: number;
    filesExtracted: number;
  } | null>(null);
  const [dragOver, setDragOver] = useState(false);
  const [rolling, setRolling] = useState(false);
  // 默认不保留压缩包 —— 2GB 存储紧张,大多数用户也用不上。勾上后归档到
  // <site_state>/archives/<site>/<ts>.zip,便于后面下载 / 再传。
  const [keepArchive, setKeepArchive] = useState(false);
  const [archives, setArchives] = useState<SiteArchive[]>([]);
  const fileInputRef = useRef<HTMLInputElement | null>(null);

  const supportsDeploy = (site.root?.trim().length ?? 0) > 0;
  const previousBackupExists =
    !!lastDeploy?.previousBackupPath ||
    !!(site.previousBackupPath && site.previousBackupPath.length > 0);

  const refreshArchives = useCallback(async () => {
    try {
      const resp = await clients.site.listSiteArchives({ siteName: site.name });
      setArchives(resp.archives);
    } catch {
      // 列归档失败不致命,默默吞掉,主流程仍可继续
    }
  }, [clients, site.name]);

  useEffect(() => {
    if (supportsDeploy) {
      void refreshArchives();
    }
  }, [supportsDeploy, refreshArchives]);

  const watchProgress = (jobId: string) =>
    new Promise<void>((resolve) => {
      const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
      const url = appendAuthQuery(
        `/api/site/deploy/progress?job_id=${encodeURIComponent(jobId)}`
      );
      const ws = new WebSocket(`${protocol}//${window.location.host}${url}`);
      ws.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data as string) as
            | { stage: "extracting"; bytes_received: number }
            | { stage: "swapping"; files_extracted: number }
            | {
                stage: "done";
                deployed_path: string;
                previous_backup_path: string;
                bytes_received: number;
                files_extracted: number;
              }
            | { stage: "error"; message: string };
          if (data.stage === "extracting") {
            setPhase("extracting");
            setExtractInfo({ bytes: data.bytes_received });
          } else if (data.stage === "swapping") {
            setPhase("swapping");
            setExtractInfo((prev) => ({ ...prev, files: data.files_extracted }));
          } else if (data.stage === "done") {
            setLastDeploy({
              deployedPath: data.deployed_path,
              previousBackupPath: data.previous_backup_path,
              bytesReceived: data.bytes_received,
              filesExtracted: data.files_extracted
            });
            setPhase("done");
            onMessage(
              t("sites.deploySucceeded", {
                files: data.files_extracted,
                bytes: formatBytes(BigInt(data.bytes_received)),
                path: data.deployed_path
              })
            );
            onChanged();
            // 刷新归档列表(用户勾了保留就会出现新条目)
            void refreshArchives();
            try {
              ws.close();
            } catch {
              // ignore
            }
            resolve();
          } else if (data.stage === "error") {
            onError(t("sites.deployFailed", { message: data.message }));
            setPhase("idle");
            try {
              ws.close();
            } catch {
              // ignore
            }
            resolve();
          }
        } catch {
          // 单条事件解析失败不致命,继续等下一条
        }
      };
      ws.onerror = () => {
        // 错误通常跟 onclose 一起到
      };
      ws.onclose = () => {
        // 主流程已经 resolve;onclose 兜底防一直挂着
        resolve();
      };
    });

  const doUpload = async (file: File) => {
    if (!supportsDeploy) {
      onError(t("sites.noWebrootUploadError"));
      return;
    }
    if (!file.name.toLowerCase().endsWith(".zip")) {
      onError(t("sites.onlyZipError"));
      return;
    }
    const chunkSize = 1024 * 1024; // 1 MB
    const totalChunks = Math.max(1, Math.ceil(file.size / chunkSize));
    const uploadId = `${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
    setPhase("uploading");
    setProgressPct(0);
    setExtractInfo({});
    try {
      let jobId: string | null = null;
      for (let i = 0; i < totalChunks; i++) {
        const start = i * chunkSize;
        const end = Math.min(start + chunkSize, file.size);
        const slice = file.slice(start, end);
        const qs = new URLSearchParams({
          name: site.name,
          upload_id: uploadId,
          chunk_index: String(i),
          total_chunks: String(totalChunks),
          keep_archive: keepArchive ? "true" : "false"
        });
        const url = appendAuthQuery(`/api/site/deploy/chunk?${qs.toString()}`);
        const resp = await fetch(url, {
          method: "POST",
          body: slice,
          headers: { "content-type": "application/octet-stream" }
        });
        if (!resp.ok) {
          const text = await resp.text();
          throw new Error(
            t("sites.uploadFailedChunk", { index: i + 1, total: totalChunks, status: resp.status, body: text.slice(0, 200) })
          );
        }
        if (i === totalChunks - 1) {
          const data = (await resp.json()) as { job_id?: string };
          jobId = data.job_id ?? null;
        }
        setProgressPct(Math.round(((i + 1) / totalChunks) * 100));
      }
      if (!jobId) {
        throw new Error(t("sites.noJobIdError"));
      }
      // 切到 WS 等解压 + swap 进度
      setPhase("extracting");
      await watchProgress(jobId);
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
      setPhase("idle");
    }
  };

  const onDrop = (event: React.DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    setDragOver(false);
    const file = event.dataTransfer.files?.[0];
    if (!file) return;
    doUpload(file);
  };

  const onPick = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;
    doUpload(file);
    event.target.value = "";
  };

  const rollback = async () => {
    setRolling(true);
    try {
      const resp = await clients.site.rollbackSite({ siteName: site.name });
      onMessage(t("sites.rolledBackTo", { path: resp.deployedPath }));
      setLastDeploy(null);
      onChanged();
    } catch (err) {
      onError(safeError(err));
    } finally {
      setRolling(false);
    }
  };

  return (
    <div className="flex flex-col gap-3">
      {!supportsDeploy ? (
        <div className="rounded border border-warning/30 bg-warning/5 p-3 text-sm">
          {t("sites.noWebrootDeployBefore")}<code>site.root</code>{t("sites.noWebrootDeployMiddle")}<code>git push</code>{t("sites.noWebrootDeployAfter")}
        </div>
      ) : (
        <>
          <div className="text-sm text-muted-foreground">
            {t("sites.dropHintPart1")}<code>.zip</code>{t("sites.dropHintPart2")}
            <code className="mx-1">{site.root}</code>{t("sites.dropHintPart3")}<code>.previous</code>{t("sites.dropHintPart4")}
          </div>
          <label className="flex items-start gap-2 text-xs cursor-pointer select-none">
            <input
              type="checkbox"
              className="mt-0.5"
              checked={keepArchive}
              disabled={phase !== "idle" && phase !== "done"}
              onChange={(event) => setKeepArchive(event.target.checked)}
            />
            <div>
              <div className="font-medium">{t("sites.keepArchiveLabel")}</div>
              <div className="text-muted-foreground">{t("sites.keepArchiveDesc")}</div>
            </div>
          </label>
          <div
            onDragOver={(e) => {
              e.preventDefault();
              setDragOver(true);
            }}
            onDragLeave={() => setDragOver(false)}
            onDrop={onDrop}
            onClick={() => fileInputRef.current?.click()}
            className={`rounded-lg border-2 border-dashed p-6 text-center cursor-pointer transition-colors ${
              dragOver
                ? "border-primary bg-primary/5"
                : "border-border hover:border-primary/50 hover:bg-muted/30"
            }`}
          >
            <input
              ref={fileInputRef}
              type="file"
              accept=".zip,application/zip"
              className="hidden"
              onChange={onPick}
            />
            <div className="text-sm font-medium">
              {phase === "uploading"
                ? t("sites.uploadingPct", { pct: progressPct })
                : phase === "extracting"
                  ? extractInfo.bytes
                    ? t("sites.extractingWithBytes", { bytes: formatBytes(BigInt(extractInfo.bytes)) })
                    : t("sites.extractingPlain")
                  : phase === "swapping"
                    ? extractInfo.files
                      ? t("sites.swappingWithFiles", { files: extractInfo.files })
                      : t("sites.swappingPlain")
                    : t("sites.dropZonePrompt")}
            </div>
            {phase === "uploading" && (
              <div className="mt-2 h-1.5 w-full rounded bg-muted overflow-hidden">
                <div
                  className="h-full bg-primary transition-all"
                  style={{ width: `${progressPct}%` }}
                />
              </div>
            )}
            {(phase === "extracting" || phase === "swapping") && (
              <div className="mt-2 flex justify-center">
                <Loader2 className="size-4 animate-spin text-muted-foreground" />
              </div>
            )}
          </div>

          {lastDeploy && phase === "done" && (
            <div className="rounded border border-success/30 bg-success/5 p-3 text-xs space-y-1">
              <div className="font-medium">{t("sites.lastDeployTitle")}</div>
              <div className="text-muted-foreground">
                {t("sites.lastDeploySummary", {
                  files: lastDeploy.filesExtracted,
                  bytes: formatBytes(BigInt(lastDeploy.bytesReceived)),
                  path: lastDeploy.deployedPath
                })}
              </div>
              {lastDeploy.previousBackupPath && (
                <div className="text-muted-foreground">
                  {t("sites.backupLabel")}<code>{lastDeploy.previousBackupPath}</code>
                </div>
              )}
            </div>
          )}

          {previousBackupExists && (
            <div className="flex items-center gap-3">
              <UIButton
                size="sm"
                variant="outline"
                onClick={() => void rollback()}
                disabled={rolling || phase !== "idle" && phase !== "done"}
              >
                <RotateCw className="size-3.5" />
                {rolling ? t("sites.rollingInProgress") : t("sites.rollbackToPrevious")}
              </UIButton>
              <span className="text-xs text-muted-foreground">{t("sites.rollbackHint")}</span>
            </div>
          )}

          {archives.length > 0 && (
            <div className="space-y-2">
              <div className="text-sm font-medium">{t("sites.keptArchivesTitle")}</div>
              <div className="rounded border border-border divide-y text-xs">
                {archives.map((arch) => (
                  <div
                    key={arch.path}
                    className="flex items-center justify-between gap-3 px-3 py-2"
                  >
                    <div className="flex-1 min-w-0">
                      <div className="font-mono truncate">{arch.name}</div>
                      <div className="text-muted-foreground">
                        {formatBytes(arch.sizeBytes)} ·{" "}
                        {arch.createdAtSeconds > 0n
                          ? formatDateTime(new Date(Number(arch.createdAtSeconds) * 1000), locale)
                          : "—"}
                      </div>
                    </div>
                    <div className="flex gap-1">
                      <UIButton
                        size="sm"
                        variant="outline"
                        onClick={() => {
                          const url = appendAuthQuery(
                            `/api/fs/download?path=${encodeURIComponent(arch.path)}`
                          );
                          window.location.href = url;
                        }}
                      >
                        {t("sites.download")}
                      </UIButton>
                      <UIButton
                        size="sm"
                        variant="ghost"
                        title={t("sites.deleteArchiveTitle")}
                        onClick={async () => {
                          try {
                            await clients.site.deleteSiteArchive({
                              siteName: site.name,
                              archiveName: arch.name
                            });
                            onMessage(t("sites.archiveDeleted", { name: arch.name }));
                            void refreshArchives();
                          } catch (err) {
                            onError(safeError(err));
                          }
                        }}
                      >
                        <Trash2 className="size-3.5 text-destructive" />
                      </UIButton>
                    </div>
                  </div>
                ))}
              </div>
              <div className="text-xs text-muted-foreground">
                {t("sites.archivesLocationHint", { path: `/var/lib/rustpanel/site/archives/${site.name}/` })}
              </div>
            </div>
          )}

          <div className="rounded border border-info/20 bg-muted/30 p-3 text-xs text-muted-foreground space-y-1">
            <div className="font-medium text-foreground">{t("sites.howToZipTitle")}</div>
            <div>
              {t("sites.zipLine1Before")}<code>public/</code>{t("sites.zipLine1Mid1")}
              <code className="mx-1">.vitepress/dist/</code>{t("sites.zipLine1Mid2")}
              <code className="mx-1">dist/</code>{t("sites.zipLine1After")}
            </div>
            <div>
              {t("sites.zipLine2Before")}<code>zip -r ../site.zip .</code>{t("sites.zipLine2Mid")}<code>.</code>
              {t("sites.zipLine2Mid2")}<code>index.html</code>{t("sites.zipLine2After")}
            </div>
            <div>{t("sites.zipLine3")}</div>
          </div>
        </>
      )}
    </div>
  );
}

function serviceStateVariant(service: SiteServiceStatus): "success" | "destructive" | "warning" | "muted" {
  if (service.error) return "destructive";
  switch (service.activeState) {
    case "active":
      return "success";
    case "failed":
      return "destructive";
    case "activating":
    case "deactivating":
    case "reloading":
      return "warning";
    default:
      return "muted";
  }
}

const splitLines = (text: string) =>
  text
    .split(/[\n,]/)
    .map((item) => item.trim())
    .filter(Boolean);

// 站点关联的 systemd 单元 / 日志文件:看状态、启停重启、看日志尾部(类似宝塔项目管理)。
function SiteServicesPanel({
  site,
  clients,
  onChanged,
  onMessage,
  onError
}: SiteSheetProps & { site: SiteItem }) {
  const { t } = useLocale();
  const [units, setUnits] = useState(site.serviceUnits.join("\n"));
  const [logPaths, setLogPaths] = useState(site.logPaths.join("\n"));
  const [services, setServices] = useState<SiteServiceStatus[]>([]);
  const [busyUnit, setBusyUnit] = useState("");
  const [logSource, setLogSource] = useState("");
  const [log, setLog] = useState({ content: "", truncated: false });

  const refresh = useCallback(async () => {
    if (site.serviceUnits.length === 0) {
      setServices([]);
      return;
    }
    try {
      const response = await clients.site.getSiteServices({ name: site.name });
      setServices(response.services);
    } catch (err) {
      onError(safeError(err));
    }
  }, [clients, site.name, site.serviceUnits.length, onError]);

  useEffect(() => {
    setUnits(site.serviceUnits.join("\n"));
    setLogPaths(site.logPaths.join("\n"));
    void refresh();
  }, [site.serviceUnits, site.logPaths, refresh]);

  const saveLinks = async () => {
    try {
      await clients.site.updateSiteServices({
        name: site.name,
        serviceUnits: splitLines(units),
        logPaths: splitLines(logPaths)
      });
      onMessage(t("sites.linksSaved", { name: site.name }));
      onChanged();
    } catch (err) {
      onError(safeError(err));
    }
  };

  const control = async (unit: string, action: SiteServiceAction, label: string) => {
    setBusyUnit(unit);
    try {
      await clients.site.controlSiteService({ name: site.name, unit, action });
      onMessage(t("sites.serviceActionDone", { unit, action: label }));
      // systemctl 用 --no-block 提交,稍等再刷新才能看到新状态
      window.setTimeout(() => void refresh(), 1500);
    } catch (err) {
      onError(safeError(err));
    } finally {
      setBusyUnit("");
    }
  };

  const loadLog = async (source: string) => {
    setLogSource(source);
    try {
      const response = await clients.site.getSiteServiceLog({ name: site.name, source, lines: 200 });
      setLog({ content: response.content, truncated: response.truncated });
    } catch (err) {
      setLog({ content: "", truncated: false });
      onError(safeError(err));
    }
  };

  const isTimer = (unit: string) => unit.endsWith(".timer");

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between">
        <div className="text-sm font-medium">{t("sites.servicesCountTitle", { count: services.length })}</div>
        <UIButton variant="outline" size="sm" onClick={() => void refresh()}>
          <RefreshCw className="size-3.5" />
          {t("sites.refresh")}
        </UIButton>
      </div>
      {services.length === 0 ? (
        <div className="empty-state text-xs">{t("sites.noServicesLinked")}</div>
      ) : (
        <div className="flex flex-col gap-2">
          {services.map((service) => (
            <div key={service.unit} className="rounded border border-border p-2 text-xs flex flex-col gap-1.5">
              <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-2 min-w-0">
                  <span className="font-mono font-medium truncate">{service.unit}</span>
                  <Badge variant={serviceStateVariant(service)}>
                    {service.error || `${service.activeState}${service.subState ? ` · ${service.subState}` : ""}`}
                  </Badge>
                </div>
                <div className="flex gap-1 shrink-0">
                  <UIButton
                    variant="outline"
                    size="sm"
                    disabled={busyUnit === service.unit}
                    onClick={() => void control(service.unit, SiteServiceAction.START, isTimer(service.unit) ? t("sites.enableTimer") : t("sites.start"))}
                  >
                    {t("sites.start")}
                  </UIButton>
                  <UIButton
                    variant="outline"
                    size="sm"
                    disabled={busyUnit === service.unit}
                    onClick={() => void control(service.unit, SiteServiceAction.RESTART, t("sites.restart"))}
                  >
                    <RotateCw className="size-3.5" />
                    {t("sites.restart")}
                  </UIButton>
                  <UIButton
                    variant="outline"
                    size="sm"
                    disabled={busyUnit === service.unit}
                    onClick={() => void control(service.unit, SiteServiceAction.STOP, t("sites.stop"))}
                  >
                    {t("sites.stop")}
                  </UIButton>
                  {!isTimer(service.unit) && (
                    <UIButton variant="outline" size="sm" onClick={() => void loadLog(service.unit)}>
                      {t("sites.logs")}
                    </UIButton>
                  )}
                </div>
              </div>
              <div className="text-muted-foreground flex flex-wrap gap-x-3">
                {service.description && <span>{service.description}</span>}
                {service.mainPid > 0 && <span>PID {service.mainPid}</span>}
                {service.memoryRssBytes > 0n && <span>{t("sites.memoryLabel", { value: formatBytes(Number(service.memoryRssBytes)) })}</span>}
                {service.activeSince && <span>{t("sites.startedAtLabel", { value: service.activeSince })}</span>}
                {service.result && service.result !== "success" && <span>{t("sites.lastResultLabel", { value: service.result })}</span>}
                {service.nextTrigger && <span>{t("sites.nextTriggerLabel", { value: service.nextTrigger })}</span>}
                {service.lastTrigger && <span>{t("sites.lastTriggerLabel", { value: service.lastTrigger })}</span>}
              </div>
            </div>
          ))}
        </div>
      )}

      {site.logPaths.length > 0 && (
        <div className="flex flex-wrap items-center gap-1">
          <span className="text-xs text-muted-foreground">{t("sites.logFilesLabel")}</span>
          {site.logPaths.map((path) => (
            <UIButton key={path} variant="outline" size="sm" onClick={() => void loadLog(path)}>
              <span className="font-mono">{path}</span>
            </UIButton>
          ))}
        </div>
      )}
      {logSource && (
        <div className="flex flex-col gap-1">
          <div className="flex items-center justify-between text-xs">
            <span className="font-mono">
              {logSource}
              {log.truncated && <span className="text-muted-foreground">{t("sites.truncatedSuffix")}</span>}
            </span>
            <UIButton variant="outline" size="sm" onClick={() => void loadLog(logSource)}>
              <RefreshCw className="size-3.5" />
              {t("sites.refreshLog")}
            </UIButton>
          </div>
          <pre className="rounded-md border border-border bg-muted/40 px-3 py-2 text-xs whitespace-pre-wrap font-mono overflow-x-auto m-0 max-h-80 overflow-y-auto">
            {log.content || t("sites.emptyLog")}
          </pre>
        </div>
      )}

      <div className="text-sm font-medium pt-2">{t("sites.linkedSettingsTitle")}</div>
      <UILabel htmlFor="site-service-units" className="text-xs">
        {t("sites.systemdUnitsLabel")}
      </UILabel>
      <textarea
        id="site-service-units"
        className="pem-input w-full"
        rows={3}
        placeholder={"jobwatch-serve.service\njobwatch-collect.service\njobwatch-collect.timer"}
        value={units}
        onChange={(event) => setUnits(event.target.value)}
      />
      <UILabel htmlFor="site-service-logs" className="text-xs">
        {t("sites.logFilesFieldLabel")}
      </UILabel>
      <textarea
        id="site-service-logs"
        className="pem-input w-full"
        rows={2}
        placeholder="/opt/jobwatch/data/log.txt"
        value={logPaths}
        onChange={(event) => setLogPaths(event.target.value)}
      />
      <div>
        <UIButton size="sm" onClick={() => void saveLinks()}>
          <Save className="size-3.5" />
          {t("sites.saveLinks")}
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
  const { t } = useLocale();
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
      onMessage(t("sites.ruleSaved", { name: form.name }));
      onChanged();
    } catch (err) {
      onError(safeError(err));
    }
  };
  const remove = async (rule: ReverseProxyRule) => {
    try {
      await clients.site.deleteReverseProxyRule({ id: rule.id });
      onMessage(t("sites.ruleDeleted", { name: rule.name }));
      onChanged();
    } catch (err) {
      onError(safeError(err));
    }
  };

  return (
    <div className="flex flex-col gap-3">
      <div className="text-sm font-medium">{t("sites.currentRulesTitle", { count: ownRules.length })}</div>
      {ownRules.length === 0 ? (
        <div className="empty-state text-xs">{t("sites.noRpRulesForDomain", { domain: primaryDomain })}</div>
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
                {t("sites.delete")}
              </UIButton>
            </div>
          ))}
        </div>
      )}

      <div className="text-sm font-medium pt-2">{t("sites.addRuleTitle")}</div>
      <div className="grid gap-2 sm:grid-cols-2">
        <div className="grid gap-1">
          <UILabel htmlFor="rp-name">{t("sites.name")}</UILabel>
          <UIInput
            id="rp-name"
            value={form.name}
            onChange={(event) => setForm({ ...form, name: event.target.value })}
          />
        </div>
        <div className="grid gap-1">
          <UILabel htmlFor="rp-path">{t("sites.pathPrefixLabel")}</UILabel>
          <UIInput
            id="rp-path"
            value={form.pathPrefix}
            onChange={(event) => setForm({ ...form, pathPrefix: event.target.value })}
          />
        </div>
        <div className="grid gap-1 sm:col-span-2">
          <UILabel htmlFor="rp-targets">{t("sites.targetsLabel")}</UILabel>
          <UIInput
            id="rp-targets"
            value={form.targets}
            onChange={(event) => setForm({ ...form, targets: event.target.value })}
          />
        </div>
        <div className="grid gap-1">
          <UILabel>{t("sites.loadBalanceLabel")}</UILabel>
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
              <SelectItem value="round_robin">{t("sites.roundRobin")}</SelectItem>
              <SelectItem value="least_conn">{t("sites.leastConn")}</SelectItem>
              <SelectItem value="ip_hash">{t("sites.ipHash")}</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <div className="grid gap-1">
          <UILabel htmlFor="rp-rate">{t("sites.rateLimitLabel")}</UILabel>
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
          <UILabel>{t("sites.enableCache")}</UILabel>
        </div>
      </div>
      <div className="flex justify-end">
        <UIButton size="sm" onClick={() => void submit()}>
          <Plus className="size-3.5" />
          {t("sites.addRuleTitle")}
        </UIButton>
      </div>
    </div>
  );
}

function RewritePanel({ templates }: { templates: RewriteTemplate[] }) {
  const { t } = useLocale();
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
        <UILabel>{t("sites.templateLabel")}</UILabel>
        <Select
          value={selectedId}
          onValueChange={(id) => {
            setSelectedId(id);
            const template = templates.find((tpl) => tpl.id === id);
            if (template) setContent(template.content);
          }}
        >
          <SelectTrigger>
            <SelectValue placeholder={t("sites.selectTemplatePlaceholder")} />
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
      <p className="text-xs text-muted-foreground">{t("sites.rewriteFooterNote")}</p>
    </div>
  );
}

// 纯前端 CSV 导出:从已加载的结果生成,不经服务器(低配不占后端内存)。
