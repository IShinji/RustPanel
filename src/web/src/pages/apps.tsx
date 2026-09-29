import { IconButton, Input, StatusPill } from "../components/form-controls";
import { Badge } from "../components/ui/badge";
import { Button as UIButton } from "../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow as UITableRow } from "../components/ui/table";
import { AppTemplate, InstalledApp } from "../gen/rustpanel/v1/appstore_pb";
import { ComposeProject, ContainerItem, ImageItem } from "../gen/rustpanel/v1/docker_pb";
import { ProxyInstance, ProxyState, VpnCapability } from "../gen/rustpanel/v1/proxy_pb";
import { SiteItem } from "../gen/rustpanel/v1/site_pb";
import { WorkloadItem, WorkloadState } from "../gen/rustpanel/v1/workload_pb";
import { formatBytes, safeError } from "../lib/format";
import { defaultComposeForm, defaultDockerQuotaForm, defaultImagePullForm, defaultImageRollbackForm, defaultMicroSiteForm, defaultProxyForm, defaultWorkloadForm, type ComposeForm, type DockerQuotaForm, type ImagePullForm, type ImageRollbackForm, type MicroSiteForm, type ProxyForm, type WorkloadForm } from "../lib/forms";
import { type Clients } from "../lib/rpc";
import { Archive, Boxes, Download, FileText, Globe, Pause, Play, Power, RefreshCw, RotateCw, Save, ShieldAlert, ShieldCheck, Square, Store, TerminalSquare, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { SoftwareStore } from "../components/software-store";
import { appStateVariant } from "../lib/labels";
import { useMountEffect } from "../lib/hooks";
import { useLocale } from "../lib/i18n/locale-provider";

export function SoftwareStorePage({ clients }: { clients: Clients }) {
  const { t } = useLocale();
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
      setMessage(t("apps.deployStarted", { name: template.name }));
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
      setMessage(t("apps.uninstalled", { name: app.appName }));
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  return (
    <section className="flex flex-col gap-5">
      <header className="flex items-start justify-between gap-4 flex-wrap">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-semibold tracking-tight m-0">{t("apps.storeTitle")}</h1>
          <p className="text-sm text-muted-foreground m-0">{t("apps.storePageSubtitle")}</p>
        </div>
        <UIButton variant="outline" size="sm" onClick={() => void load()}>
          <RefreshCw className="size-4" />
          {t("apps.refresh")}
        </UIButton>
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
          <CardTitle>{t("apps.installedEnvironments")}</CardTitle>
          <CardDescription>{t("apps.installedEnvironmentsDesc")}</CardDescription>
        </CardHeader>
        <CardContent>
          {installedApps.length === 0 ? (
            <div className="empty-state text-sm">{t("apps.noInstalledApps")}</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>{t("apps.colName")}</TableHead>
                  <TableHead>{t("apps.colImageVersion")}</TableHead>
                  <TableHead>{t("apps.colStatus")}</TableHead>
                  <TableHead className="text-right">{t("apps.colActions")}</TableHead>
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
                      <Badge variant={appStateVariant(app.state)}>{app.state || t("apps.unknown")}</Badge>
                    </TableCell>
                    <TableCell className="text-right">
                      <UIButton variant="ghost" size="sm" onClick={() => void uninstallApp(app)}>
                        <Trash2 className="size-3.5" />
                        {t("apps.uninstall")}
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

export function DockerApps({ clients }: { clients: Clients }) {
  const { t } = useLocale();
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

  useMountEffect(() => load());

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
        `${t("apps.pruneResult", {
          count: response.deletedCount,
          space: formatBytes(response.spaceReclaimedBytes),
          summary: response.summary
        })}\n`
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
          <h1>{t("apps.dockerTitle")}</h1>
          <p>
            {error ||
              t("apps.dockerSummary", {
                containers: containers.length,
                images: images.length,
                templates: templates.length
              })}
          </p>
        </div>
        <IconButton label={t("apps.refresh")} icon={RefreshCw} onClick={() => void load()} />
      </header>

      <div className="panel wide-panel">
        <div className="panel-title"><Boxes size={18} /><span>{t("apps.containerList")}</span></div>
        <div className="table-list">
          {containers.map((container) => (
            <div className="table-row" key={container.id}>
              <div>
                <strong>{container.name || container.id.slice(0, 12)}</strong>
                <small>
                  {container.image} · {container.statusText} · CPU {container.cpuLimitCores ? container.cpuLimitCores.toFixed(2) : t("apps.unlimited")} · {t("apps.memoryMb")} {container.memoryLimitBytes ? formatBytes(container.memoryLimitBytes) : t("apps.unlimited")}
                </small>
              </div>
              <StatusPill label={container.state || t("apps.unknown")} tone={container.state === "running" ? "good" : "muted"} />
              <div className="row-actions">
                <IconButton label={t("apps.start")} icon={Play} onClick={() => void action(container.id, "start")} />
                <IconButton label={t("apps.stop")} icon={Square} onClick={() => void action(container.id, "stop")} />
                <IconButton label={t("apps.restart")} icon={RotateCw} onClick={() => void action(container.id, "restart")} />
                <IconButton label={t("apps.pause")} icon={Pause} onClick={() => void action(container.id, "pause")} />
                <IconButton label={t("apps.logs")} icon={TerminalSquare} onClick={() => void watchLogs(container)} />
              </div>
            </div>
          ))}
        </div>
      </div>

      <div className="panel">
        <div className="panel-title"><Boxes size={18} /><span>{t("apps.resourceQuota")}</span></div>
        <Input label={t("apps.containerId")} value={quotaForm.containerId} onChange={(containerId) => setQuotaForm({ ...quotaForm, containerId })} />
        <Input label={t("apps.cpuCores")} value={quotaForm.cpuLimitCores} onChange={(cpuLimitCores) => setQuotaForm({ ...quotaForm, cpuLimitCores })} />
        <Input label={t("apps.memoryMb")} value={quotaForm.memoryLimitMb} onChange={(memoryLimitMb) => setQuotaForm({ ...quotaForm, memoryLimitMb })} />
        <button onClick={() => void saveQuota()} type="button"><Save size={15} />{t("apps.applyQuota")}</button>
      </div>

      <div className="panel wide-panel">
        <div className="panel-title"><Download size={18} /><span>{t("apps.imageManagement")}</span></div>
        <div className="inline-grid">
          <Input label={t("apps.image")} value={pullForm.image} onChange={(image) => setPullForm({ ...pullForm, image })} />
          <Input label={t("apps.tag")} value={pullForm.tag} onChange={(tag) => setPullForm({ ...pullForm, tag })} />
        </div>
        <div className="row-actions backup-actions">
          <button onClick={() => void pullImage()} type="button"><Download size={15} />{t("apps.pull")}</button>
          <button onClick={() => void pruneResources()} type="button"><Trash2 size={15} />{t("apps.pruneResiduals")}</button>
        </div>
        <div className="inline-grid">
          <Input label={t("apps.rollbackSource")} value={rollbackForm.sourceImage} onChange={(sourceImage) => setRollbackForm({ ...rollbackForm, sourceImage })} />
          <Input label={t("apps.targetRepository")} value={rollbackForm.targetRepository} onChange={(targetRepository) => setRollbackForm({ ...rollbackForm, targetRepository })} />
          <Input label={t("apps.targetTag")} value={rollbackForm.targetTag} onChange={(targetTag) => setRollbackForm({ ...rollbackForm, targetTag })} />
        </div>
        <button onClick={() => void rollbackImage()} type="button"><RotateCw size={15} />{t("apps.rollbackTag")}</button>
        <pre className="report-output compact-output">{pullLines.join("") || t("apps.noImageTasks")}</pre>
      </div>

      <div className="panel">
        <div className="panel-title"><Archive size={18} /><span>{t("apps.localImages")}</span></div>
        <div className="table-list compact-list">
          {images.slice(0, 8).map((image) => (
            <div className="key-row" key={image.id}>
              <strong>{image.repoTags[0] || image.id.slice(0, 18)}</strong>
              <small>{formatBytes(image.sizeBytes)} · containers {image.containers}</small>
            </div>
          ))}
          {!images.length && <div className="empty-state">{t("apps.noImages")}</div>}
        </div>
      </div>

      <div className="panel wide-panel">
        <div className="panel-title"><FileText size={18} /><span>{t("apps.composeOrchestration")}</span></div>
        <Input label={t("apps.projectName")} value={composeForm.name} onChange={(name) => setComposeForm({ ...composeForm, name })} />
        <textarea
          className="pem-input code-input"
          value={composeForm.composeYaml}
          onChange={(event) => setComposeForm({ ...composeForm, composeYaml: event.target.value })}
        />
        <div className="row-actions backup-actions">
          <button onClick={() => void saveCompose()} type="button"><Save size={15} />{t("apps.save")}</button>
          <button onClick={() => void deployCompose()} type="button"><Play size={15} />{t("apps.deploy")}</button>
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
                <IconButton label={t("apps.edit")} icon={FileText} onClick={() => setComposeForm({ name: project.name, composeYaml: project.composeYaml })} />
                <IconButton label={t("apps.deploy")} icon={Play} onClick={() => void deployCompose(project.name)} />
                <IconButton label={t("apps.delete")} icon={Trash2} onClick={() => void removeCompose(project.name)} />
              </div>
            </div>
          ))}
          {!composeProjects.length && <div className="empty-state">{t("apps.noComposeProjects")}</div>}
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
        <div className="panel-title"><Store size={18} /><span>{t("apps.installedEnvironments")}</span></div>
        <div className="table-list">
          {installedApps.map((app) => (
            <div className="table-row" key={app.appName}>
              <div>
                <strong>{app.appName}</strong>
                <small>{app.slug} {app.version} · {app.image}</small>
              </div>
              <StatusPill label={app.state || "installed"} tone="good" />
              <div className="row-actions">
                <IconButton label={t("apps.update")} icon={RefreshCw} onClick={() => void updateInstalledApp(app)} />
                <IconButton label={t("apps.uninstall")} icon={Trash2} onClick={() => void uninstallApp(app)} />
              </div>
            </div>
          ))}
          {!installedApps.length && <div className="empty-state">{t("apps.noInstalledEnvironments")}</div>}
        </div>
      </div>

      <div className="panel log-panel">
        <div className="panel-title"><TerminalSquare size={18} /><span>{t("apps.containerLogs")}</span></div>
        <pre>{logLines.join("")}</pre>
      </div>
    </section>
  );
}

export function MicroPanel({ clients }: { clients: Clients }) {
  const { t } = useLocale();
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

  useMountEffect(() => load());

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
          <h1>{t("apps.microTitle")}</h1>
          <p>
            {status ||
              t("apps.microSummary", { sites: sites.length, workloads: workloads.length, proxies: proxies.length })}
          </p>
        </div>
        <IconButton label={t("apps.refresh")} icon={RefreshCw} onClick={() => void load()} />
      </header>

      <div className="panel">
        <div className="panel-title"><Globe size={18} /><span>{t("apps.builtinStaticHosting")}</span></div>
        <Input label={t("apps.name")} value={siteForm.name} onChange={(name) => setSiteForm({ ...siteForm, name })} />
        <Input label={t("apps.directory")} value={siteForm.root} onChange={(root) => setSiteForm({ ...siteForm, root })} />
        <button onClick={() => void createSite()} type="button"><Save size={15} />{t("apps.create")}</button>
      </div>

      <div className="panel wide-panel">
        <div className="panel-title"><FileText size={18} /><span>{t("apps.staticSites")}</span></div>
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
          {!sites.length && <div className="empty-state">{t("apps.noBuiltinSites")}</div>}
        </div>
      </div>

      <div className="panel">
        <div className="panel-title"><TerminalSquare size={18} /><span>{t("apps.rustWorkloads")}</span></div>
        <Input label={t("apps.name")} value={workloadForm.name} onChange={(name) => setWorkloadForm({ ...workloadForm, name })} />
        <Input label={t("apps.command")} value={workloadForm.command} onChange={(command) => setWorkloadForm({ ...workloadForm, command })} />
        <Input label={t("apps.directory")} value={workloadForm.cwd} onChange={(cwd) => setWorkloadForm({ ...workloadForm, cwd })} />
        <Input label={t("apps.memoryMb")} value={workloadForm.memoryLimitMb} onChange={(memoryLimitMb) => setWorkloadForm({ ...workloadForm, memoryLimitMb })} />
        <button onClick={() => void saveWorkload()} type="button"><Save size={15} />{t("apps.save")}</button>
      </div>

      <div className="panel wide-panel">
        <div className="panel-title"><Power size={18} /><span>{t("apps.managedTasks")}</span></div>
        <div className="table-list">
          {workloads.map((workload) => (
            <div className="table-row" key={workload.id}>
              <div>
                <strong>{workload.name}</strong>
                <small>PID {workload.pid || "-"} · {workload.command} · {Number(workload.memoryLimitMb)}MB</small>
              </div>
              <StatusPill label={WorkloadState[workload.state]} tone={workload.state === WorkloadState.RUNNING ? "good" : "muted"} />
              <div className="row-actions">
                <IconButton label={t("apps.start")} icon={Play} onClick={() => void startWorkload(workload)} />
                <IconButton label={t("apps.stop")} icon={Square} onClick={() => void stopWorkload(workload)} />
                <IconButton label={t("apps.logs")} icon={TerminalSquare} onClick={() => void readWorkloadLog(workload)} />
              </div>
            </div>
          ))}
          {!workloads.length && <div className="empty-state">{t("apps.noManagedTasks")}</div>}
        </div>
      </div>

      <div className="panel">
        <div className="panel-title"><ShieldCheck size={18} /><span>shadowsocks-rust</span></div>
        <Input label={t("apps.name")} value={proxyForm.name} onChange={(name) => setProxyForm({ ...proxyForm, name })} />
        <Input label={t("apps.port")} value={proxyForm.listenPort} onChange={(listenPort) => setProxyForm({ ...proxyForm, listenPort })} />
        <Input label={t("apps.password")} value={proxyForm.password} onChange={(password) => setProxyForm({ ...proxyForm, password })} />
        <button onClick={() => void saveProxy()} type="button"><Save size={15} />{t("apps.save")}</button>
      </div>

      <div className="panel wide-panel">
        <div className="panel-title"><ShieldCheck size={18} /><span>{t("apps.proxyInstances")}</span></div>
        <div className="table-list">
          {proxies.map((proxy) => (
            <div className="table-row" key={proxy.id}>
              <div>
                <strong>{proxy.name}</strong>
                <small>{proxy.templateId} · {proxy.listenHost}:{proxy.listenPort} · PID {proxy.pid || "-"}</small>
              </div>
              <StatusPill label={ProxyState[proxy.state]} tone={proxy.state === ProxyState.RUNNING ? "good" : "muted"} />
              <div className="row-actions">
                <IconButton label={t("apps.start")} icon={Play} onClick={() => void startProxy(proxy)} />
                <IconButton label={t("apps.stop")} icon={Square} onClick={() => void stopProxy(proxy)} />
                <IconButton label={t("apps.logs")} icon={TerminalSquare} onClick={() => void readProxyLog(proxy)} />
              </div>
            </div>
          ))}
          {!proxies.length && <div className="empty-state">{t("apps.noProxyInstances")}</div>}
        </div>
      </div>

      <div className="panel">
        <div className="panel-title"><ShieldAlert size={18} /><span>{t("apps.vpnCapabilityDetection")}</span></div>
        <div className="table-list compact-list">
          {vpnCapabilities.map((capability) => (
            <div className="key-row" key={capability.id}>
              <strong>{capability.name}</strong>
              <small>{capability.reason}</small>
              <StatusPill
                label={capability.available ? t("components.available") : t("components.unavailable")}
                tone={capability.available ? "good" : "danger"}
              />
            </div>
          ))}
        </div>
      </div>

      <div className="panel log-panel">
        <div className="panel-title"><TerminalSquare size={18} /><span>{t("apps.microLog")}</span></div>
        <pre>{log || t("apps.noLogs")}</pre>
      </div>
    </section>
  );
}
