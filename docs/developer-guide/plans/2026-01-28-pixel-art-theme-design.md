# Pixel Art Theme System — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a cozy 8-bit pixel art theme with NES-inspired aesthetics, dithered textures, pixel fonts, and an extensible theme family architecture that enables community theme creation.

**Architecture:** Extends the existing CSS-variable theme system with structural tokens (radii, fonts, borders, shadows) via UnoCSS theme config overrides. Pixel themes share structural CSS via `[data-theme^="pixel-"]` family selector. No component logic changes.

**Tech Stack:** UnoCSS theme config, CSS custom properties, Press Start 2P font (OFL), PNG dither textures

---

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Aesthetic | Hybrid NES + modern pixel | NES nostalgia with practical UI extensions |
| Color mood | Cozy 8-bit (earth tones) | Warm, low eye-strain for long sessions |
| Typography | Pixel font UI, system font content | Retro charm without sacrificing chat readability |
| UI elements | Dithered textures | Sophisticated retro look, distinctive |
| Icons | Deferred to Phase 4 | Ship theme first, validate demand, then invest |
| Theme toggle | Existing settings panel | Consistent with current UX |
| UnoCSS migration | Theme config override | Zero component changes, validated with test |
| Future variants | Theme family architecture | dungeon, arcade, NES gray via prefix selector |

## Validated: UnoCSS Theme Config with CSS Variables

**Test result:** Adding `borderRadius: { xl: 'var(--radius-xl)' }` to `uno.config.ts` produces:

```css
.rounded-xl { border-radius: var(--radius-xl); }
```

This means all 87 files using `rounded-xl` etc. become theme-aware by editing **only `uno.config.ts`**. No component migration needed.

---

## Phase 1: Foundation (zero visual changes)

### Task 1: Consolidate ThemeName to Single Source of Truth

**Files:**
- Modify: `client/src/stores/theme.ts` (canonical location)
- Modify: `client/src/lib/types.ts:422` (import from theme.ts)
- Modify: `client/src/stores/preferences.ts:98` (derive validThemes from availableThemes)
- Modify: `client/src/components/settings/AppearanceSettings.tsx:88` (PreviewDot lookup)

**Step 1:** Export `THEME_IDS` from `theme.ts`:

```typescript
export const THEME_IDS = availableThemes.map(t => t.id);
```

**Step 2:** In `types.ts`, import and use:

```typescript
import type { ThemeName } from "../stores/theme";
// Replace hardcoded union with imported type
```

**Step 3:** In `preferences.ts`, replace hardcoded `validThemes` array:

```typescript
import { THEME_IDS } from "./theme";
// Use THEME_IDS for validation
```

**Step 4:** Verify TypeScript compiles cleanly:

```bash
cd client && bun run build
```

**Step 5:** Commit:

```bash
git add -A && git commit -m "refactor(client): consolidate ThemeName to single source of truth"
```

---

### Task 2: Add Structural CSS Tokens

**Files:**
- Create: `client/src/styles/themes-structure.css`
- Modify: `client/src/styles/global.css` (import new file)

**Step 1:** Create `themes-structure.css` with default tokens:

```css
/**
 * Structural Theme Tokens
 *
 * Shape, typography, border, and shadow tokens that themes can override.
 * Color tokens are in themes.css. These tokens handle everything else.
 *
 * HOW THIS WORKS:
 * - Default values below match the current modern UI appearance
 * - Theme families (e.g. pixel-*) override these via [data-theme^="prefix-"]
 * - UnoCSS theme config references these variables, so existing utility
 *   classes (rounded-xl, etc.) automatically respond to theme changes
 *
 * TOKEN REFERENCE:
 * --radius-sm/md/lg/xl/full  Border radius at different scales
 * --font-ui                   Font for headers, labels, buttons, nav
 * --font-content              Font for chat messages, user input, long text
 * --border-width              Default border thickness
 * --shadow-sm/md              Box shadows (none in pixel themes)
 */
:root {
  /* Shape — rounded modern defaults */
  --radius-sm: 0.25rem;
  --radius-md: 0.5rem;
  --radius-lg: 0.75rem;
  --radius-xl: 1rem;
  --radius-full: 9999px;

  /* Typography — system font stack */
  --font-ui: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
  --font-content: var(--font-ui);

  /* Borders */
  --border-width: 1px;

  /* Shadows */
  --shadow-sm: 0 1px 2px rgba(0, 0, 0, 0.1);
  --shadow-md: 0 4px 6px rgba(0, 0, 0, 0.15);
}
```

