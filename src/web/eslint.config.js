import js from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

// 此前 `bun lint` 只是 `tsc --noEmit`:类型对了就算过,hooks 依赖漏了、Promise
// 忘了 await、变量没用到一律不报。这里补上 ESLint,规则挑的是「真能出 bug」
// 的那几类,不做风格洁癖(格式化交给编辑器)。
export default tseslint.config(
  { ignores: ["dist/**", "src/gen/**", "node_modules/**"] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["**/*.{ts,tsx}"],
    plugins: { "react-hooks": reactHooks },
    languageOptions: {
      globals: {
        window: "readonly",
        document: "readonly",
        navigator: "readonly",
        console: "readonly",
        fetch: "readonly",
        setTimeout: "readonly",
        clearTimeout: "readonly",
        setInterval: "readonly",
        clearInterval: "readonly",
        localStorage: "readonly",
        WebSocket: "readonly",
        TextDecoder: "readonly",
        TextEncoder: "readonly",
        Blob: "readonly",
        File: "readonly",
        FileList: "readonly",
        FileReader: "readonly",
        URL: "readonly",
        URLSearchParams: "readonly",
        AbortController: "readonly",
        HTMLDivElement: "readonly",
        HTMLInputElement: "readonly",
        HTMLTextAreaElement: "readonly",
        HTMLElement: "readonly",
        MouseEvent: "readonly",
        KeyboardEvent: "readonly",
        Event: "readonly",
        matchMedia: "readonly",
        requestAnimationFrame: "readonly",
        ResizeObserver: "readonly"
      }
    },
    rules: {
      "react-hooks/rules-of-hooks": "error",
      // 依赖数组漏项是这个代码库最容易出的一类 bug,但存量太多,
      // 先按 warn 记账,新代码别再欠。
      "react-hooks/exhaustive-deps": "warn",
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" }
      ],
      // gen/ 之外偶尔仍需 any 兜住 proto 动态结构,降级为 warn。
      "@typescript-eslint/no-explicit-any": "warn",
      "no-empty": ["error", { allowEmptyCatch: true }]
    }
  }
);
