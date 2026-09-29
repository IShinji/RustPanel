import * as React from "react";

import {
  detectInitialLocale,
  setActiveLocale,
  translator,
  type Locale,
  type TFn
} from "./translate";

type LocaleProviderProps = {
  children: React.ReactNode;
  defaultLocale?: Locale;
  storageKey?: string;
};

type LocaleProviderState = {
  locale: Locale;
  setLocale: (locale: Locale) => void;
  t: TFn;
};

const initialState: LocaleProviderState = {
  locale: "zh-CN",
  setLocale: () => null,
  t: (key) => key
};

const LocaleProviderContext = React.createContext<LocaleProviderState>(initialState);

export function LocaleProvider({
  children,
  defaultLocale = "zh-CN",
  storageKey = "rustpanel.locale"
}: LocaleProviderProps) {
  const [locale, setLocaleState] = React.useState<Locale>(() => {
    if (typeof window === "undefined") return defaultLocale;
    // 同步读取,首次渲染就是对的语言;两套字典都是静态引入,没有异步加载,
    // 不存在"先中文闪一下再变英文"的问题。
    const detected = detectInitialLocale(storageKey);
    setActiveLocale(detected);
    return detected;
  });

  React.useEffect(() => {
    document.documentElement.lang = locale;
    setActiveLocale(locale);
  }, [locale]);

  const setLocale = React.useCallback(
    (next: Locale) => {
      try {
        localStorage.setItem(storageKey, next);
      } catch {
        // ignore quota / storage disabled
      }
      setActiveLocale(next);
      setLocaleState(next);
    },
    [storageKey]
  );

  const t = React.useMemo(() => translator(locale), [locale]);

  const value = React.useMemo<LocaleProviderState>(
    () => ({ locale, setLocale, t }),
    [locale, setLocale, t]
  );

  return <LocaleProviderContext.Provider value={value}>{children}</LocaleProviderContext.Provider>;
}

export function useLocale() {
  const ctx = React.useContext(LocaleProviderContext);
  if (!ctx) throw new Error("useLocale must be used within LocaleProvider");
  return ctx;
}
