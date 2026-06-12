# Accessibility (a11y)

> Goal 6 / Phase 7 a11y track. Kaiku targets **WCAG 2.1 AA**. This is a
> living checklist, not a one-time audit — most violations are caught at
> authoring time, not by tooling (eslint-plugin-jsx-a11y is React-oriented
> and produces false positives against Solid's component model, so it is
> not wired into CI; the checklist below is the contract instead).

## Authoring checklist (PR review)

When adding or changing UI, verify:

### Names for interactive elements
- **Icon-only buttons** need `aria-label` — `title` alone is a tooltip and
  is **not reliably announced** by screen readers or available on touch.
  Mirror the `title` into `aria-label` (they can be identical).
  ```tsx
  <button title="Create Channel" aria-label="Create Channel"><Plus /></button>
  ```
- Buttons with visible text need nothing extra.
- Links/anchors must have discernible text content.

### Forms
- Every input has an associated `<label>` (wrapping or `for`/`id`), or an
  `aria-label` when no visible label exists (e.g. a search box with only a
  placeholder — placeholders are **not** labels).

### Landmarks & navigation
- Primary nav regions use `<nav aria-label="…">` (e.g. ServerRail →
  `aria-label="Servers"`).
- The currently-selected item in a nav set carries
  `aria-current="page"` (ServerRail guild buttons already do this).

### Contrast (see also CLAUDE.md "UI Contrast Rules")
- Body text uses `text-text-primary`/`text-text-secondary`, never accent
  colors, on surfaces. Verify new colored-text-on-colored-bg with
  <https://webaim.org/resources/contrastchecker/>.
- Never `opacity-*` below 50% on text the user must read.

### Keyboard
- Everything actionable by mouse is reachable by Tab and operable by
  Enter/Space. Don't put `onClick` on non-interactive elements without a
  `role` + `tabindex` + key handler (prefer a real `<button>`).
- Modal/overlay dismissal works with Escape.

## Known gaps (future work)

- No automated a11y gate in CI (jsx-a11y/Solid incompatibility). A
  Solid-aware linter or a Playwright + axe-core pass would close this.
- Full keyboard-only mode and screen-reader pass (NVDA/VoiceOver) across
  all flows is not yet done — this checklist covers per-PR hygiene.
- Dynamic-title truncation tooltips on `<div>`s are acceptable (the full
  text is in the DOM and read by assistive tech); only **interactive**
  elements need explicit labels.

## Recently fixed

- 2026-06-12: 17 icon-only buttons across core chat UI (message input,
  channel list, category header, user panel, message actions, server
  rail) gained `aria-label`s mirroring their tooltips.
