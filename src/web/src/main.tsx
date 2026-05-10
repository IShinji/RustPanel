import React from "react";
import { createRoot } from "react-dom/client";

import App from "./App";
import { ThemeProvider } from "./components/theme-provider";
import "./styles.css";

createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ThemeProvider defaultTheme="light" storageKey="rustpanel.theme">
      <App />
    </ThemeProvider>
  </React.StrictMode>
);
