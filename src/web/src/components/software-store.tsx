import { Badge } from "../components/ui/badge";
import { Button as UIButton } from "../components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../components/ui/select";
import { AppCategory, AppTemplate, CompatibilityStatus, InstallMethod } from "../gen/rustpanel/v1/appstore_pb";
import { useLocale } from "../lib/i18n/locale-provider";
import { tGlobal, type TFn } from "../lib/i18n/translate";
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
  const { t } = useLocale();
  const groups: Array<{
    key: CompatibilityStatus;
    title: string;
    description: string;
    tone: "ok" | "warn" | "muted";
    defaultOpen: boolean;
  }> = [
    {
      key: CompatibilityStatus.COMPATIBLE,
      title: t("apps.groupAvailable"),
      description: t("apps.groupAvailableDesc"),
      tone: "ok",
      defaultOpen: true
    },
    {
      key: CompatibilityStatus.RESOURCE_SHORT,
      title: t("apps.groupResourceShort"),
      description: t("apps.groupResourceShortDesc"),
      tone: "warn",
      defaultOpen: false
    },
    {
      key: CompatibilityStatus.KERNEL_UNSUPPORTED,
      title: t("apps.groupKernelUnsupported"),
      description: t("apps.groupKernelUnsupportedDesc"),
      tone: "muted",
      defaultOpen: false
    },
    {
      key: CompatibilityStatus.NEEDS_DOCKER,
      title: t("apps.groupNeedsDocker"),
      description: t("apps.groupNeedsDockerDesc"),
      tone: "muted",
      defaultOpen: false
    }
  ];

  return (
    <div className="panel full-span flex flex-col gap-4">
      <div className="flex items-center gap-2">
        <Store className="size-4 text-primary" />
        <span className="font-semibold">{t("apps.storeTitle")}</span>
        <span className="text-xs text-muted-foreground">
          {t("apps.groupedByHostCount", { count: templates.length })}
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
  const { t } = useLocale();
  const isCompatible = template.compatibility === CompatibilityStatus.COMPATIBLE;
  return (
    <div className="flex flex-col gap-2 rounded-md border border-border bg-card p-3">
      <div className="flex items-start justify-between gap-2">
        <div className="flex flex-col min-w-0 gap-0.5">
          <div className="flex items-center gap-1.5 flex-wrap">
            <span className="font-medium truncate">{template.name}</span>
            {template.recommended && <Badge variant="info">{t("apps.recommended")}</Badge>}
          </div>
          <span className="text-xs text-muted-foreground truncate">
            {appCategoryLabel(template.category, t)} · {installMethodLabel(template.installMethod, t)}
          </span>
        </div>
        {template.homepage && (
          <a
            href={template.homepage}
            target="_blank"
            rel="noopener noreferrer"
            className="text-muted-foreground hover:text-foreground transition-colors shrink-0"
            title={t("apps.homepageTitle", { url: template.homepage })}
          >
            <ExternalLink className="size-3.5" />
          </a>
        )}
      </div>
      <p className="text-xs text-muted-foreground line-clamp-2">{template.description}</p>
      <div className="flex flex-wrap gap-1 text-[11px] text-muted-foreground">
        {template.minRamMb > 0 && (
          <Badge variant="muted">{t("apps.ramAtLeast", { value: template.minRamMb })}</Badge>
        )}
        {template.minDiskMb > 0 && (
          <Badge variant="muted">{t("apps.diskAtLeast", { value: template.minDiskMb })}</Badge>
        )}
        {template.expectedRuntimeRamMb > 0 && (
          <Badge variant="outline">{t("apps.runtimeApprox", { value: template.expectedRuntimeRamMb })}</Badge>
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
                {v.recommended && ` (${t("apps.recommended")})`}
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
            {t("apps.install")}
          </>
        ) : (
          t("apps.notAvailableYet")
        )}
      </UIButton>
    </div>
  );
}

export function appCategoryLabel(category: AppCategory, t: TFn = tGlobal): string {
  switch (category) {
    case AppCategory.WEB_SERVER:
      return t("apps.categoryWebServer");
    case AppCategory.DATABASE:
      return t("apps.categoryDatabase");
    case AppCategory.RUNTIME:
      return t("apps.categoryRuntime");
    case AppCategory.TOOL:
      return t("apps.categoryTool");
    case AppCategory.VPN:
      return t("apps.categoryVpn");
    case AppCategory.MONITOR:
      return t("apps.categoryMonitor");
    default:
      return t("apps.categoryOther");
  }
}

export function installMethodLabel(method: InstallMethod, t: TFn = tGlobal): string {
  switch (method) {
    case InstallMethod.NATIVE_PACKAGE:
      return t("apps.methodNativePackage");
    case InstallMethod.BINARY_DOWNLOAD:
      return t("apps.methodBinaryDownload");
    case InstallMethod.CARGO_INSTALL:
      return t("apps.methodCargoInstall");
    case InstallMethod.DOCKER_COMPOSE:
      return t("apps.methodDockerCompose");
    default:
      return t("apps.methodUnspecified");
  }
}

export function CapabilityRow({ label, value }: { label: string; value: boolean }) {
  const { t } = useLocale();
  return (
    <div className="flex items-center justify-between gap-2">
      <span className="text-muted-foreground">{label}</span>
      <Badge variant={value ? "success" : "muted"}>
        {value ? t("components.available") : t("components.unavailable")}
      </Badge>
    </div>
  );
}
