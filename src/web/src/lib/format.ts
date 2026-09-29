import { tGlobal, type Locale, type TFn } from "./i18n/translate";

export function formatBytes(value: number | bigint): string {
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  let size = Number(value);
  let unit = 0;

  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }

  return `${size.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

export function formatPercent(value: number): string {
  return `${Math.max(0, value).toFixed(1)}%`;
}

// t 默认取全局当前语言(tGlobal),已经在 useLocale() 里拿到 t 的调用方可以显式传,
// 这样切换语言时这段文字能立刻跟着当前组件的重渲染更新;没传的老调用点不用改。
export function formatDuration(seconds: number | bigint, t: TFn = tGlobal): string {
  const value = Number(seconds);
  const days = Math.floor(value / 86400);
  const hours = Math.floor((value % 86400) / 3600);
  const minutes = Math.floor((value % 3600) / 60);

  if (days > 0) {
    return t("common.durationDaysHours", { days, hours });
  }
  if (hours > 0) {
    return t("common.durationHoursMinutes", { hours, minutes });
  }
  return t("common.durationMinutes", { minutes });
}

// 面板里所有 toLocaleString() 日期时间显示的统一入口,按当前语言选用符合
// 当地习惯的日期顺序 / 分隔符,而不是依赖浏览器自身的系统语言设置。
export function formatDateTime(date: Date, locale: Locale): string {
  return new Intl.DateTimeFormat(locale, {
    dateStyle: "short",
    timeStyle: "medium"
  }).format(date);
}

export function safeError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