**Step 2:** Import in `global.css` (before themes.css):

```css
@import "./themes-structure.css";
```

**Step 3:** Verify no visual changes — existing themes should look identical:

```bash
cd client && bun run build
```

**Step 4:** Commit:

```bash
git add -A && git commit -m "feat(client): add structural CSS tokens for theme-aware shapes and typography"
```

---

### Task 3: Override UnoCSS Theme Config with CSS Variables

**Files:**
- Modify: `client/uno.config.ts`

**Step 1:** Add `borderRadius`, `boxShadow`, and `fontFamily` overrides to theme config:

```typescript
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
    // ... existing color config unchanged ...
    // Also convert hardcoded status colors to CSS variables:
    success: "var(--color-accent-success)",
    warning: "var(--color-accent-warning)",
    status: {
      success: "var(--color-accent-success)",
      error: "var(--color-accent-danger)",
      warning: "var(--color-accent-warning)",
    },
  },
}
```

**Step 2:** Add `--color-accent-success` and `--color-accent-warning` to each theme block in `themes.css` (they already exist in the color palette but may not be in all theme blocks as CSS variables).

**Step 3:** Verify existing `rounded-xl`, `shadow-md` etc. produce CSS-variable-backed output:

```bash
cd client && bun run build
```

**Step 4:** Commit:

```bash
git add -A && git commit -m "refactor(client): wire UnoCSS theme to CSS custom properties for structural tokens"
```

---

### Task 4: Add `family` and `preview` Fields to ThemeDefinition

**Files:**
- Modify: `client/src/stores/theme.ts`
- Modify: `client/src/components/settings/AppearanceSettings.tsx`

**Step 1:** Extend `ThemeDefinition`:

```typescript
export type ThemeFamily = "standard" | "pixel";

export interface ThemeDefinition {
  id: ThemeName;
  name: string;
  description: string;
  isDark: boolean;
  family: ThemeFamily;
  preview: {
    surface: string;
    accent: string;
    text: string;
  };
}
```

**Step 2:** Add `family` and `preview` to existing theme entries:

```typescript
{
  id: "focused-hybrid",
  name: "Focused Hybrid",
  description: "Modern dark theme with high contrast",
  isDark: true,
  family: "standard",
  preview: { surface: "#1E1E2E", accent: "#88C0D0", text: "#ECEFF4" },
},
// ... similar for solarized-dark and solarized-light
```

**Step 3:** Update `AppearanceSettings.tsx` to use `preview` from definition instead of hardcoded lookup. Remove the `PreviewDot` color map.

**Step 4:** Verify settings page renders correctly:

```bash
cd client && bun run build
```

**Step 5:** Commit:

```bash
git add -A && git commit -m "refactor(client): add family and preview fields to ThemeDefinition"
```

---

### Task 5: Add Family-Aware Theme Transition

**Files:**
- Modify: `client/src/stores/theme.ts`
- Modify: `client/src/styles/global.css`

**Step 1:** Add CSS transition class:

```css
/* In global.css */
.theme-family-transition {
  opacity: 0;
  transition: opacity 150ms ease-in-out;
}
```

**Step 2:** Update `setTheme` to fade when switching families:

```typescript
export function setTheme(newTheme: ThemeName): void {
  const oldDef = getCurrentTheme();
  const newDef = availableThemes.find(t => t.id === newTheme);

  // Cross-family switch: fade out, swap, fade in
  if (oldDef && newDef && oldDef.family !== newDef.family) {
    document.documentElement.classList.add('theme-family-transition');
    setTimeout(() => {
      updatePreference("theme", newTheme);
      requestAnimationFrame(() => {
        document.documentElement.classList.remove('theme-family-transition');
      });
    }, 150);
  } else {
    updatePreference("theme", newTheme);
  }
}
```

