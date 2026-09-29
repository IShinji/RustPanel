// 通用小组件(主题/语言切换、监控图表、网页终端等)自身的文案。
export const components = {
  themeToggleLabel: "切换主题",
  themeLight: "浅色",
  themeDark: "深色",
  themeSystem: "跟随系统",
  languageToggleLabel: "切换语言",

  // monitor-chart.tsx 的图例/坐标轴
  chartCpu: "CPU",
  chartMemory: "内存",
  chartAriaLabel: "CPU 与内存使用率趋势",

  // web-terminal.tsx
  webTerminalTitle: "Web 终端",
  ptySession: "PTY 会话",

  // software-store.tsx 的 CapabilityRow,同时被 App.tsx 的 NetworkPage 复用
  available: "可用",
  unavailable: "不可用",

  // code-editor.tsx:Monaco 懒加载完成前的占位骨架
  editorLoading: "编辑器加载中…",

  // ui/dialog.tsx、ui/sheet.tsx 的关闭按钮 sr-only 文案
  close: "关闭"
};

export type ComponentsMessages = typeof components;
