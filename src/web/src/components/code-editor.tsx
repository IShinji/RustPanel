import { lazy, Suspense } from "react";
import type { ComponentProps } from "react";
import type MonacoEditor from "@monaco-editor/react";

// Monaco 只有文件管理器和 SQL 控制台两个页面用得到,却是首屏最重的依赖之一。
// 静态 import 会让它跟着入口 chunk 一起下发,低配主机 + 慢链路上纯属浪费,
// 这里改成用到才拉。
const Editor = lazy(() => import("@monaco-editor/react"));

type EditorProps = ComponentProps<typeof MonacoEditor>;

function EditorSkeleton({ height }: { height?: EditorProps["height"] }) {
  return (
    <div
      className="flex items-center justify-center bg-muted/40 text-xs text-muted-foreground"
      style={{ height: typeof height === "number" ? `${height}px` : (height ?? "240px") }}
    >
      编辑器加载中…
    </div>
  );
}

export function CodeEditor(props: EditorProps) {
  return (
    <Suspense fallback={<EditorSkeleton height={props.height} />}>
      <Editor {...props} />
    </Suspense>
  );
}
