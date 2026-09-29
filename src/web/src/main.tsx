import React from "react";
import { createRoot } from "react-dom/client";

import App from "./App";
import { ThemeProvider } from "./components/theme-provider";
import { LocaleProvider } from "./lib/i18n/locale-provider";
import "./styles.css";

createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <LocaleProvider defaultLocale="zh-CN" storageKey="rustpanel.locale">
      <ThemeProvider defaultTheme="light" storageKey="rustpanel.theme">
        <App />
      </ThemeProvider>
    </LocaleProvider>
  </React.StrictMode>
);
