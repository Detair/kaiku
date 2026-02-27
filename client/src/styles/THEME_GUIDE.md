# Theme Creation Guide

This guide explains how to create custom themes for the VoiceChat client. Themes use CSS custom properties (variables) to define colors and structural tokens.

---

## Quick Start: Color-Only Theme (~5 min)

The fastest way to add a theme is to define only colors, inheriting all structural styles (shapes, fonts, shadows) from the defaults.

### Step 1: Register the Theme ID

Edit `client/src/lib/types.ts` and add your theme ID to the `THEME_NAMES` array:

```typescript
export const THEME_NAMES = [
  "focused-hybrid",
  "solarized-dark",
  "solarized-light",
  "pixel-cozy",
  "my-custom-theme", // Add your theme here
] as const;
```

### Step 2: Add Theme Definition

Edit `client/src/stores/theme.ts` and add an entry to `availableThemes`:

```typescript
{
  id: "my-custom-theme",
  name: "My Custom Theme",
  description: "A brief description of your theme",
  isDark: true,  // or false for light themes
  family: "standard",  // or "pixel" for pixel-art variants
  preview: {
    surface: "#1E1E2E",  // Main background color
    accent: "#88C0D0",   // Primary accent color
    text: "#ECEFF4"      // Primary text color
  },
},
```

### Step 3: Define CSS Color Variables

Edit `client/src/styles/themes.css` and add a color block:

```css
/* My Custom Theme - Description */
:root[data-theme="my-custom-theme"] {
  /* Surfaces (backgrounds) */
  --color-surface-base: #1e1e2e;
  --color-surface-layer1: #252535;
  --color-surface-layer2: #2a2a3c;
  --color-surface-highlight: #36364d;

  /* Text */
  --color-text-primary: #eceff4;
  --color-text-secondary: #9ca3af;
  --color-text-input: #ffffff;

  /* Accents */
  --color-accent-primary: #88c0d0;
  --color-accent-primary-hover: #7ab0c0;
  --color-accent-danger: #bf616a;
  --color-accent-success: #a3be8c;
  --color-accent-warning: #ebcb8b;

  /* Borders */
  --color-border-subtle: rgba(255, 255, 255, 0.05);
  --color-border-default: rgba(255, 255, 255, 0.1);

  /* Selection */
  --color-selection-bg: #88c0d0;
  --color-selection-text: #1e1e2e;

  /* Errors */
  --color-error-bg: rgba(191, 97, 106, 0.15);
  --color-error-border: rgba(191, 97, 106, 0.4);
  --color-error-text: #f0a0a8;
}
```

### Step 4: Verify Accessibility

