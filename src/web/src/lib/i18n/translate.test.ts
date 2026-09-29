import { expect, test } from "bun:test";

import { getActiveLocale, interpolate, setActiveLocale, tGlobal, translator } from "./translate";

// detectInitialLocale 依赖浏览器的 localStorage / navigator.language,bun test
// 跑在没有 DOM 的环境里(和 theme-provider.tsx 里同款的 localStorage 探测逻辑
// 一样,项目里从未也无法单测这部分),这里只测不依赖浏览器全局的纯函数。

test("interpolate substitutes named placeholders and leaves missing ones visible", () => {
  expect(interpolate("你好 {name}", { name: "Ada" })).toBe("你好 Ada");
  expect(interpolate("no placeholders")).toBe("no placeholders");
  // 缺参数时原样留下 {xxx},方便肉眼发现漏传
  expect(interpolate("缺 {missing} 参数", {})).toBe("缺 {missing} 参数");
  expect(interpolate("{count} 项", { count: 3n })).toBe("3 项");
});

test("translator resolves namespaced keys per locale and applies interpolation", () => {
  const zh = translator("zh-CN");
  const en = translator("en");
  expect(zh("app.login")).toBe("登录");
  expect(en("app.login")).toBe("Log in");
  expect(zh("common.durationMinutes", { minutes: 5 })).toBe("5分");
  expect(en("common.durationMinutes", { minutes: 5 })).toBe("5m");
});

test("unknown key falls back to the key itself instead of throwing", () => {
  const t = translator("en");
  // MessageKey 的类型本来就不允许瞎传,这里用 as 绕过类型检查,
  // 只是为了确认运行时不会因为脏数据(比如旧版 labelKey)崩渲染。
  expect(t("app.doesNotExist" as never)).toBe("app.doesNotExist");
});

test("active locale singleton is read by tGlobal for non-React callers", () => {
  const before = getActiveLocale();
  try {
    setActiveLocale("en");
    expect(tGlobal("app.login")).toBe("Log in");
    setActiveLocale("zh-CN");
    expect(tGlobal("app.login")).toBe("登录");
  } finally {
    setActiveLocale(before);
  }
});
