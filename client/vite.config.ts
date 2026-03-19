import { defineConfig } from "vite";
import solidPlugin from "vite-plugin-solid";
import UnoCSS from "unocss/vite";
import basicSsl from "@vitejs/plugin-basic-ssl";
import path from "path";

export default defineConfig(({ mode }) => ({
  plugins: [
    UnoCSS(),
    solidPlugin(),
    ...(mode !== "production" ? [basicSsl()] : []),
  ],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  optimizeDeps: {
    include: ["marked"],
  },
  server: {
    host: "0.0.0.0",
    port: 5173,
    strictPort: true,
    proxy: {
      "/api": {
        target: "http://localhost:8080",
        changeOrigin: true,
      },
      "/auth": {
        target: "http://localhost:8080",
        changeOrigin: true,
      },
      "/health": {
        target: "http://localhost:8080",
        changeOrigin: true,
      },
      "/ws": {
        target: "ws://localhost:8080",
        ws: true,
      },
    },
  },
  build: {
    target: "esnext",
  },
  ...(mode === "production" && {
    define: {
      "console.log": "(() => {})",
      "console.debug": "(() => {})",
    },
  }),
  // Prevent vite from obscuring rust errors
  clearScreen: false,
}));
