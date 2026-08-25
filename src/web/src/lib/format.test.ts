import { expect, test } from "bun:test";

import { formatBytes, formatDuration, formatPercent, safeError } from "./format";

test("formats byte values", () => {
  expect(formatBytes(1024)).toBe("1.0 KB");
  expect(formatBytes(1536)).toBe("1.5 KB");
});

test("formats percentages", () => {
  expect(formatPercent(42.345)).toBe("42.3%");
});

test("formats durations", () => {
  expect(formatDuration(3660)).toBe("1小时 1分");
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
  expect(formatDuration(59)).toBe("0分");
  expect(formatDuration(600)).toBe("10分");
  expect(formatDuration(90061n)).toBe("1天 1小时");
});

test("safeError unwraps Error and stringifies the rest", () => {
  expect(safeError(new Error("boom"))).toBe("boom");
  expect(safeError("plain")).toBe("plain");
  expect(safeError(undefined)).toBe("undefined");
});
