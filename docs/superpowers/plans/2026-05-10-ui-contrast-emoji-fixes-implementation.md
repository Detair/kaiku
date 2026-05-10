# UI Contrast & Emoji Picker Fixes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix three UI defects identified in the post-deploy e2e walkthrough on `kaiku.pmind.de` — UnoCSS alpha-modifier silently dropping (root cause of multiple selected/active state contrast failures), white-on-yellow warning banner contrast (auto-fixed by the same root-cause fix), and an emoji picker rendering at viewport `(0, 0)` due to a SolidJS reactivity bug.

**Architecture:** Three independent PRs. PR1 adds `-rgb` channel variables alongside existing hex CSS variables in `themes.css` (purely additive, no behavioral change). PR2 updates `uno.config.ts` to consume the `-rgb` variants via `rgb(var(...) / <alpha-value>)` syntax — this is where the visual change lands. PR3 fixes the emoji picker by inlining SolidJS signal accessors inside JSX so the position prop subscribes to signal updates.

**Tech Stack:** UnoCSS 66.6.8 (frontend), SolidJS 1.9.12, `@floating-ui/dom` 1.7.6, Bun (tooling), Playwright (visual verification).

**Spec:** [`docs/superpowers/specs/2026-05-10-ui-contrast-emoji-fixes-design.md`](../specs/2026-05-10-ui-contrast-emoji-fixes-design.md)

---

## How to use this plan

- **Working directory:** the design worktree at `.claude/worktrees/ui-contrast-emoji-design/` (already on `docs/ui-contrast-emoji-fixes-design`). Each phase below creates a NEW worktree off `main` for the fix branch — do NOT implement fixes inside the design worktree.
- **Source of truth for *why*:** the spec. This plan is the *how*.
- **Branch naming, worktree convention, merge strategy** all follow `CLAUDE.md` (squash-merge via `gh pr merge <N> --squash --delete-branch`; worktrees under `.claude/worktrees/<name>` cleaned up after merge).
- **Order:** PR1 first (additive), then PR3 (isolated, can land in parallel with PR1), then PR2 (depends on PR1, lands the visual change). Soak ≥24h after PR2 deploys to beta before declaring done.

## Common quality gates (every PR)

Before commit/PR:

1. `cd client && bun run lint` clean
2. `cd client && bun run build` clean (no NEW UnoCSS "unmatched utility" warnings — the existing 3 warnings for `panel` / `card` / `input-field` shortcuts predate this work)
3. `cd client && bun run test:run` clean
4. From repo root: `SQLX_OFFLINE=true cargo clippy --workspace -- -D warnings` clean (Rust unaffected, just verify nothing else regressed)

If a gate fails: fix it in the same PR. Do not silently disable rules or relax `tsconfig`/`eslint` strictness.

## Changelog discipline

Per `CLAUDE.md`, user-relevant changes go under `[Unreleased]` in `CHANGELOG.md`. PR2 is the only PR with a user-facing visual change — add a `### Fixed` entry there. PR1 (purely additive theme tokens) and PR3 (3-line bug fix) don't need entries unless you want to mention the emoji picker fix.

---

## Phase 1 — Add `-rgb` channel variables to `themes.css` (additive)

**Branch:** `fix/theme-rgb-channel-vars`
**Risk:** None. Purely additive — adds new CSS variables; nothing reads them yet.
**Files modified:** `client/src/styles/themes.css` only.

### Task 1.1: Create worktree

- [ ] **Step 1: Create worktree off main**

```bash
cd /home/detair/GIT/detair/kaiku
git worktree add .claude/worktrees/theme-rgb-vars -b fix/theme-rgb-channel-vars main
cd .claude/worktrees/theme-rgb-vars
```

Expected: worktree created on `fix/theme-rgb-channel-vars` branched from `main` head.

### Task 1.2: Capture build baseline

- [ ] **Step 1: Run baseline build**

```bash
cd client
bun install --frozen-lockfile 2>&1 | tail -5
bun run build 2>&1 | tee /tmp/build-pre-pr1.log | tail -10
cd ..
```

Expected: `✓ built in X.Xs` at the bottom. Note the byte sizes of `dist/index.html` and `dist/assets/index-*.js` for diff later.

- [ ] **Step 2: Capture UnoCSS warning count for regression check**

```bash
grep -c "\[unocss\] unmatched utility" /tmp/build-pre-pr1.log
```

Expected: `3` (panel, card, input-field — pre-existing).

### Task 1.3: Add `-rgb` variants to `focused-hybrid` block

The 12 tokens get RGB triplets. Hex-to-RGB conversions for this theme:

