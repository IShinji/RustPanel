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

export function formatDuration(seconds: number | bigint): string {
  const value = Number(seconds);
  const days = Math.floor(value / 86400);
  const hours = Math.floor((value % 86400) / 3600);
  const minutes = Math.floor((value % 3600) / 60);

  if (days > 0) {
    return `${days}天 ${hours}小时`;
  }
  if (hours > 0) {
    return `${hours}小时 ${minutes}分`;
  }
  return `${minutes}分`;
}

export function safeError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