**Step 3:** Test by switching themes (no visual change yet since no pixel theme exists):

```bash
cd client && bun run build
```

**Step 4:** Commit:

```bash
git add -A && git commit -m "feat(client): add fade transition for cross-family theme switching"
```

---

## Phase 2: Pixel Theme

### Task 6: Register pixel-cozy Theme

**Files:**
- Modify: `client/src/stores/theme.ts`
- Modify: `client/src/styles/themes.css`

**Step 1:** Add `"pixel-cozy"` to `ThemeName`:

```typescript
export type ThemeName = "focused-hybrid" | "solarized-dark" | "solarized-light" | "pixel-cozy";
```

**Step 2:** Add theme definition:

```typescript
{
  id: "pixel-cozy",
  name: "Pixel Cozy",
  description: "Cozy 8-bit RPG aesthetic with warm earth tones",
  isDark: true,
  family: "pixel",
  preview: { surface: "#2C2418", accent: "#7BAE7F", text: "#E8D8C4" },
},
```

**Step 3:** Add color variables in `themes.css`:

```css
/* Pixel Cozy — Warm 8-bit RPG aesthetic
 *
 * Muted earth tones inspired by classic RPG inventory screens.
 * Part of the pixel-* theme family (structural overrides in themes-pixel.css).
 *
 * CONTRAST NOTES:
 * - text-primary (#E8D8C4) on surface-base (#2C2418): ~8.5:1 (AAA)
 * - text-secondary (#BEB09A) on surface-layer1 (#3A3024): ~4.6:1 (AA)
 * - accent-primary (#7BAE7F) on surface-base (#2C2418): ~4.8:1 (AA)
 */
:root[data-theme="pixel-cozy"] {
  /* Surfaces — warm parchment/wood tones */
  --color-surface-base: #2C2418;
  --color-surface-layer1: #3A3024;
  --color-surface-layer2: #4A3E30;
  --color-surface-highlight: #5C4E3E;

  /* Text — cream/ivory for readability */
  --color-text-primary: #E8D8C4;
  --color-text-secondary: #BEB09A;  /* lightened for AA contrast on layer1 */
  --color-text-input: #F5EDE0;

  /* Accents — muted jewel tones */
  --color-accent-primary: #7BAE7F;
  --color-accent-primary-hover: #6B9E6F;
  --color-accent-danger: #C06050;
  --color-accent-success: #8DB87E;
  --color-accent-warning: #D4A854;

  /* Borders — subtle wood grain feel */
  --color-border-subtle: rgba(232, 216, 196, 0.08);
  --color-border-default: rgba(232, 216, 196, 0.15);

  /* Selection & Errors */
  --color-selection-bg: #7BAE7F;
  --color-selection-text: #2C2418;
  --color-error-bg: rgba(192, 96, 80, 0.15);
  --color-error-border: rgba(192, 96, 80, 0.4);
  --color-error-text: #E8A090;
}
```

Note: `--color-text-secondary` set to `#BEB09A` (not `#A89880`) to meet WCAG AA 4.5:1 contrast on `--color-surface-layer1`.

**Step 4:** Verify theme appears in settings and colors apply:

```bash
cd client && bun run build
```

**Step 5:** Commit:

```bash
git add -A && git commit -m "feat(client): add pixel-cozy theme with earth-tone color palette"
```

---

### Task 7: Create themes-pixel.css Structural Overrides

**Files:**
- Create: `client/src/styles/themes-pixel.css`
- Create: `client/public/textures/dither-check.png` (2x2, ~80 bytes)
- Create: `client/public/textures/dither-light.png` (4x4, ~100 bytes)
- Create: `client/public/textures/dither-heavy.png` (4x4, ~100 bytes)
- Modify: `client/src/styles/global.css` (import new file)

