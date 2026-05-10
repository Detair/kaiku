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
    borderRadius: {
      sm: 'var(--radius-sm)',
      DEFAULT: 'var(--radius-md)',
      md: 'var(--radius-md)',
      lg: 'var(--radius-lg)',
      xl: 'var(--radius-xl)',
      '2xl': 'var(--radius-xl)',
      full: 'var(--radius-full)',
    },
    boxShadow: {
      sm: 'var(--shadow-sm)',
      DEFAULT: 'var(--shadow-md)',
      md: 'var(--shadow-md)',
    },
    fontFamily: {
      ui: 'var(--font-ui)',
      content: 'var(--font-content)',
    },
    colors: {
      // Theme System - CSS Variables (supports runtime theme switching)
      // Tokens with -rgb variants use rgb(var(...) / <alpha-value>) so UnoCSS
      // alpha modifiers like /20, /25 actually inject alpha. Tokens used as
      // solid colors only (border, error, on-accent) keep the legacy var() form.
      surface: {
        base: "rgb(var(--color-surface-base-rgb) / <alpha-value>)",
        layer1: "rgb(var(--color-surface-layer1-rgb) / <alpha-value>)",
        layer2: "rgb(var(--color-surface-layer2-rgb) / <alpha-value>)",
        highlight: "rgb(var(--color-surface-highlight-rgb) / <alpha-value>)",
      },
      text: {
        primary: "rgb(var(--color-text-primary-rgb) / <alpha-value>)",
        secondary: "rgb(var(--color-text-secondary-rgb) / <alpha-value>)",
        muted: "rgb(var(--color-text-muted-rgb) / <alpha-value>)",
        input: "rgb(var(--color-text-input-rgb) / <alpha-value>)",
      },
      "on-accent": "var(--color-text-on-accent)",
      "on-success": "var(--color-text-on-success)",
      "on-danger": "var(--color-text-on-danger)",
      accent: {
        primary: "rgb(var(--color-accent-primary-rgb) / <alpha-value>)",
        danger: "rgb(var(--color-accent-danger-rgb) / <alpha-value>)",
        success: "rgb(var(--color-accent-success-rgb) / <alpha-value>)",
        warning: "rgb(var(--color-accent-warning-rgb) / <alpha-value>)",
      },
      error: {
        bg: "var(--color-error-bg)",
        border: "var(--color-error-border)",
        text: "var(--color-error-text)",
      },
      border: {
        subtle: "var(--color-border-subtle)",
        DEFAULT: "var(--color-border-default)",
        solid: "var(--color-border-solid)",
      },
      // Legacy compatibility (maps to new theme system)
      primary: {
        DEFAULT: "rgb(var(--color-accent-primary-rgb) / <alpha-value>)",
        hover: "var(--color-accent-primary-hover)",
      },
      background: {
        primary: "rgb(var(--color-surface-layer1-rgb) / <alpha-value>)",
        secondary: "rgb(var(--color-surface-layer2-rgb) / <alpha-value>)",
        tertiary: "rgb(var(--color-surface-base-rgb) / <alpha-value>)",
      },
      success: "rgb(var(--color-accent-success-rgb) / <alpha-value>)",
      warning: "rgb(var(--color-accent-warning-rgb) / <alpha-value>)",
      danger: "rgb(var(--color-accent-danger-rgb) / <alpha-value>)",
      // Status colors for admin panels (alias to accent tokens with same alpha behavior)
      status: {
        success: "rgb(var(--color-accent-success-rgb) / <alpha-value>)",
        error: "rgb(var(--color-accent-danger-rgb) / <alpha-value>)",
        warning: "rgb(var(--color-accent-warning-rgb) / <alpha-value>)",
      },
    },
  },
  shortcuts: {
    // Buttons
    "btn": "px-4 py-2 rounded-lg font-medium transition-all duration-200",
    "btn-primary": "btn bg-accent-primary hover:bg-accent-primary/80 text-on-accent",
    "btn-danger": "btn bg-accent-danger hover:bg-accent-danger/80 text-white",

    // Input fields
    "input-field": "w-full px-3 py-2 bg-surface-layer2 rounded-lg text-text-input placeholder-text-secondary outline-none focus:ring-2 focus:ring-accent-primary/70 focus:border-accent-primary/50 border border-white/5",

    // Panels and Cards
    "panel": "bg-surface-layer2 rounded-lg border border-white/5",
    "card": "bg-surface-layer1 rounded-lg p-4 hover:bg-surface-highlight transition-colors",

    // Interactive items
    "item-hover": "rounded-lg px-2 py-1 hover:bg-white/5 transition-colors cursor-pointer",

    // Animations
    "animate-slide-up": "animate-[slideUp_0.2s_ease-out]",
  },
  safelist: [
    "animate-slide-up",
    "bg-surface-base",
    "bg-surface-layer1",
    "bg-surface-layer2",
    "bg-surface-highlight",
    "bg-white/30",
    "bg-accent-primary/20",
    "text-text-primary",
    "text-text-secondary",
    "text-text-input",
    "text-accent-primary",
    "text-accent-danger",
    "text-white",
    "text-on-accent",
    "text-on-success",
    "text-on-danger",
    "border-white/5",
    "border-white/10",
    "border-border-subtle",
    "border-border-solid",
    "border-border-default",
    "relative",
    "z-10",
    "bg-status-success/20",
    "bg-status-error/15",
    "bg-status-error/20",
    "bg-status-warning/15",
    "text-status-success",
    "text-status-error",
    "text-status-warning",
  ],
  preflights: [
    {
      getCSS: () => `@keyframes slideUp { from { opacity: 0; transform: translateY(20px); } to { opacity: 1; transform: translateY(0); } }`,
    },
  ],
  variants: [
    {
      name: "touch",
      match(matcher: string) {
        if (!matcher.startsWith("touch:")) return;
        return {
          matcher: matcher.slice(6),
          parent: "@media (hover: none)",
        };
      },
    },
  ],
  rules: [
    [/^animate-\[slideUp/, () => ({
      animation: "slideUp 0.2s ease-out",
    })],
  ],
});