Check contrast ratios using [WebAIM Contrast Checker](https://webaim.org/resources/contrastchecker/):

| Token Pair                           | Required Ratio | Standard |
| ------------------------------------ | -------------- | -------- |
| `text-primary` on `surface-base`     | 7:1            | WCAG AAA |
| `text-secondary` on `surface-layer1` | 4.5:1          | WCAG AA  |
| `accent-primary` on `surface-base`   | 4.5:1          | WCAG AA  |

---

## Theme Families

Themes belong to "families" that share structural CSS (fonts, border-radius, shadows). This lets you create visual variants without duplicating structural code.

### How Family Selectors Work

Structural CSS uses prefix-matching selectors:

```css
/* Applies to ALL themes starting with "pixel-" */
[data-theme^="pixel-"] {
  --radius-xl: 2px;
  --font-ui: "Press Start 2P", monospace;
}
```

This means:

- `pixel-cozy` inherits these styles
- `pixel-dungeon` would also inherit them
- `my-custom-theme` would NOT (different prefix)

### Creating a Pixel Variant (Colors Only)

To add a new pixel-art theme variant:

1. Add your theme ID with the `pixel-` prefix to `THEME_NAMES`
2. Set `family: "pixel"` in your ThemeDefinition
3. Add only the color variables to `themes.css`

All structural overrides (pixel font, sharp corners, dithering) are inherited automatically from `themes-pixel.css`.

Example for a "dungeon" variant:

```typescript
// In theme.ts
{
  id: "pixel-dungeon",
  name: "Pixel Dungeon",
  description: "Dark stone dungeon aesthetic",
  isDark: true,
  family: "pixel",  // Inherits pixel structure
  preview: { surface: "#1A1A1A", accent: "#8B4513", text: "#C0C0C0" },
},
```

```css
/* In themes.css - ONLY colors needed */
:root[data-theme="pixel-dungeon"] {
  --color-surface-base: #1a1a1a;
  --color-surface-layer1: #252525;
  /* ... rest of color tokens ... */
}
```

### Creating a New Theme Family

To create an entirely new structural family (e.g., "neon-" for cyberpunk themes):

1. Create `themes-neon.css` with structural overrides
2. Use the prefix selector: `[data-theme^="neon-"]`
3. Add the family to the `ThemeFamily` type in `theme.ts`
4. Import the new CSS file in `index.tsx`

---

## Structural Tokens Reference

These tokens control non-color aspects of the UI. Override them in family CSS files.

### Border Radius

| Token           | Default | Used For                 |
| --------------- | ------- | ------------------------ |
| `--radius-sm`   | 0.25rem | Small chips, badges      |
| `--radius-md`   | 0.5rem  | Buttons, inputs          |
| `--radius-lg`   | 0.75rem | Cards, panels            |
| `--radius-xl`   | 1rem    | Large containers, modals |
| `--radius-full` | 9999px  | Pills, avatars           |

### Typography

| Token             | Default           | Used For                             |
| ----------------- | ----------------- | ------------------------------------ |
| `--font-ui`       | System font stack | Headers, buttons, labels, navigation |
| `--font-content`  | System font stack | Chat messages, user input, long text |
| `--font-ui-scale` | 1                 | Size adjustment for pixel fonts      |

### Borders & Shadows

| Token            | Default       | Used For                 |
| ---------------- | ------------- | ------------------------ |
| `--border-width` | 1px           | Default border thickness |
| `--shadow-sm`    | Light shadow  | Subtle elevation         |
| `--shadow-md`    | Medium shadow | Cards, dropdowns         |

### UnoCSS Integration

These tokens are wired to UnoCSS utilities in `uno.config.ts`:

```typescript
theme: {
  borderRadius: {
    xl: 'var(--radius-xl)',  // .rounded-xl uses this
  },
  boxShadow: {
    md: 'var(--shadow-md)',  // .shadow-md uses this
  },
}
```

This means existing utility classes automatically respond to theme changes.

---

## Dither Textures (Advanced)

Pixel themes use tiny PNG textures for visual depth.

### Texture Variables

| Variable         | Pattern          | Density | Used For              |
| ---------------- | ---------------- | ------- | --------------------- |
| `--dither-check` | 2x2 checkerboard | 50%     | Sidebar backgrounds   |
| `--dither-light` | 4x4 sparse dots  | 25%     | Hover states          |
| `--dither-heavy` | 4x4 dense dots   | 75%     | Active/pressed states |

### Creating Custom Textures

1. Create a 2x2 or 4x4 pixel PNG
2. Use transparent pixels for "off" and semi-transparent white/black for "on"
3. Place in `client/public/textures/`
4. Reference as `url("/textures/your-texture.png")`

### Application CSS

Textures are applied via `::before` pseudo-elements:

```css
[data-theme^="pixel-"] [class*="sidebar"]::before {
  content: "";
  position: absolute;
  inset: 0;
  background-image: var(--dither-check);
  background-size: 4px 4px;
  image-rendering: pixelated;
  opacity: 0.04;
  pointer-events: none;
}
```

---

## Syntax Highlighting

Code blocks use highlight.js classes. Override them in family-specific files.

For pixel themes, see `themes-pixel-highlight.css`:

```css
[data-theme^="pixel-"] .hljs-keyword {
  color: #c06050;
}
[data-theme^="pixel-"] .hljs-string {
  color: #8db87e;
}
/* ... etc ... */
```

---

## Icon Theming (Future)

Icon theming is planned for a future release. The current icon system uses Lucide icons which don't support per-theme variants.

When implemented, this section will cover:

- Icon color tokens
- Icon set switching per theme family
- Custom icon creation guidelines

---

## Checklist: Theme Submission

Before submitting a community theme:

- [ ] Theme ID added to `THEME_NAMES`
- [ ] ThemeDefinition added with all required fields
- [ ] CSS color block complete with all tokens
- [ ] Accessibility: text-primary >=7:1 contrast on surface-base
- [ ] Accessibility: text-secondary >=4.5:1 contrast on surface-layer1
- [ ] Preview colors match actual theme appearance
- [ ] Theme description is concise and accurate
- [ ] If pixel family: verified inheritance works correctly
