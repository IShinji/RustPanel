// pages/security.tsx 用到的文案。目前只有 lib/labels.ts 里几个枚举转文案函数
// 需要的 key,页面本身的迁移(commit 5)再继续往这里加。
export const security = {
  firewallAllow: "放行",
  firewallDeny: "屏蔽",
  firewallReject: "拒绝",
  directionInbound: "入站",
  directionOutbound: "出站",
  wafSqlInjection: "SQL 注入",
  wafKeyword: "关键词",
  wafScanner: "扫描器"
};

export type SecurityMessages = typeof security;
