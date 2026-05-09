import { expect, test } from "bun:test";

import { formatBytes, formatDuration, formatPercent } from "./format";

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
