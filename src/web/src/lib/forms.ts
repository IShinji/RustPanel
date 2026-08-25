import {
  FirewallAction,
  FirewallBackend,
  FirewallDirection,
  FirewallProtocol,
  SshKeyAlgorithm,
  WafRuleKind
} from "@/gen/rustpanel/v1/security_pb";

// 各页面共享的表单形状与默认值。此前和三十来个页面挤在 App.tsx 顶部,
// 页面一拆出去就互相引用不到,统一收在这里。

export type DockerQuotaForm = {
  containerId: string;
  cpuLimitCores: string;
  memoryLimitMb: string;
};
export type ImagePullForm = {
  image: string;
  tag: string;
};
export type ImageRollbackForm = {
  sourceImage: string;
  targetRepository: string;
  targetTag: string;
};
export type ComposeForm = {
  name: string;
  composeYaml: string;
};
export type WorkloadForm = {
  name: string;
  command: string;
  cwd: string;
  memoryLimitMb: string;
};
export type MicroSiteForm = {
  name: string;
  root: string;
};
export type ProxyForm = {
  name: string;
  listenPort: string;
  password: string;
};
export type ClusterPairForm = {
  name: string;
  endpoint: string;
  pairingSecret: string;
};
export type DistributionForm = {
  path: string;
  content: string;
  targetNodeId: string;
};
export type FirewallForm = {
  id: string;
  name: string;
  protocol: FirewallProtocol;
  action: FirewallAction;
  direction: FirewallDirection;
  portStart: string;
  portEnd: string;
  source: string;
  destination: string;
  enabled: boolean;
  comment: string;
};
export type SecurityOptionsForm = {
  disablePing: boolean;
  scanProtectionEnabled: boolean;
  scanBurst: number;
  scanWindowSeconds: number;
  backendPreference: FirewallBackend;
  lastApplyMessage: string;
  panelAccessPath: string;
  panelListenAddr: string;
  twoFactorRequired: boolean;
};
export type WafSettingsForm = {
  enabled: boolean;
  ccProtectionEnabled: boolean;
  captchaChallengeEnabled: boolean;
  requestsPerMinute: number;
  burst: number;
  blockDurationSeconds: number;
  nginxConfigPath: string;
  challengePagePath: string;
  lastApplyMessage: string;
};
export type WafRuleForm = {
  id: string;
  name: string;
  kind: WafRuleKind;
  pattern: string;
  enabled: boolean;
  scopeDomain: string;
  comment: string;
};
export type SshSettingsForm = {
  serviceEnabled: boolean;
  port: number;
  passwordLoginDisabled: boolean;
  autoBanEnabled: boolean;
  failedAttemptLimit: number;
  failedAttemptWindowSeconds: number;
  configPath: string;
  lastApplyMessage: string;
};
export type SshKeyForm = {
  name: string;
  algorithm: SshKeyAlgorithm;
};

export const defaultDockerQuotaForm: DockerQuotaForm = {
  containerId: "",
  cpuLimitCores: "1",
  memoryLimitMb: "512"
};
export const defaultImagePullForm: ImagePullForm = {
  image: "nginx",
  tag: "latest"
};
export const defaultImageRollbackForm: ImageRollbackForm = {
  sourceImage: "nginx:1.26-alpine",
  targetRepository: "nginx",
  targetTag: "stable"
};
export const defaultComposeForm: ComposeForm = {
  name: "demo",
  composeYaml: `services:
  web:
    image: nginx:1.27-alpine
    container_name: rustpanel-demo-web
    ports:
      - "8080:80"
    restart: unless-stopped
`
};
export const defaultWorkloadForm: WorkloadForm = {
  name: "rust-crawler",
  command: "./crawler",
  cwd: "/root",
  memoryLimitMb: "32"
};
export const defaultMicroSiteForm: MicroSiteForm = {
  name: "site",
  root: "/var/www/site"
};
export const defaultProxyForm: ProxyForm = {
  name: "ss-8388",
  listenPort: "8388",
  password: "change-me"
};
export const defaultClusterPairForm: ClusterPairForm = {
  name: "node-1",
  endpoint: "local",
  pairingSecret: "rustpanel"
};
export const defaultDistributionForm: DistributionForm = {
  path: "/tmp/rustpanel-distributed.conf",
  content: "managed_by=rustpanel\n",
  targetNodeId: ""
};
export const defaultFirewallForm: FirewallForm = {
  id: "",
  name: "SSH 管理",
  protocol: FirewallProtocol.TCP,
  action: FirewallAction.ALLOW,
  direction: FirewallDirection.INBOUND,
  portStart: "22",
  portEnd: "",
  source: "",
  destination: "",
  enabled: true,
  comment: "面板安全入口"
};
export const defaultSecurityOptions: SecurityOptionsForm = {
  disablePing: false,
  scanProtectionEnabled: false,
  scanBurst: 20,
  scanWindowSeconds: 60,
  backendPreference: FirewallBackend.UNSPECIFIED,
  lastApplyMessage: "",
  panelAccessPath: "/",
  panelListenAddr: "",
  twoFactorRequired: false
};
export const defaultWafSettings: WafSettingsForm = {
  enabled: false,
  ccProtectionEnabled: true,
  captchaChallengeEnabled: true,
  requestsPerMinute: 120,
  burst: 30,
  blockDurationSeconds: 600,
  nginxConfigPath: "",
  challengePagePath: "",
  lastApplyMessage: ""
};
export const defaultWafRuleForm: WafRuleForm = {
  id: "",
  name: "自定义关键词",
  kind: WafRuleKind.KEYWORD,
  pattern: "(badbot|malicious)",
  enabled: true,
  scopeDomain: "",
  comment: ""
};
export const defaultSshSettings: SshSettingsForm = {
  serviceEnabled: true,
  port: 22,
  passwordLoginDisabled: false,
  autoBanEnabled: true,
  failedAttemptLimit: 5,
  failedAttemptWindowSeconds: 600,
  configPath: "",
  lastApplyMessage: ""
};
export const defaultSshKeyForm: SshKeyForm = {
  name: "admin",
  algorithm: SshKeyAlgorithm.ED25519
};
