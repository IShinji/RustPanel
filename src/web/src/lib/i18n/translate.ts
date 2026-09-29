import { en } from "./locales/en";
import { zhCN, type Messages } from "./locales/zh-CN";

// 轻量自研 i18n:不引入 react-i18next / react-intl。面板前端产物经 rust-embed
// 打进后端二进制,项目一贯把体积换性能(参见去掉 recharts+d3 改手写 SVG 图表),
// 500 条字符串外加一个 {name} 插值就是全部需求,用不着一整套运行时。

export type Locale = "zh-CN" | "en";
export type { Messages };

// namespace.key 形式的联合类型,由 Messages 的实际形状推导——加/删/改字典字段,
// 这里跟着自动变,写错命名空间或键名在调用处直接是编译错误。
type Namespace = keyof Messages;
export type MessageKey = {
  [N in Namespace]: `${N & string}.${keyof Messages[N] & string}`;
}[Namespace];

export type TParams = Record<string, string | number | bigint>;
export type TFn = (key: MessageKey, params?: TParams) => string;

const DICTS: Record<Locale, Messages> = { "zh-CN": zhCN, en };

// 缺参数时把 {name} 原样留在输出里,而不是静默吞掉——开发时一眼就能看出漏传了什么。
export function interpolate(template: string, params?: TParams): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    Object.hasOwn(params, name) ? String(params[name]) : match
  );
}

export function translator(locale: Locale): TFn {
  const dict = DICTS[locale];
  return (key, params) => {
    const [ns, k] = key.split(".") as [Namespace, string];
    const namespace = dict[ns] as Record<string, string> | undefined;
    const template = namespace?.[k];
    // MessageKey 类型已经保证了 key 一定存在;这里兜底是防运行时通过
    // labelKey 等间接路径传入拼出来的非法 key 时不至于崩渲染。
    if (template === undefined) return key;
    return interpolate(template, params);
  };
}

// 给 React 渲染树之外的代码用(rpc.ts 的拦截器、lib/acme.ts 里的 window.prompt/alert)。
// LocaleProvider 在语言变化时会同步更新这个值。
let activeLocale: Locale = "zh-CN";

export function getActiveLocale(): Locale {
  return activeLocale;
}

export function setActiveLocale(locale: Locale): void {
  activeLocale = locale;
}

export function tGlobal(key: MessageKey, params?: TParams): string {
  return translator(activeLocale)(key, params);
}

// 优先用户存过的选择;没存过按浏览器语言猜(以 zh 开头→简体中文,否则英文)。
// 只在浏览器里跑,localStorage / navigator.language 均可能不存在(隐私模式、
// 非浏览器运行时),两处都做防御式判空,不让语言探测本身炸掉渲染。
export function detectInitialLocale(storageKey: string): Locale {
  try {
    const stored = localStorage.getItem(storageKey);
    if (stored === "zh-CN" || stored === "en") return stored;
  } catch {
    // 存储被禁用(隐私模式等),退到语言探测
  }
  const lang = typeof navigator === "undefined" ? "" : (navigator.language ?? "").toLowerCase();
  return lang.startsWith("zh") ? "zh-CN" : "en";
}
