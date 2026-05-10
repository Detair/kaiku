# UI Contrast & Emoji Picker Fixes — Design

**Date:** 2026-05-10
**Status:** Design — pending review
**Author:** Mal Detair (with Claude Opus 4.7)
**Branch:** `docs/ui-contrast-emoji-fixes-design`
**Trigger:** Post-deploy e2e walkthrough on `kaiku.pmind.de` after the dep-update sweep deployed Phases 0–6b.

---

## Summary

Three UI defects surfaced during e2e inspection of the beta deploy:

1. **Selected/active states fail WCAG contrast.** `bg-accent-primary/20`, `bg-status-success/20`, `bg-status-warning/20` and similar alpha-modifier classes silently render as **fully opaque** color. Selected nav items in Settings and Admin compute to **1.74:1** contrast (WCAG AA needs 4.5:1; CLAUDE.md targets 7:1 AAA). Affects ~30+ surfaces across admin panels, modals, status pills, and warning banners.
2. **"Session Not Elevated" warning banner: white text on solid yellow** (1.35:1). Auto-fixed by Issue 1's root cause fix.
3. **Emoji picker renders at viewport `(0, 0)`** instead of anchored near the trigger. Picker is functional but visually dislocated.

Issues 1 and 2 share a root cause (UnoCSS cannot inject alpha into a hex color held behind `var(--color-X)`). Issue 3 is a separate SolidJS reactivity bug.

