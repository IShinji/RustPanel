import { expect, test } from "bun:test";

import { formatBytes, formatDateTime, formatDuration, formatPercent, safeError } from "./format";
import { translator } from "./i18n/translate";

const zh = translator("zh-CN");
const en = translator("en");

test("formats byte values", () => {
  expect(formatBytes(1024)).toBe("1.0 KB");
  expect(formatBytes(1536)).toBe("1.5 KB");
});

test("formats percentages", () => {
  expect(formatPercent(42.345)).toBe("42.3%");
});

test("formats durations", () => {
  expect(formatDuration(3660, zh)).toBe("1小时 1分");
  expect(formatDuration(3660, en)).toBe("1h 1m");
});

test("formats byte edge cases", () => {
  expect(formatBytes(0)).toBe("0 B");
  expect(formatBytes(999)).toBe("999 B");
  // 面板到处传 protobuf uint64,BigInt 必须照样能格式化。
  expect(formatBytes(3n * 1024n * 1024n * 1024n)).toBe("3.0 GB");
  // 超出单位表就停在最大单位,不会溢出成 undefined。
  expect(formatBytes(1024 ** 6)).toBe("1024.0 PB");
});

test("clamps negative percentages to zero", () => {
  expect(formatPercent(-3)).toBe("0.0%");
  expect(formatPercent(0)).toBe("0.0%");
});

test("formats duration by largest unit", () => {
  expect(formatDuration(59, zh)).toBe("0分");
  expect(formatDuration(600, zh)).toBe("10分");
  expect(formatDuration(90061n, zh)).toBe("1天 1小时");
  expect(formatDuration(90061n, en)).toBe("1d 1h");
});

test("formatDuration defaults to the global active locale when no translator is passed", () => {
  // 大多数既有调用点不会改,靠这个默认值继续工作;lib/i18n/translate.test.ts
  // 已经单独测过 tGlobal 会跟随 setActiveLocale,这里只确认 formatDuration 接了上去。
  expect(formatDuration(600)).toBe("10分");
});

test("formatDateTime uses locale-appropriate date order", () => {
  const date = new Date(Date.UTC(2026, 0, 5, 8, 30, 0));
  expect(formatDateTime(date, "zh-CN")).toContain("26");
  expect(formatDateTime(date, "en")).toContain("26");
  // 顺序不同:zh-CN 是 年/月/日,en-US 风格是 月/日/年——不深究具体格式字符串
  // (那是 ICU/Intl 的活),只确认两种 locale 产出确实不同,没有共用一份格式。
  expect(formatDateTime(date, "zh-CN")).not.toBe(formatDateTime(date, "en"));
});

test("safeError unwraps Error and stringifies the rest", () => {
  expect(safeError(new Error("boom"))).toBe("boom");
  expect(safeError("plain")).toBe("plain");
  expect(safeError(undefined)).toBe("undefined");
});