**Step 1:** Create dither texture PNGs. These are tiny pixel patterns:
- `dither-check.png`: 2x2 checkerboard (50% density)
- `dither-light.png`: 4x4 sparse dots (25% density)
- `dither-heavy.png`: 4x4 dense dots (75% density)

Use any pixel editor or generate programmatically. Colors: semi-transparent black/white.

**Step 2:** Create `themes-pixel.css`:

```css
/**
 * Pixel Theme Family — Structural Overrides
 *
 * These styles apply to ALL pixel-* themes via [data-theme^="pixel-"].
 * Each pixel theme variant only needs to define its own colors in themes.css.
 * Fonts, shapes, textures, and behaviors are shared across the family.
 *
 * HOW TO ADD A PIXEL THEME VARIANT:
 * 1. Add your theme ID (e.g. "pixel-dungeon") to ThemeName in theme.ts
 * 2. Add a ThemeDefinition entry with family: "pixel"
 * 3. Add a color block in themes.css with :root[data-theme="pixel-dungeon"]
 * 4. Done — structural styles below are inherited automatically
 *
 * SECTIONS:
 * 1. Structural tokens (radii, borders, shadows)
 * 2. Typography (pixel font for UI chrome)
 * 3. Dithered textures (background patterns for surfaces)
 * 4. Animations (step-based instead of smooth)
 * 5. Scrollbar styling
 */

/* ============================================================================
 * 1. Structural Tokens
 * Override shape tokens for sharp, blocky pixel appearance.
 * These are consumed by UnoCSS via the theme config in uno.config.ts.
 * ============================================================================ */
:root[data-theme^="pixel-"] {
  --radius-sm: 0;
  --radius-md: 0;
  --radius-lg: 2px;
  --radius-xl: 2px;
  --radius-full: 2px;

  --border-width: 2px;

  --shadow-sm: none;
  --shadow-md: none;
}

/* ============================================================================
 * 2. Typography
 * Pixel font for UI chrome (headers, labels, buttons, channel names).
 * Chat messages and user input keep the system font for readability.
 *
 * IMPORTANT: Press Start 2P renders ~1.5x larger than system fonts at the
 * same font-size. UI elements using --font-ui need smaller sizes.
 * The --font-ui-scale factor compensates for this.
 * ============================================================================ */
:root[data-theme^="pixel-"] {
  --font-ui: "Press Start 2P", monospace;
  --font-content: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
  --font-ui-scale: 0.65;
}

/* Apply pixel font to UI elements with size compensation */
[data-theme^="pixel-"] h1,
[data-theme^="pixel-"] h2,
[data-theme^="pixel-"] h3,
[data-theme^="pixel-"] h4,
[data-theme^="pixel-"] button,
[data-theme^="pixel-"] label,
[data-theme^="pixel-"] nav,
[data-theme^="pixel-"] [class*="sidebar"] [class*="header"],
[data-theme^="pixel-"] [class*="channel-name"] {
  font-family: var(--font-ui);
  font-size: calc(1em * var(--font-ui-scale));
  line-height: 1.6;
  letter-spacing: 0.05em;
}

/* Keep content font for chat and input areas */
[data-theme^="pixel-"] [class*="message-content"],
[data-theme^="pixel-"] textarea,
[data-theme^="pixel-"] input[type="text"],
[data-theme^="pixel-"] input[type="search"],
[data-theme^="pixel-"] [contenteditable] {
  font-family: var(--font-content);
  font-size: 1em;
  line-height: 1.5;
}

/* ============================================================================
 * 3. Dithered Textures
 * Classic pixel art dithering patterns applied to static surfaces.
 * Uses tiny external PNGs for easy community customization.
 *
 * PATTERNS:
 * - dither-check.png: 2x2 checkerboard (50% density) — backgrounds
 * - dither-light.png: 4x4 sparse dots (25% density) — hover states
 * - dither-heavy.png: 4x4 dense dots (75% density) — active/pressed
 *
 * PERFORMANCE NOTE: Only applied to static surfaces, never to scrolling
 * message lists. Uses image-rendering: pixelated for crisp scaling.
 * ============================================================================ */
[data-theme^="pixel-"] {
  --dither-check: url("/textures/dither-check.png");
  --dither-light: url("/textures/dither-light.png");
  --dither-heavy: url("/textures/dither-heavy.png");
}

/* Subtle overlay on sidebar and panel backgrounds */
[data-theme^="pixel-"] [class*="sidebar"]::before {
  content: "";
  position: absolute;
  inset: 0;
  background-image: var(--dither-check);
  background-size: 4px 4px;
  image-rendering: pixelated;
  opacity: 0.04;
  pointer-events: none;
  z-index: 0;
}

/* ============================================================================
 * 4. Animations
 * Pixel art uses stepped transitions instead of smooth easing.
 * ============================================================================ */
[data-theme^="pixel-"] * {
  transition-timing-function: steps(3) !important;
}

/* ============================================================================
 * 5. Scrollbar Styling
 * Square, chunky scrollbars matching pixel aesthetic.
 * ============================================================================ */
[data-theme^="pixel-"] ::-webkit-scrollbar {
  width: 12px;
}

[data-theme^="pixel-"] ::-webkit-scrollbar-track {
  background: var(--color-surface-base);
  border-left: var(--border-width) solid var(--color-border-default);
}

[data-theme^="pixel-"] ::-webkit-scrollbar-thumb {
  background: var(--color-surface-highlight);
  border: var(--border-width) solid var(--color-border-default);
}
```

