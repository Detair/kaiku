# Dependency Update — Design

**Date:** 2026-05-08
**Status:** Approved (brainstormed); ready for implementation plan
**Scope:** All workspace `Cargo.toml`, `Cargo.lock`, `client/package.json`, `client/bun.lock`, transitive overrides, and `deny.toml` advisory ignores.

## Context

`cargo update --workspace` reports zero changes — every Rust dependency is already at its latest version *within the existing semver bound*. Any further movement therefore requires version-bound edits and is, by definition, a breaking-version change. The frontend has the same shape: `bun update` (without `--latest`) yields only patch-level movement.

`cargo audit` reports 4 errors (1 ignored — rsa Marvin Attack; 3 covered by an in-flight worktree — rustls-webpki 0.101.7) and 27 warnings (mostly transitive Tauri/GTK3 unmaintained chains). `bun audit` reports 12 advisories (4 high, 8 moderate), all attributable to a small number of fixable direct-dep gaps and one missing override.

There is an in-flight worktree `fix/rustls-webpki-advisories` containing a single commit that drops the legacy rustls 0.21 chain by switching `aws-sdk-s3` and `aws-config` from feature `rustls` (legacy alias for the `legacy-rustls-ring` path that drags rustls 0.21.12 + rustls-webpki 0.101.7 + hyper 0.14 into the tree) to feature `default-https-client` (modern path on rustls 0.23.x via `rustls-aws-lc`). That fix lands as Phase 0 of this plan; the corresponding three RUSTSEC ignores are removed once it is on `main`.

### Late-discovery finding: RustCrypto suite cannot bump in this round

Pre-flight check on **2026-05-08** confirmed that `vodozemac 0.10.0` still pins `sha2 ^0.10.9`, `hkdf ^0.12.4`, and `hmac ^0.12.1` — identical to `vodozemac 0.9`. The RustCrypto ecosystem has not aligned on the 0.11/0.13 series. Bumping our workspace `sha1`/`sha2`/`hkdf`/`hmac` directly would either (a) fail to resolve because vodozemac requires `^0.10`, or (b) introduce dual-version compilation (sha2 0.10 + 0.11) of crypto primitives — which is precisely the surface we want to keep narrow. **The RustCrypto suite bump is moved out of scope for this round** and recorded under "Out of scope" for a follow-up plan.

## Decisions

- **Scope:** everything — Rust workspace, frontend npm tree, lockfiles, transitive overrides.
- **Risk appetite:** aggressive — accept all major-version bumps that have a clear destination, including those requiring source-code changes.
- **Delivery shape:** phased PRs by risk-and-coupling. One PR per phase, independently reviewable, mergeable, and revertible.
- **Frontend tooling majors:** included in this plan (TypeScript 6, ESLint 10, jsdom 29, marked 18, lucide-solid 1, @solidjs/router 0.16) — each gets its own phase entry.
- **`@types/node`:** target `^24.12.3` (next-LTS-aligned), not the current/odd Node 25 line.
- **scap fork:** keep. CapSoftware/scap issue #178 last activity 2025-10-26; latest release v0.1.0-beta.1 (2025-08-04). Recheck at end of plan; do not chase mid-flight.
- **Tauri major:** stay on 2.x. Tauri 3 is still alpha; not in scope.

## Goals

1. Bring every direct dependency to the upstream-verified latest stable version.
2. Eliminate every `cargo audit` error and every `bun audit` advisory we can without introducing new ones.
3. Keep `cargo deny check` green at every phase boundary.
4. Don't break the beta deployment (`kaiku.pmind.de`) or the desktop client's golden paths.

## Non-goals