| Token | Hex | RGB triplet |
|---|---|---|
| `--color-surface-base` | `#242933` | `36 41 51` |
| `--color-surface-layer1` | `#2E3440` | `46 52 64` |
| `--color-surface-layer2` | `#3B4252` | `59 66 82` |
| `--color-surface-highlight` | `#434C5E` | `67 76 94` |
| `--color-text-primary` | `#eceff4` | `236 239 244` |
| `--color-text-secondary` | `#D8DEE9` | `216 222 233` |
| `--color-text-muted` | `#7b88a0` | `123 136 160` |
| `--color-text-input` | `#eceff4` | `236 239 244` |
| `--color-accent-primary` | `#88c0d0` | `136 192 208` |
| `--color-accent-danger` | `#bf616a` | `191 97 106` |
| `--color-accent-success` | `#a3be8c` | `163 190 140` |
| `--color-accent-warning` | `#ebcb8b` | `235 203 139` |

- [ ] **Step 1: Edit `client/src/styles/themes.css`**

Open the file, locate `:root[data-theme="focused-hybrid"]` (line 46). After each existing color line, add the corresponding `-rgb` line. The result for that block:

```css
:root[data-theme="focused-hybrid"] {
  --color-surface-base: #242933;
  --color-surface-base-rgb: 36 41 51;
  --color-surface-layer1: #2E3440;
  --color-surface-layer1-rgb: 46 52 64;
  --color-surface-layer2: #3B4252;
  --color-surface-layer2-rgb: 59 66 82;
  --color-surface-highlight: #434C5E;
  --color-surface-highlight-rgb: 67 76 94;
  --color-text-primary: #eceff4;
  --color-text-primary-rgb: 236 239 244;
  --color-text-secondary: #D8DEE9;
  --color-text-secondary-rgb: 216 222 233;
  --color-text-muted: #7b88a0;
  --color-text-muted-rgb: 123 136 160;
  --color-text-input: #eceff4;
  --color-text-input-rgb: 236 239 244;
  --color-accent-primary: #88c0d0;
  --color-accent-primary-rgb: 136 192 208;
  --color-accent-primary-hover: #7ab0c0;
  --color-text-on-accent: #2E3440;
  --color-accent-danger: #bf616a;
  --color-accent-danger-rgb: 191 97 106;
  --color-accent-success: #a3be8c;
  --color-accent-success-rgb: 163 190 140;
  --color-accent-warning: #ebcb8b;
  --color-accent-warning-rgb: 235 203 139;
  --color-text-on-success: #2E3440;
  --color-text-on-danger: #ECEFF4;
  --color-border-subtle: rgba(216, 222, 233, 0.06);
  --color-border-default: rgba(216, 222, 233, 0.12);
  --color-border-solid: #4C566A;
  --color-selection-bg: #88c0d0;
  --color-selection-text: #242933;
  --color-error-bg: rgba(191, 97, 106, 0.15);
  --color-error-border: rgba(191, 97, 106, 0.4);
  --color-error-text: #f4b8bf;

  /* UI Icons */
  --icon-accept: url('../assets/icons/accept_request_icon.png');
  --icon-decline: url('../assets/icons/decline_request_icon.png');
  --icon-pickup: url('../assets/icons/pickup_call_icon.png');
  --icon-leave: url('../assets/icons/end_call_icon.png');
  --icon-mute: url('../assets/icons/mute_icon.png');
  --icon-deafen: url('../assets/icons/deafen_icon.png');

  color-scheme: dark;
}
```

Note: `--color-accent-primary-hover` does NOT get a `-rgb` variant because it's not used with alpha modifiers. Verify with:

```bash
grep -rn "accent-primary-hover/" client/src --include="*.tsx" --include="*.ts" --include="*.css"
```

Expected: no output (no usages of the alpha-modifier form). If output is non-empty, add a `-rgb` variant for consistency.

### Task 1.4: Add `-rgb` variants to `solarized-dark` block

Hex-to-RGB conversions:

| Token | Hex | RGB triplet |
|---|---|---|
| `--color-surface-base` | `#002b36` | `0 43 54` |
| `--color-surface-layer1` | `#073642` | `7 54 66` |
| `--color-surface-layer2` | `#0e4c5a` | `14 76 90` |
| `--color-surface-highlight` | `#145766` | `20 87 102` |
| `--color-text-primary` | `#93a1a1` | `147 161 161` |
| `--color-text-secondary` | `#657b83` | `101 123 131` |
| `--color-text-muted` | `#4f6068` | `79 96 104` |
| `--color-text-input` | `#fdf6e3` | `253 246 227` |
| `--color-accent-primary` | `#268bd2` | `38 139 210` |
| `--color-accent-danger` | `#dc322f` | `220 50 47` |
| `--color-accent-success` | `#859900` | `133 153 0` |
| `--color-accent-warning` | `#b58900` | `181 137 0` |

- [ ] **Step 1: Add the 12 `-rgb` lines to the `solarized-dark` block**

Same pattern as Task 1.3 — insert each `-rgb` line directly after its corresponding hex line within `:root[data-theme="solarized-dark"]` (line 84 in the original file).

### Task 1.5: Add `-rgb` variants to `solarized-light` block

Hex-to-RGB conversions:

| Token | Hex | RGB triplet |
|---|---|---|
| `--color-surface-base` | `#fdf6e3` | `253 246 227` |
| `--color-surface-layer1` | `#eee8d5` | `238 232 213` |
| `--color-surface-layer2` | `#e8e2cf` | `232 226 207` |
| `--color-surface-highlight` | `#ddd6c1` | `221 214 193` |
| `--color-text-primary` | `#586e75` | `88 110 117` |
| `--color-text-secondary` | `#93a1a1` | `147 161 161` |
| `--color-text-muted` | `#b0b8b8` | `176 184 184` |
| `--color-text-input` | `#073642` | `7 54 66` |
| `--color-accent-primary` | `#268bd2` | `38 139 210` |
| `--color-accent-danger` | `#dc322f` | `220 50 47` |
| `--color-accent-success` | `#859900` | `133 153 0` |
| `--color-accent-warning` | `#b58900` | `181 137 0` |

- [ ] **Step 1: Add the 12 `-rgb` lines to the `solarized-light` block** (line 114 in the original file).

### Task 1.6: Add `-rgb` variants to `pixel-cozy` block

Hex-to-RGB conversions:

| Token | Hex | RGB triplet |
|---|---|---|
| `--color-surface-base` | `#2c2418` | `44 36 24` |
| `--color-surface-layer1` | `#3a3024` | `58 48 36` |
| `--color-surface-layer2` | `#4a3e30` | `74 62 48` |
| `--color-surface-highlight` | `#5c4e3e` | `92 78 62` |
| `--color-text-primary` | `#e8d8c4` | `232 216 196` |
| `--color-text-secondary` | `#beb09a` | `190 176 154` |
| `--color-text-muted` | `#8a7e6c` | `138 126 108` |
| `--color-text-input` | `#f5ede0` | `245 237 224` |
| `--color-accent-primary` | `#7bae7f` | `123 174 127` |
| `--color-accent-danger` | `#c06050` | `192 96 80` |
| `--color-accent-success` | `#8db87e` | `141 184 126` |
| `--color-accent-warning` | `#d4a854` | `212 168 84` |

- [ ] **Step 1: Add the 12 `-rgb` lines to the `pixel-cozy` block** (line 153 in the original file).

### Task 1.7: Add `-rgb` variants to `:root` default fallback block

Same triplets as `focused-hybrid` (the fallback mirrors `focused-hybrid` per the file header). Use the same RGB values from Task 1.3.

- [ ] **Step 1: Add the 12 `-rgb` lines to the `:root` block** (line 192 in the original file).

### Task 1.8: Verify CSS file integrity

- [ ] **Step 1: Count `-rgb` lines added**

```bash
grep -c "\-rgb:" client/src/styles/themes.css
```

Expected: `60` (12 tokens × 5 theme blocks).

- [ ] **Step 2: Verify no syntax errors via build**

```bash
cd client
bun run build 2>&1 | tee /tmp/build-post-pr1.log | tail -10
cd ..
```

Expected: `✓ built in X.Xs` at the bottom. Build must succeed.

- [ ] **Step 3: Confirm no NEW UnoCSS warnings**

```bash
grep -c "\[unocss\] unmatched utility" /tmp/build-post-pr1.log
```

Expected: `3` (same count as baseline). If higher, a typo was introduced.

- [ ] **Step 4: Confirm no other regressions**

```bash
diff <(grep -E "^dist/" /tmp/build-pre-pr1.log) <(grep -E "^dist/" /tmp/build-post-pr1.log) | head -20
```