**Step 3:** Import in `global.css` after `themes.css`:

```css
@import "./themes-pixel.css";
```

**Step 4:** Test pixel-cozy theme — verify sharp corners, pixel font, dithering, scrollbars:

```bash
cd client && bun run build
```

**Step 5:** Commit:

```bash
git add -A && git commit -m "feat(client): add pixel theme family structural overrides with dithering"
```

---

### Task 8: Bundle and Preload Press Start 2P Font

**Files:**
- Create: `client/public/fonts/PressStart2P-Regular.woff2`
- Modify: `client/src/styles/themes-pixel.css` (add @font-face)
- Modify: `client/index.html` (add preload link)

**Step 1:** Download Press Start 2P from Google Fonts (OFL license). Convert to woff2 if needed.

```bash
# Download and place in public/fonts/
curl -o client/public/fonts/PressStart2P-Regular.woff2 \
  "https://fonts.gstatic.com/s/pressstart2p/v15/e3t4euO8T-267oIAQAu6jDQyK3nVivM.woff2"
```

**Step 2:** Add `@font-face` at the top of `themes-pixel.css`:

```css
/**
 * Press Start 2P — 8-bit Pixel Font
 * License: SIL Open Font License 1.1 (OFL)
 * Source: https://fonts.google.com/specimen/Press+Start+2P
 * Bundled locally to avoid network dependency and FOUT on theme switch.
 */
@font-face {
  font-family: "Press Start 2P";
  src: url("/fonts/PressStart2P-Regular.woff2") format("woff2");
  font-weight: 400;
  font-style: normal;
  font-display: block;
}
```

**Step 3:** Add preload in `index.html` `<head>` to prevent FOUT on theme switch:

```html
<link rel="preload" href="/fonts/PressStart2P-Regular.woff2" as="font" type="font/woff2" crossorigin>
```

**Step 4:** Verify font loads and renders correctly in pixel-cozy theme:

```bash
cd client && bun run build
```

**Step 5:** Commit:

```bash
git add -A && git commit -m "feat(client): bundle Press Start 2P pixel font with preloading"
```

---

### Task 9: Create Pixel Syntax Highlighting Overrides

**Files:**
- Create: `client/src/styles/themes-pixel-highlight.css`
- Modify: `client/src/styles/global.css` (import new file)

**Step 1:** Create earthy-toned syntax highlighting for pixel themes:

