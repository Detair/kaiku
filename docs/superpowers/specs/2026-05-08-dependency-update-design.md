# Dependency Update — Design

**Date:** 2026-05-08
**Status:** Approved (brainstormed); ready for implementation plan
**Scope:** All workspace `Cargo.toml`, `Cargo.lock`, `client/package.json`, `client/bun.lock`, transitive overrides, and `deny.toml` advisory ignores.

## Context

`cargo update --workspace` reports zero changes — every Rust dependency is already at its latest version *within the existing semver bound*. Any further movement therefore requires version-bound edits and is, by definition, a breaking-version change. The frontend has the same shape: `bun update` (without `--latest`) yields only patch-level movement.

`cargo audit` reports 4 errors (1 ignored — rsa Marvin Attack; 3 covered by an in-flight worktree — rustls-webpki 0.101.7) and 27 warnings (mostly transitive Tauri/GTK3 unmaintained chains). `bun audit` reports 12 advisories (4 high, 8 moderate), all attributable to a small number of fixable direct-dep gaps and one missing override.

There is an in-flight worktree `fix/rustls-webpki-advisories` containing a single commit that drops the legacy rustls 0.21 chain by switching `aws-sdk-s3` to `default-https-client`. That fix lands as Phase 0 of this plan; the corresponding three RUSTSEC ignores are removed once it is on `main`.

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
| `axum-tracing-opentelemetry` | 0.32 | 0.33.1 | 4 | minor (pre-1.0 breaking) |
| `env-libvpx-sys` | 4 | 5.1.3 | 8 | libvpx FFI sub-crate |
| `hkdf` | 0.12 | 0.13.0 | 5 | RustCrypto suite |
| `hmac` | 0.12 | 0.13.0 | 5 | RustCrypto suite |
| `infer` | 0.16 | 0.19.0 | 4 | 3 majors |
| `init-tracing-opentelemetry` | 0.36 | 0.37.0 | 4 | minor (pre-1.0 breaking) |
| `keyring` | 2 | 4.0.0 | 9 | 2 majors; clears `derivative` warning |
| `reqwest` (server) | 0.12 | 0.13.3 | 4 | align with vc-client (already 0.13) |
| `rusqlite` | 0.32 | 0.39.0 | 9 | 7 minor majors |
| `sentry`, `sentry-tracing` | 0.47 | 0.48.1 | 4 | minor (pre-1.0 breaking) |
| `sha1` | 0.10 | 0.11.0 | 5 | RustCrypto suite |
| `sha2` | 0.10 | 0.11.0 | 5 | RustCrypto suite |
| `smol_str` | 0.2 | 0.3.6 | 4 | shared workspace dep |
| `sysinfo` | 0.38 | 0.39.0 | 4 | minor (pre-1.0 breaking) |
| `tauri` ecosystem | 2.10.x | 2.11.x | 3 | minor; aligns with npm `@tauri-apps/api` 2.11 |
| `thiserror` (vp8-decoder) | 1 | 2.0.18 | 4 | align with workspace |
| `tokio-tungstenite` | 0.28 | 0.29.0 | 4 | minor (pre-1.0 breaking) |
| `vodozemac` | 0.9 | 0.10.0 | 7 | E2EE crypto |
| `vpx-encode` | 0.3 | 0.6.2 | 8 | client video |
| `webrtc` | 0.11 | 0.17.1 | 8 | server + client + vp8-decoder |
| `zip` | 2 | 8.6.0 | 4 | 6 majors |

### Rust direct dependencies — already at latest major

