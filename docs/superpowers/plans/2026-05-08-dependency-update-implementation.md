# Dependency Update Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land all 10 phases of the dependency-update sweep documented in [`docs/superpowers/specs/2026-05-08-dependency-update-design.md`](../specs/2026-05-08-dependency-update-design.md), eliminating `cargo audit` and `bun audit` advisories without breaking the beta deployment (`kaiku.pmind.de`) or the desktop client's golden paths.

**Architecture:** One PR per phase (sub-letters where the spec splits a phase). Phases ship sequentially with a 24-hour beta-soak window between merges. Phase 0 has already merged as PR #548.

**Tech Stack:** Cargo workspace (Rust), Bun + npm registry (frontend), `cargo-audit`, `cargo-deny`, OSV-Scanner, vitest, Playwright, Tauri 2.x.

**Spec:** [`docs/superpowers/specs/2026-05-08-dependency-update-design.md`](../specs/2026-05-08-dependency-update-design.md)

---

## Status (as of 2026-05-10)

This document is the original plan, not a live tracker. Phase status reflects what has shipped to `main`:

| Phase | Status | PR |
|---|---|---|
| 0  | Shipped 2026-05-08 | [#548](https://github.com/Detair/kaiku/pull/548) |
| 1  | Shipped 2026-05-08 | [#549](https://github.com/Detair/kaiku/pull/549) |
| 2  | Shipped 2026-05-08 | [#550](https://github.com/Detair/kaiku/pull/550) |
| 3  | Shipped 2026-05-08 | [#551](https://github.com/Detair/kaiku/pull/551) |
| 4a | Shipped 2026-05-09 | [#552](https://github.com/Detair/kaiku/pull/552) |
| 4b | Abandoned (upstream-blocked) | — |
| 4c | Shipped 2026-05-09 | [#553](https://github.com/Detair/kaiku/pull/553) |
| 6a | Shipped 2026-05-09 | [#554](https://github.com/Detair/kaiku/pull/554) |
| 6b, 6c, 6d, 7, 8, 9, 10 | Pending | — |

Per-phase narration below is preserved as-written (TODO checkboxes, "create worktree" steps, etc.) so the historical reasoning stays intact. For execution of remaining phases, re-query versions and re-read the spec first — the registry has moved since 2026-05-08.

---

## How to use this plan

- **Plan owner workflow:** read the relevant phase, dispatch a subagent (or work inline) using the bite-sized tasks below.
- **Source of truth for *why*:** the spec. This plan only encodes the *how* (commands, edits, verifications). When the spec and the plan disagree, the spec wins — re-read it.
- **Re-query versions before each PR.** The spec was written 2026-05-08; the verified versions inside it are baseline. At PR-creation time, re-query `https://crates.io/api/v1/crates/<name>` and `https://registry.npmjs.org/<pkg>/latest` and pin to whatever the registry returns then. Treat any version below the spec's table as a regression and ask before pinning.
- **Branch naming, worktree convention, merge strategy** all follow `CLAUDE.md` (squash-merge via `gh pr merge <N> --squash --delete-branch`; worktrees under `.claude/worktrees/<name>` with the recipe `git worktree remove .claude/worktrees/<name> && git branch -d <branch> && git fetch --prune` after merge).
- **Soak window:** after merging Phase N to `main`, wait at least 24 hours (or until the next reasonable check) for the beta deploy on `kaiku.pmind.de` to take the new build before opening Phase N+1's PR. Phase 0 doesn't soak — already done. Phase 8 has an additional 30-minute multi-client canary on top of the soak.

## Common quality gates (every phase)

These run before merge on every phase. They are the implicit floor for "phase done", not listed in every task:

1. `cargo fmt --check` clean
2. `SQLX_OFFLINE=true cargo clippy --workspace -- -D warnings` clean
3. `cargo test --workspace` green
4. `cargo deny check` green (advisories + licenses + bans + sources)
5. `cargo audit` — error count monotone-non-increasing vs. prior phase; no new direct-dep advisories
6. Frontend phases: `bun run lint`, `bun run test:run`, `bun run build` all green
7. CI on the PR — every required check (CI, Tauri Build, Android Build, Frontend, Rust Lint, License Compliance, Docs Governance, Observability Contract, Secrets Scan) green

If any gate fails, fix it in the same PR. Do not silently disable rules, expand the ignore lists, or relax `tsconfig` strictness — if a rule must go, the commit message says why.

## Changelog discipline

Per `CLAUDE.md`, every user-relevant change is recorded under `[Unreleased]` in `CHANGELOG.md`. Pure dep bumps **without behavioural change** do not require a changelog entry. The exceptions in this plan are:

- Phase 2 (frontend security advisories cleared) → `### Security` entry.
- Phase 7 (vodozemac 0.10) → `### Changed` if E2EE behaviour differs perceptibly, else `### Security`.
- Phase 8 (webrtc stack) → `### Changed` for any user-visible voice/screenshare differences (e.g. PLI workaround removed).
- Phase 10 (cleanup) → no entry.

When in doubt, write the entry; an unnecessary changelog line is reverted in seconds, a missing one disappears into git history.

---

## Phase 0 — DONE

Merged as **PR #548** (`39aadef9`) on 2026-05-08.

`aws-sdk-s3` and `aws-config` switched from feature `rustls` (legacy `aws-smithy-runtime/tls-rustls` → `legacy-rustls-ring` chain on rustls 0.21.12 + rustls-webpki 0.101.7) to feature `default-https-client` (modern `rustls-aws-lc` chain on rustls 0.23.x). RUSTSEC-2026-0098, RUSTSEC-2026-0099, RUSTSEC-2026-0104 removed from `deny.toml`.

No further action.

---

## Phase 1 — Within-bound housekeeping

**Branch:** `chore/deps-within-bound`
**Risk:** Low. Patch-level movement only.
**Pre-flight gate:** none beyond version re-query.

### Task 1.1: Re-query latest versions

Bumps in this phase from the spec inventory: `cargo update` rolls everything in-bound; `bun update` (no `--latest`) picks up:

- `@playwright/test ^1.58.2 → ^1.59.1`
- `@tanstack/solid-virtual ^3.13.23 → ^3.13.24`
- `@unocss/preset-icons` / `preset-uno` / `reset` `^66.6.6 → ^66.6.8`
- `mermaid ^11.13.0 → ^11.14.0`
- `prettier ^3.8.1 → ^3.8.3`
- `solid-js ^1.9.11 → ^1.9.12`
- `typescript-eslint ^8.57.1 → ^8.59.2`
- `vite ^8.0.0 → ^8.0.11`
- `vite-plugin-solid ^2.11.11 → ^2.11.12`
- `vitest ^4.1.0 → ^4.1.5`

Override bumps (existing block):

- `rollup ^4.60.1 → ^4.60.3`
- `lodash-es ^4.17.24 → ^4.18.1`
- `defu ^6.1.5 → ^6.1.7`
- `picomatch` stays at `^4.0.4`
- `brace-expansion` `^1.1.13 → ^1.1.13 || ^5.0.6` (range union; do not drop the 1.x bound)

- [ ] **Step 1: Re-query each version above**

```bash
for pkg in @playwright/test @tanstack/solid-virtual @unocss/preset-icons mermaid prettier solid-js typescript-eslint vite vite-plugin-solid vitest rollup lodash-es defu; do
  printf '%-30s ' "$pkg"
  curl -sf "https://registry.npmjs.org/$pkg/latest" | python3 -c "import json,sys;print(json.load(sys.stdin)['version'])"
done
```

Expected: every value ≥ the spec's "Latest (2026-05-08)". Any value below the spec → stop and ask before proceeding.

### Task 1.2: Create worktree

- [ ] **Step 1: Create the worktree**

```bash
git worktree add .claude/worktrees/deps-within-bound -b chore/deps-within-bound
cd .claude/worktrees/deps-within-bound
```

Expected: worktree created on a fresh branch off `main`.

### Task 1.3: Refresh Rust lockfile

- [ ] **Step 1: Run `cargo update --workspace`**

```bash
cargo update --workspace 2>&1 | tail -30
```

Expected: 0 or more in-bound updates. Any "removed" / "added" line for a *direct* dep (not transitive) is unexpected and should be investigated before continuing.

### Task 1.4: Refresh frontend lockfile

- [ ] **Step 1: Run `bun update`**

```bash
cd client
bun update 2>&1 | tail -30
cd ..
```

Expected: `client/bun.lock` updated; `client/package.json` `^` ranges may move but no major-version changes. If a `^` range did change, that is a breaking-version movement — stop and reclassify the bump into the appropriate later phase.

### Task 1.5: Update existing overrides in `client/package.json`

- [ ] **Step 1: Edit `client/package.json` overrides block**

Replace the existing block (lines 61–68 today):

```json
  "overrides": {
    "picomatch": "^4.0.4",
    "rollup": "^4.60.1",
    "flatted": "^3.4.2",
    "brace-expansion": "^1.1.13",
    "lodash-es": "^4.17.24",
    "defu": "^6.1.5"
  }
```

with:

```json
  "overrides": {
    "picomatch": "^4.0.4",
    "rollup": "^4.60.3",
    "flatted": "^3.4.2",
    "brace-expansion": "^1.1.13 || ^5.0.6",
    "lodash-es": "^4.18.1",
    "defu": "^6.1.7"
  }
```

- [ ] **Step 2: Re-resolve the lockfile**

```bash
cd client
bun install 2>&1 | tail -10
cd ..
```

Expected: `bun install` succeeds; `client/bun.lock` regenerates with the override pins.

### Task 1.6: Verify advisory delta

- [ ] **Step 1: Run `bun audit`**

```bash
cd client
bun audit 2>&1 | tail -40
cd ..
```

Expected: 5 of the prior 12 advisories cleared (the ones tied to the override bumps — typically the dev-time `flatted`/`lodash-es` paths and `rollup` transitive vuln chain). The remaining 7 (dompurify ×4, postcss, uuid, vite-nested) move to Phase 2.

- [ ] **Step 2: Run `cargo audit` and confirm error count is unchanged**

```bash
cargo audit --ignore RUSTSEC-2025-0008 --ignore RUSTSEC-2023-0071 --ignore RUSTSEC-2026-0002 --ignore RUSTSEC-2026-0009 2>&1 | tail -5
echo "EXIT=$?"
```

Expected: `EXIT=0`. Phase 1 doesn't touch Rust crates beyond `cargo update --workspace`, so the error count must be identical to Phase 0's exit state.

### Task 1.7: Quality gates

- [ ] **Step 1: Run frontend gates**

```bash
cd client && bun run lint && bun run test:run && bun run build && cd ..
```

Expected: all three green. If `bun run build` produces a TypeScript diagnostic, that diagnostic is pre-existing — capture it in the PR description; do not silently fix it (TypeScript major bump is Phase 6b).

- [ ] **Step 2: Run Rust gates**

```bash
cargo fmt --check && \
SQLX_OFFLINE=true cargo clippy --workspace -- -D warnings && \
cargo test --workspace
```

Expected: all green.

- [ ] **Step 3: Run cargo-deny**

```bash
cargo deny check 2>&1 | tail -10
```

Expected: `advisories ok`, `licenses ok`, `bans ok`, `sources ok`.

### Task 1.8: Commit and open PR

- [ ] **Step 1: Commit**

```bash
git add Cargo.lock client/bun.lock client/package.json
git commit -m "$(cat <<'EOF'
chore(infra): refresh in-bound dep versions and override pins

Phase 1 of the dep-update sweep (spec:
docs/superpowers/specs/2026-05-08-dependency-update-design.md). No
direct-dep semver edits — only `cargo update --workspace`, `bun update`,
and override pin bumps.

- Cargo.lock: refreshed within existing semver bounds
- bun.lock: refreshed within existing semver bounds
- client/package.json overrides:
  - rollup ^4.60.1 → ^4.60.3
  - lodash-es ^4.17.24 → ^4.18.1
  - defu ^6.1.5 → ^6.1.7
  - brace-expansion ^1.1.13 → ^1.1.13 || ^5.0.6 (range union)

Clears 5 of the 12 bun-audit advisories.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 2: Push and open PR**

```bash
git push -u origin chore/deps-within-bound
gh pr create --title "chore(infra): Phase 1 — refresh in-bound dep versions and override pins" --body "$(cat <<'EOF'
## Summary
- Phase 1 of dep-update sweep ([spec](docs/superpowers/specs/2026-05-08-dependency-update-design.md)).
- `cargo update --workspace` + `bun update` (no `--latest`) — patch-level movement only.
- 4 override pins bumped; `brace-expansion` widened to range union to keep 1.x consumers resolvable.

## Test plan
- [x] `cargo fmt --check`
- [x] `SQLX_OFFLINE=true cargo clippy --workspace -- -D warnings`
- [x] `cargo test --workspace`
- [x] `cargo deny check`
- [x] `cargo audit` — error count unchanged from Phase 0
- [x] `bun run lint && bun run test:run && bun run build`
- [x] `bun audit` — 5 advisories cleared, 7 remain (Phase 2 scope)

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 3: Watch CI to green, then squash-merge and clean up**

```bash
gh pr checks <N> --watch --fail-fast=false
gh pr merge <N> --squash --delete-branch
cd /home/detair/GIT/detair/kaiku
git worktree remove .claude/worktrees/deps-within-bound
git branch -D chore/deps-within-bound
git fetch --prune
```

### Phase 1 → Phase 2 soak

Wait ≥24 h. The beta deploy on `kaiku.pmind.de` should take the new server image and the next client build before Phase 2 opens. Frontend is the rebuilt-on-VPS path; verify at least one client build has completed since merge.

---

## Phase 2 — Frontend security advisories

**Branch:** `fix/frontend-security-deps`
**Risk:** Low–Medium. Override pins on shared transitives can break a dev consumer.
**Pre-flight gate:** confirm overrides resolve cleanly.

### Task 2.1: Pre-flight — verify overrides resolve

- [ ] **Step 1: Re-query target versions**

```bash
for pkg in dompurify @sentry/browser @vitejs/plugin-basic-ssl postcss uuid; do
  printf '%-30s ' "$pkg"
  curl -sf "https://registry.npmjs.org/$pkg/latest" | python3 -c "import json,sys;print(json.load(sys.stdin)['version'])"
done
```

Expected: dompurify ≥ 3.4.2, @sentry/browser ≥ 10.52.0, @vitejs/plugin-basic-ssl ≥ 2.3.0, postcss ≥ 8.5.10, uuid ≥ 11.1.1.

### Task 2.2: Create worktree

- [ ] **Step 1: Create worktree**

```bash
git worktree add .claude/worktrees/frontend-security-deps -b fix/frontend-security-deps
cd .claude/worktrees/frontend-security-deps
```

### Task 2.3: Bump direct deps

- [ ] **Step 1: Edit `client/package.json` direct deps**

In `client/package.json`:

- `"dompurify": "^3.3.3"` → `"dompurify": "^3.4.2"`
- `"@sentry/browser": "^10.44.0"` → `"@sentry/browser": "^10.52.0"`
- `"@vitejs/plugin-basic-ssl": "^2.2.0"` → `"@vitejs/plugin-basic-ssl": "^2.3.0"`

Pin to whatever Task 2.1 returned if newer.

### Task 2.4: Add new overrides

- [ ] **Step 1: Edit `client/package.json` overrides block**

Add four entries (vite, postcss, uuid, dompurify), keep existing five:

```json
  "overrides": {
    "picomatch": "^4.0.4",
    "rollup": "^4.60.3",
    "flatted": "^3.4.2",
    "brace-expansion": "^1.1.13 || ^5.0.6",
    "lodash-es": "^4.18.1",
    "defu": "^6.1.7",
    "vite": "^8.0.0",
    "postcss": "^8.5.10",
    "uuid": "^11.1.1",
    "dompurify": "^3.4.2"
  }
```

### Task 2.5: Resolve and verify lockfile

- [ ] **Step 1: Re-resolve**

```bash
cd client
bun install 2>&1 | tail -10
```

- [ ] **Step 2: Confirm `vite@7.x` is gone**

```bash
bun pm ls --all | grep "vite@"
```

Expected: zero `vite@7.x` entries; only `vite@8.x` (the override target).

- [ ] **Step 3: Confirm mermaid's nested `dompurify` followed**

```bash
bun pm ls --all | grep dompurify
```

Expected: every entry on `^3.4.2` or higher.

### Task 2.6: Verify advisory clear

- [ ] **Step 1: Run `bun audit`**

```bash
bun audit 2>&1 | tail -20
```

Expected: 0 advisories (the 7 remaining from Phase 1 all clear).

- [ ] **Step 2: Run frontend gates**

```bash
bun run lint && bun run test:run && bun run build
cd ..
```

Expected: all green. Vite 8.x override is harmless because every plugin's peer range admits 8.

### Task 2.7: Update changelog

- [ ] **Step 1: Add `### Security` entry under `[Unreleased]` in `CHANGELOG.md`**

```markdown
- Frontend: cleared all 12 bun-audit advisories (dompurify XSS ×4, postcss XSS, uuid buffer, vite dev-server ×3 paths, lodash transitive ×2, flatted) by bumping direct deps and pinning transitive overrides
```

### Task 2.8: Commit and open PR

- [ ] **Step 1: Commit**

```bash
git add CHANGELOG.md client/bun.lock client/package.json
git commit -m "$(cat <<'EOF'
fix(client): clear all bun-audit advisories via direct bumps + overrides

Phase 2 of the dep-update sweep. Bumps dompurify 3.3.3 → 3.4.2,
@sentry/browser 10.44.0 → 10.52.0, @vitejs/plugin-basic-ssl 2.2.0 →
2.3.0; adds overrides for vite, postcss, uuid, dompurify to ensure
nested copies follow direct deps.

Clears the 7 advisories not addressed in Phase 1 (dompurify ×4, postcss,
uuid, vite-nested). bun audit now reports 0 advisories.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 2: Push, PR, watch, merge, clean up**

(Same recipe as Task 1.8 Steps 2–3, with the `fix/frontend-security-deps` branch.)

### Phase 2 → Phase 3 soak

Wait ≥24 h.

---

## Phase 3 — Tauri 2.10 → 2.11 alignment

**Branch:** `chore/tauri-2.11-bump`
**Risk:** Medium. Tauri 2.11 is a minor with limited surface, but IPC/plugin behaviour can shift subtly.
**Pre-flight gate:** read Tauri 2.11 release notes for breaking changes in plugin-shell, plugin-notification, plugin-global-shortcut.

### Task 3.1: Pre-flight — read 2.11 release notes

- [ ] **Step 1: Fetch Tauri 2.11 release notes**

Browse https://github.com/tauri-apps/tauri/releases (filter to v2.11.0+). Note any breaking change in:

- The `invoke` IPC contract (return-type shape, error-shape, transport)
- `plugin-shell` (we use the "open in browser" path)
- `plugin-notification` (we use the permission prompt + send)
- `plugin-global-shortcut` (we register PTT and other shortcuts)

If any breaking change touches our usage, document the migration in the commit message before bumping.

### Task 3.2: Create worktree

- [ ] **Step 1**

```bash
git worktree add .claude/worktrees/tauri-2.11-bump -b chore/tauri-2.11-bump
cd .claude/worktrees/tauri-2.11-bump
```

### Task 3.3: Bump Rust Tauri ecosystem

- [ ] **Step 1: Run targeted `cargo update`**

```bash
cargo update -p tauri -p tauri-build -p tauri-plugin-shell -p tauri-plugin-notification -p tauri-plugin-global-shortcut 2>&1 | tail -20
```

Expected: all five crates jump to 2.11.x.

### Task 3.4: Bump npm Tauri packages

- [ ] **Step 1: Edit `client/package.json`**

- `"@tauri-apps/api": "^2.10.1"` → `"@tauri-apps/api": "^2.11.0"`
- `"@tauri-apps/cli": "^2.10.1"` → `"@tauri-apps/cli": "^2.11.1"`

- [ ] **Step 2: Re-resolve**

```bash
cd client && bun install && cd ..
```

### Task 3.5: Build both halves

- [ ] **Step 1: Frontend build**

```bash
cd client && bun run build && cd ..
```

Expected: `tsc` + `vite build` succeed.

- [ ] **Step 2: Native client build**

```bash
cargo build --release -p vc-client 2>&1 | tail -20
```

Expected: succeeds. Long build (~10 min). Any compile error is a 2.11 surface change — stop and document.

### Task 3.6: Manual smoke test

- [ ] **Step 1: Launch dev client and verify four IPC paths**

```bash
cd client && bun run tauri dev
```

In the running client:

1. Log in to the dev server.
2. Trigger an `invoke<T>` call (e.g. switch to a channel — exercises `get_current_user`).
3. Trigger plugin-shell — click an "open in browser" link from a markdown message.
4. Trigger plugin-notification — generate an incoming DM in a background channel and confirm the OS notification fires.
5. Trigger plugin-global-shortcut — press the configured PTT key with the client unfocused.

Expected: each behaves identically to the 2.10 baseline. If any fails, halt and revert.

### Task 3.7: Quality gates and commit

- [ ] **Step 1: Common gates**

(`cargo fmt --check`, `cargo clippy`, `cargo test --workspace`, `cargo deny check`, `cargo audit`, `bun run lint`, `bun run test:run`.)

- [ ] **Step 2: Commit**

```bash
git add Cargo.lock client/bun.lock client/package.json
git commit -m "$(cat <<'EOF'
chore(client): bump Tauri ecosystem 2.10 → 2.11

Phase 3 of the dep-update sweep. Aligned bump of Rust crates
(tauri, tauri-build, tauri-plugin-shell, tauri-plugin-notification,
tauri-plugin-global-shortcut) and npm packages (@tauri-apps/api,
@tauri-apps/cli) so wire and IPC contracts stay matched.

Smoke-tested IPC, plugin-shell, plugin-notification, and
plugin-global-shortcut on a dev build; no behavioural delta.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 3: Push, PR, watch, merge, clean up.**

### Phase 3 → Phase 4a soak

Wait ≥24 h.

---

## Phase 4a — Rust low-risk minors batch

**Branch:** `chore/rust-minors-batch`
**Risk:** Medium. Eight pre-1.0 / minor bumps in one PR. Each is small but the surface adds up.
**Pre-flight gate:** none beyond version re-query.

### Task 4a.1: Re-query and create worktree

- [ ] **Step 1: Re-query**

```bash
for crate in axum-tracing-opentelemetry init-tracing-opentelemetry infer sentry sentry-tracing smol_str sysinfo thiserror tokio-tungstenite; do
  printf '%-30s ' "$crate"
  curl -sf "https://crates.io/api/v1/crates/$crate" | python3 -c "import json,sys;print(json.load(sys.stdin)['crate']['max_stable_version'])"
done
```

- [ ] **Step 2: Create worktree**

```bash
git worktree add .claude/worktrees/rust-minors-batch -b chore/rust-minors-batch
cd .claude/worktrees/rust-minors-batch
```

### Task 4a.2: Edit workspace `Cargo.toml`

- [ ] **Step 1: Apply these line-level changes in the root `Cargo.toml`'s `[workspace.dependencies]`**

| Line | From | To |
|---|---|---|
| `axum-tracing-opentelemetry = ...` | `0.32` | `0.33` (or 0.33.1) |
| `init-tracing-opentelemetry = ...` | `0.36` | `0.37` |
| `infer = ...` | `0.16` | `0.19` |
| `smol_str = ...` | `0.2` | `0.3` |
| `sysinfo = ...` | `0.38` | `0.39` |
| `tokio-tungstenite = ...` | `0.28` | `0.29` |

(Keep feature lists unchanged.)

- [ ] **Step 2: Hoist `tempfile` to `[workspace.dependencies]`**

Add to the root `Cargo.toml` `[workspace.dependencies]`:

```toml
tempfile = "3"
```

Replace the literal `tempfile = "3"` declaration in `server/Cargo.toml` (main deps) with `tempfile.workspace = true`. Same change for any `tempfile = "3"` occurrence in `client/src-tauri/Cargo.toml` (likely `[dev-dependencies]`).

### Task 4a.3: Bump sentry on the client

- [ ] **Step 1: Edit `client/src-tauri/Cargo.toml`**

- `sentry = { version = "0.47", features = ["tracing", "backtrace", "contexts", "panic"] }` → `sentry = { version = "0.48", features = ["tracing", "backtrace", "contexts", "panic"] }`
- `sentry-tracing = "0.47"` → `sentry-tracing = "0.48"`

### Task 4a.4: Bump thiserror in vp8-decoder

- [ ] **Step 1: Edit `client/src-tauri/vp8-decoder/Cargo.toml`**

Replace the literal `thiserror = "1"` with `thiserror.workspace = true`. The workspace already pins `thiserror = "2"`, so this is a 1 → 2 bump for that one sub-crate.

### Task 4a.5: Resolve, fix, verify per-bump

- [ ] **Step 1: `cargo update`**

```bash
cargo update --workspace 2>&1 | tail -20
```

- [ ] **Step 2: Fix `smol_str` `From<&str>` call sites**

The `From<&str>` route changed in 0.3. Find call sites:

```bash
grep -rn "SmolStr::from\|: SmolStr =\|smol_str::SmolStr" server/src client/src-tauri/src shared/ 2>/dev/null | head -30
```

For each call site, replace any direct `&str → SmolStr` coercion with the explicit `SmolStr::new(...)` constructor where the implicit `From` no longer compiles.

- [ ] **Step 3: Verify `infer` MIME paths**

`infer 0.16 → 0.19` updated the signature database. Find usage:

```bash
grep -rn "infer::" server/src 2>/dev/null
```

Inspect each call site that maps the returned MIME to one of our handlers' expected types. Run the upload tests:

```bash
SQLX_OFFLINE=true cargo test -p vc-server --test '*upload*' 2>&1 | tail -30
```

Expected: pass.

- [ ] **Step 4: Verify `tokio-tungstenite` `MaybeTlsStream` usage**

The variant set widened in 0.29. Find uses:

```bash
grep -rn "MaybeTlsStream" server/src client/src-tauri/src 2>/dev/null
```

Add an explicit catch-all `_ => ...` arm where any `match` on `MaybeTlsStream` would now be non-exhaustive.

- [ ] **Step 5: Verify `sentry` 0.48 client integration**

The `tower` integration moved out of the `sentry` crate in 0.48. Confirm we don't use it:

```bash
grep -rn "sentry::tower\|sentry_tower" client/src-tauri/src 2>/dev/null
```

Expected: zero hits (we use `sentry-tracing`, not the tower middleware).

- [ ] **Step 6: Run server + client builds**

```bash
SQLX_OFFLINE=true cargo build -p vc-server 2>&1 | tail -10
cargo build -p vc-client 2>&1 | tail -10
cargo build -p vp8-decoder 2>&1 | tail -10
```

Expected: all three succeed.

### Task 4a.6: Quality gates and commit

- [ ] **Step 1: Common gates** (fmt, clippy, test, deny, audit, frontend lint/test/build).

- [ ] **Step 2: Commit**

```bash
git add Cargo.lock Cargo.toml server/Cargo.toml client/src-tauri/Cargo.toml client/src-tauri/vp8-decoder/Cargo.toml
git commit -m "$(cat <<'EOF'
chore(infra): batch bump Rust low-risk minors (Phase 4a)

axum-tracing-opentelemetry 0.32 → 0.33
init-tracing-opentelemetry 0.36 → 0.37
infer 0.16 → 0.19 (MIME db updated; upload tests pass)
sentry, sentry-tracing 0.47 → 0.48 (Tauri client only)
smol_str 0.2 → 0.3 (From<&str> migration in N call sites)
sysinfo 0.38 → 0.39
thiserror 1 → 2 (vp8-decoder; aligned with workspace)
tokio-tungstenite 0.28 → 0.29 (MaybeTlsStream variant widening)
tempfile hoisted to workspace.dependencies (was duplicated)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 3: Push, PR, watch, merge, clean up.**

### Phase 4a → Phase 4b soak

Wait ≥24 h.

---

## Phase 4b — `reqwest 0.12 → 0.13` (server)

**Branch:** `refactor/reqwest-0.13-server`
**Risk:** Medium. TLS feature surface reorganized; default chain may shift.
**Pre-flight gate:** read reqwest 0.13 migration notes; identify TLS feature renames.

### Task 4b.1: Pre-flight — TLS feature audit

- [ ] **Step 1: Read reqwest 0.13 release notes**

Skim https://github.com/seanmonstar/reqwest/blob/master/CHANGELOG.md from 0.12.x→0.13.0. Identify:

- Renamed/removed TLS features (`native-tls` / `rustls-tls` / `rustls-tls-native-roots` etc.)
- Default-feature changes
- Connector / pool API changes

- [ ] **Step 2: Audit current usage**

```bash
grep -rn "reqwest::Client\|ClientBuilder\|reqwest::tls" server/src 2>/dev/null
```

Note every construction site and feature it relies on.

### Task 4b.2: Create worktree

```bash
git worktree add .claude/worktrees/reqwest-0.13-server -b refactor/reqwest-0.13-server
cd .claude/worktrees/reqwest-0.13-server
```

### Task 4b.3: Bump and resolve

- [ ] **Step 1: Edit `server/Cargo.toml`**

Find the `reqwest = { version = "0.12", features = [...] }` line. Bump to `"0.13"`. If the spec's pre-flight identified renamed features, update the feature list inline.

- [ ] **Step 2: `cargo update`**

```bash
cargo update -p reqwest 2>&1 | tail
SQLX_OFFLINE=true cargo build -p vc-server 2>&1 | tail -30
```

Resolve compile errors at each `Client::builder()` / TLS configuration site.

### Task 4b.4: Targeted tests

- [ ] **Step 1: HTTP-using server tests**

Run the integration tests that exercise reqwest paths:

```bash
SQLX_OFFLINE=true cargo test -p vc-server --test '*lettre*' --test '*oidc*' --test '*otlp*' --test '*s3*' 2>&1 | tail -30
```

Expected: green. If any test fails, the failure is the TLS / Client surface drift — fix in this PR.

- [ ] **Step 2: License delta**

```bash
cargo deny check licenses 2>&1 | tail -10
```

Expected: green. reqwest 0.13 may pull a different default TLS chain; if a new license appears, escalate.

### Task 4b.5: Commit and open PR

(Standard commit + push + PR + merge + cleanup recipe; commit message describes the TLS feature migration if any.)

### Phase 4b → Phase 4c soak

Wait ≥24 h.

---

## Phase 4c — `zip 2 → 8` (server)

**Branch:** `refactor/zip-8-server`
**Risk:** Medium-High. The crate was rewritten between 2.x and 3.x; APIs continued to evolve.
**Pre-flight gate:** read zip 3, 4, 5, 6, 7, 8 release notes; map current call sites to 8.x equivalents.

### Task 4c.1: Pre-flight — API audit

- [ ] **Step 1: Find current zip usage**

```bash
grep -rn "zip::\|ZipWriter\|ZipArchive\|FileOptions" server/src 2>/dev/null
```

Document each call site: which methods (`start_file`, `finish`, `write_all`, `extract`, `read_to_end`, `FileOptions` builder), and any feature flags consumed.

- [ ] **Step 2: Read release notes**

`zip` major notes: https://github.com/zip-rs/zip2/releases (zip 3+) plus the older zip 2.x→3.x migration guide (typically pinned to the README of zip 3.0). For each method identified above, confirm the 8.x signature.

### Task 4c.2: Create worktree

```bash
git worktree add .claude/worktrees/zip-8-server -b refactor/zip-8-server
cd .claude/worktrees/zip-8-server
```

### Task 4c.3: Bump and migrate

- [ ] **Step 1: Edit root `Cargo.toml` `[workspace.dependencies]`**

`zip = { version = "2", default-features = false, features = ["deflate"] }` → `zip = { version = "8", default-features = false, features = ["deflate"] }`

- [ ] **Step 2: Resolve and fix call sites**

```bash
cargo update -p zip 2>&1 | tail
SQLX_OFFLINE=true cargo build -p vc-server 2>&1 | tail -30
```

For each compile error, port the call to the 8.x API. Likely:

- `FileOptions::default()` → may now require a generic parameter for `<NoCompression>` or `<DEFLATE>`.
- `ZipWriter::start_file(name, options)` → method may now take `name: impl Into<...>` differently.
- `finish()` → may now consume self instead of `&mut self`.

Resolve incrementally; don't change behavior beyond the API rename.

### Task 4c.4: Verify archive output compatibility

- [ ] **Step 1: Server media-path tests**

```bash
SQLX_OFFLINE=true cargo test -p vc-server --test '*export*' --test '*archive*' --test '*media*' 2>&1 | tail -30
```

- [ ] **Step 2: Manual round-trip check**

If the server has a "download channel as zip" or "export user data" endpoint, exercise it on a dev server and confirm the produced archive opens in `unzip`/`bsdtar`. If not, write a focused test that:

1. Constructs a `ZipWriter` with the production `FileOptions`.
2. Adds a file with known bytes.
3. Closes and reads the buffer back through `ZipArchive`.
4. Asserts the bytes round-trip.

### Task 4c.5: Commit and open PR

(Standard recipe.)

### Phase 4c → Phase 6a soak

Wait ≥24 h. (Phase 5 was retired in spec; do not produce a Phase 5.)

---

## Phase 6a — Lint + Type-checker tooling (ESLint 10)

**Branch:** `chore/eslint-10-bump`
**Risk:** Medium. ESLint 10 may rename or remove rules.
**Pre-flight gate:** capture baseline `bun run lint` diagnostic count.

### Task 6a.1: Pre-flight — lint baseline

- [ ] **Step 1: Capture baseline**

```bash
cd client
bun run lint 2>&1 | tee /tmp/lint-baseline-9.log | tail -10
cd ..
```

Note the warning + error count.

### Task 6a.2: Create worktree

```bash
git worktree add .claude/worktrees/eslint-10-bump -b chore/eslint-10-bump
cd .claude/worktrees/eslint-10-bump
```

### Task 6a.3: Bump

- [ ] **Step 1: Edit `client/package.json`**

- `"@eslint/js": "^9.0.0"` → `"@eslint/js": "^10.0.1"`
- `"eslint": "^9.0.0"` → `"eslint": "^10.3.0"`

- [ ] **Step 2: Re-resolve**

```bash
cd client && bun install && cd ..
```

### Task 6a.4: Run lint and migrate

- [ ] **Step 1: Run lint**

```bash
cd client
bun run lint 2>&1 | tee /tmp/lint-after-10.log | tail -40
cd ..
```

- [ ] **Step 2: Compare diffs**

```bash
diff /tmp/lint-baseline-9.log /tmp/lint-after-10.log
```

For each new diagnostic:

- If the rule was renamed: update `eslint.config.*` to the new name.
- If the rule was removed: remove from config (note in commit message).
- If the rule changed semantics: fix the source if possible; only disable the rule with an inline comment + commit-message rationale if the source change is out of scope.

### Task 6a.5: Quality gates and commit

(Standard recipe; commit message lists every rule action taken — rename, remove, disable-with-rationale.)

### Phase 6a → Phase 6b soak

Wait ≥24 h.

---

## Phase 6b — TypeScript 6 (own PR)

**Branch:** `chore/typescript-6-bump`
**Risk:** Medium-High. TS 6 surfaces new strict diagnostics; `tsconfig` strictness must NOT be relaxed.
**Pre-flight gate:** capture `bun run build` baseline.

### Task 6b.1: Pre-flight — type-check baseline

- [ ] **Step 1: Baseline under TS 5.9**

```bash
cd client
bun run build 2>&1 | tee /tmp/tsc-baseline-5.9.log | tail -10
cd ..
```

### Task 6b.2: Create worktree, bump, build

```bash
git worktree add .claude/worktrees/typescript-6-bump -b chore/typescript-6-bump
cd .claude/worktrees/typescript-6-bump
```

- [ ] **Step 1: Edit `client/package.json`**

`"typescript": "^5.9.3"` → `"typescript": "^6.0.3"`

- [ ] **Step 2: Re-resolve and build**

```bash
cd client
bun install
bun run build 2>&1 | tee /tmp/tsc-after-6.log | tail -50
cd ..
```

- [ ] **Step 3: Compare diagnostics**

```bash
diff /tmp/tsc-baseline-5.9.log /tmp/tsc-after-6.log
```

### Task 6b.3: Fix or split

- [ ] **Step 1: For each new TS 6 diagnostic, fix the source**

Do NOT relax `tsconfig.json` strictness. If the diagnostic is genuine, fix it.

- [ ] **Step 2: If diagnostic count is too large for one PR, split**

Open a precursor PR (`chore/ts-precursor-strict-fixes`) that fixes the easy diagnostics under TS 5.9, merge it, then return to this branch for the bump.

### Task 6b.4: Commit and open PR

(Standard recipe.)

### Phase 6b → Phase 6c soak

Wait ≥24 h.

---

## Phase 6c — Test infrastructure majors

**Branch:** `chore/jsdom-marked-types-node-bump`
**Risk:** Medium. jsdom + marked are exercised in the test suite and chat-message renderer.
**Pre-flight gate:** read jsdom 28 + 29 release notes; read marked 18 release notes.

### Task 6c.1: Pre-flight — release-note skim

- [ ] **Step 1: jsdom**

Browse https://github.com/jsdom/jsdom/releases for v28.0.0 and v29.0.0. Note any DOM API tightening that could break tests (DOMParser, CustomElements, Worker, fetch).

- [ ] **Step 2: marked**

Browse https://github.com/markedjs/marked/releases for v18.0.0. Audit the renderer extension API — we use marked in the chat-message renderer. Locate it:

```bash
grep -rn "import.*marked\|from 'marked'" client/src 2>/dev/null
```

### Task 6c.2: Create worktree, bump

```bash
git worktree add .claude/worktrees/jsdom-marked-types-node-bump -b chore/jsdom-marked-types-node-bump
cd .claude/worktrees/jsdom-marked-types-node-bump
```

- [ ] **Step 1: Edit `client/package.json`**

- `"@types/node": "^22.15.0"` → `"@types/node": "^24.12.3"`
- `"jsdom": "^27.4.0"` → `"jsdom": "^29.1.1"`
- `"marked": "^17.0.4"` → `"marked": "^18.0.3"`

### Task 6c.3: Resolve, build, test, fix

- [ ] **Step 1: Re-resolve**

```bash
cd client && bun install && cd ..
```

- [ ] **Step 2: Build + test**

```bash
cd client
bun run build 2>&1 | tail -20
bun run test:run 2>&1 | tail -30
cd ..
```

- [ ] **Step 3: Fix renderer-extension breakage**

If marked 18 changed renderer extension APIs in the chat-message renderer, port to the new API. Common change: `renderer.code(code, infostring, escaped)` signature → object-arg form.

- [ ] **Step 4: Fix jsdom-tightened test breakage**

If jsdom 29 is stricter about DOMParser / CustomElements behavior, fix the test (if the test was relying on lax jsdom semantics) or fix the source (if the source was relying on lax DOM semantics).

### Task 6c.4: Commit and open PR

(Standard recipe.)

### Phase 6c → Phase 6d soak

Wait ≥24 h.

---

## Phase 6d — UI majors (icons + router)

**Branch:** `chore/lucide-router-majors`
**Risk:** High. Lucide icon renames cause runtime import failures; `@solidjs/router` 0.16 has historically rotated APIs.
**Pre-flight gate:** generate icon-name diff; read router 0.16 release notes.

### Task 6d.1: Pre-flight — icon name diff

- [ ] **Step 1: List icons we import**

```bash
grep -roE "from 'lucide-solid'.*\{[^}]*\}" client/src | \
  grep -oE "\{[^}]*\}" | tr -d '{}' | tr ',' '\n' | sed 's/^ *//; s/ *$//' | sort -u > /tmp/lucide-imports.txt
wc -l /tmp/lucide-imports.txt
```

- [ ] **Step 2: Compare exports between 0.577 and 1.14**

```bash
mkdir -p /tmp/lucide-diff/{0.577,1.14}
cd /tmp/lucide-diff/0.577 && bun add lucide-solid@0.577.0 && cd ../1.14 && bun add lucide-solid@1.14.0
ls /tmp/lucide-diff/0.577/node_modules/lucide-solid/dist | sort > /tmp/lucide-0.577.txt
ls /tmp/lucide-diff/1.14/node_modules/lucide-solid/dist | sort > /tmp/lucide-1.14.txt
diff /tmp/lucide-0.577.txt /tmp/lucide-1.14.txt
```

For every icon in `/tmp/lucide-imports.txt` that is not in `/tmp/lucide-1.14.txt`, find the rename in lucide release notes (https://github.com/lucide-icons/lucide/releases) and record it in the PR description.

### Task 6d.2: Pre-flight — router 0.16 surface

- [ ] **Step 1: Read router release notes**

https://github.com/solidjs/solid-router/releases v0.16.0 release notes.

- [ ] **Step 2: Identify call-site exposure**

```bash
grep -rn "import.*from '@solidjs/router'" client/src 2>/dev/null
grep -rn "useRoutes\|useNavigate\|useParams\|useLocation\|<Route\|<Routes\|<Router" client/src 2>/dev/null | head -40
```

Map any deprecated API to its 0.16 replacement before opening the PR.

### Task 6d.3: Create worktree, bump, migrate

```bash
git worktree add .claude/worktrees/lucide-router-majors -b chore/lucide-router-majors
cd .claude/worktrees/lucide-router-majors
```

- [ ] **Step 1: Edit `client/package.json`**

- `"lucide-solid": "^0.577.0"` → `"lucide-solid": "^1.14.0"`
- `"@solidjs/router": "^0.15.4"` → `"@solidjs/router": "^0.16.1"`

- [ ] **Step 2: Apply icon renames**

For each rename collected in Task 6d.1, edit the imports + JSX usage in the call sites identified.

- [ ] **Step 3: Apply router migrations**

For each deprecated router API identified in Task 6d.2, replace with the 0.16 equivalent.

- [ ] **Step 4: Build + test**

```bash
cd client && bun install && bun run build && bun run test:run && cd ..
```

### Task 6d.4: Commit and open PR

(Standard recipe; PR description lists every icon rename and every router migration applied.)

### Phase 6d → Phase 7 soak

Wait ≥24 h.

---

## Phase 7 — E2EE crypto bump (vodozemac 0.10)

**Branch:** `chore/vodozemac-0.10`
**Risk:** Critical. This is the cryptographic core; a bad bump can break message decryption irreversibly across deployed clients.
**Pre-flight gate:** verify dep deltas + run round-trip session-format test.

### Task 7.1: Pre-flight — verify dep delta

- [ ] **Step 1: Confirm vodozemac 0.10's transitive deltas vs 0.9**

```bash
cargo info vodozemac@0.10.0 2>&1 | grep -A 30 "dependencies"
cargo info vodozemac@0.9.0 2>&1 | grep -A 30 "dependencies"
```

Expected delta (per spec): only `prost 0.13 → 0.14` and `base64ct 1.6 → 1.8`. All cryptographic primitive deps identical to 0.9 (the spec lists the canonical pin set; re-verify against `cargo info` output).

If the delta extends beyond `prost` and `base64ct`, **stop**. The spec assumed primitive parity; a broader delta needs design review.

### Task 7.2: Create worktree, bump

```bash
git worktree add .claude/worktrees/vodozemac-0.10 -b chore/vodozemac-0.10
cd .claude/worktrees/vodozemac-0.10
```

- [ ] **Step 1: Edit root `Cargo.toml`**

`vodozemac = "0.9"` → `vodozemac = "0.10"`

- [ ] **Step 2: Resolve**

```bash
cargo update -p vodozemac 2>&1 | tail
SQLX_OFFLINE=true cargo build -p vc-server -p vc-crypto 2>&1 | tail -10
```

### Task 7.3: Round-trip compatibility test

- [ ] **Step 1: Write the round-trip test in `shared/vc-crypto/tests/vodozemac_0_10_roundtrip.rs`**

```rust
//! Phase 7 gate: vodozemac 0.10 must read sessions serialized with our
//! existing storage path (which uses vodozemac 0.9 wire formats).

use vc_crypto::{olm::Session, storage::serialize_session};

#[test]
fn olm_session_round_trip() {
    let alice = Session::new_outbound(/* canonical fixture */);
    let bob = Session::new_inbound(/* canonical fixture */);

    let serialized = serialize_session(&alice);
    let restored = Session::deserialize(&serialized).expect("deserialize");

    let plaintext = b"phase-7-canary";
    let cipher = restored.encrypt(plaintext);
    let decrypted = bob.decrypt(&cipher).expect("decrypt");

    assert_eq!(decrypted, plaintext);
}
```

Adjust the test imports to match the actual `vc-crypto` storage API.

- [ ] **Step 2: Run the test**

```bash
SQLX_OFFLINE=true cargo test -p vc-crypto --test vodozemac_0_10_roundtrip 2>&1 | tail -10
```

Expected: PASS. If FAIL, the on-wire serialization changed despite the design's expectation; **halt** and surface to the user. Do not merge a vodozemac bump that fails this test.

- [ ] **Step 3: Run the existing E2EE round-trip suite**

```bash
SQLX_OFFLINE=true cargo test -p vc-server --test '*e2ee*' --test '*megolm*' --test '*olm*' 2>&1 | tail -30
```

Expected: green.

### Task 7.4: Commit and open PR

(Standard recipe; PR description prominently links the round-trip test result.)

### Phase 7 → Phase 8 soak

Wait ≥48 h. E2EE storage corruption can lurk; double the soak.

---

## Phase 8 — WebRTC stack

**Branch:** `chore/webrtc-stack-bump`
**Risk:** Critical. Highest-risk phase in the plan.
**Pre-flight gate:** audit every webrtc-rs release note from 0.12 to 0.17; catalogue API breakage in advance.

### Task 8.1: Pre-flight — webrtc-rs API survey

- [ ] **Step 1: Read every release note**

https://github.com/webrtc-rs/webrtc/releases — every version from 0.12 → 0.17.1. Capture in a temporary doc:

- Renamed types
- Renamed methods on `RTCPeerConnection`, `RTCRtpSender`, `RTCRtpReceiver`, `Interceptor`, `Track`, `Sample`
- Behavior changes (especially around RTCP feedback, write_rtcp delivery)

- [ ] **Step 2: Inventory current usage**

```bash
grep -rn "webrtc::\|RTCPeerConnection\|RTCRtpSender\|RTCRtpReceiver\|Interceptor::\|track::Track\|Sample" server/src client/src-tauri/src 2>/dev/null > /tmp/webrtc-callsites.txt
wc -l /tmp/webrtc-callsites.txt
```

Map each call-site to the 0.17 API.

- [ ] **Step 3: Verify scap fork compatibility**

The Detair scap fork has no webrtc dep (per spec). Confirm:

```bash
grep -A20 "^\[dependencies\]" client/src-tauri/Cargo.toml | grep webrtc
grep -A20 "^\[dependencies\]" $(find / -path '*/scap*/Cargo.toml' 2>/dev/null | head -1) | grep webrtc || echo "scap has no webrtc dep — confirmed"
```

### Task 8.2: Create worktree

```bash
git worktree add .claude/worktrees/webrtc-stack-bump -b chore/webrtc-stack-bump
cd .claude/worktrees/webrtc-stack-bump
```

### Task 8.3: Bump and migrate

- [ ] **Step 1: Edit root `Cargo.toml` `[workspace.dependencies]`**

- `webrtc = "0.11"` → `webrtc = "0.17"`
- `vpx-encode = "0.3"` → `vpx-encode = "0.6"`
- `env-libvpx-sys = "4"` → `env-libvpx-sys = "5"`

Apply the same `webrtc` bump in `client/src-tauri/vp8-decoder/Cargo.toml`.

- [ ] **Step 2: Resolve and migrate**

```bash
cargo update -p webrtc -p vpx-encode -p env-libvpx-sys 2>&1 | tail -20
SQLX_OFFLINE=true cargo build -p vc-server 2>&1 | tail -50
cargo build -p vc-client 2>&1 | tail -50
cargo build -p vp8-decoder 2>&1 | tail -50
```

For each compile error, port the call to the 0.17 API. Prefer mechanical 1:1 renames; do not refactor the voice path while bumping.

### Task 8.4: PLI workaround re-evaluation

- [ ] **Step 1: Test write_rtcp delivery**

In `webrtc 0.17`, write a focused test that:

1. Creates a peer connection.
2. Calls `peer.write_rtcp(...)` with a PLI message.
3. Confirms the message arrives at the receiver.

- [ ] **Step 2: Decide PLI workaround disposition**

If the test passes (write_rtcp works in 0.17): remove the interval-PLI workaround in client code. Document removal in commit message and update memory `feedback_webrtc_rs_rtcp.md`.

If the test fails: keep the interval-PLI workaround; port any code changes that the bump required.

### Task 8.5: Multi-platform canary

- [ ] **Step 1: Local lab test**

```bash
SQLX_OFFLINE=true cargo test -p vc-server --test '*voice*' --test '*webrtc*' --test '*sfu*' 2>&1 | tail -30
```

- [ ] **Step 2: Beta canary deploy**

Deploy the server image from this branch to `kaiku.pmind.de` via a temp branch (do **not** merge this PR to main yet):

```bash
./infra/scripts/deploy.sh --server-only --branch chore/webrtc-stack-bump
```

- [ ] **Step 3: 30-minute three-client voice + screenshare**

Coordinate three real clients:
1. Tauri client #1 (Linux or Windows desktop)
2. Tauri client #2 (macOS desktop, ideally a different OS than #1)
3. Android native client

For 30 continuous minutes:
- Voice on all three.
- Screen share from #1 to #2 and #3.
- Cross-mute / unmute cycles.
- One disconnect/reconnect per client.

Watch for: dropped audio, frozen video, stale ICE state, server CPU/RAM spike, RTCP feedback storms in logs.

If any anomaly: revert the canary deploy, halt the phase, surface to user.

### Task 8.6: Post-merge audit re-run

- [ ] **Step 1: Re-run cargo audit after merge**

```bash
cargo audit 2>&1 | tail -10
```

Expected: `bincode 1` warning may have cleared via webrtc-dtls's bumped pin. Document the delta in the PR description.

### Task 8.7: Commit and open PR

(Standard recipe; PR description includes the canary report — clients, duration, anomalies.)

### Phase 8 → Phase 9 soak

Wait ≥72 h. Voice regressions can be slow to surface; triple the soak. If a paying user reports a voice issue during this window, halt the cadence and investigate before Phase 9.

---

## Phase 9 — Client storage and native deps

**Branch:** `chore/rusqlite-keyring-bump`
**Risk:** Medium-High. `cargo deny check --all-features` may surface a libsqlite3-sys conflict between rusqlite 0.39 and sqlx 0.8.
**Pre-flight gate:** verify cargo-deny resolution.

### Task 9.1: Pre-flight — `cargo deny --all-features` test

- [ ] **Step 1: Apply just the rusqlite bump in a scratch worktree**

```bash
git worktree add /tmp/cargo-deny-rusqlite-test -b scratch/cargo-deny-rusqlite-test
cd /tmp/cargo-deny-rusqlite-test
sed -i 's/rusqlite = "0.32"/rusqlite = "0.39"/' client/src-tauri/Cargo.toml
cargo update -p rusqlite 2>&1 | tail
cargo deny check 2>&1 | tail -30
```

- [ ] **Step 2: Decide**

If `cargo deny check` is green: no action required; clean up the scratch worktree and proceed to Task 9.2.

If `cargo deny check` flags a `libsqlite3-sys` conflict (because `all-features = true` activates `sqlx-sqlite` which wants 0.30 while rusqlite 0.39 wants 0.37): switch `deny.toml`'s `[graph]` from `all-features = true` to explicit features. Document this change in Phase 9's PR.

```bash
git worktree remove /tmp/cargo-deny-rusqlite-test
git branch -D scratch/cargo-deny-rusqlite-test
```

### Task 9.2: Create worktree

```bash
git worktree add .claude/worktrees/rusqlite-keyring-bump -b chore/rusqlite-keyring-bump
cd .claude/worktrees/rusqlite-keyring-bump
```

### Task 9.3: Bump

- [ ] **Step 1: Edit `client/src-tauri/Cargo.toml`**

- `rusqlite = { version = "0.32", features = ["bundled"] }` → `rusqlite = { version = "0.39", features = ["bundled"] }`
- `keyring = "2"` → `keyring = "4"`

- [ ] **Step 2: If pre-flight required, edit `deny.toml`**

If Task 9.1 surfaced the `libsqlite3-sys` conflict, change `deny.toml`'s `[graph]` block:

```toml
[graph]
targets = []
all-features = true
```

to (keeping the spec's `default-https-client` story consistent):

```toml
[graph]
targets = []
features = ["default"]
```

Document the rationale inline.

- [ ] **Step 3: Resolve and build**

```bash
cargo update -p rusqlite -p keyring 2>&1 | tail
cargo build -p vc-client 2>&1 | tail -20
```

### Task 9.4: Smoke test client storage paths

- [ ] **Step 1: Launch dev client, exercise keyring**

```bash
cd client && bun run tauri dev
```

In the running client:
1. Log in (writes keyring entries for OIDC tokens).
2. Force-quit the client.
3. Relaunch — confirm session restores from keyring.
4. Log out — confirm keyring entries are deleted.

- [ ] **Step 2: Exercise sqlite cache roundtrip**

In the running client:
1. Visit a channel with messages — confirm messages render.
2. Force-quit the client.
3. Relaunch and re-open the same channel — confirm cached messages render before live fetch completes.

### Task 9.5: Post-merge audit

- [ ] **Step 1: Re-run cargo audit after merge**

```bash
cargo audit 2>&1 | tail -10
```

Expected: `derivative` warning may have cleared (keyring 4 dropped that dep).

### Task 9.6: Commit and open PR

(Standard recipe.)

### Phase 9 → Phase 10 soak

Wait ≥24 h.

---

## Phase 10 — Cleanup and follow-ups

**Branch:** `chore/dep-update-cleanup`
**Risk:** Very low. Comment + doc edits.
**Pre-flight gate:** none.

### Task 10.1: Update RUSTSEC-2026-0097 ignore comment

- [ ] **Step 1: Edit `deny.toml`**

Find the `RUSTSEC-2026-0097` ignore (rand). Replace the comment block above it with text that acknowledges:

- 0.7.3, 0.8.5, 0.9.2, **and** 0.10.0 are all flagged.
- The exploit requires a `log::Log` implementation that calls `rand::thread_rng()` during logging.
- We use `tracing` / `tracing-subscriber`, not the `log` facade — so the exploit is unreachable regardless of which rand version is in tree.

Drop the misleading "0.8.5 only" text from the existing comment.

### Task 10.2: Re-run all advisory gates

- [ ] **Step 1**

```bash
cargo audit 2>&1 | tail -10
cargo deny check 2>&1 | tail -10
cd client && bun audit 2>&1 | tail -10 && cd ..
```

Capture the final state in the PR description.

### Task 10.3: Update LICENSE_COMPLIANCE.md

- [ ] **Step 1: Diff the dep tree before/after the entire plan**

If a baseline pre-Phase-0 cargo-tree dump exists, diff against it:

```bash
cargo tree --workspace --target all --prefix none --no-default-features 2>&1 | sort -u > /tmp/tree-after-plan.txt
diff /tmp/tree-pre-phase-0.txt /tmp/tree-after-plan.txt > /tmp/tree-delta.txt
```

For every license category that appears in the post tree but not the pre tree, add a line to `LICENSE_COMPLIANCE.md`. The transitive Tauri/GTK3 chain warnings stay called out (the plan does not eliminate them).

### Task 10.4: Recheck `CapSoftware/scap` upstream

- [ ] **Step 1: Check upstream**

```bash
curl -sf https://api.github.com/repos/CapSoftware/scap/releases/latest | python3 -c "import json,sys;r=json.load(sys.stdin);print(r['tag_name'], r['published_at'])"
curl -sf https://api.github.com/repos/CapSoftware/scap/issues/178 | python3 -c "import json,sys;r=json.load(sys.stdin);print(r['state'], r['updated_at'])"
```

- [ ] **Step 2: Decide**

If a release with the Linux Frame enum fix has shipped and is tagged: drop the Detair fork in `client/src-tauri/Cargo.toml`, switch back to upstream `scap`, update CLAUDE.md memory accordingly.

If not: update the inline `# Last re-check: 2026-MM-DD` comment in `client/src-tauri/Cargo.toml` with today's date.

### Task 10.5: THIRD_PARTY_NOTICES.md (best-effort)

- [ ] **Step 1: Scan for new licenses**

```bash
cargo deny check licenses 2>&1 | grep -E "license-not-encountered|unmatched" | head
```

If the diff produces new license categories, hand-edit `THIRD_PARTY_NOTICES.md`. Building a regeneration script is out-of-scope for this phase (logged as out-of-scope in the spec).

### Task 10.6: Update CLAUDE.md memory entries

- [ ] **Step 1: Edit `feedback_webrtc_rs_rtcp.md`**

If Phase 8 removed the interval-PLI workaround: rewrite the memory to reflect that webrtc 0.17 delivers `write_rtcp` properly. If not: keep but note the version checked.

- [ ] **Step 2: Edit `project_webrtc_screen_share.md`**

If Phase 8 landed: update version mentions to webrtc 0.17.

- [ ] **Step 3: Add new memory entry**

Create `project_dep_update_2026_05.md` summarizing:
- Which phases shipped, which were deferred (RustCrypto, getrandom, Tauri 3 — all out-of-scope per the spec).
- Net effect on `cargo audit` and `bun audit` counts.
- Any scope-changing discoveries (e.g. unexpected breakage that forced scope shrink).

Add a one-line entry to `MEMORY.md` pointing at the new file.

### Task 10.7: Commit and open PR

```bash
git add deny.toml LICENSE_COMPLIANCE.md THIRD_PARTY_NOTICES.md client/src-tauri/Cargo.toml \
        ~/.claude/projects/-home-detair-GIT-detair-kaiku/memory/
git commit -m "$(cat <<'EOF'
chore(infra): dep-update sweep cleanup (Phase 10)

- deny.toml: rewrite RUSTSEC-2026-0097 ignore comment (all rand
  versions flagged; tracing-vs-log facade rationale unchanged)
- LICENSE_COMPLIANCE.md: diff post-plan tree
- scap: re-checked upstream; <kept fork | switched to upstream>
- CLAUDE.md memory: updated webrtc-rs notes; added dep-update summary

Closes the dep-update sweep started 2026-05-08.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

Push, PR, watch, merge, clean up.

---

## Plan-wide rollback discipline

If any phase's PR introduces a regression that cannot be hotfixed in the same PR:

1. Revert the merge commit on `main` (`git revert -m 1 <merge-sha>`).
2. Open a "phase N rollback" PR with the revert + a commit-message explanation.
3. Halt the cadence — do NOT open Phase N+1 until the regression is understood.
4. Surface to user before re-attempting the phase.

The 24/48/72-hour soak windows exist precisely to surface regressions before the next phase compounds the risk.

## When to abandon a phase mid-flight

A phase is held or cut from this plan if:

- Its pre-flight gate fails and the cause is upstream (e.g. a release was yanked, a registry response shows the version regressed).
- A discovery during execution invalidates the spec's assumption (e.g. vodozemac 0.10's transitive delta is broader than `prost`+`base64ct`).
- A canary regression repeats after rollback + retry.

In any of these cases: write a short note in `docs/superpowers/specs/2026-05-08-dependency-update-design.md` under a new `## Execution notes` section with the cause + decision, commit it as a docs PR, and surface to user.

## Done when

- All 10 phases (Phase 0 already done) merged to `main`.
- `cargo audit` errors: 0 (or only the documented `RUSTSEC-2023-0071` + `RUSTSEC-2026-0097` ignores).
- `bun audit`: 0 advisories.
- OSV Scanner CI job on `main`: green.
- `LICENSE_COMPLIANCE.md` reflects the post-sweep tree.
- `CHANGELOG.md` `[Unreleased]` has user-facing security/changed entries for Phases 2, 7, 8 as applicable.
- A summary memory entry at `~/.claude/projects/-home-detair-GIT-detair-kaiku/memory/project_dep_update_2026_05.md` records the outcome.