```css
/**
 * Pixel Theme — Syntax Highlighting
 *
 * Overrides highlight.js colors for pixel theme family.
 * Uses the cozy earth-tone palette for consistency.
 * Also overrides code block border-radius to match pixel aesthetic.
 */

/* Code block shape */
[data-theme^="pixel-"] .hljs {
  border-radius: var(--radius-lg);
  border: var(--border-width) solid var(--color-border-default);
}

/* Syntax colors — earth-tone palette */
[data-theme^="pixel-"] .hljs-keyword,
[data-theme^="pixel-"] .hljs-selector-tag { color: #C06050; }

[data-theme^="pixel-"] .hljs-string,
[data-theme^="pixel-"] .hljs-addition { color: #8DB87E; }

[data-theme^="pixel-"] .hljs-number,
[data-theme^="pixel-"] .hljs-literal { color: #D4A854; }

[data-theme^="pixel-"] .hljs-comment,
[data-theme^="pixel-"] .hljs-quote { color: #8A7A62; }

[data-theme^="pixel-"] .hljs-function,
[data-theme^="pixel-"] .hljs-title { color: #7BAE7F; }

[data-theme^="pixel-"] .hljs-type,
[data-theme^="pixel-"] .hljs-built_in { color: #BEB09A; }

[data-theme^="pixel-"] .hljs-attr,
[data-theme^="pixel-"] .hljs-attribute { color: #D4A854; }

[data-theme^="pixel-"] .hljs-deletion { color: #C06050; }
```

**Step 2:** Import in `global.css`:

```css
@import "./themes-pixel-highlight.css";
```

**Step 3:** Verify code blocks look correct in pixel-cozy theme:

```bash
cd client && bun run build
```

**Step 4:** Commit:

```bash
git add -A && git commit -m "feat(client): add pixel theme syntax highlighting overrides"
```

---

## Phase 3: Polish & Documentation

### Task 10: Add In-Code Comments to Theme Files

**Files:**
- Modify: `client/src/styles/themes.css` (add header comment with token reference)
- Modify: `client/src/stores/theme.ts` (add registration guide comment)

**Step 1:** Add comprehensive header comment to `themes.css`:

```css
/**
 * Theme Color Definitions
 *
 * Each theme defines semantic color tokens as CSS custom properties.
 * Themes are activated via the data-theme attribute on <html>.
 *
 * HOW TO ADD A NEW COLOR THEME:
 * 1. Add your theme ID to ThemeName in src/stores/theme.ts
 * 2. Add a ThemeDefinition entry to the availableThemes array
 * 3. Copy any existing color block below and change the data-theme value
 * 4. Replace all color values with your palette
 * 5. (Optional) For pixel themes, set family: "pixel" — structural styles
 *    in themes-pixel.css are inherited automatically via [data-theme^="pixel-"]
 *
 * TOKEN REFERENCE:
 * --color-surface-base       Main app background (most visible area)
 * --color-surface-layer1     Sidebar, panels, cards (one level above base)
 * --color-surface-layer2     Nested panels, dropdowns, popovers
 * --color-surface-highlight  Active/selected/hovered items
 * --color-text-primary       Main text, headings (high contrast required)
 * --color-text-secondary     Muted text: timestamps, hints, descriptions
 * --color-text-input         Text inside input fields
 * --color-accent-primary     Buttons, links, active indicators
 * --color-accent-primary-hover  Hovered state of accent-primary
 * --color-accent-danger      Delete actions, errors, destructive UI
 * --color-accent-success     Online status, success messages, confirmations
 * --color-accent-warning     Warnings, pending states, caution indicators
 * --color-border-subtle      Faint dividers between content sections
 * --color-border-default     Visible borders on inputs, cards, panels
 * --color-selection-bg       Text selection highlight background
 * --color-selection-text     Text color when selected
 * --color-error-bg           Error banner/toast background
 * --color-error-border       Error banner/toast border
 * --color-error-text         Error message text color
 *
 * ACCESSIBILITY:
 * Ensure text-primary achieves >=7:1 contrast on surface-base (AAA).
 * Ensure text-secondary achieves >=4.5:1 contrast on surface-layer1 (AA).
 * Use https://webaim.org/resources/contrastchecker/ to verify.
 */
```

