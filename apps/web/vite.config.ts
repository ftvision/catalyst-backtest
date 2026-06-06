import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  server: {
    host: "127.0.0.1",
    port: 4173,
  },
  build: {
    chunkSizeWarningLimit: 650,
    rollupOptions: {
      output: {
        manualChunks: {
          mantine: ["@mantine/core", "@mantine/hooks"],
          charts: ["@mantine/charts", "recharts", "lightweight-charts"],
        },
      },
    },
  },
});
