import { defineConfig } from "vitest/config";
import path from "path";
import solidPlugin from "vite-plugin-solid";

export default defineConfig({
  // Compile Solid JSX in imported .tsx modules. Vite 8 (oxc) no longer
  // falls back to esbuild's tsconfig-driven JSX transform, which the
  // previously nested vite 7.3.1 relied on (cf. PR #517).
  plugins: [solidPlugin()],
  test: {
    environment: "jsdom",
    globals: true,
    include: ["src/**/*.test.ts", "src/**/*.test.tsx", "scripts/**/*.test.ts"],
    exclude: ["node_modules"],
    server: {
      deps: {
        // Ensure solid-js store uses the same browser build as user code
        inline: [/solid-js/, /@solidjs/],
      },
    },
  },
  resolve: {
    conditions: ["browser"],
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
});
