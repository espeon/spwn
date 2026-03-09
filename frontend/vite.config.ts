import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "path";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
    dedupe: ["react", "react-dom"],
  },
  server: {
    allowedHosts: ["localhost", "ami"],
    proxy: {
      "/auth": "http://localhost:3019",
      "/api": "http://localhost:3019",
    },
  },
});