`aes-gcm 0.10`, `aho-corasick 1`, `anyhow 1`, `arboard 3`, `argon2 0.5`, `aws-config 1`, `aws-sdk-s3 1`, `aws-smithy-async 1`, `aws-smithy-types 1`, `axum 0.8`, `axum-extra 0.12`, `base64 0.22`, `bitflags 2`, `blurhash 0.2`, `bs58 0.5`, `bytes 1`, `chrono 0.4`, `cpal 0.17`, `dashmap 6`, `dotenvy 0.15`, `fred 10`, `futures 0.3`, `getrandom 0.2/0.4` (split — see below), `hex 0.4`, `http-body-util 0.1`, `image 0.25`, `jsonwebtoken 10`, `lazy_static 1`, `lettre 0.11`, `mime_guess 2`, `nnnoiseless 0.5`, `nokhwa 0.10`, `openidconnect 4`, `opentelemetry 0.31` (suite), `opus 0.3`, `pulldown-cmark 0.13`, `regex 1`, `rodio 0.22`, `rustls 0.23`, `serde 1`, `serde_bytes 0.11`, `serde_json 1`, `sqlx 0.8`, `tempfile 3`, `thiserror 2`, `time 0.3`, `tokio 1`, `tokio-test 0.4`, `tokio-util 0.7`, `totp-rs 5`, `tower 0.5`, `tower-http 0.6`, `tracing 0.1`, `tracing-opentelemetry 0.32`, `tracing-opentelemetry-instrumentation-sdk 0.32`, `tracing-subscriber 0.3`, `unicode-segmentation 1`, `url 2`, `utoipa 5`, `utoipa-swagger-ui 9`, `uuid 1`, `validator 0.20`, `woothee 0.13`, `zeroize 1`.

`getrandom` is at 0.2 in our workspace; latest is 0.4.2. Held: bumping our direct usage to 0.4 does not eliminate the 0.2/0.3 transitive copies pulled by Tauri/RustCrypto/quinn. Defer until a phase where it would actually compress the dep tree.

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
| `@types/node` | 22.19.15 | 24.12.3 | 6 | LTS-aligned |
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

The `brace-expansion` override should bump from `^1.1.13` to `^5.0.6` (or be split `^1.1.13 || ^5.0.6` if any consumer hard-pins to 1.x). The `lodash-es` override should bump to `^4.18.1`. These are mechanical; they belong in Phase 1.

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
- Bump existing `overrides` to verified latest: `rollup 4.60.1→4.60.3`, `brace-expansion 1.1.13→5.0.6`, `lodash-es 4.17.24→4.18.1`, `defu 6.1.5→6.1.7`, `picomatch 4.0.4` (no change).
- This phase is 100% mechanical; CI must stay green.

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
- Build the Tauri client and verify `bun run tauri dev` and `bun run tauri build` succeed.
- Smoke test: window open, IPC invoke, plugin shell command, plugin notification permission prompt.

### Phase 4 — Rust low-risk minors batch

**Branch:** `chore/rust-minors-batch`

Single PR bumping the workspace declarations:

- `axum-tracing-opentelemetry 0.32 → 0.33.1`
- `init-tracing-opentelemetry 0.36 → 0.37.0`
- `infer 0.16 → 0.19.0`
- `reqwest 0.12 → 0.13` in `server/Cargo.toml` (align with `vc-client`)
- `sentry 0.47 → 0.48.1` and `sentry-tracing 0.47 → 0.48.1` in `client/src-tauri/Cargo.toml`
- `smol_str 0.2 → 0.3.6`
- `sysinfo 0.38 → 0.39.0`
- `thiserror 1 → 2` in `client/src-tauri/vp8-decoder/Cargo.toml` (use the existing workspace `thiserror = "2"` instead of a direct version)
- `tokio-tungstenite 0.28 → 0.29.0`
- `zip 2 → 8.6.0`
- Hoist `tempfile = "3"` to `[workspace.dependencies]` and replace the literal `tempfile = "3"` declarations in `server/Cargo.toml` (main deps) and `client/src-tauri/Cargo.toml` (dev-deps) with `tempfile.workspace = true`.

Per-bump concerns:
- `reqwest 0.12 → 0.13`: TLS feature flags moved; verify `features = ["json"]` plus default `rustls`/`native-tls` resolution still works on server.
- `sentry 0.48`: changelog notes the `tower` integration moved to a separate crate; verify our usage on Tauri client is unaffected (we use `sentry-tracing`, not the tower middleware).
- `tokio-tungstenite 0.29`: the `MaybeTlsStream` variant set widened; check both server and client websocket code compiles.
- `zip 2 → 8`: the API has been heavily restructured. We use it in the server media path. Verify `ZipWriter::start_file` / `finish` API is still used in our forms; this may be the largest single-file change in this phase.
- `smol_str 0.2 → 0.3`: API mostly stable; the `From<&str>` route changed in 0.3. Audit call sites.
- `infer 0.16 → 0.19`: signature database updated; verify our MIME detection paths still match expected types.

### Phase 5 — RustCrypto suite alignment

**Branch:** `chore/rustcrypto-suite-bump`