Expected: minimal or no diff in dist file sizes (CSS variables don't affect built output until something references them).

### Task 1.9: Smoke test in dev server

- [ ] **Step 1: Start dev server**

```bash
cd client
bun run dev 2>&1 &
DEV_PID=$!
sleep 8
cd ..
```

- [ ] **Step 2: Verify dev server is up**

```bash
curl -sf http://localhost:5173/ -o /dev/null -w "%{http_code}\n"
```

Expected: `200`.

- [ ] **Step 3: Visual sanity (manual)**

Open `http://localhost:5173/` in a browser. Confirm: home view renders, no console errors beyond the expected `/auth/refresh → 401` if running unauthenticated. Cycle through all 4 themes via Settings → Appearance — each renders without visual glitches. The `-rgb` variables are unused at this point, so the rendered output should be **identical** to baseline.

- [ ] **Step 4: Stop dev server**

```bash
kill $DEV_PID 2>/dev/null
wait $DEV_PID 2>/dev/null
```

### Task 1.10: Commit and open PR

- [ ] **Step 1: Commit**

```bash
git add client/src/styles/themes.css
git commit -m "$(cat <<'EOF'
fix(client): add -rgb channel variables to theme tokens

Adds space-separated RGB triplet variants for the 12 color tokens
that get used with UnoCSS alpha modifiers (bg-accent-primary/20 etc).
The legacy hex variables stay untouched, so the 146 inline
`style="background-color: var(--color-X)"` usages keep working.

This is purely additive — no class behavior changes. The follow-up
PR (fix/uno-alpha-modifier) updates uno.config.ts to consume these
new variants, which is where the alpha-modifier fix actually lands.

Surfaced by post-deploy e2e contrast audit; design at
docs/superpowers/specs/2026-05-10-ui-contrast-emoji-fixes-design.md.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 2: Push and open PR**

```bash
git push -u origin fix/theme-rgb-channel-vars
gh pr create --base main --title "fix(client): add -rgb channel variables to theme tokens" --body "$(cat <<'EOF'
## Summary
- Adds \`--color-X-rgb\` (space-separated RGB triplet) variants alongside the existing hex \`--color-X\` tokens in \`client/src/styles/themes.css\`.
- 12 tokens × 5 theme blocks = 60 new lines. Purely additive.
- No class behavior change; nothing reads the new variants yet. Visual output identical.

## Why
Sets up the follow-up PR (\`fix/uno-alpha-modifier\`) which will switch \`uno.config.ts\` to consume these variants via \`rgb(var(--color-X-rgb) / <alpha-value>)\` so UnoCSS classes like \`bg-accent-primary/20\` render with their intended alpha tint instead of silently rendering as solid color.

## Spec
\`docs/superpowers/specs/2026-05-10-ui-contrast-emoji-fixes-design.md\` (Phase A).

## Test plan
- [ ] \`bun run build\` clean, no new UnoCSS warnings
- [ ] \`bun run lint\` clean
- [ ] \`bun run test:run\` clean
- [ ] Manually cycle all 4 themes in dev — visual output identical to pre-PR

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

Expected: PR opened against `main`. Note the PR number for the merge step.

### Task 1.11: Wait for CI, merge, clean up

- [ ] **Step 1: Wait for CI green**

```bash
gh pr checks <PR_NUMBER> --watch
```

- [ ] **Step 2: Merge**

```bash
gh pr merge <PR_NUMBER> --squash --delete-branch
```

- [ ] **Step 3: Clean up local worktree**

```bash
cd /home/detair/GIT/detair/kaiku
git worktree remove .claude/worktrees/theme-rgb-vars
git fetch --prune origin
```

### Phase 1 → Phase 2 soak

PR1 is purely additive. **No soak required.** Proceed directly to Phase 2 (or to Phase 3 — both can run in parallel after PR1).

---

## Phase 2 — Update `uno.config.ts` to consume `-rgb` variants (visual change)

**Branch:** `fix/uno-alpha-modifier`
**Risk:** Medium. Changes how every `/<N>` alpha-modifier class renders across the app.
**Pre-flight gate:** Phase 1 (`-rgb` variables) must be merged to `main` first.
**Files modified:** `client/uno.config.ts` only, plus `CHANGELOG.md`.

### Task 2.1: Verify Phase 1 is on main

- [ ] **Step 1: Check main has the `-rgb` variables**

```bash
cd /home/detair/GIT/detair/kaiku
git fetch origin main
git show origin/main:client/src/styles/themes.css | grep -c '\-rgb:'
```

Expected: `60`. If `0`, Phase 1 isn't merged yet — stop and merge Phase 1 first.

### Task 2.2: Create worktree

- [ ] **Step 1: Create worktree off main**

```bash
git worktree add .claude/worktrees/uno-alpha-fix -b fix/uno-alpha-modifier main
cd .claude/worktrees/uno-alpha-fix
```

### Task 2.3: Capture pre-fix contrast baseline (programmatic)

This is the "failing test" for this phase: we capture current contrast ratios that prove the bug, then verify they fix after the change.

- [ ] **Step 1: Start dev server**

```bash
cd client && bun install --frozen-lockfile 2>&1 | tail -3
bun run dev 2>&1 &
DEV_PID=$!
sleep 8
cd ..
```

- [ ] **Step 2: Capture pre-fix contrast for 5 anchor elements**

Use playwright (or any headless-browser script) to navigate to `http://localhost:5173`, log in (manual or with stored credentials), and compute contrast on these elements. Expected ratios with the bug:

| # | Location | Expected pre-fix ratio | Anchor selector hint |
|---|---|---|---|
| 1 | Settings → My Account selected tab | ~1.74:1 | `button` with text "My Account" inside Settings dialog |
| 2 | Settings → Appearance → Focused Hybrid card title | ~2.00:1 | `div`/`span` with text starting "Focused Hybrid" |
| 3 | Settings → My Account → Active Sessions → "Current" pill | ~1.76:1 | `span` with text exactly "Current" |
| 4 | Admin Dashboard → Overview selected nav | ~1.74:1 | `button`/`a` with text "Overview" in admin sidebar |
| 5 | Admin Dashboard → "Session Not Elevated" warning | ~1.35:1 | `div`/`p` containing "Session Not Elevated" |

Save the actual measured ratios to `/tmp/contrast-pre-pr2.txt` for comparison.

The contrast computation logic (paste into the browser dev tools console or a playwright `page.evaluate`):

```javascript
const luminance = ([r,g,b]) => {
  const f = c => { c = c/255; return c <= 0.03928 ? c/12.92 : Math.pow((c+0.055)/1.055, 2.4); };
  return 0.2126*f(r) + 0.7152*f(g) + 0.0722*f(b);
};
const contrast = (a, b) => {
  const l1 = luminance(a), l2 = luminance(b);
  return (Math.max(l1,l2) + 0.05) / (Math.min(l1,l2) + 0.05);
};
const parse = (s) => {
  const m = s.match(/rgba?\(([^)]+)\)/);
  if (m) return m[1].split(',').map(x => parseFloat(x.trim().split('/')[0]));
  return null;
};
const measure = (el) => {
  if (!el) return null;
  const cs = getComputedStyle(el);
  const text = parse(cs.color);
  let bg = parse(cs.backgroundColor);
  let parent = el.parentElement;
  // Walk up if bg is transparent or has alpha to find effective surface
  if (!bg || bg.length < 4 || bg[3] === undefined || bg[3] === 0) {
    while (parent) {
      const pc = parse(getComputedStyle(parent).backgroundColor);
      if (pc && (pc[3] === undefined || pc[3] === 1) && (pc[0] || pc[1] || pc[2])) {
        bg = pc; break;
      }
      parent = parent.parentElement;
    }
  }
  // Alpha-blend bg over its parent if needed
  if (bg && bg[3] !== undefined && bg[3] < 1) {
    let underBg = [0,0,0,1];
    while (parent) {
      const pc = parse(getComputedStyle(parent).backgroundColor);
      if (pc && (pc[3] === undefined || pc[3] === 1)) { underBg = pc; break; }
      parent = parent.parentElement;
    }
    const a = bg[3];
    bg = [bg[0]*a + underBg[0]*(1-a), bg[1]*a + underBg[1]*(1-a), bg[2]*a + underBg[2]*(1-a)];
  }
  return { text: cs.color, bg: cs.backgroundColor, ratio: contrast(text, bg).toFixed(2) };
};
```

- [ ] **Step 3: Stop dev server**

```bash
kill $DEV_PID 2>/dev/null
wait $DEV_PID 2>/dev/null
```

### Task 2.4: Apply the `uno.config.ts` fix

- [ ] **Step 1: Edit `client/uno.config.ts`**

Replace the `colors` object in the `theme:` block (currently lines 30-82). Each tokens that has a `-rgb` variant gets switched to the alpha-aware form. The full new `colors` object:

```ts
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
```

Note specifically:
- `accent.primary-hover` keeps `var(--color-accent-primary-hover)` because it isn't used with alpha modifiers (verified via grep).
- `error.{bg,border,text}` keep legacy form — used as solid only.
- `border.{subtle,default,solid}` keep legacy form — used as solid only.
- `on-accent`, `on-success`, `on-danger` keep legacy form — text color tokens, no alpha use case.

- [ ] **Step 2: Run typecheck and build**

```bash
cd client
bun run build 2>&1 | tee /tmp/build-post-pr2.log | tail -10
cd ..
```

Expected: `✓ built in X.Xs`. Build must succeed.

- [ ] **Step 3: Confirm UnoCSS warning count unchanged**

```bash
grep -c "\[unocss\] unmatched utility" /tmp/build-post-pr2.log
```

Expected: `3` (same pre-existing 3 warnings; no new ones from the config change).

### Task 2.5: Verify the fix programmatically

- [ ] **Step 1: Start dev server with new build**

```bash
cd client && bun run dev 2>&1 &
DEV_PID=$!
sleep 8
cd ..
```

- [ ] **Step 2: Re-measure contrast on the 5 anchor elements**

Use the same script from Task 2.3 step 2. Expected post-fix ratios:

| # | Element | Pre-fix | Post-fix | Target |
|---|---|---|---|---|
| 1 | Settings: My Account selected tab | ~1.74 | ≥7:1 | ≥7:1 (AAA) |
| 2 | Settings: Focused Hybrid card title | ~2.00 | ≥7:1 | ≥7:1 (AAA) |
| 3 | Settings: "Current" pill | ~1.76 | ≥6:1 | ≥4.5:1 (AA, smaller text 10px) |
| 4 | Admin: Overview selected nav | ~1.74 | ≥7:1 | ≥7:1 (AAA) |
| 5 | Admin: "Session Not Elevated" warning | ~1.35 | ≥6:1 | ≥4.5:1 (AA) |

Save measurements to `/tmp/contrast-post-pr2.txt`.

If any of #1, #2, #4 read below 7:1 OR any of #3, #5 read below 4.5:1: STOP. Either the fix didn't apply (re-check uno.config.ts edit), or the alpha is too low for that specific element. In the latter case, the call site's class needs to use a higher alpha (e.g., `/30` instead of `/20`) — fix at the call site, not by reverting the config.

- [ ] **Step 3: Spot-check `@mention` regression**

Navigate to `https://localhost:5173/` (or local dev), open Wolftown #test channel, locate the existing `@Detair` mention, measure its contrast. Expected: ~8.57:1, **unchanged** from pre-fix (uses `color()` syntax with explicit alpha — different code path, not affected).

- [ ] **Step 4: Spot-check 3 direct-style usages**

Navigate to: (1) any admin panel section header (uses `style="background-color: var(--color-surface-layer1)"`), (2) the SessionExpiredModal if reachable, (3) any chat composer area with explicit `--color-surface-layer2` styling.

For each: confirm the rendered background color is unchanged from pre-fix. These usages reference the legacy hex variable which is still defined.

- [ ] **Step 5: Theme switch verification**

In Settings → Appearance, switch to Solarized Dark, then Solarized Light, then Pixel Cozy, then back to Focused Hybrid. For each theme, the selected nav item / active states should now show **soft-tint** (the intended look), not saturated. No visual breakage when switching.

- [ ] **Step 6: Stop dev server**

```bash
kill $DEV_PID 2>/dev/null
wait $DEV_PID 2>/dev/null
```

### Task 2.6: Update CHANGELOG

- [ ] **Step 1: Add `### Fixed` entry under `[Unreleased]` in `CHANGELOG.md`**

```markdown
### Fixed
- UI contrast: selected nav items, active state pills, and warning banners now render with their intended alpha tint instead of silently rendering as solid color. UnoCSS alpha-modifier classes (`bg-accent-primary/20` etc.) had been dropping their alpha because the underlying CSS variables held hex values; switched the affected tokens in `uno.config.ts` to consume new `-rgb` channel variants via the `rgb(var(...) / <alpha-value>)` pattern.
```

### Task 2.7: Commit and open PR

- [ ] **Step 1: Commit**

```bash
git add client/uno.config.ts CHANGELOG.md
git commit -m "$(cat <<'EOF'
fix(client): make UnoCSS alpha modifiers actually apply alpha

Switches affected color tokens in uno.config.ts to the
rgb(var(--color-X-rgb) / <alpha-value>) pattern so UnoCSS classes
like bg-accent-primary/20 render their intended alpha tint instead
of silently rendering as solid color.

Affected tokens: surface.*, text.*, accent.*, status.*, plus the
legacy aliases primary, background.*, success, warning, danger.

Solid-only tokens (error.*, border.*, on-accent, on-success,
on-danger, accent-primary-hover) keep the legacy var() form.

Visual outcome:
- Selected nav contrast 1.74:1 → ~7:1 (Settings + Admin sidebars)
- Yellow warning banner 1.35:1 → ~6.5:1 (admin "Session Not Elevated")
- "Current" session pill 1.76:1 → ~7:1
- @mention pattern unchanged (uses color() syntax, separate path)

Depends on PR fix/theme-rgb-channel-vars (already on main) which
introduced the -rgb variants this PR consumes.

Surfaced by post-deploy e2e contrast audit; design at
docs/superpowers/specs/2026-05-10-ui-contrast-emoji-fixes-design.md.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 2: Push and open PR**

```bash
git push -u origin fix/uno-alpha-modifier
gh pr create --base main --title "fix(client): make UnoCSS alpha modifiers actually apply alpha" --body "$(cat <<'EOF'
## Summary
- Switches \`uno.config.ts\` color bindings for tokens that get used with alpha modifiers from \`var(--color-X)\` to \`rgb(var(--color-X-rgb) / <alpha-value>)\`.
- Fixes ~30+ UI surfaces where \`bg-accent-primary/20\`-style classes were silently rendering as solid color.

## Visual outcome
| Element | Pre | Post |
|---|---|---|
| Settings: selected nav tab | 1.74:1 | ~7:1 |
| Settings: selected theme card | 2.00:1 | ~7:1 |
| Active Sessions: \"Current\" pill | 1.76:1 | ~7:1 |
| Admin: selected nav | 1.74:1 | ~7:1 |
| Admin: \"Session Not Elevated\" banner | 1.35:1 | ~6.5:1 |

## Spec
\`docs/superpowers/specs/2026-05-10-ui-contrast-emoji-fixes-design.md\` (Phase B).

## Test plan
- [ ] \`bun run build\`, \`bun run lint\`, \`bun run test:run\` clean
- [ ] Programmatic contrast audit on the 5 anchor elements — all ≥AA, selected nav ≥AAA
- [ ] @mention contrast unchanged (~8.57:1)
- [ ] Direct \`style=\"background-color: var(--color-X)\"\` usages render unchanged
- [ ] Cycle all 4 themes — each renders soft-tint correctly
- [ ] Manual smoke: home / Wolftown #test / Settings tabs / Admin Dashboard

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

### Task 2.8: Wait for CI, merge, deploy, soak

- [ ] **Step 1: Wait for CI green**

```bash
gh pr checks <PR_NUMBER> --watch
```

- [ ] **Step 2: Merge**

```bash
gh pr merge <PR_NUMBER> --squash --delete-branch
```

- [ ] **Step 3: Clean up worktree**

```bash
cd /home/detair/GIT/detair/kaiku
git worktree remove .claude/worktrees/uno-alpha-fix
git fetch --prune origin
```

- [ ] **Step 4: Deploy to beta**

```bash
./infra/scripts/deploy.sh
```

Expected: `[+] Deploy complete.` Server health check returns OK.

- [ ] **Step 5: Post-deploy programmatic re-verification**

Re-run the contrast audit script against `https://kaiku.pmind.de/` (login required) on the same 5 anchor elements. All ≥4.5:1, with #1/#2/#4 ≥7:1.

- [ ] **Step 6: Soak ≥24h**

Wait at least 24 hours for the visual change to absorb on beta. Watch for any user reports of contrast/readability complaints. If complaints surface, fix at the call site (raise alpha, or switch to dark-text-on-saturated pattern).

---

## Phase 3 — Emoji picker reactivity fix (isolated)

**Branch:** `fix/emoji-picker-reactivity`
**Risk:** None. Three-line change in one file, isolated reactivity bug.
**Files modified:** `client/src/components/emoji/PositionedEmojiPicker.tsx` only.
**Can run in parallel with Phase 1 or Phase 2** — no dependency.

### Task 3.1: Create worktree

- [ ] **Step 1: Create worktree off main**

```bash
cd /home/detair/GIT/detair/kaiku
git worktree add .claude/worktrees/emoji-picker-fix -b fix/emoji-picker-reactivity main
cd .claude/worktrees/emoji-picker-fix
```

### Task 3.2: Reproduce the bug locally

This is the failing-test step. Confirm the picker actually reproduces at viewport `(0, 0)` on this codebase before changing anything — if it doesn't reproduce, the bug may have already been fixed and the change isn't needed.

- [ ] **Step 1: Start dev server**

```bash
cd client && bun install --frozen-lockfile 2>&1 | tail -3
bun run dev 2>&1 &
DEV_PID=$!
sleep 8
cd ..
```

- [ ] **Step 2: Verify reproduction**

Open `http://localhost:5173/`, log in, navigate to any text channel. Click the emoji picker button (smiley icon at right end of composer). Observe: picker should render at viewport `(0, 0)` (top-left corner).

If the picker renders correctly anchored near the trigger: **STOP**. The bug is already fixed by some other change since the e2e audit. Close this branch and skip Phase 3.

- [ ] **Step 3: Stop dev server**

```bash
kill $DEV_PID 2>/dev/null
wait $DEV_PID 2>/dev/null
```

### Task 3.3: Apply the fix

- [ ] **Step 1: Read current code**

```bash
sed -n '105,120p' client/src/components/emoji/PositionedEmojiPicker.tsx
```

Current code captures signal values **before** the JSX:

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
          "z-index": "9999",
          ...(height ? { "max-height": `${height}px` } : {}),
        }}
      >
