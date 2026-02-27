<!-- Parent: ../AGENTS.md -->

# styles

## Purpose

Global CSS definitions, theme color schemes, and syntax highlighting styles. Provides design system foundation via CSS custom properties.

## Key Files

- `themes.css` - Theme color definitions (focused-hybrid, solarized-dark, solarized-light)
- `global.css` - Base styles, resets, scrollbar customization
- `highlight-theme.css` - Syntax highlighting colors for code blocks (highlight.js)

## For AI Agents

### Theme System

Themes use CSS custom properties (`--color-*`) for dynamic switching:

```css
:root[data-theme="focused-hybrid"] {
  --color-surface-base: #1e1e2e;
  --color-accent-primary: #88c0d0;
  /* ... */
}
```

Applied via `data-theme` attribute on root element:

```typescript
// stores/theme.ts
document.documentElement.setAttribute("data-theme", themeId);
```

### Color Tokens

Semantic color system (not direct color values):

**Surfaces** (background layers):

- `--color-surface-base` - App background
- `--color-surface-layer1` - Panel background (sidebars, modals)
- `--color-surface-layer2` - Elevated surfaces (code blocks, inputs)
- `--color-surface-highlight` - Hover/active states

**Text**:

- `--color-text-primary` - Main content
- `--color-text-secondary` - Muted/helper text
- `--color-text-input` - Input field text

**Accents**:

- `--color-accent-primary` - Brand color (links, buttons)
- `--color-accent-danger` - Destructive actions
- `--color-accent-success` - Confirmations
- `--color-accent-warning` - Warnings

**Borders**:

- `--color-border-subtle` - Low-contrast dividers
- `--color-border-default` - Standard borders

**States**:

- `--color-selection-bg` / `--color-selection-text` - Text selection
- `--color-error-bg` / `--color-error-border` / `--color-error-text` - Error messages

### UnoCSS Integration

Theme colors mapped to UnoCSS utilities in `uno.config.ts`:

```typescript
theme: {
  colors: {
    'surface-base': 'var(--color-surface-base)',
    'accent-primary': 'var(--color-accent-primary)',
    // ...
  }
}
```

Used in components as utility classes:

```tsx
<div class="bg-surface-layer1 text-text-primary">...</div>
```

### Available Themes

1. **focused-hybrid** (default) - Modern dark theme, high contrast
2. **solarized-dark** - Precision colors, machine-readable
3. **solarized-light** - Warm light theme

### Syntax Highlighting

`highlight-theme.css` defines `.hljs-*` classes:

- Matches theme system via CSS variables
- Used by CodeBlock component
- Language-specific tokens (keyword, string, comment, etc.)
- Consistent with overall theme palette

### Global Styles

`global.css` provides:

- CSS reset (box-sizing, margin, padding)
- Custom scrollbar styling (thin, themed)
- Base typography (system font stack)
- Link styles
- Code block container styles

### Adding New Themes

1. Add color definition in `themes.css`:
   ```css
   :root[data-theme="new-theme"] {
     /* colors */
   }
   ```
2. Register in `stores/theme.ts`:
   ```typescript
   availableThemes: [{ id: "new-theme", name: "...", description: "..." }];
   ```
3. Test with ThemeDemo page

### Dark Mode Only

Currently all themes are dark-optimized. Light themes (solarized-light) use warm tones to reduce eye strain.
