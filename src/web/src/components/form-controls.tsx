import type { LucideIcon } from "lucide-react";

// App.tsx 里到处复用的表单/展示原子组件。此前和三十来个页面挤在同一个文件里,
// 拆出来后各页面才可能独立成模块。

export function StatusPill({ label, tone }: { label: string; tone: "good" | "danger" | "muted" }) {
  return <span className={`status-pill ${tone}`}>{label}</span>;
}

export function IconButton({
  icon: Icon,
  label,
  onClick
}: {
  icon: LucideIcon;
  label: string;
  onClick: () => void;
}) {
  return (
    <button className="icon-button" onClick={onClick} title={label} type="button">
      <Icon size={16} />
    </button>
  );
}

export function ToggleRow({
  checked,
  label,
  onChange
}: {
  checked: boolean;
  label: string;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="toggle-row">
      <span>{label}</span>
      <input checked={checked} onChange={(event) => onChange(event.target.checked)} type="checkbox" />
    </label>
  );
}

export function SelectRow({
  label,
  onChange,
  options,
  value
}: {
  label: string;
  onChange: (value: string) => void;
  options: Array<[number, string]>;
  value: number;
}) {
  return (
    <label className="input-row">
      <span>{label}</span>
      <select onChange={(event) => onChange(event.target.value)} value={value}>
        {options.map(([optionValue, optionLabel]) => (
          <option key={optionValue} value={optionValue}>{optionLabel}</option>
        ))}
      </select>
    </label>
  );
}

export function NumberInput({
  label,
  value,
  onChange
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
}) {
  return (
    <Input
      label={label}
      onChange={(nextValue) => onChange(Number(nextValue || 0))}
      type="number"
      value={String(value)}
    />
  );
}

export function Input({
  label,
  type = "text",
  value,
  onChange
}: {
  label: string;
  type?: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <label className="input-row">
      <span>{label}</span>
      <input onChange={(event) => onChange(event.target.value)} type={type} value={value} />
    </label>
  );
}