- `sha1 0.10 → 0.11`
- `sha2 0.10 → 0.11`
- `hkdf 0.12 → 0.13`
- `hmac 0.12 → 0.13`
- Pre-flight check: confirm `aes-gcm 0.10` does not require an upstream-coordinated bump alongside sha2 0.11. If RustCrypto has released a synchronized `aes-gcm 0.11`, include it here.
- Test crypto paths end-to-end: Argon2 password hash + verify, JWT sign + verify, HMAC media tokens, secure-storage encrypt/decrypt roundtrip.

### Phase 6 — Frontend major tooling

**Branch:** `chore/frontend-tooling-majors`

Single PR. Each bump is mechanically small but each one needs to pass lint+typecheck+tests.

- `@types/node ^22.19.15 → ^24.12.3`
- `eslint ^9 → ^10.3.0`
- `@eslint/js ^9 → ^10.0.1` (these two travel together)
- `jsdom ^27.4.0 → ^29.1.1`
- `marked ^17 → ^18.0.3`
- `lucide-solid ^0.577.0 → ^1.14.0` (post-1.0; verify icon import paths haven't moved)
- `@solidjs/router ^0.15.4 → ^0.16.1`
- `typescript ^5.9.3 → ^6.0.3`

Validation: `bun run lint`, `bun run test:run`, `bun run build`, `bun run test:e2e:frontend`.

If TypeScript 6 produces a flood of new diagnostics, split it into a follow-up commit on the same branch — but keep all bumps in this single PR for atomic review.

### Phase 7 — E2EE crypto bump

**Branch:** `chore/vodozemac-0.10`

- `vodozemac 0.9 → 0.10` in `[workspace.dependencies]`.
- Read the upstream `0.10` changelog for Olm/Megolm session-format compatibility. If serialized session blobs produced by 0.9 are not loadable in 0.10, this phase requires a one-off migration plan and gets postponed.
- Test: full E2EE message roundtrip in DM, group DM, megolm session lifecycle.

### Phase 8 — WebRTC stack

**Branch:** `chore/webrtc-stack-bump`

The single highest-risk phase.

- `webrtc 0.11 → 0.17.1` in `[workspace.dependencies]` and in `client/src-tauri/vp8-decoder/Cargo.toml`.
- `vpx-encode 0.3 → 0.6.2`
- `env-libvpx-sys 4 → 5.1.3`
- Audit all `webrtc-rs` API call sites: `RTCPeerConnection`, `RTCRtpSender`, `RTCRtpReceiver`, `Interceptor`, `Track`, `Sample`. The crate has rotated breaking changes across 0.12 → 0.17.
- **Open question to resolve in this phase:** in `webrtc 0.17`, does `write_rtcp` actually deliver PLI? If yes, the interval-PLI workaround in client code (documented in `feedback_webrtc_rs_rtcp.md` user memory) can be removed. If no, port the workaround forward.
- Pre-merge: deploy the server image to the canary VPS. Run a 30-minute voice-call + screenshare canary with two real clients before merging Phase 8 to `main`.
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

- Update `deny.toml` `RUSTSEC-2026-0097` ignore comment: rewrite to acknowledge that 0.7.3, 0.8.5, 0.9.2, and 0.10.0 are all flagged, not just 0.8.5. Reaffirm the justification (we use `tracing`, not the `log` facade).
- Re-run `cargo audit`, `cargo deny check all`, `bun audit`. Document residual warnings in `LICENSE_COMPLIANCE.md` (transitive Tauri/GTK3 chain).
- Recheck `CapSoftware/scap` upstream. If a release with the Linux Frame enum fix has shipped, drop the Detair fork in `client/src-tauri/Cargo.toml` and update CLAUDE.md memory `feedback_webrtc_rs_rtcp.md` accordingly. If not, update the inline comment's "Last re-check" date.
- Refresh `THIRD_PARTY_NOTICES.md` if any new licenses appeared in the tree (run the existing `LICENSE_COMPLIANCE` regeneration script).
- Update CLAUDE.md memory entries: `project_sqlx_test_migration.md` (no change), `project_webrtc_screen_share.md` (note webrtc 0.17 if Phase 8 landed), `feedback_webrtc_rs_rtcp.md` (revise if PLI workaround was removed).

## Quality gates

Every phase must pass before merge:

1. `cargo fmt --check`
2. `SQLX_OFFLINE=true cargo clippy --workspace -- -D warnings`
3. `cargo test --workspace`
4. `cargo deny check` — must be green; no new ignores added without inline justification comment.
5. `cargo audit` — diff against the prior phase. The error count must be monotone-non-increasing; warning count may shift but must not introduce new direct-dep advisories.
6. Frontend phases: `bun run lint`, `bun run test:run`, `bun run build`.
7. Phases 3, 7, 8, 9: manual dev-build smoke test.
8. **Phase 8 only:** beta-canary deploy on `kaiku.pmind.de` from a temp branch, 30-min voice + screenshare canary with two real clients before merge.

## Risk and rollback

| Phase | Failure mode | Rollback |
|---|---|---|
| 0 | aws-sdk-s3 S3 calls fail with the new https client | revert single-commit PR |
| 1 | Patch-level npm breakage | revert `bun.lock`/`package.json` |
| 2 | Override pin breaks a transitive consumer | drop the offending override; reopen the advisory |
| 3 | Tauri 2.11 IPC behavior change | revert PR; tauri 2.11 is small surface |
| 4 | reqwest/zip/smol_str API change unhandled | revert PR; per-bump damage is isolated |
| 5 | RustCrypto cross-version incompatibility | revert PR; downstream `argon2`, `vodozemac` consume sha2 — verify before merge |
| 6 | TS6 / ESLint10 type or rule cascade | split TS6 into its own follow-up; keep eslint10 + jsdom29 + marked18 |
| 7 | vodozemac 0.10 changes Olm session serialization format | revert PR; gate behind a session-format compatibility test before merge |
| 8 | webrtc 0.17 breaks voice path on real clients | revert PR; this is why Phase 8 has a canary gate |
| 9 | rusqlite 0.39 breaks bundled build, or `cargo deny --all-features` fails | revert PR; Phase 9 is isolated |
| 10 | Documentation/comment-only cleanup | revert PR (very low risk) |

## Verification rules

- Every version listed in this document was queried live against `crates.io/api/v1/crates/<name>` or `registry.npmjs.org/<pkg>/latest` on **2026-05-08**. No version is assumed.
- Each phase's implementing PR must re-run the same registry lookups at PR-creation time. Versions move between this design and the actual implementation; pin to whatever the registry returns then, not to the table above.
- Pre-1.0 crates are treated as breaking on every minor bump (`smol_str`, `sentry`, `vodozemac`, `tokio-tungstenite`, `sysinfo`, `init-tracing-opentelemetry`, `axum-tracing-opentelemetry`, `webrtc`, `vpx-encode`, `keyring` is now post-1.0 but treated as breaking from 2 → 4, `rusqlite`, `infer`, `nokhwa`).
- All phase PRs must pass `cargo deny check`, including the licenses table. If a new dependency introduces a license not in the existing allow-list, escalate and discuss before merging.

## Open questions to resolve during execution

1. **Phase 8:** Does `webrtc 0.17` deliver a working `write_rtcp` / PLI path? Determines whether the interval-PLI workaround in the client can be removed.
2. **Phase 9:** Does `cargo deny check` (all-features) still resolve once `rusqlite 0.39` and `sqlx 0.8.6` coexist in the lockfile? If not, switch `deny.toml`'s `[graph]` to explicit features.
3. **Phase 5:** Does `aes-gcm 0.10` link cleanly against `sha2 0.11`? RustCrypto sometimes lags one cycle on aead crates. Verify before merging the suite bump; if not, hold sha2/sha1/hkdf/hmac and revisit.
4. **Phase 7:** Is `vodozemac 0.9` → `0.10` Olm/Megolm session-blob compatible? If not, gate the bump behind a one-shot migration plan instead of merging here.
5. **Phase 10:** Has `CapSoftware/scap` cut a release that fixes the Linux Frame enum? If yes, drop the Detair fork; if no, update the inline tracking comment.

## Out of scope (recorded for follow-up plans)

- Tauri 2 → 3 migration (Tauri 3 is alpha as of 2026-05-08).
- Replacing `keyring`'s `secret-service` chain (the source of `derivative` and `zbus` warnings) before keyring 4 ships.
- Replacing `pulldown-cmark` or migrating server markdown rendering to `marked` parity.
- Migrating the legacy `lazy_static` call sites to `std::sync::LazyLock`.
- Updating Tauri's `wry`/`tao` GTK3 chain (blocked upstream).

## Next step

Hand off to the writing-plans skill to produce a step-by-step implementation plan, one section per phase.
