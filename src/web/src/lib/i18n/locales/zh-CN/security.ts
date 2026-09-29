// pages/security.tsx,以及被它复用的 lib/labels.ts 里几个枚举转文案函数。
export const security = {
  // labels.ts 复用
  firewallAllow: "放行",
  firewallDeny: "屏蔽",
  firewallReject: "拒绝",
  directionInbound: "入站",
  directionOutbound: "出站",
  wafSqlInjection: "SQL 注入",
  wafKeyword: "关键词",
  wafScanner: "扫描器",

  title: "安全管理",
  ruleCount: "{count} 条防火墙规则",
  refresh: "刷新",
  newRule: "新建规则",

  entryProtection: "入口防护",
  accessPath: "访问路径",
  listenAddr: "监听地址",
  twoFactorLogin: "2FA 登录",
  disablePing: "禁 Ping",
  antiScan: "防扫描",
  triggerCount: "触发次数",
  windowSeconds: "窗口秒数",
  backend: "后端",
  autoDetect: "自动检测",
  saveToggles: "保存开关",

  editRule: "编辑规则",
  name: "名称",
  protocol: "协议",
  action: "动作",
  direction: "方向",
  startPort: "起始端口",
  endPort: "结束端口",
  sourceIpCidr: "来源 IP/CIDR",
  destIpCidr: "目标 IP/CIDR",
  comment: "备注",
  enabled: "启用",
  disabled: "停用",
  saveRule: "保存规则",

  firewallRules: "防火墙规则",
  noRules: "暂无规则",
  edit: "编辑",
  delete: "删除",

  wafProtection: "WAF 防护",
  wafMasterSwitch: "WAF 总开关",
  antiCc: "抗 CC",
  captchaChallenge: "验证码挑战",
  requestsPerMinute: "每分钟请求",
  burstRequests: "突发请求",
  blockSeconds: "封禁秒数",
  nginxFragment: "Nginx 片段",
  challengePage: "挑战页",
  saveWaf: "保存 WAF",

  editWafRule: "编辑 WAF 规则",
  newWafRule: "新建 WAF 规则",
  kind: "类型",
  matchPattern: "匹配规则",
  scopeDomain: "站点域名",

  wafRuleLibrary: "WAF 规则库",

  attackSources: "攻击来源",
  attackIpRanking: "攻击 IP 排名",
  noBlockRecords: "暂无拦截记录",

  sshHardening: "SSH 加固",
  serviceEnabled: "服务启用",
  sshPort: "SSH 端口",
  disablePasswordLogin: "禁用密码",
  autoBan: "自动封禁",
  failThreshold: "失败阈值",
  configFile: "配置文件",
  saveSsh: "保存 SSH",

  sshKeys: "SSH 密钥",
  algorithm: "算法",
  generate: "生成",
  noKeys: "暂无密钥",

  sshAudit: "SSH 登录审计",
  success: "成功",
  failure: "失败",
  banned: "已封禁",
  notBanned: "未封禁",
  noAuditRecords: "暂无审计记录",

  ruleBackup: "规则备份",
  export: "导出",
  importOverwrite: "导入覆盖",

  exampleRuleName: "SSH 管理",
  exampleRuleComment: "面板安全入口",
  exampleWafRuleName: "自定义关键词",

  optionsSaved: "安全选项已保存",
  backupExported: "规则备份已生成",
  backupImported: "规则备份已导入",
  wafSaved: "WAF 配置已保存",
  sshSaved: "SSH 配置已保存"
};

export type SecurityMessages = typeof security;
