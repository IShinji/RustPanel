import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react()],
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
