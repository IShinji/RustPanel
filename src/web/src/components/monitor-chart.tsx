import {
  CartesianGrid,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis
} from "recharts";

export type ChartPoint = {
  time: string;
  timestamp: number;
  cpu: number;
  memory: number;
};

export type ChartClickState = {
  activePayload?: Array<{ payload?: ChartPoint }>;
};

// recharts + d3 是包体里最大的一块(未压缩约 350KB),只有监控页用得到。
// 单独成文件后由 App 侧 lazy() 按需加载,首屏不再为它买单。
export default function MonitorChart({
  data,
  onPointClick
}: {
  data: ChartPoint[];
  onPointClick: (state: ChartClickState) => void;
}) {
  return (
    <ResponsiveContainer width="100%" height={260}>
      <LineChart data={data} onClick={(state) => onPointClick(state as ChartClickState)}>
        <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
        <XAxis dataKey="time" minTickGap={24} stroke="var(--muted-foreground)" fontSize={12} />
        <YAxis
          domain={[0, 100]}
          stroke="var(--muted-foreground)"
          fontSize={12}
          tickFormatter={(value) => `${Math.round(Number(value))}%`}
          width={40}
        />
        <Tooltip
          contentStyle={{
            background: "var(--popover)",
            border: "1px solid var(--border)",
            borderRadius: 8,
            color: "var(--popover-foreground)"
          }}
          formatter={(value) => {
            const num = typeof value === "number" ? value : Number(value);
            return Number.isFinite(num) ? `${num.toFixed(1)}%` : String(value);
          }}
        />
        <Line dataKey="cpu" dot={false} stroke="var(--chart-1)" strokeWidth={2} name="CPU" />
        <Line dataKey="memory" dot={false} stroke="var(--chart-2)" strokeWidth={2} name="内存" />
      </LineChart>
    </ResponsiveContainer>
  );
}
