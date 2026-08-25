import { FitAddon } from "@xterm/addon-fit";
import { Terminal as XTerm } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useEffect, useRef } from "react";

import { appendAuthQuery } from "@/lib/rpc";

// xterm + fit addon + 样式表将近 300KB,只有开终端页时才用得到。
// 整个面板放在这个单独模块里,由 App 侧 lazy() 加载,首屏不再下发。
export default function WebTerminal({ cwd }: { cwd: string }) {
  const terminalRef = useRef<HTMLDivElement | null>(null);
  const socketRef = useRef<WebSocket | null>(null);

  useEffect(() => {
    const terminal = new XTerm({
      cursorBlink: true,
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
      fontSize: 13,
      // 没显式设置 allowProposedApi 时 fit 用默认窗口大小;让 xterm 跑成 256 色 + 终端响应
      allowProposedApi: true,
      convertEol: false,
      scrollback: 5000,
      theme: {
        background: "#101418",
        foreground: "#eef2f3",
        cursor: "#eef2f3"
      }
    });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(terminalRef.current as HTMLDivElement);
    fit.fit();

    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    // 浏览器无法给 WebSocket 设置自定义 header,只能用 ?token= 把 JWT 一起发出去
    const wsUrl = appendAuthQuery(`/api/terminal/ws?cwd=${encodeURIComponent(cwd)}`);
    const socket = new WebSocket(`${protocol}//${window.location.host}${wsUrl}`);
    socket.binaryType = "arraybuffer";
    socketRef.current = socket;

    // PTY 初始尺寸是 120x30,fit 算完真实尺寸后必须显式同步给后端,
    // 否则 top/htop 这种全屏 TUI 会按 PTY 默认尺寸渲染,不匹配可视区
    const sendResize = (cols: number, rows: number) => {
      if (socket.readyState === WebSocket.OPEN) {
        socket.send(JSON.stringify({ type: "resize", cols, rows }));
      }
    };
    socket.onopen = () => {
      sendResize(terminal.cols, terminal.rows);
      terminal.focus();
    };
    socket.onmessage = (event) => {
      const text =
        typeof event.data === "string"
          ? event.data
          : new TextDecoder().decode(new Uint8Array(event.data));
      terminal.write(text);
    };
    terminal.onData((data) => socket.readyState === WebSocket.OPEN && socket.send(data));
    terminal.onResize((size) => sendResize(size.cols, size.rows));
    const resize = () => fit.fit();
    window.addEventListener("resize", resize);

    return () => {
      window.removeEventListener("resize", resize);
      socket.close();
      terminal.dispose();
    };
  }, [cwd]);

  return (
    <section className="page-grid terminal-layout">
      <header className="section-header full-span">
        <div>
          <h1>Web 终端</h1>
          <p>{cwd} · PTY 会话</p>
        </div>
      </header>
      <div className="terminal-surface full-span" ref={terminalRef} />
    </section>
  );
}
