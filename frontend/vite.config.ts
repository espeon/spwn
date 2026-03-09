import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    allowedHosts: ["localhost", "ami"],
    proxy: {
      "/auth": "http://localhost:3019",
      "/api": "http://localhost:3019",
    },
  },
});