- Migrating Tauri to 3.x.
- Replacing `scap` (kept on the Detair fork until upstream lands the Linux Frame enum fix).
- Eliminating the 23 transitive Tauri/GTK3 unmaintained warnings — these will not move until Tauri's `wry`/`tao` migrate.
- Chasing rand 0.10 to "fix" RUSTSEC-2026-0097. The `cargo audit` output shows rand 0.7.3, 0.8.5, 0.9.2, **and** 0.10.0 all flagged. Bumping does not help; the ignore stays. The comment in `deny.toml` is updated to reflect this.
- Eliminating the `bincode 1`, `derivative`, `fxhash`, `glib`, `lru`, `instant`, `paste`, `proc-macro-error`, `unic-*` warnings beyond what a single direct-dep bump can clear (they live in deep transitive chains we don't own).

## Advisory ignore disposition

| Advisory | Today | After plan |
|---|---|---|
| `RUSTSEC-2023-0071` (rsa Marvin Attack) | ignored | **stays ignored**. No fix exists; pulled via `openidconnect`, `jsonwebtoken`, and `sqlx-mysql` macro path. |
| `RUSTSEC-2026-0097` (rand unsoundness) | ignored | **stays ignored**. All tested rand versions affected. Comment in `deny.toml` is rewritten to drop the misleading "0.8.5 only" claim. |
| `RUSTSEC-2026-0098` (rustls-webpki name constraints / URI) | ignored | **removed** at end of Phase 0. |
| `RUSTSEC-2026-0099` (rustls-webpki name constraints / wildcard) | ignored | **removed** at end of Phase 0. |
| `RUSTSEC-2026-0104` (rustls-webpki CRL panic) | ignored | **removed** at end of Phase 0. |

## Verified version inventory

All versions below were queried live on 2026-05-08 via `https://crates.io/api/v1/crates/<name>` and `https://registry.npmjs.org/<pkg>/latest`. No version is assumed; pre-1.0 minor bumps are treated as breaking.

Every phase MUST re-run the same registry lookups at PR-creation time and pin to whatever has been published since this design was written.

### Rust direct dependencies — bumps required

| Crate | Current | Latest (2026-05-08) | Phase | Notes |
|---|---|---|---|---|
| `axum-tracing-opentelemetry` | 0.32 | 0.33.1 | 4a | minor (pre-1.0 breaking) |
| `env-libvpx-sys` | 4 | 5.1.3 | 8 | libvpx FFI sub-crate |
| `infer` | 0.16 | 0.19.0 | 4a | 3 majors; verify MIME signature paths |
| `init-tracing-opentelemetry` | 0.36 | 0.37.0 | 4a | minor (pre-1.0 breaking) |
| `keyring` | 2 | 4.0.0 | 9 | 2 majors; clears `derivative` warning |
| `reqwest` (server) | 0.12 | 0.13.3 | 4b | own PR — TLS feature surface reorganized |
| `rusqlite` | 0.32 | 0.39.0 | 9 | 7 minor majors |
| `sentry`, `sentry-tracing` | 0.47 | 0.48.1 | 4a | minor (pre-1.0 breaking) |
| `smol_str` | 0.2 | 0.3.6 | 4a | `From<&str>` route changed |
| `sysinfo` | 0.38 | 0.39.0 | 4a | minor (pre-1.0 breaking) |
| `tauri` ecosystem | 2.10.x | 2.11.x | 3 | minor; aligns with npm `@tauri-apps/api` 2.11 |
| `thiserror` (vp8-decoder) | 1 | 2.0.18 | 4a | align with workspace |
| `tokio-tungstenite` | 0.28 | 0.29.0 | 4a | minor (pre-1.0 breaking) |
| `vodozemac` | 0.9 | 0.10.0 | 7 | E2EE crypto; only `prost 0.13→0.14` + `base64ct 1.6→1.8` change vs 0.9 |
| `vpx-encode` | 0.3 | 0.6.2 | 8 | client video |
| `webrtc` | 0.11 | 0.17.1 | 8 | server + client + vp8-decoder |
| `zip` | 2 | 8.6.0 | 4b | own PR — 6 majors, API rewritten |
| ~~`hkdf` 0.12 → 0.13~~ | — | — | OUT | RustCrypto suite — vodozemac 0.10 still on 0.12 |
| ~~`hmac` 0.12 → 0.13~~ | — | — | OUT | RustCrypto suite — vodozemac 0.10 still on 0.12 |
| ~~`sha1` 0.10 → 0.11~~ | — | — | OUT | RustCrypto suite — vodozemac 0.10 still on 0.10 |
| ~~`sha2` 0.10 → 0.11~~ | — | — | OUT | RustCrypto suite — vodozemac 0.10 still on 0.10 |

### Rust direct dependencies — already at latest major

`aes-gcm 0.10`, `aho-corasick 1`, `anyhow 1`, `arboard 3`, `argon2 0.5`, `aws-config 1`, `aws-sdk-s3 1`, `aws-smithy-async 1`, `aws-smithy-types 1`, `axum 0.8`, `axum-extra 0.12`, `base64 0.22`, `bitflags 2`, `blurhash 0.2`, `bs58 0.5`, `bytes 1`, `chrono 0.4`, `cpal 0.17`, `dashmap 6`, `dotenvy 0.15`, `fred 10`, `futures 0.3`, `hex 0.4`, `http-body-util 0.1`, `image 0.25`, `jsonwebtoken 10`, `lazy_static 1`, `lettre 0.11`, `mime_guess 2`, `nnnoiseless 0.5`, `nokhwa 0.10`, `openidconnect 4`, `opentelemetry 0.31` (suite), `opus 0.3`, `pulldown-cmark 0.13`, `regex 1`, `rodio 0.22`, `rustls 0.23`, `serde 1`, `serde_bytes 0.11`, `serde_json 1`, `sqlx 0.8`, `tempfile 3`, `thiserror 2`, `time 0.3`, `tokio 1`, `tokio-test 0.4`, `tokio-util 0.7`, `totp-rs 5`, `tower 0.5`, `tower-http 0.6`, `tracing 0.1`, `tracing-opentelemetry 0.32`, `tracing-opentelemetry-instrumentation-sdk 0.32`, `tracing-subscriber 0.3`, `unicode-segmentation 1`, `url 2`, `utoipa 5`, `utoipa-swagger-ui 9`, `uuid 1`, `validator 0.20`, `woothee 0.13`, `zeroize 1`.

`getrandom` is held at 0.2. Latest stable is 0.4.2, but bumping our direct usage to 0.4 would not eliminate the 0.2/0.3 transitive copies pulled by Tauri, vodozemac (which still consumes `getrandom ^0.2.15`), `quinn`, and the wider RustCrypto chain. The bump compresses nothing until those upstreams move. Defer to the same follow-up plan that addresses the RustCrypto suite.

### Frontend npm direct dependencies — bumps required

| Package | Current | Latest (2026-05-08) | Phase | Notes |
|---|---|---|---|---|
| `@eslint/js` | 9.39.4 | 10.0.1 | 6 | major; ESLint flat-config impact |
| `@playwright/test` | 1.58.2 | 1.59.1 | 1 | minor |
| `@sentry/browser` | 10.44.0 | 10.52.0 | 2 | minor |
| `@solidjs/router` | 0.15.4 | 0.16.1 | 6 | pre-1.0 breaking |
| `@tanstack/solid-virtual` | 3.13.23 | 3.13.24 | 1 | patch |
| `@tauri-apps/api` | 2.10.1 | 2.11.0 | 3 | align with Rust tauri 2.11 |
| `@tauri-apps/cli` | 2.10.1 | 2.11.1 | 3 | align with Rust tauri 2.11 |
| `@types/node` | 22.15.0 | 24.12.3 | 6 | LTS-aligned |
| `@unocss/preset-icons` / `preset-uno` / `reset` | 66.6.6 | 66.6.8 | 1 | patch |
| `@vitejs/plugin-basic-ssl` | 2.2.0 | 2.3.0 | 2 | minor |
| `dompurify` | 3.3.3 | 3.4.2 | 2 | clears 4 XSS advisories |
| `eslint` | 9.39.4 | 10.3.0 | 6 | major |
| `eslint-plugin-solid` | 0.14.5 | 0.14.5 | — | already latest |
| `jsdom` | 27.4.0 | 29.1.1 | 6 | 2 majors |
| `lucide-solid` | 0.577.0 | 1.14.0 | 6 | post-1.0 stable |
| `marked` | 17.0.4 | 18.0.3 | 6 | major |
| `mermaid` | 11.13.0 | 11.14.0 | 2 | brings in fixed `uuid` |
| `prettier` | 3.8.1 | 3.8.3 | 1 | patch |
| `solid-js` | 1.9.11 | 1.9.12 | 1 | patch |
| `typescript` | 5.9.3 | 6.0.3 | 6 | major |
| `typescript-eslint` | 8.57.1 | 8.59.2 | 1 | patch |
| `unocss` | 66.6.6 | 66.6.8 | 1 | patch |
| `vite` | 8.0.0 | 8.0.11 | 1 + 2 override | patch on direct; override needed for nested copy (see below) |
| `vite-plugin-solid` | 2.11.11 | 2.11.12 | 1 | patch |
| `vitest` | 4.1.0 | 4.1.5 | 1 | patch |

### Frontend npm direct dependencies — already at latest

`@floating-ui/dom 1.7.6`, `@solidjs/testing-library 0.8.10`, `@tauri-apps/plugin-global-shortcut 2.3.1`, `@tauri-apps/plugin-notification 2.3.3`, `@tauri-apps/plugin-shell 2.3.5`, `blurhash 2.0.5`, `highlight.js 11.11.1`, `qrcode 1.5.4`, `uplot 1.6.32`, `@types/qrcode 1.5.6`, `@rollup/plugin-commonjs 29.0.2`, `eslint-plugin-solid 0.14.5`.

### Frontend transitive overrides

Existing `overrides` block in `client/package.json`: `picomatch 4`, `rollup 4.60.1`, `flatted 3.4.2`, `brace-expansion 1.1.13`, `lodash-es 4.17.24`, `defu 6.1.5`. Latest verified versions on 2026-05-08: picomatch 4.0.4, rollup 4.60.3, flatted 3.4.2, brace-expansion 5.0.6, lodash-es 4.18.1, defu 6.1.7.

New overrides required in Phase 2:
- `vite ^8.0.0` — `vitest@4.1.0` resolved a second copy at `vite 7.3.1` via its inline `dependencies` (range `^6.0.0 || ^7.0.0 || ^8.0.0-0` matched 7.3.1). All four plugins (vite-plugin-solid, @vitejs/plugin-basic-ssl, @unocss/vite, @vitest/mocker) declare vite 8 in their peer ranges, so the override is safe.
- `postcss ^8.5.10` — clears the postcss XSS advisory pulled via vite.
- `uuid ^11.1.1` — clears the buffer-bounds advisory pulled via mermaid.
- `dompurify ^3.4.2` — direct bump in Phase 2 plus an override to ensure mermaid's transitive copy follows.

The `brace-expansion` override changes to a range union `^1.1.13 || ^5.0.6` (rather than dropping the 1.x bound), so any tool in the dev tree that hard-pins to 1.x still resolves. The `lodash-es` override bumps to `^4.18.1`. These are mechanical; they belong in Phase 1.

## Phase plan

Each phase = one PR. Each PR squash-merges to `main`. Branches follow the existing convention (`fix/`, `chore/`, `refactor/`).

### Phase 0 — Land in-flight rustls fix

**Branch:** `fix/rustls-webpki-advisories` (already exists at `.claude/worktrees/fix-rustls-webpki-advisories/`).

- Open PR for the existing single commit `df949b8f fix(infra): drop legacy rustls 0.21 chain via aws-sdk-s3 default-https-client`.
- Before removing the ignores, verify with `cargo audit` that the three rustls-webpki 0.101.7 advisories no longer fire (i.e. the legacy chain is genuinely gone from the lockfile).
- Then remove `RUSTSEC-2026-0098`, `RUSTSEC-2026-0099`, `RUSTSEC-2026-0104` from `deny.toml` `advisories.ignore` and verify `cargo deny check advisories` is still `ok`.
- The deny.toml change can ride in the same PR or follow as a small cleanup PR — it is the gating signal that Phase 0 is complete.

### Phase 1 — Within-bound housekeeping

**Branch:** `chore/deps-within-bound`

- Run `cargo update --workspace` (commit a refreshed `Cargo.lock` even if the diff is small — captures any registry-side patch movement since lock was last touched).
- Run `bun update` (no `--latest`). Picks up: `@playwright/test 1.58.2→1.59.1`, `@tanstack/solid-virtual 3.13.23→3.13.24`, `@unocss/* 66.6.6→66.6.8`, `mermaid 11.13.0→11.14.0`, `prettier 3.8.1→3.8.3`, `solid-js 1.9.11→1.9.12`, `typescript-eslint 8.57.1→8.59.2`, `vite 8.0.0→8.0.11`, `vite-plugin-solid 2.11.11→2.11.12`, `vitest 4.1.0→4.1.5`.
- Bump existing `overrides` to verified latest:
  - `rollup ^4.60.1 → ^4.60.3`
  - `lodash-es ^4.17.24 → ^4.18.1`
  - `defu ^6.1.5 → ^6.1.7`
  - `picomatch ^4.0.4` (no change)
  - `brace-expansion`: change to range union `^1.1.13 || ^5.0.6` rather than dropping the 1.x bound, so any tool in the dev tree that hard-pins to 1.x still resolves.
- After `bun install`, run `bun audit` and confirm the 5 vulnerabilities tied to these overrides have cleared. The remaining 7 (dompurify ×4, vite ×3 occurrences worth of paths, postcss, uuid) are addressed in Phase 2.
- This phase has zero `Cargo.toml`/`package.json` source-code edits beyond the override-version bumps; CI must stay green.

### Phase 2 — Frontend security advisories

**Branch:** `fix/frontend-security-deps`

- Bump direct deps: `dompurify ^3.4.2`, `@sentry/browser ^10.52.0`, `@vitejs/plugin-basic-ssl ^2.3.0`. (`mermaid` is already on 11.14 from Phase 1.)
- Add `overrides`: `dompurify ^3.4.2`, `vite ^8.0.0`, `postcss ^8.5.10`, `uuid ^11.1.1`.
- After merge, `bun audit` must report zero advisories.
- **Tauri NPM bumps stay in Phase 3** so Rust and npm Tauri move together in a single PR.

### Phase 3 — Tauri 2.10 → 2.11 alignment

**Branch:** `chore/tauri-2.11-bump`

- Rust: tauri-related crates are already declared with caret semver; `cargo update -p tauri -p tauri-build -p tauri-plugin-shell -p tauri-plugin-notification -p tauri-plugin-global-shortcut` will pick up 2.11.
- NPM: bump `@tauri-apps/api ^2.10.1 → ^2.11.0` and `@tauri-apps/cli ^2.10.1 → ^2.11.1` in `client/package.json`. Both Rust and npm sides move in this single PR so versions stay aligned.
- Build the Tauri client: `cd client && bun install && bun run build`, then `cargo build --release -p vc-client`. Both must succeed.
- Smoke test (manual on a dev build of the desktop client):
  1. Launch the client, log in to the dev server.
  2. Trigger an IPC `invoke<T>` call from the frontend (any existing command — e.g. `get_current_user`).
  3. Trigger a plugin-shell command (the existing "open in browser" link path).
  4. Trigger the notification-permission prompt (incoming DM in a background channel).
  Each must behave identically to the 2.10 baseline. If any of the four fails, revert.

### Phase 4 — Rust low-risk minors

Phase 4 is split into two PRs because two of the original bumps (`reqwest 0.12 → 0.13`, `zip 2 → 8`) are not low-risk and deserve isolated review.

#### Phase 4a — Genuinely low-risk minors batch

**Branch:** `chore/rust-minors-batch`

Single PR bumping the workspace declarations:

- `axum-tracing-opentelemetry 0.32 → 0.33.1`
- `init-tracing-opentelemetry 0.36 → 0.37.0`
- `infer 0.16 → 0.19.0`
- `sentry 0.47 → 0.48.1` and `sentry-tracing 0.47 → 0.48.1` in `client/src-tauri/Cargo.toml`
- `smol_str 0.2 → 0.3.6` (audit `From<&str>` and `&str → SmolStr` call sites)
- `sysinfo 0.38 → 0.39.0`
- `thiserror 1 → 2` in `client/src-tauri/vp8-decoder/Cargo.toml` (replace the literal `thiserror = "1"` with `thiserror.workspace = true`)
- `tokio-tungstenite 0.28 → 0.29.0` (the `MaybeTlsStream` variant set widened — re-verify both server and client WS sites compile)
- Hoist `tempfile = "3"` to `[workspace.dependencies]` and replace the literal `tempfile = "3"` declarations in `server/Cargo.toml` (main deps) and `client/src-tauri/Cargo.toml` (dev-deps) with `tempfile.workspace = true`.

Per-bump concerns:
- `sentry 0.48`: changelog notes the `tower` integration moved to a separate crate; verify our usage on Tauri client is unaffected (we use `sentry-tracing`, not the tower middleware).
- `infer 0.16 → 0.19`: signature database changed; verify our MIME detection paths still produce the types our handlers expect (run server upload tests).

#### Phase 4b — reqwest 0.12 → 0.13 (server)

**Branch:** `refactor/reqwest-0.13-server`

Own PR. Aligns server with vc-client (already on 0.13).

- Bump `reqwest = { version = "0.13", features = ["json"] }` in `server/Cargo.toml`.
- Audit all `reqwest::Client`/`ClientBuilder` construction sites — TLS feature flags moved between 0.12 and 0.13 (the default `native-tls`/`rustls-tls` selection and the `connect`/`pool` config surface changed).
- Verify `cargo deny check licenses` is unchanged — reqwest 0.13 may pull a different default TLS chain than 0.12.
- Run all server tests that use the HTTP client (lettre webhook callbacks, OIDC discovery, OTLP export over HTTP, S3 sidechannel).

#### Phase 4c — zip 2 → 8 (server)

**Branch:** `refactor/zip-8-server`

Own PR. The crate was rewritten between 2.x and 3.x; subsequent majors continued to evolve the API.

- Bump `zip = { version = "8", default-features = false, features = ["deflate"] }` in `[workspace.dependencies]`.
- Audit every `ZipWriter` / `ZipArchive` call site in the server. Likely changes: `start_file`, `finish`, `write_all`, `extract`, `read_to_end`, the `FileOptions` builder.
- Verify the deflate-only feature still produces archives our consumers can extract.
- Run server media-path tests (any test that exports a `.zip`).

(Skipped: phase formerly numbered 5 — RustCrypto suite — moved to Out of scope, see Context.)

### Phase 6 — Frontend major tooling

Phase 6 is split into four PRs. The original "all in one PR" approach packed eight major bumps into a single review surface; that's impractical even with a generous reviewer.

#### Phase 6a — Lint + Type-checker tooling

**Branch:** `chore/eslint-10-bump`

- `eslint ^9 → ^10.3.0`
- `@eslint/js ^9 → ^10.0.1` (travels with eslint)
- (typescript-eslint patch was already taken in Phase 1)

Run `bun run lint` and read the diagnostic delta. If a rule was renamed or removed, update `eslint.config.*` accordingly. Do not silently disable rules — if a rule must go, document why in the commit message.

#### Phase 6b — TypeScript 6 (own PR)

**Branch:** `chore/typescript-6-bump`

- `typescript ^5.9.3 → ^6.0.3`

TypeScript 6 surfaces new strict diagnostics. Run `bun run build` (which calls `tsc && vite build`). If new type errors appear, fix the source — do not relax `tsconfig.json` strictness. If the diagnostic count is too large for a single PR, split out a precursor PR that fixes the easy diagnostics under TypeScript 5.9 first, then bump.

#### Phase 6c — Test infrastructure majors

**Branch:** `chore/jsdom-marked-types-node-bump`

- `@types/node ^22.19.15 → ^24.12.3` (next-LTS-aligned)
- `jsdom ^27.4.0 → ^29.1.1`
- `marked ^17 → ^18.0.3`

Run `bun run test:run` after the bump. jsdom 28→29 historically tightens Web API compliance — expect minor test fixes around DOMParser/CustomElements behavior. marked 18 changed renderer extension APIs — audit any custom marked extension we use in the chat-message renderer.

#### Phase 6d — UI majors (icons + router)

**Branch:** `chore/lucide-router-majors`

- `lucide-solid ^0.577.0 → ^1.14.0`
- `@solidjs/router ^0.15.4 → ^0.16.1`

**Pre-flight gate (lucide):** before opening the PR, run a script that diffs the icon export list between 0.577.0 and 1.14.0 (e.g. `npm view lucide-solid@0.577.0 main` vs `@1.14.0 main` and `cd` into the `node_modules/lucide-solid` of each install to enumerate exports). For every icon name we import (`grep -roE "from 'lucide-solid'.*\\{[^}]*\\}" client/src`), verify the name still exists in 1.14. If any icon was renamed or removed, list each call site in the PR description and update.

**Pre-flight gate (router):** read the `@solidjs/router` 0.16 release notes (or the changelog at the repo). The router has rotated APIs in past minor bumps (`useRoutes`, `Route` element vs route-config object). Compile-fail any unmigrated call sites.

### Phase 7 — E2EE crypto bump

**Branch:** `chore/vodozemac-0.10`

- `vodozemac 0.9 → 0.10` in `[workspace.dependencies]`.
- Pre-flight check confirms `vodozemac 0.10` only changes `prost 0.13 → 0.14` and `base64ct 1.6 → 1.8` against 0.9. Crypto primitives (`sha2 ^0.10.9`, `hkdf ^0.12.4`, `hmac ^0.12.1`, `curve25519-dalek ^4.1.3`, `ed25519-dalek ^2.1.1`, `aes ^0.8.4`, `chacha20poly1305 ^0.10.1`, `x25519-dalek ^2.0.1`, `matrix-pickle ^0.2.1`) are unchanged. Olm/Megolm wire and serialization formats are therefore expected to be compatible.
- **Compatibility test (must run before merge):** in a checkout with vodozemac 0.10, write an integration test that constructs an Olm session, serializes it (via the existing `vc-crypto` storage path), then loads the serialized form and decrypts a sample message. The session must round-trip. Skip the bump until this test passes.
- Run the existing E2EE message roundtrip tests in DM, group DM, and Megolm group session lifecycle.

### Phase 8 — WebRTC stack

**Branch:** `chore/webrtc-stack-bump`

The single highest-risk phase.

- `webrtc 0.11 → 0.17.1` in `[workspace.dependencies]` and in `client/src-tauri/vp8-decoder/Cargo.toml`.
- `vpx-encode 0.3 → 0.6.2`
- `env-libvpx-sys 4 → 5.1.3`
- Audit all `webrtc-rs` API call sites: `RTCPeerConnection`, `RTCRtpSender`, `RTCRtpReceiver`, `Interceptor`, `Track`, `Sample`. The crate has rotated breaking changes across 0.12 → 0.17.
- **Open question to resolve in this phase:** in `webrtc 0.17`, does `write_rtcp` actually deliver PLI? If yes, the interval-PLI workaround in client code (documented in `feedback_webrtc_rs_rtcp.md` user memory) can be removed. If no, port the workaround forward.
- **Mobile interaction (verified):** the Android client uses native Android WebRTC (Java/Kotlin) — it does not link `webrtc-rs`, so the bump does not break the Android build. However the wire protocol (SDP offer/answer shape, RTCP feedback messages, ICE candidates) the Android client observes is generated by `webrtc-rs` on the server. Any RTCP feedback semantics that change between 0.11 and 0.17 must be tested cross-platform: server bumped + Tauri client + Android client in a real call.
- **scap fork (verified):** the `Detair/scap` fork has no `webrtc` dependency (`scap` Cargo.toml lists only `futures`, `sysinfo`, `thiserror` plus per-OS native deps). The webrtc bump does not break the fork.
- Pre-merge: deploy the server image to the canary VPS. Run a 30-minute voice-call + screenshare canary with **three real clients (Tauri × 2 + Android × 1)** before merging Phase 8 to `main`.
- Post-merge: re-run `cargo audit` — `bincode 1` warning may have cleared via `webrtc-dtls`'s upgrade.

### Phase 9 — Client storage and native deps

**Branch:** `chore/rusqlite-keyring-bump`

- `rusqlite 0.32 → 0.39.0` in `client/src-tauri/Cargo.toml` (keeps `features = ["bundled"]`).
- `keyring 2 → 4.0.0` in `client/src-tauri/Cargo.toml`.
- **Open question to resolve in this phase:** does `cargo deny check` (which uses `all-features = true`) still resolve cleanly with both `rusqlite 0.39` (wants `libsqlite3-sys 0.37`) and `sqlx 0.8.6` (whose feature-gated `sqlx-sqlite` wants `libsqlite3-sys 0.30`) in the lockfile? In actual builds, `sqlx-sqlite` is never pulled (server uses `default-features = false, features = ["postgres", ...]`), but cargo-deny's `all-features` resolver may flag the conflict.
  - If it does, set explicit features in `deny.toml`'s `[graph]` table instead of `all-features = true`.
  - If it does not, no action.
- Test: client launch, secret read/write through `keyring`, sqlite cache roundtrip.
- Post-merge: re-run `cargo audit` — `derivative` warning should clear.

### Phase 10 — Cleanup and follow-ups

**Branch:** `chore/dep-update-cleanup`

- Update `deny.toml` `RUSTSEC-2026-0097` ignore comment: rewrite to acknowledge that 0.7.3, 0.8.5, 0.9.2, and 0.10.0 are all flagged, not just 0.8.5. Reaffirm the justification (the exploit requires a `log::Log` implementation that calls `rand::thread_rng()` during logging; we use `tracing`, not the `log` facade, and this remains true regardless of the rand version in tree).
- Re-run `cargo audit`, `cargo deny check all`, `bun audit`. Update `LICENSE_COMPLIANCE.md` to reflect any tree changes; the residual transitive Tauri/GTK3 chain warnings are expected and called out in that document.
- Recheck `CapSoftware/scap` upstream. If a release with the Linux Frame enum fix has shipped, drop the Detair fork in `client/src-tauri/Cargo.toml` and update CLAUDE.md memory accordingly. If not, update the inline comment's "Last re-check" date in the same file.
- `THIRD_PARTY_NOTICES.md` — there is no automation script in the repo today. If new licenses appeared in the dep tree, hand-edit the file (or out-of-scope: build a generation script in a follow-up). Diff the new tree against the old one with `cargo tree --workspace --target all --prefix none --no-default-features` before/after the plan to spot license category changes.
- Update CLAUDE.md memory entries: `project_webrtc_screen_share.md` (note webrtc 0.17 if Phase 8 landed), `feedback_webrtc_rs_rtcp.md` (revise if PLI workaround was removed). Add a new memory entry summarizing this plan and its outcomes (which phases shipped, which were deferred).

## Pre-flight gates

These must run *before* a phase's PR is opened. Each gate's failure means the phase is held or its scope shrunk — do not "discover" these issues in CI.

| Phase | Pre-flight gate |
|---|---|
| 0 | Verify the worktree commit's `cargo audit` shows `RUSTSEC-2026-0098/-0099/-0104` no longer fire on the new tree (already verified at design time; re-run before opening PR). |
| 1 | Confirm `bun audit` count after `bun update` matches the expected residual (5 of the 12 advisories cleared via override bumps; 7 remain for Phase 2). |
| 2 | Confirm overrides resolve: `bun install` then `bun pm ls vite` should show no `vite@7.x` entry; mermaid's `dompurify` should be `>=3.4.2`. |
| 3 | Read the upstream Tauri 2.11 changelog for any breaking API surface in our usage (we use plugin-shell, plugin-notification, plugin-global-shortcut). |
| 4a | None — bumps are independent within the batch. |
| 4b | Read the `reqwest 0.13` migration guide (or release notes). Identify TLS-feature renames before opening the PR. |
| 4c | Read the `zip 3` and `zip 4`+ release notes; map the API used in the server media path to the 8.x equivalents. |
| 6a | Run `bun run lint` against the existing tree first to establish a baseline diagnostic count; then bump and compare. |
| 6b | Run `bun run build` (TypeScript 5.9) baseline; bump TS to 6.0; compare diagnostics. |
| 6c | Read jsdom 28 + 29 release notes (focus on DOM API tightening); read marked 18 release notes (renderer extension API). |
| 6d | Generate icon-name diff between `lucide-solid` 0.577 and 1.14 vs. `grep -roE "from 'lucide-solid'.*\\{[^}]*\\}" client/src` import list. List renames in PR. Read `@solidjs/router` 0.16 release notes. |
| 7 | Confirm vodozemac 0.10 dep deltas (already verified: only `prost 0.13→0.14` and `base64ct 1.6→1.8`). Run the Olm session round-trip compatibility test. |
| 8 | Audit `webrtc-rs` 0.12, 0.13, 0.14, 0.15, 0.16, 0.17 release notes to catalog API breakage. Identify call sites in advance. |
| 9 | Run `cargo deny check all` against a worktree with only the rusqlite bump applied; confirm no all-features resolution conflict between rusqlite 0.39 (libsqlite3-sys 0.37) and sqlx 0.8 (sqlx-sqlite → libsqlite3-sys 0.30). If it fails, switch `deny.toml`'s `[graph]` away from `all-features = true` to explicit features. |
| 10 | None — cleanup phase. |

## Quality gates

Every phase must pass before merge:

1. `cargo fmt --check`
2. `SQLX_OFFLINE=true cargo clippy --workspace -- -D warnings`
3. `cargo test --workspace`
4. `cargo deny check` — must be green; no new ignores added without inline justification comment.
5. `cargo audit` — diff against the prior phase. The error count must be monotone-non-increasing; warning count may shift but must not introduce new direct-dep advisories.
6. Frontend phases: `bun run lint`, `bun run test:run`, `bun run build`.
7. Phases 3, 7, 8, 9: manual dev-build smoke test.
8. **Phase 8 only:** beta-canary deploy on `kaiku.pmind.de` from a temp branch, 30-min voice + screenshare canary with **three** real clients (Tauri × 2 + Android × 1) before merge.

## Merge cadence

Phases ship sequentially:

- A phase's PR opens only after the previous phase has merged to `main` and its CI is green.
- After merging Phase N to `main`, the beta deploy (`kaiku.pmind.de`) takes the new build and runs for **at least 24 hours** (or until the next reasonable check) before Phase N+1's PR opens. This soak window is the failsafe for regressions that don't show up in CI (RAM leaks, voice latency drift, OIDC certificate quirks). Phase 0 doesn't need a soak — it's already a fix.
- Phases 4a, 4b, 4c, 6a, 6b, 6c, 6d are all sequenced in order. They look like sub-phases but each is a separate PR with the same merge-and-soak rule.
- Exception: if `cargo audit` introduces a new high-severity advisory mid-plan, halt the cadence and address it before continuing.

## Risk and rollback

| Phase | Failure mode | Rollback |
|---|---|---|
| 0 | aws-sdk-s3 S3 calls fail with the new https client | revert single-commit PR |
| 1 | Patch-level npm breakage | revert `bun.lock`/`package.json` |
| 2 | Override pin breaks a transitive consumer | drop the offending override; reopen the advisory |
| 3 | Tauri 2.11 IPC behavior change | revert PR; tauri 2.11 is small surface |
| 4a | sentry/sysinfo/infer/smol_str/sentry-tracing/tokio-tungstenite API change | revert PR; per-bump damage is isolated |
| 4b | reqwest 0.12 → 0.13 TLS feature mismatch | revert PR; isolated |
| 4c | zip 2 → 8 archive output incompatible | revert PR; isolated |
| 6a | ESLint 10 rule cascade | revert PR; isolated |
| 6b | TypeScript 6 type-checker explosion | revert PR; isolated by design |
| 6c | jsdom/marked/@types/node compat | revert PR; isolated |
| 6d | Renamed lucide icon at runtime / router API change | revert PR; pre-flight gate should have caught this |
| 7 | vodozemac 0.10 changes Olm session serialization format despite verified deps | revert PR; round-trip test should have caught this |
| 8 | webrtc 0.17 breaks voice path on real clients (Tauri or Android) | revert PR; this is why Phase 8 has a canary gate |
| 9 | rusqlite 0.39 breaks bundled build, or `cargo deny --all-features` fails | revert PR; pre-flight gate should have caught this |
| 10 | Documentation/comment-only cleanup | revert PR (very low risk) |

## Verification rules

- Every version listed in this document was queried live against `crates.io/api/v1/crates/<name>` or `registry.npmjs.org/<pkg>/latest` on **2026-05-08**. No version is assumed.
- Each phase's implementing PR must re-run the same registry lookups at PR-creation time. Versions move between this design and the actual implementation; pin to whatever the registry returns then, not to the table above.
- Pre-1.0 crates are treated as breaking on every minor bump (`smol_str`, `sentry`, `vodozemac`, `tokio-tungstenite`, `sysinfo`, `init-tracing-opentelemetry`, `axum-tracing-opentelemetry`, `webrtc`, `vpx-encode`, `rusqlite`, `infer`). `keyring` is post-1.0 but treated as breaking from 2 → 4.
- All phase PRs must pass `cargo deny check`, including the licenses table. If a new dependency introduces a license not in the existing allow-list, escalate and discuss before merging.

## Open questions to resolve during execution

These remain open after the design pass and are gated by the pre-flight checks in their respective phases.

1. **Phase 8:** Does `webrtc 0.17` deliver a working `write_rtcp` / PLI path? Determines whether the interval-PLI workaround in the client can be removed.
2. **Phase 9:** Does `cargo deny check` (all-features) still resolve once `rusqlite 0.39` and `sqlx 0.8.6` coexist in the lockfile? If not, switch `deny.toml`'s `[graph]` to explicit features.
3. **Phase 7:** Is the `vodozemac 0.9` → `0.10` round-trip session-format compatible? Pre-flight verified the dep deltas are non-cryptographic (only `prost` and `base64ct`), but the round-trip test must still pass; if the serialization layer changed despite no crypto changes, postpone.
4. **Phase 10:** Has `CapSoftware/scap` cut a release that fixes the Linux Frame enum? If yes, drop the Detair fork; if no, update the inline tracking comment's "Last re-check" date.

## Execution notes

### 2026-05-09 — Phase 4b abandoned (reqwest 0.12 → 0.13 upstream-blocked)

Discovered during Phase 4b execution: bumping `reqwest` to 0.13 is blocked by **multiple** upstream crates that still require `reqwest ^0.12`:

| Crate | Version | Status | Notes |
|---|---|---|---|
| `opentelemetry-otlp` | 0.31.1 (latest) | requires `reqwest ^0.12` | Workaroundable: setting `default-features = false` and dropping `reqwest-blocking-client` from features removes the OTel-side reqwest pull. Our server uses `.with_tonic()` for all three OTLP exports (trace/metrics/logs), so the HTTP exporter path is dead code. This cleanup is worth doing on its own. |
| `oauth2` | 5.0.0 (latest, stable since 2025-01-21) | requires `reqwest ^0.12` | **Hard block.** No `oauth2` 6.x or pre-release supports reqwest 0.13. Switching off reqwest would mean choosing one of `curl`/`ureq` and rewriting our OIDC token-exchange flow. |
| `openidconnect` | 4.0.1 (latest) | inherits oauth2's reqwest requirement | Same hard block as oauth2. |

Phase 4b was abandoned without a PR. Reopen once `oauth2` ships a version that supports reqwest 0.13 (track upstream).

The opentelemetry-otlp feature-flag cleanup was extracted as a follow-up note (see Out of scope) — small, no-op when reqwest is already in tree via openidconnect, but worth doing once Phase 4b unblocks because it removes ~one MB of dead transitive wire-format code from the server binary.

The bonus that motivated Phase 4b — eliminating the `openssl 0.10.x` cluster from the OSV scan via reqwest 0.13's default rustls — is also blocked. The openssl chain comes from `reqwest 0.12`'s `native-tls` default, which is reachable through openidconnect's reqwest pull. Until oauth2/openidconnect ship reqwest 0.13 support, the openssl cluster stays.

## Out of scope (recorded for follow-up plans)

- **`reqwest 0.12 → 0.13` bump** (deferred from Phase 4b on 2026-05-09). Blocked by `oauth2 5.0.0` and `openidconnect 4.0.1`, both still pinning `reqwest ^0.12` with no upcoming release. Track upstream; reopen as a follow-up phase when oauth2 6.x (or whatever pins reqwest 0.13) ships. **Linked benefit:** eliminating the openssl 0.10.x cluster from `bun audit` / `cargo audit` will also unlock here.
- **opentelemetry-otlp default-features cleanup.** Setting `default-features = false` and dropping `reqwest-blocking-client` removes a no-op transitive (we use only the `.with_tonic()` gRPC path). Defer to the same follow-up that lands reqwest 0.13.
- **RustCrypto suite bump (sha1/sha2/hkdf/hmac → 0.11/0.13).** Blocked by `vodozemac 0.10`, which still pins `sha2 ^0.10.9`. Reopen as a follow-up plan once vodozemac (and likely also `jsonwebtoken`, `argon2`) ship versions on the new RustCrypto series.
- **`getrandom` 0.2 → 0.4 in workspace.** Blocked by transitive consumers (vodozemac, Tauri, RustCrypto) still on 0.2. Reopen with the RustCrypto follow-up.
- Tauri 2 → 3 migration (Tauri 3 is alpha as of 2026-05-08).
- Replacing `keyring`'s `secret-service` chain (the source of `derivative` and `zbus` warnings) before keyring 4 ships.
- Replacing `pulldown-cmark` or migrating server markdown rendering to `marked` parity.
- Migrating the legacy `lazy_static` call sites to `std::sync::LazyLock`.
- Updating Tauri's `wry`/`tao` GTK3 chain (blocked upstream).
- Building a `THIRD_PARTY_NOTICES.md` regeneration script (does not exist today; out-of-scope cleanup task).

## Next step

Hand off to the writing-plans skill to produce a step-by-step implementation plan, one section per phase.
