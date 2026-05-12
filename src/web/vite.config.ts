import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

export default defineConfig({
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes("node_modules")) return;
          if (id.includes("@monaco-editor") || id.includes("monaco-editor")) return "vendor-editor";
          if (id.includes("@xterm")) return "vendor-terminal";
          if (id.includes("recharts") || id.includes("d3-")) return "vendor-charts";
          if (
            id.includes("@radix-ui") ||
            id.includes("lucide-react") ||
            id.includes("class-variance-authority") ||
            id.includes("clsx") ||
            id.includes("tailwind-merge")
          ) {
            return "vendor-ui";
          }
          if (id.includes("react") || id.includes("react-dom") || id.includes("scheduler")) return "vendor-react";
          if (id.includes("@connectrpc") || id.includes("@bufbuild")) return "vendor-rpc";
          return;
        }
      }
    }
  },
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url))
    }
  },
  server: {
    proxy: {
      "/rustpanel.v1.": "http://127.0.0.1:8080",
      "/api": {
        target: "http://127.0.0.1:8080",
        ws: true
      }
    }
  }
});
