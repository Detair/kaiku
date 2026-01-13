import { defineConfig, presetUno, presetIcons } from "unocss";

export default defineConfig({
  presets: [
    presetUno(),
    presetIcons({
      scale: 1.2,
      cdn: "https://esm.sh/",
    }),
  ],
  theme: {
    colors: {
      // Theme System - CSS Variables (supports runtime theme switching)
      surface: {
        base: "var(--color-surface-base)",
        layer1: "var(--color-surface-layer1)",
        layer2: "var(--color-surface-layer2)",
        highlight: "var(--color-surface-highlight)",
      },
      text: {
        primary: "var(--color-text-primary)",
        secondary: "var(--color-text-secondary)",
      },
      accent: {
        primary: "var(--color-accent-primary)",
        danger: "var(--color-accent-danger)",
      },
      // Legacy compatibility (maps to new theme system)
      primary: {
        DEFAULT: "var(--color-accent-primary)",
        hover: "var(--color-accent-primary-hover)",
      },
      background: {
        primary: "var(--color-surface-layer1)",
        secondary: "var(--color-surface-layer2)",
        tertiary: "var(--color-surface-base)",
      },
      success: "#A3BE8C",
      warning: "#EBCB8B",
      danger: "var(--color-accent-danger)",
    },
  },
  shortcuts: {
    // Buttons
    "btn": "px-4 py-2 rounded-xl font-medium transition-all duration-200",
    "btn-primary": "btn bg-accent-primary hover:bg-accent-primary/80 text-surface-base",
    "btn-danger": "btn bg-accent-danger hover:bg-accent-danger/80 text-white",

    // Input fields
    "input-field": "w-full px-3 py-2 bg-surface-layer2 rounded-xl text-text-primary outline-none focus:ring-2 focus:ring-accent-primary/50 border border-white/5",

    // Panels and Cards
    "panel": "bg-surface-layer2 rounded-xl border border-white/5",
    "card": "bg-surface-layer1 rounded-xl p-4 hover:bg-surface-highlight transition-colors",

    // Interactive items
    "item-hover": "rounded-xl px-2 py-1 hover:bg-white/5 transition-colors cursor-pointer",

    // Animations
    "animate-slide-up": "animate-[slideUp_0.2s_ease-out]",
  },
  safelist: [
    "animate-slide-up",
    "bg-surface-base",
    "bg-surface-layer1",
    "bg-surface-layer2",
    "bg-surface-highlight",
    "text-text-primary",
    "text-text-secondary",
    "text-accent-primary",
    "text-accent-danger",
    "border-white/5",
    "border-white/10",
  ],
  rules: [
    [/^animate-\[slideUp/, () => ({
      "@keyframes slideUp": `{
        from { opacity: 0; transform: translateY(20px); }
        to { opacity: 1; transform: translateY(0); }
      }`,
      animation: "slideUp 0.2s ease-out",
    })],
  ],
});
