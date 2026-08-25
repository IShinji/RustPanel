import {
  FirewallAction,
  FirewallDirection,
  FirewallProtocol,
  SshKeyAlgorithm,
  WafRuleKind
} from "@/gen/rustpanel/v1/security_pb";

// 枚举 → 中文短语,以及几个纯字符串工具。都是无状态纯函数,单独成模块方便
// 各页面各自引用,也方便直接写单测。

export function auditLevelVariant(level: string): "destructive" | "warning" | "info" | "muted" {
  const normalized = level.toLowerCase();
  if (normalized === "error" || normalized === "critical" || normalized === "alert") return "destructive";
  if (normalized === "warn" || normalized === "warning") return "warning";
  if (normalized === "info" || normalized === "notice") return "info";
  return "muted";
}

export function firewallProtocolLabel(protocol: FirewallProtocol): string {
  if (protocol === FirewallProtocol.TCP) return "TCP";
  if (protocol === FirewallProtocol.UDP) return "UDP";
  if (protocol === FirewallProtocol.ICMP) return "ICMP";
  return "-";
}

export function firewallActionLabel(action: FirewallAction): string {
  if (action === FirewallAction.ALLOW) return "放行";
  if (action === FirewallAction.DENY) return "屏蔽";
  if (action === FirewallAction.REJECT) return "拒绝";
  return "-";
}

export function firewallDirectionLabel(direction: FirewallDirection): string {
  if (direction === FirewallDirection.INBOUND) return "入站";
  if (direction === FirewallDirection.OUTBOUND) return "出站";
  return "-";
}

export function wafKindLabel(kind: WafRuleKind): string {
  if (kind === WafRuleKind.CC) return "CC";
  if (kind === WafRuleKind.SQL_INJECTION) return "SQL 注入";
  if (kind === WafRuleKind.XSS) return "XSS";
  if (kind === WafRuleKind.KEYWORD) return "关键词";
  if (kind === WafRuleKind.SCANNER) return "扫描器";
  return "-";
}

export function sshAlgorithmLabel(algorithm: SshKeyAlgorithm): string {
  if (algorithm === SshKeyAlgorithm.ED25519) return "Ed25519";
  if (algorithm === SshKeyAlgorithm.RSA) return "RSA";
  return "-";
}

export function languageForPath(name: string): string {
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

export function parentPath(path: string): string {
  if (path === "/") return "/";
  const parts = path.split("/").filter(Boolean);
  parts.pop();
  return `/${parts.join("/")}`;
}

export function appStateVariant(
  state: string
): "success" | "destructive" | "warning" | "muted" {
  const normalized = state.toLowerCase();
  if (normalized.includes("running") || normalized === "up" || normalized === "active") return "success";
  if (normalized.includes("error") || normalized.includes("fail") || normalized === "dead") return "destructive";
  if (normalized.includes("paused") || normalized.includes("restart") || normalized.includes("starting")) return "warning";
  return "muted";
}