The contrast issue is **not a regression** from the recent dep-update sweep — the `var(--color-X)` theme pattern dates from `2026-01-13` and has been silently dropping alpha for ~4 months. Past contrast PRs (#393, #409) addressed individual elements rather than the systemic root cause.

---

## Goals

- Make `bg-accent-primary/N`, `bg-status-X/N`, `text-text-primary/N` etc. render with their intended alpha tint instead of as solid color.
- Restore the soft-tint visual the original design clearly intended (matching the literal class semantics).
- Bring selected nav items, status pills, and warning banners to **≥7:1 contrast on `surface-base`** (WCAG AAA, the CLAUDE.md target).
- Fix the emoji picker rendering at `(0, 0)`.

## Non-goals

- Migrating all 146 inline `style="background-color: var(--color-X)"` usages to a new pattern.
- Color palette changes, new tokens, dark/light theme rebalancing.
- Touching elements that already pass contrast (e.g. the `Manage Users` button uses dark-on-cyan and stays as-is).
- Investigating why past contrast PRs missed the root cause (out of scope).

---

## Root cause analysis

### Issue 1: UnoCSS alpha-modifier dropped

`client/uno.config.ts` defines colors via CSS variable references:

```ts
accent: {
  primary: "var(--color-accent-primary)",
  ...
}
```

`themes.css` defines those variables as hex literals:

```css
--color-accent-primary: #88c0d0;
```

When a class like `bg-accent-primary/25` is processed, UnoCSS needs to take the color value and inject `0.25` as alpha. With a hex literal already inside a `var()`, there is no syntactic place to inject the alpha — the alpha modifier is silently dropped and the rendered `background-color` becomes the fully-opaque hex.

This is a **well-known UnoCSS / Tailwind pattern**. The canonical fix is the "RGB channel variable" pattern: the variable holds a space-separated RGB triplet (no `rgb(...)` wrapper), and the theme references it via `rgb(var(--color-X-rgb) / <alpha-value>)`. UnoCSS substitutes the alpha into the `<alpha-value>` placeholder; legacy CSS that uses `var(--color-X-rgb)` directly is unaffected.

### Issue 2: Yellow warning banner

`AdminDashboard.tsx:139` and similar use `bg-status-warning/20 text-text-primary`. With Issue 1's bug, the rendered background is solid `#ebcb8b` (Nord yellow), and `text-text-primary` (`#eceff4` near-white) on that yields **1.35:1**.

Once Issue 1 is fixed, the same class produces `rgba(235, 203, 139, 0.20)` over `surface-layer2` — an effective dark muted tan around `#4c4d43` — and the white text reads at ≈AAA. **No per-element fix is required.**

### Issue 3: Emoji picker positioning

`client/src/components/emoji/PositionedEmojiPicker.tsx:106-108` calls SolidJS signals **outside** the JSX:

```tsx
const pos = position();        // captures {x:0, y:0} at render time
const height = maxHeight();
return (
  <Portal>
    <div ref={pickerRef} style={{ left: `${pos.x}px`, top: `${pos.y}px`, ... }}>
```

`floating-ui`'s `computePosition` runs in `onMount` and calls `setPosition({x, y})` later. The signal updates, but the component's JSX never re-evaluates the captured `pos` const, so the inline `style` keeps the initial `{x:0, y:0}` forever. Result: picker renders at viewport `(0, 0)` regardless of trigger position.

Fix: inline the signal calls inside the JSX so SolidJS subscribes the `style` prop to signal updates.

---

## Architecture

Three independent fixes, each on its own PR.

### Fix A — Add parallel `-rgb` CSS variables to themes (additive)

For every color token used with an `/<alpha>` modifier in the codebase, add a sibling variable holding the **space-separated RGB triplet** (no `#`, no commas, no wrapper). The legacy hex variable stays.

**Files:** `client/src/styles/themes.css`, `client/src/styles/themes-pixel.css`.

**Tokens that need `-rgb` variants** (each in every theme block — Nord Hybrid, Solarized Dark, Solarized Light, Pixel Cozy, etc.):

- `--color-accent-primary-rgb`
- `--color-accent-danger-rgb`
- `--color-accent-success-rgb`
- `--color-accent-warning-rgb`
- `--color-text-primary-rgb`
- `--color-text-secondary-rgb`
- `--color-text-muted-rgb`
- `--color-text-input-rgb`
- `--color-surface-base-rgb`
- `--color-surface-layer1-rgb`
- `--color-surface-layer2-rgb`
- `--color-surface-highlight-rgb`

Status tokens map to accent tokens (per existing `uno.config.ts:status`):

- `--color-status-success-rgb` → same as `accent-success-rgb`
- `--color-status-error-rgb` → same as `accent-danger-rgb`
- `--color-status-warning-rgb` → same as `accent-warning-rgb`

(Status colors don't need their own variables; `uno.config.ts` can point them at the accent `-rgb` vars.)

**Example diff for the default Nord block in `themes.css`:**

```css
:root, [data-theme="focused-hybrid"] {
  --color-accent-primary: #88c0d0;
  --color-accent-primary-rgb: 136 192 208;          /* NEW */
  --color-accent-primary-hover: #7ab0c0;
  --color-text-primary: #eceff4;
  --color-text-primary-rgb: 236 239 244;            /* NEW */
  /* ... */
}
```

This PR is purely additive — no `bg-X/Y` class behavior changes yet. Safe to land alone.

### Fix B — Update `client/uno.config.ts` to use the `-rgb` variants

Switch the 15 token bindings (4 accent + 4 text + 4 surface + 3 status) from the legacy `var(--color-X)` form to the alpha-aware form:

```ts
// Before
accent: {
  primary: "var(--color-accent-primary)",
}

// After
accent: {
  primary: "rgb(var(--color-accent-primary-rgb) / <alpha-value>)",
}
```

UnoCSS substitutes `<alpha-value>` based on the modifier:

- `bg-accent-primary` → `<alpha-value>` becomes `1` → `rgb(... / 1)` → fully opaque (unchanged from current)
- `bg-accent-primary/25` → `<alpha-value>` becomes `0.25` → `rgb(... / 0.25)` → soft tint (CURRENTLY BROKEN, FIXED)

**Files:** `client/uno.config.ts` only.

**Tokens migrated:** `accent.{primary,danger,success,warning}`, `text.{primary,secondary,muted,input}`, `surface.{base,layer1,layer2,highlight}`, `status.{success,error,warning}`. Plus the legacy compatibility aliases (`primary.DEFAULT`, `success`, `warning`, `danger`, `background.*`) keep pointing at the same underlying tokens.

**Visual outcome:** every `/<N>` alpha tint goes from saturated solid color to soft alpha-blend over the underlying surface. Final contrast ratios depend on the exact alpha and underlying surface; estimates from manual computation:

- Selected nav (`bg-accent-primary/25` over `surface-base`): 1.74:1 → ≈7:1
- Yellow warning banner (`bg-status-warning/20` over `surface-layer2`): 1.35:1 → ≈6.5:1
- "Current" pill (`bg-status-success/20` over card): 1.76:1 → ≈7:1

Pre-merge `getComputedStyle` audit (test plan §PR2) verifies actual ratios. No per-element overrides needed.

**Tokens NOT migrated** (used as solid colors only, no alpha modifier needed):

- `error.{bg,border,text}` — already used as solid hex
- `border.{subtle,DEFAULT,solid}` — solid only
- `on-accent`, `on-success`, `on-danger` — text-on-color, solid only

### Fix C — Emoji picker reactivity

**File:** `client/src/components/emoji/PositionedEmojiPicker.tsx`.

**Before** (lines 106-115, simplified):

```tsx
const pos = position();
const height = maxHeight();

return (
  <Portal>
    <div
      ref={pickerRef}
      style={{
        position: "fixed",
        left: `${pos.x}px`,
        top: `${pos.y}px`,
        ...(height ? { "max-height": `${height}px` } : {}),
      }}
    >
```

**After:**

```tsx
return (
  <Portal>
    <div
      ref={pickerRef}
      style={{
        position: "fixed",
        left: `${position().x}px`,
        top: `${position().y}px`,
        ...(maxHeight() ? { "max-height": `${maxHeight()}px` } : {}),
      }}
    >
```

The signal accessors are now inside the `style` prop, so SolidJS subscribes the prop to signal updates and re-renders when `floating-ui` produces real coordinates.

**Risk:** essentially zero. Either the picker now positions correctly or it stays where it is and revert restores current behavior.

---

## PR sequencing

| PR | Branch | Files | Risk | Visual change | Depends on |
|---|---|---|---|---|---|
| 1 | `fix/theme-rgb-channel-vars` | 2 (theme CSS files) | None | None | — |
| 2 | `fix/uno-alpha-modifier` | 1 (`uno.config.ts`) | Medium | App-wide tint softening | PR 1 |
| 3 | `fix/emoji-picker-reactivity` | 1 (`PositionedEmojiPicker.tsx`) | None | Picker positions correctly | — |

**Recommended order:** PR1 → PR3 → soak → PR2. PR1 and PR3 can ship in either order; PR2 must follow PR1.

**Soak windows:** PR1 is purely additive (no behavioral change), so no soak needed before deploying to beta. The meaningful soak is **after PR2 deploys** — wait ≥24h for the visual change to absorb on `kaiku.pmind.de` before declaring the workstream done. PR3 doesn't need a soak — isolated change.

---

## Test plan

### Build gates (every PR)

- `bun run lint` clean.
- `bun run build` clean and **no new** UnoCSS "unmatched utility" warnings (existing `panel`, `card`, `input-field` shortcut warnings predate this work).
- `bun run test:run` clean.

### Per-PR verification

**PR 1 (theme `-rgb` vars):**

- `git diff` shows only **additive** lines (no deletions in theme blocks).
- `bun run dev`, switch through every theme variant, confirm no visual regression on home, settings, admin.

**PR 2 (uno.config alpha-aware bindings):**

1. **Programmatic contrast audit.** Re-run the playwright `getComputedStyle` + WCAG calculator from the e2e session on five anchor elements, all should compute **≥7:1**:
   - Settings → My Account selected tab (was 1.74:1)
   - Settings → Appearance → Focused Hybrid card title (was 2.00:1)
   - Settings → Active Sessions → "Current" pill (was ~1.76:1)
   - Admin Dashboard → Overview selected nav (was 1.74:1)
   - Admin Dashboard → "Session Not Elevated" warning (was 1.35:1)
2. **Visual diff.** Take fresh screenshots of: home, Wolftown #test, Settings → My Account, Settings → Appearance, Admin Dashboard. Compare against pre-fix screenshots — pattern identical except the listed tints are now soft instead of saturated.
3. **`@mention` regression check.** Open Wolftown #test where alpha's `@Detair` has 8.57:1 contrast; confirm unchanged (uses `color()` syntax, isn't affected by this fix).
4. **Direct-style usages.** Spot-check 3-4 of the 146 `style="background-color: var(--color-X)"` usages — these reference the legacy hex variable, which still exists, so they must render unchanged.
5. **Theme switch.** Cycle every theme in `Appearance` and confirm tints render correctly per theme.

**PR 3 (emoji picker):**

- Open emoji picker on Wolftown #test composer. Confirm picker appears anchored near the trigger button (offset ~4px below it per `floating-ui` config), not at viewport `(0, 0)`.
- Drag the message-list scrollbar to put the composer near each viewport edge (top/bottom/right) and re-open the picker. Confirm `flip` middleware repositions the picker so it stays in view.
- `Escape` key closes picker (existing behavior — verify not regressed).

### Cross-cutting check after PR2 ships

- Re-deploy to `kaiku.pmind.de` (`./infra/scripts/deploy.sh`).
- Confirm `/health` green.
- 5-minute auth smoke test: login → home → settings → admin → exit. Confirm no console errors beyond the known `/auth/refresh → 401` noise.

---

## Risks & mitigations

| # | Risk | Likelihood | Mitigation |
|---|---|---|---|
| 1 | Some token I migrate to `rgb(var(...) / <alpha-value>)` is also referenced by code that expects the legacy hex form | Low | Migrating only `uno.config.ts` color bindings, NOT changing `--color-X` definitions. Direct `style="background-color: var(--color-X)"` usages stay intact. |
| 2 | A `bg-accent-primary` (no slash) usage breaks because `<alpha-value>` defaults differently than expected | Low | UnoCSS `<alpha-value>` defaults to `1` when no modifier; `rgb(... / 1)` is fully opaque, semantically identical to current. |
| 3 | Some `/<N>` element was deliberately styled to *look* saturated (designer wanted the broken-alpha look) | Low | Visual review on beta after PR2 deploys; if any element looks wrong, fix at the call site (switch to non-alpha class or dark-text-on-saturated pattern like `Manage Users`). |
| 4 | Other themes (Solarized Dark/Light, Pixel Cozy) break because a `-rgb` variant was missed | Low | Each theme block in `themes.css` / `themes-pixel.css` gets the same set of `-rgb` additions. Visual smoke includes theme switch. |
| 5 | Emoji picker fix introduces new bug (over-eager re-render, scroll loop) | Very low | The fix removes a Solid reactivity bug; correctness of `floating-ui` integration was already there. |
| 6 | `<alpha-value>` syntax is UnoCSS-version-specific and breaks on a future UnoCSS major | Very low | Documented since UnoCSS 0.50+; current beta uses 66.6.8. Tailwind 3+ uses identical syntax. |

**Worst-case rollback:** revert PR2 in <60 seconds. Themes still have the `-rgb` vars from PR1 (harmless and unused), and the legacy `var(--color-X)` references in `uno.config.ts` are restored.

---

## Out of scope (deferred)

- Migrating the 146 inline `style="background-color: var(--color-X)"` usages to use UnoCSS classes. The legacy pattern works; touching it adds noise without value here.
- A separate audit of components that have *correct* contrast today via dark-on-saturated (like `Manage Users` Quick Action). These don't need to change.
- Investigating why prior contrast PRs (#393, #409) addressed individual elements rather than the root cause. The historical note is captured here; future Claude/contributors have it.
- Adding contrast tests to CI. Worth doing later but adds scope to this work.

---

## Done when

- Three PRs merged in sequence (1 → 3 → 2).
- After PR2 deploy:
  - `/health` green on beta.
  - Programmatic contrast audit on the five anchor elements all read ≥7:1.
  - Visual diff confirms soft-tint pattern across selected/active/pill/banner states.
  - Emoji picker positions correctly relative to its trigger.
- This design doc remains in `docs/superpowers/specs/` as the historical record.
