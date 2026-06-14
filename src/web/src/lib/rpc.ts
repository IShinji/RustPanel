import { Code, ConnectError, type Interceptor } from "@connectrpc/connect";
import { createClient } from "@connectrpc/connect";
import { createGrpcWebTransport } from "@connectrpc/connect-web";

import { AppStoreService } from "../gen/rustpanel/v1/appstore_pb";
import { AuthService } from "../gen/rustpanel/v1/auth_pb";
import { AuditService } from "../gen/rustpanel/v1/audit_pb";
import { BackupService } from "../gen/rustpanel/v1/backup_pb";
import { CapabilityService } from "../gen/rustpanel/v1/capability_pb";
import { ClusterService } from "../gen/rustpanel/v1/cluster_pb";
import { CronService } from "../gen/rustpanel/v1/cron_pb";
import { DatabaseService } from "../gen/rustpanel/v1/db_pb";
import { DockerService } from "../gen/rustpanel/v1/docker_pb";
import { FileSystemService } from "../gen/rustpanel/v1/fs_pb";
import { MonitorService } from "../gen/rustpanel/v1/monitor_pb";
import { NotificationService } from "../gen/rustpanel/v1/notification_pb";
import { ProxyService } from "../gen/rustpanel/v1/proxy_pb";
import { RollbackService } from "../gen/rustpanel/v1/rollback_pb";
import { SecurityService } from "../gen/rustpanel/v1/security_pb";
import { SiteService } from "../gen/rustpanel/v1/site_pb";
import { SslService } from "../gen/rustpanel/v1/ssl_pb";
import { SystemService } from "../gen/rustpanel/v1/system_pb";
import { VsmtpAliasService } from "../gen/rustpanel/v1/vsmtp_pb";
import { WorkloadService } from "../gen/rustpanel/v1/workload_pb";

const TOKEN_KEY = "rustpanel.token";
const AUTH_CHANGED_EVENT = "rustpanel:auth-changed";

// 浏览器关闭标签即清空 token,降低长期暴露风险
export function getAuthToken(): string | null {
  return sessionStorage.getItem(TOKEN_KEY);
}

export function setAuthToken(token: string): void {
  sessionStorage.setItem(TOKEN_KEY, token);
  window.dispatchEvent(new CustomEvent(AUTH_CHANGED_EVENT));
}

export function clearAuthToken(): void {
  sessionStorage.removeItem(TOKEN_KEY);
  window.dispatchEvent(new CustomEvent(AUTH_CHANGED_EVENT));
}

export function onAuthChanged(handler: () => void): () => void {
  window.addEventListener(AUTH_CHANGED_EVENT, handler);
  return () => window.removeEventListener(AUTH_CHANGED_EVENT, handler);
}

// 给 fetch / WebSocket / 下载链接用的辅助:HTTP 走 Authorization 头,
// WebSocket / SSE 因为浏览器无法设置自定义头,通过 ?token= 查询参数下发
export function authFetch(input: RequestInfo | URL, init: RequestInit = {}): Promise<Response> {
  const token = getAuthToken();
  const headers = new Headers(init.headers);
  if (token) {
    headers.set("Authorization", `Bearer ${token}`);
  }
  return fetch(input, { ...init, headers }).then(async (response) => {
    if (response.status === 401) {
      clearAuthToken();
    }
    return response;
  });
}

export function appendAuthQuery(url: string): string {
  const token = getAuthToken();
  if (!token) return url;
  const separator = url.includes("?") ? "&" : "?";
  return `${url}${separator}token=${encodeURIComponent(token)}`;
}

// 给所有 gRPC-Web 请求注入 Authorization,401 时清空 token 触发跳转
const authInterceptor: Interceptor = (next) => async (req) => {
  const token = getAuthToken();
  if (token) {
    req.header.set("Authorization", `Bearer ${token}`);
  }
  try {
    return await next(req);
  } catch (error) {
    if (error instanceof ConnectError && error.code === Code.Unauthenticated) {
      clearAuthToken();
    }
    throw error;
  }
};

export function createRpcClients(baseUrl = window.location.origin) {
  const transport = createGrpcWebTransport({
    baseUrl,
    interceptors: [authInterceptor]
  });

  return {
    appStore: createClient(AppStoreService, transport),
    auth: createClient(AuthService, transport),
    audit: createClient(AuditService, transport),
    backup: createClient(BackupService, transport),
    capability: createClient(CapabilityService, transport),
    cluster: createClient(ClusterService, transport),
    cron: createClient(CronService, transport),
    database: createClient(DatabaseService, transport),
    docker: createClient(DockerService, transport),
    files: createClient(FileSystemService, transport),
    monitor: createClient(MonitorService, transport),
    notification: createClient(NotificationService, transport),
    proxy: createClient(ProxyService, transport),
    rollback: createClient(RollbackService, transport),
    security: createClient(SecurityService, transport),
    site: createClient(SiteService, transport),
    ssl: createClient(SslService, transport),
    system: createClient(SystemService, transport),
    vsmtpAlias: createClient(VsmtpAliasService, transport),
    workload: createClient(WorkloadService, transport)
  };
}
