// App.tsx 的 TerminalPanel(只是 WebTerminal 的 Suspense 包装)。
export const terminal = {
  loadingFallback: "终端加载中…"
};

export type TerminalMessages = typeof terminal;
