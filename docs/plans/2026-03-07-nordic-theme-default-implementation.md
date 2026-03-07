# Nordic Default Theme Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Update the default client theme to use true Nord colors matching the landing page, and add solid border lines between major layout areas.

**Architecture:** Pure CSS variable and UnoCSS config changes for colors. Layout components get updated border utility classes. No logic changes, no new components.

**Tech Stack:** CSS custom properties, UnoCSS config, Solid.js JSX (class attributes only)

---

### Task 1: Update theme colors and add border-solid token

**Files:**
- Modify: `client/src/styles/themes.css`

**Step 1: Update focused-hybrid color block (lines 43-63)**

Replace the focused-hybrid theme CSS variables with Nord values and add `--color-border-solid`:

```css
/* Focused Hybrid - Nordic dark theme aligned with CachyOS */
:root[data-theme="focused-hybrid"] {
  --color-surface-base: #242933;
  --color-surface-layer1: #2E3440;
  --color-surface-layer2: #3B4252;
  --color-surface-highlight: #434C5E;
  --color-text-primary: #eceff4;
  --color-text-secondary: #D8DEE9;
  --color-text-input: #eceff4;
  --color-accent-primary: #88c0d0;
  --color-accent-primary-hover: #7ab0c0;
  --color-accent-danger: #bf616a;
  --color-accent-success: #a3be8c;
  --color-accent-warning: #ebcb8b;
  --color-border-subtle: rgba(216, 222, 233, 0.06);
  --color-border-default: rgba(216, 222, 233, 0.12);
  --color-border-solid: #4C566A;
  --color-selection-bg: #88c0d0;
  --color-selection-text: #242933;
  --color-error-bg: rgba(191, 97, 106, 0.15);
  --color-error-border: rgba(191, 97, 106, 0.4);
  --color-error-text: #f0a0a8;
}
```

**Step 2: Update :root fallback block (lines 153-173)**

Apply the same Nord values to the `:root` fallback:

```css
/* Default theme fallback (same as focused-hybrid) */
:root {
  --color-surface-base: #242933;
  --color-surface-layer1: #2E3440;
  --color-surface-layer2: #3B4252;
  --color-surface-highlight: #434C5E;
  --color-text-primary: #eceff4;
  --color-text-secondary: #D8DEE9;
  --color-text-input: #eceff4;
  --color-accent-primary: #88c0d0;
  --color-accent-primary-hover: #7ab0c0;
  --color-accent-danger: #bf616a;
  --color-accent-success: #a3be8c;
  --color-accent-warning: #ebcb8b;
  --color-border-subtle: rgba(216, 222, 233, 0.06);
  --color-border-default: rgba(216, 222, 233, 0.12);
  --color-border-solid: #4C566A;
  --color-selection-bg: #88c0d0;
  --color-selection-text: #242933;
  --color-error-bg: rgba(191, 97, 106, 0.15);
  --color-error-border: rgba(191, 97, 106, 0.4);
  --color-error-text: #f0a0a8;
}
```

**Step 3: Add `--color-border-solid` to other themes**

Add the token to each existing theme so it's always available:

- `solarized-dark`: `--color-border-solid: #0e4c5a;` (after `--color-border-default`)
- `solarized-light`: `--color-border-solid: #c9c2b1;` (after `--color-border-default`)
- `pixel-cozy`: `--color-border-solid: #5c4e3e;` (after `--color-border-default`)

**Step 4: Commit**

```bash
git add client/src/styles/themes.css
git commit -m "style(client): update default theme to Nord palette"
```

---

### Task 2: Update focused-hybrid preview colors in theme store

**Files:**
- Modify: `client/src/stores/theme.ts:57`

**Step 1: Update preview colors**

Change the focused-hybrid preview to match the new surface color:

```typescript
preview: { surface: "#242933", accent: "#88C0D0", text: "#ECEFF4" },
```

**Step 2: Run existing tests**

Run: `cd client && bun run test:run -- --reporter=verbose src/stores/__tests__/theme.test.ts`
Expected: All tests PASS (tests are structural, not color-dependent)

**Step 3: Commit**

```bash
git add client/src/stores/theme.ts
git commit -m "style(client): update focused-hybrid preview to Nord surface"
```

---

### Task 3: Map border tokens in UnoCSS config

**Files:**
- Modify: `client/uno.config.ts`

**Step 1: Add border color mappings**

In the `colors` section of the theme config (after the `error` block, around line 53), add:

```typescript
border: {
  subtle: "var(--color-border-subtle)",
  DEFAULT: "var(--color-border-default)",
  solid: "var(--color-border-solid)",
},
```

**Step 2: Update safelist**

Add the new border utility classes to the safelist array:

```typescript
"border-border-subtle",
"border-border-solid",
"border-border-default",
```

**Step 3: Commit**

```bash
git add client/uno.config.ts
git commit -m "style(client): map border theme tokens in UnoCSS config"
```

---

### Task 4: Apply solid borders to layout dividers

**Files:**
- Modify: `client/src/components/layout/ServerRail.tsx:66`
- Modify: `client/src/components/layout/AppShell.tsx:48`
- Modify: `client/src/components/layout/Sidebar.tsx:94`

**Step 1: ServerRail — solid right border**

Line 66, change:
```
border-r border-white/10
```
to:
```
border-r border-border-solid
```

Full class string becomes:
```
w-[72px] flex flex-col items-center py-3 gap-2 bg-surface-base border-r border-border-solid z-20
```

**Step 2: AppShell main stage — solid left border**

Line 48, change:
```
border-l border-white/10
```
to:
```
border-l border-border-solid
```

Full class string becomes:
```
flex-1 flex flex-col min-w-0 bg-surface-layer1 relative border-l border-border-solid
```

**Step 3: Sidebar — solid right border**

Line 94, change:
```
border-r border-white/10
```
to:
```
border-r border-border-solid
```

Full class string becomes:
```
w-[240px] flex flex-col bg-surface-layer2 z-10 transition-all duration-300 border-r border-border-solid
```

**Step 4: Commit**

```bash
git add client/src/components/layout/ServerRail.tsx client/src/components/layout/AppShell.tsx client/src/components/layout/Sidebar.tsx
git commit -m "style(client): apply solid borders to layout dividers"
```

---

### Task 5: Apply subtle borders to internal dividers

**Files:**
- Modify: `client/src/components/layout/Sidebar.tsx:96,140`
- Modify: `client/src/components/layout/UserPanel.tsx:75`

**Step 1: Sidebar header — subtle bottom border**

Line 96, change:
```
border-b border-white/10
```
to:
```
border-b border-border-subtle
```

**Step 2: Sidebar separator — subtle top border**

Line 140, change:
```
border-t border-white/10
```
to:
```
border-t border-border-subtle
```

**Step 3: UserPanel — subtle top border**

Line 75, change:
```
border-t border-white/10
```
to:
```
border-t border-border-subtle
```

**Step 4: Commit**

```bash
git add client/src/components/layout/Sidebar.tsx client/src/components/layout/UserPanel.tsx
git commit -m "style(client): apply subtle borders to internal dividers"
```

---

### Task 6: Verify build and tests

**Step 1: Run the full client test suite**

Run: `cd client && bun run test:run`
Expected: All tests PASS

**Step 2: Run type check**

Run: `cd client && bun run tsc --noEmit`
Expected: No type errors

**Step 3: Visual verification (optional)**

If the dev environment is running, check:
- ServerRail | Sidebar | Main Stage have visible solid border lines between them
- Sidebar header and UserPanel have subtle separators
- All surfaces use blue-grey Nord tones, not purple-tinted
- Text is readable, accents match the landing page
