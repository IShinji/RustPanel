// App.tsx:登录页、顶栏、侧边导航、回滚横幅。Dashboard/NetworkPage/TerminalPanel
// 的文案随它们各自的迁移提交再加进来。
export const app = {
  brandSubtitle: "控制面板",
  sidebarNav: "RustPanel 导航",
  logout: "退出登录",
  account: "账户",

  loginSubtitle: "请使用管理员账户登录",
  username: "用户名",
  password: "密码",
  totpCode: "两步验证码",
  totpRequired: "请输入两步验证码",
  loginFailed: "登录失败",
  loggingIn: "登录中...",
  login: "登录",
  loginHintBefore: "初始密码在安装时打印,也可查看 ",
  loginHintMiddle: "(位于 ",
  loginHintAfter: ")。",

  navDashboard: "仪表盘",
  navSites: "网站",
  navFtp: "FTP",
  navDatabase: "数据库",
  navFiles: "文件",
  navCron: "计划任务",
  navAppstore: "软件商店",
  navVsmtp: "邮件别名",
  navDocker: "容器",
  navSecurity: "安全",
  navAudit: "日志",
  navCluster: "集群",
  navTerminal: "终端",
  navMicro: "Micro",
  navNetwork: "网络与端口",
  navNotifications: "通知",
  navBackup: "备份",
  navToolbox: "工具箱",
  navAccesslog: "访问统计",
  navDns: "DNS",
  navUsers: "用户",
  navSettings: "面板设置",

  groupOverview: "总览",
  groupHost: "主机",
  groupResource: "资源",
  groupSecurity: "安全",
  groupTools: "工具",
  groupSystem: "系统",

  rollbackPrefix: " · 还剩 ",
  rollbackSuffix: " 秒自动回滚",
  rollbackKeep: "保留(我能登录)"
};

export type AppMessages = typeof app;
