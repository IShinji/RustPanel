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

export function SoftwareStorePage({ clients }: { clients: Clients }) {
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

export function DockerApps({ clients }: { clients: Clients }) {
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

export function MicroPanel({ clients }: { clients: Clients }) {
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
