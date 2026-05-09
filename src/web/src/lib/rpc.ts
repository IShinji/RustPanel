import { createClient } from "@connectrpc/connect";
import { createGrpcWebTransport } from "@connectrpc/connect-web";

import { AppStoreService } from "../gen/rustpanel/v1/appstore_pb";
import { AuthService } from "../gen/rustpanel/v1/auth_pb";
import { CronService } from "../gen/rustpanel/v1/cron_pb";
import { DatabaseService } from "../gen/rustpanel/v1/db_pb";
import { DockerService } from "../gen/rustpanel/v1/docker_pb";
import { FileSystemService } from "../gen/rustpanel/v1/fs_pb";
import { MonitorService } from "../gen/rustpanel/v1/monitor_pb";
import { SecurityService } from "../gen/rustpanel/v1/security_pb";
import { SiteService } from "../gen/rustpanel/v1/site_pb";
import { SslService } from "../gen/rustpanel/v1/ssl_pb";
import { SystemService } from "../gen/rustpanel/v1/system_pb";

export function createRpcClients(baseUrl = window.location.origin) {
  const transport = createGrpcWebTransport({ baseUrl });

  return {
    appStore: createClient(AppStoreService, transport),
    auth: createClient(AuthService, transport),
    cron: createClient(CronService, transport),
    database: createClient(DatabaseService, transport),
    docker: createClient(DockerService, transport),
    files: createClient(FileSystemService, transport),
    monitor: createClient(MonitorService, transport),
    security: createClient(SecurityService, transport),
    site: createClient(SiteService, transport),
    ssl: createClient(SslService, transport),
    system: createClient(SystemService, transport)
  };
}
