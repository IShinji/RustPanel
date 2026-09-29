import { useLayoutEffect, useRef, useState } from "react";

export type ChartPoint = {
  time: string;
  timestamp: number;
  cpu: number;
  memory: number;
};

export type ChartClickState = {
  activePayload?: Array<{ payload?: ChartPoint }>;
};

const HEIGHT = 260;
const PAD = { top: 10, right: 12, bottom: 24, left: 40 };
const Y_TICKS = [0, 25, 50, 75, 100];
const SERIES = [
  { key: "cpu" as const, label: "CPU", color: "var(--chart-1)" },
  { key: "memory" as const, label: "内存", color: "var(--chart-2)" }
];

// 手写 SVG 折线图:只画 CPU / 内存两条 0~100% 的线,外加网格、坐标轴、悬浮提示和点选。
// 此前用 recharts + d3,为这点功能多背了约 350KB 的包体(面板二进制内嵌前端,低配机上更明显)。
export default function MonitorChart({
  data,
  onPointClick
}: {
  data: ChartPoint[];
  onPointClick: (state: ChartClickState) => void;
}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [width, setWidth] = useState(0);
  const [hover, setHover] = useState<number | null>(null);

  useLayoutEffect(() => {
    const element = containerRef.current;
    if (!element) return;
    // 首帧同步量一次宽度,不等 ResizeObserver 回调(否则会先闪一帧空白)
    setWidth(Math.floor(element.clientWidth));
    const observer = new ResizeObserver((entries) => {
      setWidth(Math.floor(entries[0]?.contentRect.width ?? 0));
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const plotWidth = Math.max(width - PAD.left - PAD.right, 1);
  const plotHeight = HEIGHT - PAD.top - PAD.bottom;
  const x = (index: number) =>
    PAD.left + (data.length <= 1 ? plotWidth / 2 : (index / (data.length - 1)) * plotWidth);
  const y = (value: number) =>
    PAD.top + plotHeight - (Math.min(Math.max(value, 0), 100) / 100) * plotHeight;

  const path = (key: "cpu" | "memory") =>
    data.map((point, index) => `${index === 0 ? "M" : "L"}${x(index).toFixed(1)},${y(point[key]).toFixed(1)}`).join("");

  // X 轴标签按像素间距抽稀,避免挤成一团
  const labelEvery = Math.max(1, Math.ceil(data.length / Math.max(1, Math.floor(plotWidth / 80))));

  const indexAt = (clientX: number) => {
    const rect = containerRef.current?.getBoundingClientRect();
    if (!rect || data.length === 0) return null;
    const ratio = (clientX - rect.left - PAD.left) / plotWidth;
    return Math.min(data.length - 1, Math.max(0, Math.round(ratio * (data.length - 1))));
  };

  const hovered = hover === null ? undefined : data[hover];

  return (
    <div
      ref={containerRef}
      className="relative w-full select-none"
      style={{ height: HEIGHT }}
      onMouseMove={(event) => setHover(indexAt(event.clientX))}
      onMouseLeave={() => setHover(null)}
      onClick={(event) => {
        const index = indexAt(event.clientX);
        if (index !== null) onPointClick({ activePayload: [{ payload: data[index] }] });
      }}
    >
      {width > 0 && (
        <svg width={width} height={HEIGHT} role="img" aria-label="CPU 与内存使用率趋势">
          {Y_TICKS.map((tick) => (
            <g key={tick}>
              <line
                x1={PAD.left}
                x2={PAD.left + plotWidth}
                y1={y(tick)}
                y2={y(tick)}
                stroke="var(--border)"
                strokeDasharray="3 3"
              />
              <text
                x={PAD.left - 6}
                y={y(tick)}
                textAnchor="end"
                dominantBaseline="middle"
                fontSize={12}
                fill="var(--muted-foreground)"
              >
                {tick}%
              </text>
            </g>
          ))}
          {data.map((point, index) =>
            index % labelEvery === 0 ? (
              <text
                key={point.timestamp}
                x={x(index)}
                y={HEIGHT - 6}
                textAnchor="middle"
                fontSize={12}
                fill="var(--muted-foreground)"
              >
                {point.time}
              </text>
            ) : null
          )}
          {SERIES.map((series) => (
            <path
              key={series.key}
              d={path(series.key)}
              fill="none"
              stroke={series.color}
              strokeWidth={2}
              strokeLinejoin="round"
            />
          ))}
          {hovered && hover !== null && (
            <g>
              <line
                x1={x(hover)}
                x2={x(hover)}
                y1={PAD.top}
                y2={PAD.top + plotHeight}
                stroke="var(--muted-foreground)"
                strokeOpacity={0.5}
              />
              {SERIES.map((series) => (
                <circle
                  key={series.key}
                  cx={x(hover)}
                  cy={y(hovered[series.key])}
                  r={3.5}
                  fill={series.color}
                />
              ))}
            </g>
          )}
        </svg>
      )}
      {hovered && hover !== null && (
        <div
          className="pointer-events-none absolute top-2 rounded-lg border border-border bg-popover px-3 py-2 text-xs text-popover-foreground shadow-sm"
          style={
            x(hover) > width / 2
              ? { right: width - x(hover) + 12 }
              : { left: x(hover) + 12 }
          }
        >
          <div className="mb-1 font-medium">{hovered.time}</div>
          {SERIES.map((series) => (
            <div key={series.key} className="flex items-center gap-2">
              <span className="inline-block size-2 rounded-full" style={{ background: series.color }} />
              <span>{series.label}</span>
              <span className="ml-auto tabular-nums">{hovered[series.key].toFixed(1)}%</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