```

- [ ] **Step 2: Edit `client/src/components/emoji/PositionedEmojiPicker.tsx`**

Remove the two `const` lines and inline the signal accessors inside the JSX `style` prop. The result:

```tsx
  return (
    <Portal>
      <div
        ref={pickerRef}
        style={{
          position: "fixed",
          left: `${position().x}px`,
          top: `${position().y}px`,
          "z-index": "9999",
          ...(maxHeight() ? { "max-height": `${maxHeight()}px` } : {}),
        }}
      >
```

Why this works: SolidJS subscribes the `style` prop to whichever signals are accessed during evaluation. With the captured `const` form, the access happens once at component creation and never again. With the inlined form, each access goes through `position()` / `maxHeight()`, registering the JSX as a dependent of those signals. When `floating-ui` calls `setPosition(...)` later, the JSX re-evaluates the `style` and the picker repositions.

- [ ] **Step 3: Run typecheck**

```bash
cd client
bun run build 2>&1 | tail -10
cd ..
```

Expected: `✓ built in X.Xs`. No new TypeScript errors.

- [ ] **Step 4: Run lint**

```bash
cd client && bun run lint 2>&1 | tail -10 ; cd ..
```

Expected: clean.

### Task 3.4: Verify the fix

- [ ] **Step 1: Start dev server**

```bash
cd client && bun run dev 2>&1 &
DEV_PID=$!
sleep 8
cd ..
```

- [ ] **Step 2: Re-test emoji picker positioning**

Open `http://localhost:5173/`, log in, navigate to any text channel. Click the emoji picker button.

Expected: picker appears anchored ~4px below the trigger button (per `floating-ui`'s `offset(4)` middleware). Should NOT be at viewport `(0, 0)`.

- [ ] **Step 3: Test viewport-edge handling**

Resize the window so the composer is near the bottom, then near the right edge. Open the picker each time. Expected: `floating-ui` `flip` and `shift` middleware reposition the picker to stay in viewport.

- [ ] **Step 4: Test close behaviors**

Open picker, then:
- Press `Escape` → picker closes
- Click outside the picker → picker closes
- Scroll the message list → picker closes (per the `handleScroll` listener)

Each should still work.

- [ ] **Step 5: Stop dev server**

```bash
kill $DEV_PID 2>/dev/null
wait $DEV_PID 2>/dev/null
```

### Task 3.5: Commit and open PR

- [ ] **Step 1: Commit**

```bash
git add client/src/components/emoji/PositionedEmojiPicker.tsx
git commit -m "$(cat <<'EOF'
fix(client): emoji picker positioning at viewport (0,0)

PositionedEmojiPicker was capturing position() and maxHeight()
signals into consts BEFORE the JSX, so SolidJS never subscribed
the style prop to signal updates. floating-ui's computePosition
fired in onMount and called setPosition({x, y}), but the JSX
re-render never happened — the captured {x:0, y:0} stuck.

Inlining the accessors inside the JSX style prop registers the
JSX as a dependent of the signals, so position updates trigger
re-render and the picker anchors correctly to its trigger.

Surfaced by post-deploy e2e walkthrough on kaiku.pmind.de;
design at docs/superpowers/specs/2026-05-10-ui-contrast-emoji-fixes-design.md.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 2: Push and open PR**

```bash
git push -u origin fix/emoji-picker-reactivity
gh pr create --base main --title "fix(client): emoji picker positioning at viewport (0,0)" --body "$(cat <<'EOF'
## Summary
- 3-line change to \`PositionedEmojiPicker.tsx\`: inline the SolidJS signal accessors inside the JSX \`style\` prop instead of capturing them into consts before the return.
- Fixes the picker rendering at viewport \`(0, 0)\` regardless of trigger location.

## Why
Capturing \`position()\` and \`maxHeight()\` into \`const\`s outside the JSX defeats SolidJS reactivity — the \`style\` prop never subscribes to the signals, so when \`floating-ui\` produces real coordinates the picker doesn't re-render.

## Spec
\`docs/superpowers/specs/2026-05-10-ui-contrast-emoji-fixes-design.md\` (Phase C).

## Test plan
- [ ] \`bun run build\` clean
- [ ] \`bun run lint\` clean
- [ ] Manual: open emoji picker on a text channel — appears anchored to trigger, not at (0, 0)
- [ ] Manual: viewport-edge handling (composer near bottom/right) — picker flips/shifts to stay in view
- [ ] Manual: close behaviors — Escape, click-outside, scroll all still close picker

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

### Task 3.6: Wait for CI, merge, clean up

- [ ] **Step 1: Wait for CI green**

```bash
gh pr checks <PR_NUMBER> --watch
```

- [ ] **Step 2: Merge**

```bash
gh pr merge <PR_NUMBER> --squash --delete-branch
```

- [ ] **Step 3: Clean up worktree**

```bash
cd /home/detair/GIT/detair/kaiku
git worktree remove .claude/worktrees/emoji-picker-fix
git fetch --prune origin
```

- [ ] **Step 4: Deploy to beta** (if not already covered by Phase 2 deploy)

```bash
./infra/scripts/deploy.sh --client-only
```

The picker fix is client-only — no server image change needed.

---

## Done when

All three PRs merged on `main`:

- [ ] PR 1 (`fix/theme-rgb-channel-vars`) merged.
- [ ] PR 3 (`fix/emoji-picker-reactivity`) merged.
- [ ] PR 2 (`fix/uno-alpha-modifier`) merged.
- [ ] Beta `kaiku.pmind.de` deployed with all three fixes.
- [ ] Post-deploy contrast audit on the 5 anchor elements: ≥4.5:1 each (≥7:1 for #1, #2, #4).
- [ ] Manual emoji picker test on beta: opens anchored to trigger.
- [ ] 24h soak post-PR2-deploy with no new contrast/readability complaints.

The design worktree (`.claude/worktrees/ui-contrast-emoji-design/`) and the `docs/ui-contrast-emoji-fixes-design` branch can be cleaned up after the design and plan docs are merged via their own PR.

## When to abandon a phase mid-flight

- **Phase 1:** Don't abandon — purely additive, can always proceed even if the eventual visual change is rejected.
- **Phase 2:** If post-fix contrast still reads <4.5:1 on any anchor element, that means the alpha for that specific class is too low. Don't abandon the PR — fix the call site (raise alpha or switch to dark-on-saturated pattern), then continue.
- **Phase 3:** If the bug doesn't reproduce locally before the change, abandon. Someone else fixed it in a different PR.

## Plan-wide rollback discipline

Each PR is a single commit (squash merge). Reverting a PR is one command: `gh pr revert <PR_NUMBER>`.

- Reverting Phase 1: harmless — drops unused variables. No impact.
- Reverting Phase 2: restores legacy `var(--color-X)` form. App returns to saturated-tint look. Phase 1's `-rgb` variables remain on `main` (harmless, unused).
- Reverting Phase 3: restores broken positioning. No data loss.