**Step 2:** Add registration guide to `theme.ts`:

```typescript
/**
 * Available Themes
 *
 * Central registry of all themes. This is the SINGLE SOURCE OF TRUTH
 * for theme definitions. ThemeName type and validation derive from this array.
 *
 * To add a new theme:
 * 1. Add your theme ID string to the ThemeName type union above
 * 2. Add a ThemeDefinition entry below
 * 3. Add matching CSS color variables in src/styles/themes.css
 * 4. For pixel themes: set family: "pixel" to inherit structural overrides
 *    from themes-pixel.css (fonts, borders, dithering, etc.)
 *
 * Theme families: Themes with the same family share structural CSS.
 * The pixel-* family uses [data-theme^="pixel-"] selector.
 * Standard themes use the default structural tokens.
 */
```

**Step 3:** Commit:

```bash
git add -A && git commit -m "docs(client): add comprehensive theme system comments for community contributors"
```

---

### Task 11: Write THEME_GUIDE.md

**Files:**
- Create: `client/src/styles/THEME_GUIDE.md`

**Content outline:**

1. **Quick Start: Color-Only Theme** (~5 min)
   - Add ThemeName entry
   - Copy CSS color block, replace values
   - Accessibility checklist (contrast ratios)

2. **Theme Families** (advanced)
   - How `[data-theme^="prefix-"]` works
   - Creating a new pixel variant (just colors)
   - Creating a new family (structural overrides)

3. **Structural Tokens Reference**
   - All `--radius-*`, `--font-*`, `--border-*`, `--shadow-*` tokens
   - UnoCSS integration explanation

4. **Dither Textures** (advanced)
   - How to create/replace texture PNGs
   - Application rules

5. **Icon Theming** (future)
   - Placeholder for when icon system is built

**Step 1:** Write the guide document.

**Step 2:** Commit:

```bash
git add -A && git commit -m "docs(client): add THEME_GUIDE.md for community theme creation"
```

---

### Task 12: Add Theme Testing

**Files:**
- Create: `client/src/stores/__tests__/theme.test.ts`

**Tests to write:**

1. **Theme completeness:** Every theme in `availableThemes` has a matching CSS variable block
2. **ThemeName consistency:** `THEME_IDS` matches `ThemeName` type (runtime check)
3. **Preview completeness:** Every theme has `preview.surface`, `preview.accent`, `preview.text`
4. **Family field:** Every theme has a valid `family` value
5. **Theme switching:** `setTheme()` updates the `data-theme` attribute correctly

**Step 1:** Write tests using vitest.

**Step 2:** Run tests:

```bash
cd client && bun run test:run
```

**Step 3:** Commit:

```bash
git add -A && git commit -m "test(client): add theme system consistency tests"
```

---

### Task 13: Update CHANGELOG.md and LICENSE_COMPLIANCE.md

**Files:**
- Modify: `CHANGELOG.md`
- Modify: `LICENSE_COMPLIANCE.md`

**Step 1:** Add to CHANGELOG.md under `[Unreleased]`:

```markdown
### Added
- Pixel art theme system with "Pixel Cozy" warm earth-tone theme
- Theme family architecture allowing community theme variants
- Structural CSS tokens (radii, fonts, shadows) for theme-aware UI
- Dithered texture patterns for pixel theme backgrounds
- Press Start 2P pixel font for UI elements
- Theme creation guide (THEME_GUIDE.md) for community contributors
- Cross-family fade transition when switching between theme types

### Changed
- Consolidated ThemeName to single source of truth
- UnoCSS theme config now uses CSS custom properties for border-radius and shadows
- Theme settings panel groups themes by family
```

**Step 2:** Add Press Start 2P to `LICENSE_COMPLIANCE.md`:

