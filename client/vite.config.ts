import { defineConfig } from "vite";
import solidPlugin from "vite-plugin-solid";
import UnoCSS from "unocss/vite";
import path from "path";

export default defineConfig({
  plugins: [
    UnoCSS(),
    solidPlugin(),
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
    port: 5173,
    strictPort: true,
  },
  build: {
    target: "esnext",
    commonjsOptions: {
      transformMixedEsModules: true,
    },
  },
  // Prevent vite from obscuring rust errors
  clearScreen: false,
});
