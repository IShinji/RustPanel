import { Badge } from "../components/ui/badge";
import { Button as UIButton } from "../components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../components/ui/select";
import { AppCategory, AppTemplate, CompatibilityStatus, InstallMethod } from "../gen/rustpanel/v1/appstore_pb";
import { cn } from "../lib/utils";
import { ChevronDown, ExternalLink, Play, Store } from "lucide-react";
import { useState } from "react";

export function SoftwareStore({
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

export function SoftwareGroup({
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

export function SoftwareCard({
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

export function appCategoryLabel(category: AppCategory): string {
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

export function installMethodLabel(method: InstallMethod): string {
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

export function CapabilityRow({ label, value }: { label: string; value: boolean }) {
  return (
    <div className="flex items-center justify-between gap-2">
      <span className="text-muted-foreground">{label}</span>
      <Badge variant={value ? "success" : "muted"}>{value ? "可用" : "不可用"}</Badge>
    </div>
  );
}