```markdown
### Press Start 2P (Font)
- **License:** SIL Open Font License 1.1 (OFL-1.1)
- **Source:** https://fonts.google.com/specimen/Press+Start+2P
- **Author:** CodeMan38
- **Usage:** Bundled font for pixel art theme UI elements
- **Compatibility:** OFL-1.1 is compatible with MIT/Apache-2.0 dual license
```

**Step 3:** Commit:

```bash
git add -A && git commit -m "docs: update CHANGELOG and LICENSE_COMPLIANCE for pixel art theme"
```

---

## Phase 4: Icons (Deferred — Separate Effort)

> Ship the pixel theme without custom icons first. If users love it, invest in Phase 4.

### Task 14: Create Icon Wrapper Component

Create `<Icon name="mic" />` wrapper that checks pixel theme and sprite availability, falling back to Lucide.

### Task 15: Migrate Lucide Imports to Icon Component

Incrementally replace direct `lucide-solid` imports across 68+ files.

### Task 16: Build SVG Sprite Sheet Pipeline

Script to combine individual 16x16 PNG icons into an SVG sprite sheet during build.

### Task 17: Generate and Integrate Pixel Icons

User generates icons via AI tool using the prompt spec below, integrates into sprite sheet.

**AI Icon Generation Prompt Spec:**

```
Style: 16x16 pixel art icon on transparent background.
Aesthetic: NES/Famicom era, cozy RPG inventory style.
Color palette (use ONLY these colors):
  - Primary:    #E8D8C4 (aged parchment / main icon color)
  - Secondary:  #BEB09A (faded ink / detail color)
  - Accent:     #7BAE7F (sage green / highlight)
  - Dark:       #2C2418 (dark walnut / outlines)
  - Warning:    #D4A854 (gold coin / special states)
  - Danger:     #C06050 (brick red / alert states)

Rules:
  - Exactly 16x16 pixels, no anti-aliasing
  - 1px outline maximum (not required for all icons)
  - No sub-pixel rendering or gradients
  - Transparent background
  - Export as PNG at 16x16, crisp edges

Icon: [NAME]
Description: [WHAT IT REPRESENTS]
Reference: [OPTIONAL REAL-WORLD REFERENCE]
```

**Priority tiers:**
- Tier 1 (25 icons): Hash, Mic, MicOff, Headphones, Volume2, VolumeX, PhoneOff, Signal, Crown, Shield, Lock, Bell, BellOff, Plus, X, Send, Check, Trash2, Users, User, UserPlus, Settings, Home, ChevronDown, ChevronRight
- Tier 2 (30 icons): Voice extended, editing, actions, status, UI, files
- Tier 3 (37 icons): Remaining — use Lucide fallback

---

## Verification Checklist

### Phase 1 Complete
- [ ] ThemeName defined in exactly one place
- [ ] `bun run build` succeeds with no type errors
- [ ] Existing themes look visually identical
- [ ] `rounded-xl` generates `border-radius: var(--radius-xl)` in CSS output
- [ ] Theme preview dots use ThemeDefinition.preview (no hardcoded lookup)

### Phase 2 Complete
- [ ] pixel-cozy appears in theme settings
- [ ] Selecting pixel-cozy shows earth-tone colors
- [ ] All corners are sharp (0 or 2px radius)
- [ ] UI headers/buttons use pixel font
- [ ] Chat messages use system font
- [ ] Dither texture visible on sidebar background
- [ ] Code blocks use earth-tone syntax colors
- [ ] Scrollbars are chunky/square
- [ ] Cross-family theme switch shows fade transition
- [ ] Font renders without flash on theme switch

### Phase 3 Complete
- [ ] Theme files have comprehensive comments
- [ ] THEME_GUIDE.md explains 3 levels of theme creation
- [ ] Theme tests pass
- [ ] CHANGELOG.md updated
- [ ] LICENSE_COMPLIANCE.md includes Press Start 2P

### Accessibility
- [ ] text-primary on surface-base: >= 7:1 (AAA)
- [ ] text-secondary on surface-layer1: >= 4.5:1 (AA)
- [ ] accent-primary on surface-base: >= 4.5:1 (AA)
- [ ] All error/success/warning states distinguishable
