# v1.0 Family Release — Before-Rollout Backlog (Phase 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close every review finding rated *Before rollout* that Phase 1 (PRs #658, #660–#666) and the other Phase-2 plans (push, i18n, athene deployment) do not already close, so the family can be onboarded without a known security gap, a misleading UI or a voice path that fails silently.

**Architecture:** 24 PR-sized tasks (1–23 plus 11b) in six batches (server security, server, desktop UI, desktop native voice, Android UI/UX, ops). The batches are file-disjoint across areas and can run in parallel worktrees; order inside a batch is given per task. July features that are not family-critical (forum and announcement channels, scheduled events, membership screening) are **hidden behind a server flag** instead of being fixed or given UI (spec freeze rule). Four new cross-stream contracts (C22–C25) are proposed below.

**Tech Stack:** Rust (axum 0.8, sqlx, fred/Valkey, webrtc-rs, cpal), Solid.js + TypeScript (vitest, ESLint), Tauri 2, Kotlin/Compose (Hilt, Ktor, kotlinx-serialization, Coil), GitHub Actions, Python 3 guardrails.

**Spec:** `docs/superpowers/specs/2026-10-20-v1-family-release-goal-design.md`
**Review:** `docs/reviews/2026-10-22-v1-family-release-readiness.md` (Before-rollout list; full IDs in the maintainer's private archive — this plan names IDs and fixes, never exploit paths)
**Contracts:** `docs/superpowers/plans/2026-10-22-v1-family-contracts.md` (C1–C14, C11/C12 clarifications) plus the proposals below.
**Builds on:** `origin/main` @ `047bd54e` (#658 release pipeline merged) and the open Phase-1 PRs #660/#662 (Android), #661/#665 (desktop), #663/#666 (server). Every task here starts after the PRs it names are merged. Sibling Phase-2 plans: `2026-10-23-v1-push-phase2.md` (push), `2026-10-23-v1-i18n-phase2.md` (strings), and the private athene deployment plan (`kaiku-reviews-private/plans/2026-10-22-athene-kaiku-deploy.md`: backups, alerting, image pins, live config).

---

## 1. Triage of every *Before rollout* finding

Status: **FIXED** = closed by a merged/open Phase-1 PR; **OTHER** = owned by a sibling plan; **OPEN** = not handled anywhere yet. Decision for OPEN rows: **keep** (task here), **defer** (post-1.0), **drop** (not relevant for the family audience).

### Lanes 1–3 (security, server, desktop)

| ID | Finding (short) | Status | Decision | Reason / owner |
|---|---|---|---|---|
| S-03 | SSRF redirects, push DNS re-resolve, http push, no subscription cap | FIXED #666 T9 (no-redirect pinned client, https only) + OTHER push T2 (10 devices/user) | — | Fully covered between #666 and the push plan |
| S-04 | Backend port possibly public, XFF trusted from anyone | FIXED #666 T10 (`RATE_LIMIT_TRUSTED_PROXIES`) + athene (Traefik only) | — | — |
| S-05 | Admin elevation without re-auth, not bound to session | OPEN | **keep → Task 2** | Stolen admin token must not reach the admin API |
| S-06 | Backup codes regenerable with only an access token | OPEN | **keep → Task 2** | Same re-auth helper as S-05 |
| S-07 | No reliable kill switch for a stolen phone | OPEN | **keep → Task 1** | Threat #2 of the spec; ban/revoke/logout must end sessions and sockets |
| S-08 | Public file endpoint serves any key | FIXED #666 T8 | — | — |
| S-09 | `EVERYONE_FORBIDDEN` misses the four July permission bits | OPEN | **keep → Task 3** | One-line fix plus a classification test |
| S-10 | Updater key in PR builds, no least privilege, mutable action refs | FIXED #658 (key only in `release.yml` + Environment, `createUpdaterArtifacts=false` in CI) | **keep rest → Task 23** | `ci.yml`/`security.yml`/`tauri-build.yml` lack top-level `permissions:`; `@master`/`@main` refs remain |
| S-11 | Unsigned `:latest` deploys, broken release image, client built as root | FIXED #658 (C8 image, no `latest` from main) + athene (pinned `<ver>`, no browser SPA per athene D1) | — | — |
| S-12 | mermaid/DOMPurify advisories; CSP `connect-src https:` | OPEN | **keep advisories → Task 22**; **defer CSP** | `bun audit --prod` today: dompurify ≤3.4.11, seroval ≤1.5.2, katex, mermaid <11.16.1. CSP must allow a user-chosen server; mermaid already runs `securityLevel: "strict"` |
| S-13 | Backups unencrypted, on-host, `.env` not escrowed | OTHER athene T6–T7 | — | rclone crypt + off-host pull + SOPS |
| S-14 | Unpinned stateful images | OTHER athene T1–T3 | — | postgres 16 / valkey 8.1 / RustFS pinned with renovate |
| S-15 | Repo compose cannot reproduce live | FIXED #666 T11 + athene owns live config | — | — |
| S-16 | Protected `/auth/*` routes unlimited, sync Argon2 | OPEN | **keep → Task 2** | Brute force of the current password with a stolen token |
| S-17 | Forum endpoints guild-level checks; pending members; filter bypass | OPEN | **keep via flag → Task 4** | Forum and screening off on the server; their endpoints answer 404 |
| B-07 | Presence clobbered across devices, friends-only | OPEN | **keep → Task 7** (cut 2nd) | "Dad shows offline while gaming" confuses evening voice |
| B-08 | Push dispatcher shape | OTHER push T1–T4 | — | — |
| B-09 | DM calls: channel-only ring, 409 lock, abandoned call | OPEN (server) / push ringing out of scope in push plan | **keep server → Task 8** (cut 1st); **defer call push** | Re-calling a relative must not fail for 90 s |
| B-10 | Unread counts own messages, threads, hidden channels | OPEN | **keep → Task 6** | Permanent wrong badges and channel-name leak |
| B-11 | Crosspost payload/"Deleted User"; follow into any channel | OPEN | **keep via flag → Task 4** | Announcements off → no follows, no crossposts |
| B-12 | Event reminders unreliable | OPEN | **keep via flag → Task 4**; **defer reminder fixes** | Events off for v1.0 |
| B-13 | Core-flow test gaps | FIXED in large part (#663 `ws_e2e`, #666 voice/crosspost tests) | **drop as a task** | Every task below adds its own tests |
| D-07 | E2EE DM dead path, false padlock | FIXED #661 T5 | — | — |
| D-08 | PTT focus-only; global binding swallows game keys; fires while typing | OPEN | **keep → Task 13** (scoped); **defer non-consuming hook** | Gaming evenings; a hook crate is X11/accessibility-dependent |
| D-09 | Notifications gated on `document.hidden` | OPEN | **keep → Task 13** | Kaiku visible on a 2nd monitor = no notification |
| D-10 | Close-to-tray traps users, mic live while hidden | OPEN | **keep → Task 14** | User "hangs up" by closing and keeps transmitting |
| D-11 | Native voice/WS path unfinished | FIXED #665 T8 (`ws_send`, token-aware WS, raw passthrough) + desktop P1 T11 (ICE servers, pending) | **keep residual → Task 15** | ICE candidates before remote description are not buffered; duplicate `voice:*` listeners |
| D-12 | Native audio hard-codes 48 kHz mono, silent failures, no device switch, per-user volume no-op | OPEN | **keep → Task 15** | Native is now the only desktop voice path (Option A) |
| D-13 | Native capture panics on frame-size mismatch | OPEN | **keep → Task 16** | Sharing a game window must not crash |
| D-14 | Announcement/forum never unread on desktop | OPEN | **drop via flag** (Tasks 4, 9) | Those channel types are hidden |

### Lanes 4–6 (Android, UI, release/ops)

| ID | Finding (short) | Status | Decision | Reason / owner |
|---|---|---|---|---|
| A-05 | Images never load | FIXED #662 T8 | — | — |
| A-06 | Errors never shown | FIXED #662 T6/T7 | — | — |
| A-07 | Back from voice hangs up; overlay unreachable | OPEN (re-dial guard only, #662) | **keep → Task 19** | Checking a link must not end the call |
| A-08 | Audio routing no-op | FIXED #662 T6 | — | Device row in the maintainer checklist |
| A-09 | Zombie WS | FIXED #662 T4; re-subscribe/catch-up = Android P1 T5 (waits #663) | — | — |
| A-10 | First run: empty URL, bare host rejected, no change-server, Register always shown | OPEN | **keep → Task 17** | First minute decides whether a relative gives up |
| A-11 | No join-server UI, no auto-select | OPEN | **keep auto-select + auto-join → Tasks 5, 17**; **defer invite dialog** | Admin-created accounts are added to the family server (C23) |
| A-12 | DM title UUID, DM unread not live, mark-read no body | FIXED #662 T9 (title, body) | **keep rest → Task 18** | DM unread still not updated live |
| A-13 | Unread suppressed after leaving a channel; guild `/read` never sent | OPEN | **keep → Task 18** | — |
| A-14 | Logout leaves session, WS, voice and data alive | OPEN (push T7 adds unregister-first) | **keep → Task 19** | Shared tablet; mic after logout |
| A-15 | No notification permission flow/channels | FIXED #662 (manifest) + OTHER push T5 | — | — |
| A-16 | Release build never exercised | FIXED #658 (assembleRelease + R8 + APK verify in CI) | tests blocking → Task 23 | — |
| A-17 | EncryptedSharedPreferences read-time crash loop | OPEN | **keep → Task 20** | App unusable after an OS update until Clear Data |
| A-18 | Category UUID headers | FIXED #662 T9 | — | — |
| A-19 | Dead Edit item, UTC timestamps, typing UUIDs, reactions not rendered | Edit FIXED #662 T7 | **keep rest → Task 20** | Times 1 h off in CET; 👍 invisible |
| A-20 | No photo upload from the phone | OPEN | **keep → Task 21** (cut 1st per spec) | Spec item 6 |
| A-21 | WebRTC factory/ADM never released | OPEN | **keep → Task 19** | Same files as A-07 |
| U-04 | Forum posts not clickable; announcement follow without UI | OPEN | **keep via flag → Tasks 4, 9** | Hidden, no UI built |
| U-05 | Solarized themes fail contrast, offered in onboarding | OPEN | **keep → Task 10** (hide) | — |
| U-06 | Wrong on-colour pairs on solid fills (+ Android `NordOnError`) | OPEN | **keep → Task 10** | CLAUDE.md contrast rules |
| U-07 | Non-existent colour tokens | OPEN | **keep → Task 10** | Delete/Leave items unstyled |
| U-08 | Invite link flow broken (`tauri://` links) | OPEN | **keep reduced → Task 11** | athene serves no browser SPA (D1), so links cannot work: copy the code |
| U-09 | Register shown when registration is closed | OPEN | **keep → Task 11** (desktop), **Task 17** (Android) | — |
| U-10 | Empty Home after onboarding | OPEN | **keep → Tasks 5, 12** | Auto-join + land on the family server |
| U-11 | Voice-join toast has no reason | OPEN | **keep → Task 11** | Mic denied → user gives up |
| U-12 | "guild" in user-facing text, `canis-recovery-key.txt`, UUID voice tooltip | Android FIXED #662; desktop copy in migrated files OTHER i18n (copy-fix rule) | **keep rest → Task 11** | Tooltip logic, file names, files outside i18n scope, catalog guard |
| U-13 | No string catalog (web) | FIXED #661 T1 + OTHER i18n | — | — |
| U-14 | Android strings not in resources | OTHER i18n T10–T12 | — | — |
| U-15 | Escape in a modal marks read; error-dismiss submits form | OPEN | **keep → Task 11b** (with a11y basics) | Functional bugs |
| R-06 | `tauri-build.yml` releases on tags | FIXED #658 | — | — |
| R-07 | Updater endpoint ignores pre-releases | FIXED #658 + maintainer M7 | — | — |
| R-08 | Prod runs `:latest` = main | FIXED #658 + athene `<ver>` pin | — | — |
| R-09 | Nothing notifies the maintainer | OTHER athene T5–T7 (blackbox probe, backup alerts) | — | — |
| R-10 | Repo compose ≠ live | FIXED #666 T11; live = athene | — | — |
| R-11 | Backups same disk, plaintext, secrets not backed up | OTHER athene T6–T7 + maintainer escrow | — | — |
| R-12 | Desktop code signing | maintainer decision (M4/M5 hooks in #658) | — | — |
| R-13 | Security audit drift, unpinned actions, no Dependabot | partly (#659/#664 keep-green) | **keep → Task 23**; **defer** ignore-list unification | — |
| R-14 | Advisory gates (gitleaks, Android tests, Windows) | key part FIXED #658 | **keep → Task 23** | Windows is a family platform; Android tests now pass (249/249 in #660, 314/314 in #662) |
| R-15 | Stale release body | FIXED #658 | — | — |
| R-16 | Stateful images on rolling tags | OTHER athene | — | — |
| U-01, S-08 | (downgraded Blockers) | FIXED #661 T5, #666 T8 | — | — |

Not *Before rollout*, recorded for the coordinator: the QR-redeem value binding (residual of Blocker S-01) is **deferred** — Valkey is now password-protected and isolated (#666 T11, athene T2). D-15 (updater init not idempotent, rated Post-1.0) is folded into Task 14 because the updater is live since #665.

---

## 2. Proposed contracts (C22+; coordinator may renumber — C15–C17 are taken by the i18n plan, C15–C18 are proposed by the push plan)

**C22 — Optional features (server → desktop; Android unaffected via C5).**
Env `KAIKU_OPTIONAL_FEATURES` = comma list of `forum`, `announcements`, `events`, `screening` (unset/empty = all off; unknown name = startup warning, ignored). `GET /api/settings` gains
`"features": { "forum_channels": bool, "announcement_channels": bool, "scheduled_events": bool, "membership_screening": bool }`.
While off: creating or retyping a channel to that type → 400 `FEATURE_DISABLED`; the feature's routes (`/api/channels/{id}/posts|tags|follow|followers`, `/api/forum/*`, `/api/guilds/{id}/events*`, `/api/guilds/{id}/screening*`) → 404 `FEATURE_DISABLED`; existing channels of a disabled type are still listed (clients hide them); crossposting and the event-reminder scheduler do not run; new members are never placed in `pending`. A client that finds no `features` field (older server) treats every feature as on.

**C23 — Admin-created accounts join servers (server → desktop admin UI, ops doc).**
`POST /api/admin/users` (C4) body gains `"guild_ids": [uuid]` (optional, default `[]`, max 10). All ids must exist or the request fails with 400 and creates nothing. The user becomes an active member of each guild (read state initialised as for an invite join). Response unchanged (`UserSummary`).

**C24 — Re-authentication for sensitive actions (server → desktop).**
`POST /api/admin/elevate` body: `{ "password": string, "mfa_code": string|null, "reason": string|null }`. `POST /auth/mfa/backup-codes` body: `{ "mfa_code": string }` (a current TOTP code; backup codes are not accepted). Errors: 400 `VALIDATION_ERROR` (missing field), 400 `MFA_CODE_REQUIRED` (MFA on, no code), 401 `INVALID_CREDENTIALS` (wrong password or code), 400 `REAUTH_UNAVAILABLE` (account has no local password), 429 when rate-limited. An elevation belongs to the access token's session (`sid`); other sessions of the same admin are not elevated.

**C25 — Session end semantics (server → all clients).**
A global ban, an admin ban, a single-session revoke, "revoke all other sessions" and a password change with `revoke_others` end the affected sessions **immediately**: their access tokens fail (401), their refresh fails (401, or 403 `ACCOUNT_BANNED` for a banned user, also on login), and their open sockets receive an `error` event `SESSION_REVOKED` followed by close 1008 (C11 → refresh → login). `POST /auth/logout` with only the bearer token ends the caller's session; the refresh token in the body/cookie becomes optional.

---

## Global Constraints

- Repo rules (`CLAUDE.md`): never commit to `main`; one branch + worktree per task (`fix/…`, `feature/…`, `refactor/…`); squash-merge PRs. Commit `type(scope): subject`, ≤72 chars, imperative, scopes only `auth, voice, chat, db, api, ws, ratelimit, infra, client, crypto`; end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Spec freeze:** no new features. A July feature that is broken is hidden behind C22, not fixed and not given UI.
- **Public repo:** security changes are described by behaviour and test only — no exploit narratives in code, commits, PR bodies or CHANGELOG.
- **Infrastructure is the maintainer's.** No task needs a live server, a secret, an Environment or a repo setting; every task merges green without the maintainer actions in §4. Deploying to athene needs the maintainer's explicit OK.
- **i18n-ready (spec + i18n plan C15/C16):** every new or changed user-facing string goes through `t()` (web, key `area.screen.purpose`, area files `client/src/i18n/en/<area>.ts` once i18n Task 1 lands) or `strings.xml` (Android, mirrored snake_case name). Error text only via `errorText()` (i18n C16) once it exists. Copy says **"server"**, never "guild".
- UI colours only from tokens (`CLAUDE.md` "UI Contrast Rules"): readable text `text-text-primary`/`text-text-secondary`, on-colour text `text-on-accent`/`text-on-danger`/`text-on-success`, never accent-on-accent, no text opacity below 50 %. Android: `MaterialTheme.colorScheme` only.
- License: only MIT, Apache-2.0, BSD-2/3, ISC, Zlib, MPL-2.0 (+ the others allowed in `CLAUDE.md`); `cargo deny check licenses` whenever a crate is added. The only new crate in this plan is `rubato` (MIT) in Task 15, if the implementer does not keep the in-tree resampler.
- New env vars are documented in `docs/admin-guide/configuration/environment.md` (C9; CI enforces).
- Gates per area (run before every push):
  - Server: `make docker-up`, `cargo nextest run -p vc-server`, `cargo fmt --check`, `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`; commit `.sqlx/*.json` if a `query!` macro changes.
  - Desktop TS: `cd client && bunx tsc --noEmit && bun run lint && bun run test:run` (+ `bun run i18n:check` once i18n Task 3 is merged).
  - Desktop Rust: `cargo fmt --check`, `cargo test -p vc-client --lib`, clippy clean on touched files.
  - Android: push the branch; CI `Android Build` green and the task's tests shown as passed in the log (no local JDK).
  - Ops: `python3 -m unittest discover -s scripts/tests -v`, `python3 scripts/check_ci_guardrails.py`.
- CHANGELOG `[Unreleased]` entry for every user-visible change, from the user's perspective (Android entries prefixed "Android:").
- Review gate (`docs/developer-guide/agents/multi-model-workflow.md`): every PR gets a Claude review (Fable for tasks marked *security/concurrency*, Sonnet otherwise) **plus one external model** (Gemini 4 Argon, Codex GPT-6-Astra, or Kimi K3 while those are rate-limited).
- Shared files: `ws/handlers.rs`, `auth/mod.rs`, `ci.yml`, `AuthRepository.kt`, `MessageItem.kt` and `stores/websocket/index.ts` are also edited by sibling plans. The later-merging PR rebases; never reformat unrelated code.
- **Cut order if time runs short** (cut from the top): Task 8 → Task 21 → Task 7 → Task 16 → Task 20 (reactions/typing part) → Task 6. **Never cut:** Tasks 1, 2, 3, 4, 9, 10, 15, 19 (logout part), 22, 23.

## Review Focus

1. **Session ended while the phone is offline** (ban or revoke, then the phone wakes days later): the app must land on Login once — not loop on refresh/reconnect and not keep showing cached data as if signed in. Test: `test_revoked_session_refresh_and_ws_are_refused` (Task 1) + `auth rejected logout clears repositories` (Task 19).
2. **Server with an existing announcement channel and follow row, flag off** (a self-hoster upgrading, or beta data): sending into the source channel must succeed with no crosspost, listings must not 500, and the desktop must hide the channel. Tests: `test_existing_announcement_channel_works_without_crosspost_when_disabled` (Task 4), `hidden channel types are not listed` (Task 9).
3. **Admin with two sessions (PC + laptop) elevates on one**: the other stays non-elevated; a wrong MFA code repeated 11 times is throttled and never elevates. Tests: `test_elevation_is_bound_to_session`, `test_reauth_attempts_are_rate_limited` (Task 2).
4. **Headset whose capture side only offers 44.1 kHz stereo, or a USB mic unplugged mid-call**: voice is audible after resampling, or the user sees "Microphone stopped working" — never "Connected" with a dead mic. Tests: `falls_back_to_device_default_and_resamples`, `capture_stream_error_emits_audio_error` (Task 15).
5. **User closes the window during a voice call**: Kaiku must not keep transmitting unnoticed — the tray shows the call with "Leave voice", and the user gets one notice. Test: `close_in_voice_hides_with_voice_notice` (Task 14) + manual checklist row.

---

## 3. Tasks

### Batch A — Server security hardening

### Task 1: Ending sessions immediately — bans, revokes, logout (S-07, C25) · *security/concurrency* · **Opus**

Branch `fix/session-kill-switch`. After #663 and #666 merge (rebases over their `ws/handlers.rs` and `auth/middleware.rs`).

**Files:**
- Modify: `server/src/auth/middleware.rs` (`AuthUser.session_id`, ban check in `authenticate`), `server/src/auth/login.rs` (login, OIDC callback and refresh refuse banned users; `logout` by session id), `server/src/auth/profile.rs` (`update_password` revokes by session id), `server/src/auth/sessions.rs` (revoke one / all others), `server/src/auth/revocation.rs`, `server/src/auth/queries.rs`, `server/src/auth/error.rs`, `server/src/admin/users.rs` (ban handler), `server/src/ws/handlers.rs` (socket remembers its `sid`; internal `session_revoked` message), `server/src/chat/uploads.rs` (`?token=` download path honours the denylist), `CHANGELOG.md`
- Create: `server/tests/integration/session_revocation.rs` (+ one `mod` line in `main.rs`)

**Interfaces:**
- Produces:
  - `AuthUser.session_id: Option<Uuid>` — parsed from `claims.sid` (None for legacy tokens).
  - `pub async fn is_user_globally_banned(pool: &PgPool, user_id: Uuid) -> sqlx::Result<bool>` in `auth/queries.rs` (active = `expires_at IS NULL OR expires_at > now()`).
  - `pub async fn delete_user_sessions_except(tx: &mut PgConnection, user_id: Uuid, keep: Option<Uuid>) -> sqlx::Result<Vec<Uuid>>` (`DELETE … RETURNING id`; `keep = None` deletes all).
  - `pub async fn end_sessions(state: &AppState, user_id: Uuid, session_ids: &[Uuid]) -> Result<(), AuthError>` in `auth/revocation.rs`: denylists each id for `jwt_access_expiry` (existing `revoke_session`) and publishes `{"type":"session_revoked","session_ids":[…]}` on Valkey `user:{user_id}`. The socket loop consumes it (never forwards it): if its own sid is listed it sends `error` `SESSION_REVOKED`, then closes 1008.
  - `AuthError::AccountBanned` → 403, code `ACCOUNT_BANNED`.
  - `logout`: the refresh token is optional; with none, the session is `auth.session_id`.
- Consumes: `ws_connect`/`next_ws_event` test helpers from #663.

- [ ] **Step 1: Failing tests** in `session_revocation.rs`:
  - `test_banned_user_cannot_login_or_refresh` → login and refresh answer 403 `ACCOUNT_BANNED`.
  - `test_ban_ends_existing_tokens_and_sockets` → after the admin ban, `GET /auth/me` with the old access token is 401, and the open socket receives `error` `SESSION_REVOKED` then close code 1008 within 2 s.
  - `test_password_change_revoke_others_with_bearer_only` → sessions A and B; A posts `/auth/me/password` with `revoke_others: true` and no cookie/header → B's access token 401, B's refresh 401, A still 200.
  - `test_revoke_one_session_closes_only_that_socket` → two sockets with different sids; `DELETE /auth/sessions/{B}` → only socket B closes.
  - `test_logout_without_refresh_token_ends_current_session` → bearer-only logout 200; same access token 401; that session's refresh 401.
  - `test_revoked_session_refresh_and_ws_are_refused` (Review Focus 1) → revoke, then refresh 401 and a new WS handshake refused (401/403 or 1008).
  - `test_attachment_download_token_respects_revocation`.
- [ ] **Step 2: Run** `cargo test -p vc-server --test integration session_revocation` → FAIL.
- [ ] **Step 3: Implement** as specified; ban check runs after the user lookup in `authenticate` and in the WS auth path.
- [ ] **Step 4: Run** the tests + server gates → PASS.
- [ ] **Step 5: CHANGELOG** `### Security`: "Banning an account, signing out a session, or changing your password with 'sign out other devices' now ends those sessions right away, including open connections." `### Fixed`: "Signing out of the desktop and Android apps now always ends the session on the server."
- [ ] **Step 6: Commit** `fix(auth): end banned and revoked sessions immediately`

---

### Task 2: Re-authentication for admin elevation and backup codes; limits on protected auth routes (S-05, S-06, S-16, C24) · *security* · **Opus**

Branch `fix/auth-reauth`. After Task 1 (uses `AuthUser.session_id`).

**Files:**
- Create: `server/src/auth/reauth.rs`, `server/tests/integration/reauth.rs` (+ `mod` line)
- Modify: `server/src/auth/types.rs` (`ReauthRequest`, `BackupCodesRequest`), `server/src/auth/mfa.rs` (`mfa_generate_backup_codes` takes the body; backup-code checks via `spawn_blocking`), `server/src/auth/mod.rs` (rate-limit layer on sensitive protected routes), `server/src/admin/types.rs` (`ElevateRequest`), `server/src/admin/system.rs` (`elevate_session`), `server/src/admin/middleware.rs` (`require_elevated` matches the session), `server/src/admin/queries.rs` (elevated-session lookup by session), the Redis elevated-status cache (key by session id instead of user id), `docs/admin-guide/ops/creating-accounts.md` (elevate example), `CHANGELOG.md`

**Interfaces:**
- Produces:
  - `#[derive(Deserialize, Validate, ToSchema)] pub struct ElevateRequest { pub password: String, pub mfa_code: Option<String>, pub reason: Option<String> }`; `pub struct BackupCodesRequest { pub mfa_code: String }`.
  - `pub async fn verify_reauth(state: &AppState, user_id: Uuid, password: Option<&str>, mfa_code: Option<&str>, require_password: bool, client_ip: IpAddr) -> Result<(), AuthError>` — Argon2 in `tokio::task::spawn_blocking`; TOTP required when the user has MFA; each failure calls `RateLimiter::record_failed_auth`. Errors per C24.
  - `elevate_session` binds the elevation to `auth.session_id` (no more `find_latest_active_session_id`); `require_elevated` looks up `(user_id, session_id)`.
  - Routes `/auth/me/password`, `/auth/mfa/verify`, `/auth/mfa/disable`, `/auth/mfa/backup-codes` and `/api/admin/elevate` get `rate_limit_by_user` + `with_category(RateLimitCategory::AuthOther)`.
- Consumes: `AuthUser.session_id` (Task 1).

- [ ] **Step 1: Failing tests** in `reauth.rs`:
  - `test_elevate_requires_password` → `{}` 400; wrong password 401 `INVALID_CREDENTIALS`; right password 200.
  - `test_elevate_requires_totp_when_mfa_enabled` → no code 400 `MFA_CODE_REQUIRED`; valid TOTP 200.
  - `test_elevation_is_bound_to_session` → admin sessions A and B; elevate on A; `GET /api/admin/users` on B → 403 elevation required, on A → 200.
  - `test_backup_code_regeneration_requires_totp` → no body 400; a backup code as `mfa_code` 401; valid TOTP 200 with 10 codes.
  - `test_reauth_attempts_are_rate_limited` → 11 wrong elevation attempts within the window → the 11th is 429 and a correct one right after is still refused.
  - `test_password_change_is_rate_limited` → same for `/auth/me/password`.
- [ ] **Step 2: Run** `cargo test -p vc-server --test integration reauth` → FAIL.
- [ ] **Step 3: Implement**; update the elevate example in `creating-accounts.md` to the C24 body.
- [ ] **Step 4: Run** tests + gates → PASS.
- [ ] **Step 5: CHANGELOG** `### Security`: "Unlocking admin actions and creating new MFA backup codes now ask for your password (and authenticator code if MFA is on); password and MFA endpoints are rate limited."
- [ ] **Step 6: Commit** `fix(auth): require re-authentication for elevation and backup codes`

---

### Task 3: @everyone can never hold the July permission bits (S-09) · *security* · **Sonnet**

Branch `fix/everyone-forbidden-bits`. Independent.

**Files:** Modify `server/src/permissions/guild.rs` (`EVERYONE_FORBIDDEN`, new `EVERYONE_ALLOWED`); create `server/migrations/20261110000000_strip_everyone_forbidden.sql`; `CHANGELOG.md`.

**Interfaces:**
- `EVERYONE_FORBIDDEN` additionally contains `MANAGE_POSTS`, `SEND_ANNOUNCEMENTS`, `MANAGE_EVENTS`, `MANAGE_WEBHOOKS` (this also makes such roles non-self-assignable via `is_role_self_assignable`).
- `pub const EVERYONE_ALLOWED: Self` — every remaining bit, written out explicitly.
- Migration: `UPDATE guild_roles SET permissions = permissions & ~((1::bigint << 26) | (1::bigint << 27) | (1::bigint << 28) | (1::bigint << 29)) WHERE is_default;`

- [ ] **Step 1: Failing tests** (in-module): `every_permission_bit_is_classified` → `EVERYONE_FORBIDDEN | EVERYONE_ALLOWED == GuildPermissions::all()` and `EVERYONE_FORBIDDEN & EVERYONE_ALLOWED` is empty; `july_bits_are_forbidden_for_everyone` → `!GuildPermissions::MANAGE_WEBHOOKS.validate_for_everyone()` (same for the other three).
- [ ] **Step 2: Run** `cargo test -p vc-server --lib permissions::guild` → FAIL.
- [ ] **Step 3: Implement** + migration.
- [ ] **Step 4: Run** + gates (migration applies in `nextest`) → PASS.
- [ ] **Step 5: CHANGELOG** `### Security`: "The default role can no longer be given webhook, post, event or announcement management rights."
- [ ] **Step 6: Commit** `fix(api): forbid July management bits on the default role`

---

### Batch B — Server

### Task 4: Optional features behind a server flag (C22; U-04, S-17, B-11, B-12, D-14 server half) · *security* · **Opus**

Branch `feature/optional-features-flag`. After #666 (uses `chat/send.rs`).

**Files:**
- Create: `server/src/features.rs`, `server/tests/integration/optional_features.rs` (+ `mod` line)
- Modify: `server/src/lib.rs`/`main.rs` (module; skip spawning the event scheduler when events are off), `server/src/config.rs` (`optional_features: OptionalFeatures` from `KAIKU_OPTIONAL_FEATURES`), `server/src/settings/mod.rs` (`features`), `server/src/chat/mod.rs` and `server/src/guild/mod.rs` (route layers), `server/src/chat/channels.rs` (create/update type check), `server/src/chat/send.rs` (skip crosspost when announcements are off), `server/src/guild/invites.rs` + `server/src/discovery/queries.rs` (never `pending` when screening is off), `docs/admin-guide/configuration/environment.md`, `CHANGELOG.md`

**Interfaces:**
- Produces:
  - `#[derive(Clone, Copy, Debug, Default, PartialEq)] pub struct OptionalFeatures { pub forum: bool, pub announcements: bool, pub events: bool, pub screening: bool }` with `pub fn parse(list: &str) -> (Self, Vec<String>)` (second value = unknown names, logged as warnings) and `pub fn is_enabled(&self, f: OptionalFeature) -> bool`.
  - `#[derive(Clone, Copy, Debug)] pub enum OptionalFeature { Forum, Announcements, Events, Screening }`.
  - `pub async fn feature_gate(State((state, feature)): State<(AppState, OptionalFeature)>, req: Request, next: Next) -> Response` — 404 JSON `{"error":"FEATURE_DISABLED","message":"This feature is turned off on this server."}` when off. Applied with `route_layer` to the C22 route list.
  - `ServerSettingsResponse.features: FeaturesResponse { forum_channels, announcement_channels, scheduled_events, membership_screening }`.
- Decision: default all off; existing rows are never deleted.

- [ ] **Step 1: Failing tests:** unit `parse_known_unknown_and_empty` (`"forum, events"` → forum+events; `"nope"` → unknown `["nope"]`; `""` → all off). Integration: `test_settings_reports_features_off_by_default`; `test_create_forum_channel_rejected_when_disabled` (400 `FEATURE_DISABLED`; with `config.optional_features.forum = true` → 201); `test_forum_follow_events_screening_routes_404_when_disabled`; `test_existing_announcement_channel_works_without_crosspost_when_disabled` (Review Focus 2: insert channel + follow row in the DB, post → 201, follower channel gets nothing, channel list 200); `test_join_never_pending_when_screening_disabled`.
- [ ] **Step 2: Run** `cargo test -p vc-server --lib features && cargo test -p vc-server --test integration optional_features` → FAIL.
- [ ] **Step 3: Implement**; tests set the flag on the test config the same way Phase-1 server Task 4 set `cors_allowed_origins`.
- [ ] **Step 4: Run** + gates → PASS.
- [ ] **Step 5: CHANGELOG** `### Changed`: "Forum and announcement channels, scheduled events and membership screening are turned off by default. Server operators can turn them on with `KAIKU_OPTIONAL_FEATURES`."
- [ ] **Step 6: Commit** `feat(api): gate optional channel types and events behind config`

---

### Task 5: Admin-created accounts join servers (C23; U-10/A-11 server half) · **Sonnet**

Branch `feature/admin-create-user-guilds`. After #663.

**Files:** Modify `server/src/admin/types.rs` (`CreateUserRequest.guild_ids`), `server/src/admin/users.rs` (`create_user`), `server/src/admin/queries.rs` (transactional insert), `server/tests/integration/admin_create_user.rs`, `docs/admin-guide/ops/creating-accounts.md`, `CHANGELOG.md`.

**Interfaces:**
- `#[serde(default)] #[validate(length(max = 10))] pub guild_ids: Vec<Uuid>` on `CreateUserRequest`.
- `create_user`: one transaction — check every guild exists (else 400 `VALIDATION_ERROR` "unknown server"), insert the user, `guild::queries::insert_member_idempotent(&mut tx, guild_id, user_id)` per guild; after commit `guild::members::initialize_channel_read_state(&db, guild_id, user_id)` per guild (best effort, as in `join_via_invite`). Audit entry gains `guild_ids`.

- [ ] **Step 1: Failing tests:** `test_create_user_joins_requested_guilds` (member of the guild, `GET /api/guilds` as the new user after password change lists it); `test_create_user_with_unknown_guild_creates_nothing` (400; same username can then be created); `test_create_user_without_guild_ids_unchanged`.
- [ ] **Step 2: Run** `cargo test -p vc-server --test integration admin_create_user` → FAIL.
- [ ] **Step 3: Implement**; docs: add `guild_ids` to the request example and say "add the family server here".
- [ ] **Step 4: Run** + gates → PASS.
- [ ] **Step 5: CHANGELOG** `### Added`: "Admins can add a new account to servers while creating it."
- [ ] **Step 6: Commit** `feat(api): let admin-created accounts join servers`

---

### Task 6: Unread counts match what the user can see (B-10) · **Opus**

Branch `fix/unread-counts`. Independent.

**Files:** Modify `server/src/db/queries.rs` (`get_unread_aggregate`), `server/src/chat/unread.rs`, `server/src/chat/queries.rs` (`dm_unread_count`), `server/src/guild/queries/core.rs` (guild channel unread), `server/src/chat/channels.rs` (`mark_as_read` accepts every text-like type); test `server/tests/integration/unread.rs` (create if absent, + `mod` line); `CHANGELOG.md`.

**Interfaces:**
- One shared predicate `pub(crate) const UNREAD_MESSAGE_PREDICATE: &str = "m.deleted_at IS NULL AND m.parent_id IS NULL AND m.user_id IS DISTINCT FROM $1";` used by every unread query; a missing read cursor counts from `guild_members.joined_at` (DM: participant join time), never from the beginning.
- The aggregate is filtered to channels the user may view, using the same check as `require_channel_access`, batched per guild.

- [ ] **Step 1: Failing tests:** `test_own_messages_are_not_unread`, `test_thread_replies_not_counted_in_channel`, `test_deleted_messages_not_counted`, `test_aggregate_hides_channels_without_view_permission` (no count and no channel name in the response), `test_new_member_starts_with_zero_unread`, `test_dm_own_message_not_unread`.
- [ ] **Step 2: Run** `cargo test -p vc-server --test integration unread` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** + gates → PASS.
- [ ] **Step 5: CHANGELOG** `### Fixed`: "Unread badges no longer count your own messages, thread replies or deleted messages, and never include channels you can't open."
- [ ] **Step 6: Commit** `fix(chat): count unread only for visible messages from others`

---

### Task 7: Presence across devices and for server members (B-07) · *concurrency* · **Opus** · cut 3rd

Branch `fix/presence-multi-device`. After #663 (rebases on its `ws/handlers.rs`).

**Files:** Create `server/src/presence/connections.rs`; modify `server/src/presence/mod.rs`, `server/src/ws/handlers.rs` (connect/disconnect/ping hooks, subscription set), `server/tests/integration/ws_e2e.rs`, `CHANGELOG.md`.

**Interfaces:**
- Valkey sorted set `presence:sockets:{user_id}` (member = socket id, score = expiry ms, 90 s TTL refreshed on every ping).
  - `pub async fn socket_online(redis: &Client, user_id: Uuid, socket_id: Uuid, now_ms: i64) -> Result<bool, PresenceError>` — returns true when this is the first live socket (0 → 1).
  - `pub async fn socket_offline(redis: &Client, user_id: Uuid, socket_id: Uuid, now_ms: i64) -> Result<bool, PresenceError>` — true when no live socket remains (1 → 0).
  - `pub fn live_count(scores: &[i64], now_ms: i64) -> usize` (pure, for tests).
- Broadcast presence only on 0→1 and 1→0. Connect sets `online` only when the stored status is offline/absent (a manual `dnd`/`idle`/`invisible` stays). Presence subscriptions = friends ∪ members of the user's guilds (capped at 500 ids).

- [ ] **Step 1: Failing tests:** unit `expired_sockets_are_not_live`; `ws_e2e`: `test_second_device_disconnect_keeps_user_online`, `test_last_device_disconnect_broadcasts_offline`, `test_guild_comember_receives_presence_without_friendship`, `test_manual_dnd_survives_reconnect`.
- [ ] **Step 2: Run** `cargo test -p vc-server --test integration ws_e2e presence` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** + gates → PASS.
- [ ] **Step 5: CHANGELOG** `### Fixed`: "You stay online while any of your devices is connected, and people on your servers see your status without being friends."
- [ ] **Step 6: Commit** `fix(ws): track presence per device and share it with server members`

---

### Task 8: DM call lifecycle (B-09 server half) · *concurrency* · **Opus** · cut 1st

Branch `fix/dm-call-lifecycle`. After #666 (uses `voice/leave.rs`).

**Files:** Modify `server/src/voice/call_handlers.rs`, `server/src/voice/call_service.rs`, `server/src/voice/leave.rs`; create `server/tests/integration/dm_calls.rs` (+ `mod` line); `CHANGELOG.md`.

**Interfaces:**
- `IncomingCall` is sent with `broadcast_to_user` to each DM participant except the caller (not to the channel), so a callee without an open DM still rings.
- `pub async fn end_call(state: &AppState, channel_id: Uuid, reason: CallEndReason) -> Result<(), CallError>` deletes the call state/stream keys. Called on decline, on ring timeout, when the last SFU peer of the DM channel leaves (hook in `leave_room_internal`), and when the caller's socket drops while ringing.
- No cool-down: a new call right after `end_call` succeeds. Call push (ringing a sleeping phone) stays post-1.0 (push plan out of scope).

- [ ] **Step 1: Failing tests:** `test_incoming_call_reaches_callee_without_channel_subscription`, `test_recall_after_decline_succeeds_immediately`, `test_call_ends_when_last_participant_leaves`, `test_abandoned_ringing_call_does_not_block_new_call`.
- [ ] **Step 2: Run** `cargo test -p vc-server --test integration dm_calls` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** + gates → PASS.
- [ ] **Step 5: CHANGELOG** `### Fixed`: "Calling someone again right after a missed or dropped call works, and calls ring even when the other person has the conversation closed."
- [ ] **Step 6: Commit** `fix(voice): end DM calls cleanly and ring the callee directly`

---

### Batch C — Desktop UI

### Task 9: Desktop hides features the server has turned off (C22 client; U-04, D-14) · **Sonnet**

Branch `fix/desktop-hide-optional-features`. After Task 4 and #665.

**Files:** Create `client/src/stores/serverFeatures.ts`, `client/src/stores/__tests__/serverFeatures.test.ts`; modify `client/src/lib/types/auth.ts` (`ServerSettings.features?`), `client/src/stores/auth.ts` (load after login/restore), `client/src/components/channels/CreateChannelModal.tsx` (hide forum/announcement cards), `client/src/stores/channels.ts` (visible-channel selector), `client/src/components/channels/ChannelList.tsx` (use it), `client/src/components/layout/Sidebar.tsx` + `client/src/views/Main.tsx` (events entry, forum route), guild settings screening tab, `ScreeningGate` mount site, `CHANGELOG.md`.

**Interfaces:**
- `export type OptionalFeatureKey = "forum_channels" | "announcement_channels" | "scheduled_events" | "membership_screening";`
- `export async function loadServerFeatures(): Promise<void>` (from `fetchServerSettings`), `export function isFeatureEnabled(key: OptionalFeatureKey): boolean` (false until loaded; a response without `features` → all true, per C22).
- `export function visibleChannels(channels: Channel[]): Channel[]` in `stores/channels.ts` — drops `forum`/`announcement` when their feature is off.

- [ ] **Step 1: Failing tests:** `missing features field means enabled`, `explicit false disables`, `visibleChannels drops disabled types` (Review Focus 2), `create channel modal offers only text and voice when disabled`, `events entry hidden when scheduled_events is off`.
- [ ] **Step 2: Run** `cd client && bunx vitest run src/stores/__tests__/serverFeatures.test.ts src/stores/__tests__/channels.test.ts` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** desktop gates → PASS.
- [ ] **Step 5: CHANGELOG** `### Changed`: "The desktop app hides forum, announcement, event and screening features when the server has them turned off."
- [ ] **Step 6: Commit** `fix(client): hide optional features the server turned off`

---

### Task 10: Readable themes and real colour tokens (U-05, U-06, U-07) · **Sonnet**

Branch `fix/theme-token-hygiene`. After #661.

**Files:** Modify `client/src/stores/theme.ts`, `client/src/components/OnboardingWizard.tsx` (theme list; `hover:bg-accent-hover` → `hover:bg-primary-hover`), `client/src/components/settings/AppearanceSettings.tsx` (list; `text-white` on accent → `text-on-accent`), `client/src/components/channels/ChannelSettingsModal.tsx` (same), `client/src/components/messages/MessageComponents.tsx` (on-colour tokens), `client/src/components/ui/ContextMenu.tsx` (`accent-error` → `accent-danger`), `client/src/components/admin/SafetyTab.tsx` (undefined CSS vars → existing surface/status tokens), `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/shared/KaikuTheme.kt` (`NordOnError = Color(0xFFECEFF4)`); tests `client/src/stores/__tests__/theme.test.ts`, `client/src/styles/__tests__/tokens.test.ts` (new); `CHANGELOG.md`.

**Interfaces:**
- `ThemeDefinition.hidden?: boolean`; `solarized-dark` and `solarized-light` get `hidden: true`.
- `export const selectableThemes: ThemeDefinition[]` (not hidden). Pickers render `selectableThemes`, plus the current theme when it is hidden (a stored choice keeps working).
- Every replacement token must exist in `client/uno.config.ts`; the test enforces it.

- [ ] **Step 1: Failing tests:** `solarized themes are not selectable`; `stored hidden theme still applies`; `tokens.test.ts`: `no source file uses undefined colour tokens` (scan `src/**/*.tsx` for `accent-error`, `accent-hover`, `--color-surface-raised`, `--color-success-` → none) and `no text-white on accent fills` (no class string contains both `bg-accent-primary` and `text-white`).
- [ ] **Step 2: Run** `bunx vitest run src/stores/__tests__/theme.test.ts src/styles/__tests__/tokens.test.ts` → FAIL.
- [ ] **Step 3: Implement** (Android colour change rides along; CI Android job must stay green).
- [ ] **Step 4: Run** desktop gates → PASS.
- [ ] **Step 5: CHANGELOG** `### Fixed`: "Hard-to-read Solarized themes are no longer offered, and buttons, menu actions and Android warning buttons use readable colours."
- [ ] **Step 6: Commit** `fix(client): hide low-contrast themes and fix colour tokens`

---

### Task 11: Family-facing wording, invite codes, Register link, voice-join reason (U-08, U-09, U-11, U-12 rest) · **Sonnet**

Branch `fix/desktop-family-wording`. After i18n Tasks 4, 6, 7, 8 (they migrate `Login`, `InviteJoin`, `UnreadModule`, `ChannelItem`, `VoiceChannelView`; change copy through their catalog keys).

**Files:** Modify `client/src/views/Login.tsx` (Register link only for `registration_policy === "open"`), `client/src/views/Register.tsx` (closed-policy copy), `client/src/views/InviteJoin.tsx` (`?redirect=` → `returnUrl`, as `Login.tsx` reads it), `client/src/components/guilds/InvitesTab.tsx` (copy the code), `client/src/components/OnboardingWizard.tsx` (invite field: no `maxLength`, parse with `extractInviteCode`), `client/src/components/voice/VoiceChannelView.tsx` (reason + action), `client/src/components/channels/ChannelItem.tsx` (participant names), `client/src/components/settings/RecoveryKeyModal.tsx` + `BackupCodesDisplay.tsx` (file names), `client/src/components/discovery/DiscoveryView.tsx`, `client/src/components/guilds/GeneralTab.tsx` (copy outside i18n scope); tests next to each; `client/src/i18n/__tests__/catalog-wording.test.ts` (new); `CHANGELOG.md`.

**Interfaces / decisions:**
- Invites: "Copy invite code" copies `invite.code` (no link — the family server has no browser client). `export function extractInviteCode(input: string): string | null` (reuse if present, else create in `client/src/lib/invites.ts`): accepts a bare code or any URL ending in `/invite/<code>`.
- Register (closed policy): `auth.register.closed` = "Ask the person who runs this server to create an account for you."
- Voice join failure: `errorText(err)` (i18n C16); for a denied/missing microphone `voice.join.micUnavailable` = "Kaiku can't use your microphone." with action `voice.join.openAudioSettings` = "Audio settings" opening Settings → Audio.
- Voice tooltip uses the participant's display name (members store), never an id slice.
- File names `kaiku-recovery-key.txt`, `kaiku-backup-codes.txt`.

- [ ] **Step 1: Failing tests:** `register link hidden when registration is closed`, `register link shown when open`; `copies the invite code, not a link`; `extractInviteCode accepts a code and a link`; `mic failure shows reason and audio settings action`; `voice tooltip shows display names`; `catalog-wording.test.ts`: `no English catalog value says guild` (every value of `en` matches no `/\bguilds?\b/i`).
- [ ] **Step 2: Run** the new test files → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** desktop gates → PASS.
- [ ] **Step 5: CHANGELOG** `### Fixed`: "The app says 'server' everywhere, invite codes can be copied and pasted reliably, the sign-in screen no longer offers registration when it is closed, and a failed voice join says why."
- [ ] **Step 6: Commit** `fix(client): family-facing wording, invite codes and join errors`

---

### Task 11b: Dialog keyboard and screen-reader basics on core flows (U-15 + a11y basics) · **Sonnet**

Branch `fix/dialog-a11y-core`. After #661/#665 and desktop P1 Task 12.

**Files:** Create `client/src/lib/a11y/dialog.ts`, `client/src/lib/a11y/__tests__/dialog.test.tsx`; modify `client/src/components/settings/SettingsModal.tsx`, `QrLoginModal.tsx`, `client/src/components/OnboardingWizard.tsx`, `client/src/components/channels/CreateChannelModal.tsx`, `ChannelSettingsModal.tsx`, `client/src/components/admin/CreateAccountModal.tsx`, `client/src/components/messages/MessageInput.tsx` (`type="button"` on every non-submit button; `aria-label`s), `client/src/components/messages/MessageList.tsx` (Escape ignores events from inside a dialog), `client/src/components/voice/VoiceControls.tsx` (`aria-label` on icon buttons); tests next to each; `CHANGELOG.md`.

**Interfaces:**
- `export function createDialog(opts: { onClose: () => void; initialFocus?: () => HTMLElement | undefined }): { ref: (el: HTMLElement) => void; titleId: string; dialogProps: { role: "dialog"; "aria-modal": "true"; "aria-labelledby": string; tabIndex: -1 } }` — Escape calls `onClose` and stops propagation; Tab/Shift+Tab stay inside; focus returns to the opener on cleanup. Task 12's `ReauthDialog` uses it.

- [ ] **Step 1: Failing tests:** `Escape calls onClose once and does not reach document listeners`; `Tab from last focusable wraps to first`; `focus returns to the opener`; `Escape inside settings does not mark the channel read`; `dismissing an upload error does not send the draft`; `every voice control button has an accessible name`; `core modals expose role dialog with a label` (render each listed modal, assert `getByRole("dialog", { name: … })`).
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4: Run** desktop gates → PASS.
- [ ] **Step 5: CHANGELOG** `### Fixed`: "Closing a dialog with Escape no longer marks the channel as read, dismissing an upload error no longer sends your message, and the main dialogs work with the keyboard and screen readers."
- [ ] **Step 6: Commit** `fix(client): keyboard-safe dialogs on core flows`

---

### Task 12: Re-auth dialog, server choice on account creation, landing on the family server (C24, C23, U-10 client) · **Sonnet**

Branch `feature/desktop-reauth-and-landing`. After Tasks 2, 5, 11b and desktop P1 Tasks 12/13.

**Files:** Create `client/src/components/auth/ReauthDialog.tsx` (+ test); modify `client/src/lib/types/auth.ts` (`ReauthRequest`), `client/src/lib/tauri/admin.ts` (`elevateSession(req)`), `client/src/lib/tauri/auth.ts` (`mfaGenerateBackupCodes(mfaCode)`), `client/src/stores/admin.ts`, `client/src/components/admin/AdminQuickModal.tsx`, `client/src/components/admin/AdminSettings.tsx`, `client/src/components/settings/SecuritySettings.tsx`, `client/src/components/admin/CreateAccountModal.tsx` (server checklist), `client/src/stores/guilds.ts`, `client/src/components/OnboardingWizard.tsx` (skip join step for members); `CHANGELOG.md`.

**Interfaces:**
- `export interface ReauthRequest { password: string; mfa_code: string | null }`.
- `ReauthDialog` props: `{ title: string; mode: "password" | "mfaOnly"; mfaEnabled: boolean; onSubmit: (req: ReauthRequest) => Promise<void>; onClose: () => void }`; shows `errorText()` for `INVALID_CREDENTIALS`/`MFA_CODE_REQUIRED`/rate limit; uses `createDialog` (Task 11b).
- `elevateSession(req: ReauthRequest & { reason?: string }): Promise<ElevateResponse>`; `mfaGenerateBackupCodes(mfaCode: string): Promise<string[]>`.
- `CreateAccountModal` sends `guild_ids` = checked servers (default: every server the admin belongs to).
- `export function landOnFirstServerIfIdle(): void` in `stores/guilds.ts`: after login/restore, when no guild/DM is selected and no deep link is pending, select the first guild and its first visible text channel.

- [ ] **Step 1: Failing tests:** `ReauthDialog requires a code when MFA is enabled`; `ReauthDialog shows invalid password error`; `elevate sends password and code`; `backup code regeneration sends the code`; `create account sends the checked servers`; `lands on the first text channel when nothing is selected`; `onboarding skips the join step for members`.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4: Run** desktop gates → PASS.
- [ ] **Step 5: CHANGELOG** `### Changed`: "Unlocking admin actions and creating new backup codes ask for your password or authenticator code. New accounts can be added to servers right away, and people land in their server after signing in."
- [ ] **Step 6: Commit** `feat(client): re-auth dialog and landing on the family server`

---

### Task 13: Notifications while Kaiku is visible but unfocused; push-to-talk safety (D-09, D-08 scoped) · **Opus**

Branch `fix/desktop-notify-ptt`. After #665.

**Files:** Modify `client/src/lib/notifications.ts`, `client/src/stores/websocket/messageEvents.ts`, `client/src/lib/pttManager.ts`, the PTT binding UI in voice settings; tests `client/src/lib/__tests__/notifications.test.ts`, `client/src/lib/__tests__/pttManager.test.ts`; `docs/developer-guide/testing/desktop-manual-checklist.md` (two rows); `CHANGELOG.md`.

**Interfaces / decisions:**
- `export async function isAppFocused(): Promise<boolean>` — native: `getCurrentWindow().isFocused()`; browser: `document.hasFocus()`. Replaces every `document.hidden` gate. Suppress only when focused **and** the message's channel is open.
- `export type BindingCheck = { ok: true } | { ok: false; reason: "plainKey" }`; `export function checkGlobalPttBinding(b: KeyBinding): BindingCheck` — allowed: F13–F24, or any key with Ctrl/Alt/Meta. A plain key (e.g. `V`, `Space`) is refused for the global (native) shortcut because the OS reserves it for Kaiku; the UI shows `voice.ptt.plainKeyRefused` = "Choose a key with Ctrl, Alt or ⌘, or F13–F24, so your game still gets the key." An existing plain binding falls back to "only while Kaiku is focused" with one toast.
- Window-focused PTT ignores keydown whose target is an input, textarea or contenteditable.
- Non-consuming system key hooks are post-1.0.

- [ ] **Step 1: Failing tests:** `notifies when the window is visible but not focused`; `stays quiet when focused on the same channel`; `keydown in a text field does not transmit`; `plain V refused for global binding`; `Ctrl+V and F13 accepted`; `stored plain binding falls back to focus-only`.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4: Run** desktop gates → PASS.
- [ ] **Step 5: CHANGELOG** `### Fixed`: "Desktop notifications also appear when Kaiku is open on another screen, and push-to-talk no longer fires while typing or steals plain keys from games."
- [ ] **Step 6: Commit** `fix(client): notify when unfocused and make push-to-talk game-safe`

---

### Task 14: Closing the window never leaves a hidden live mic; one update check (D-10, D-15) · *concurrency* · **Opus**

Branch `fix/desktop-close-to-tray`. After #665.

**Files:** Modify `client/src-tauri/src/lib.rs` (close handler, `RunEvent::Reopen` on macOS, tray availability), `client/src-tauri/src/tray.rs` (voice menu items), `client/src-tauri/src/commands/tray.rs` (`set_tray_voice_state`), `client/src/stores/voice.ts` (report voice state to the tray), `client/src/lib/updater.ts` (single start); tests in `tray.rs` and `client/src/lib/__tests__/updater.test.ts`; manual checklist rows; `CHANGELOG.md`.

**Interfaces:**
- `pub enum CloseAction { HideToTray { notify_voice: bool, first_time: bool }, Minimize }` and `pub fn close_action(tray_available: bool, in_voice: bool, notice_shown: bool) -> CloseAction` (pure).
- No tray (e.g. GNOME without an AppIndicator extension) → `Minimize`, never an unreachable hidden window. In voice → hide plus a native notification `"Kaiku is still in the voice channel. Use the tray icon to leave."`; the tray menu shows "Leave voice" and "Mute"/"Unmute" while in voice.
- `#[tauri::command] fn set_tray_voice_state(in_voice: bool, muted: bool)`; macOS Dock click (`RunEvent::Reopen`) shows the main window.
- `initUpdater()` starts at most once per app session (module flag), so a remounted layout cannot start parallel downloads.

- [ ] **Step 1: Failing tests:** `close_without_tray_minimizes`; `close_in_voice_hides_with_voice_notice` (Review Focus 5); `first_close_shows_notice_once`; TS `initUpdater twice schedules one check`.
- [ ] **Step 2: Run** `cargo test -p vc-client --lib tray && bunx vitest run src/lib/__tests__/updater.test.ts` → FAIL.
- [ ] **Step 3: Implement**; add checklist rows "close during voice → tray shows Leave voice", "GNOME without tray → window minimises", "macOS Dock click reopens".
- [ ] **Step 4: Run** desktop gates → PASS.
- [ ] **Step 5: CHANGELOG** `### Fixed`: "Closing the window during a voice call tells you Kaiku is still connected and lets you leave from the tray; the window can always be reopened."
- [ ] **Step 6: Commit** `fix(client): safe close-to-tray during voice`

---

### Batch D — Desktop native voice

### Task 15: Native audio works with real devices and says when it doesn't (D-12, D-11 residual) · *concurrency* · **Opus** (consult `expert-webrtc-voice`)

Branch `fix/native-audio-robustness`. After #665 and desktop P1 Task 11.

**Files:** Modify `client/src-tauri/src/audio/mod.rs`, `client/src-tauri/src/audio/handle.rs`, `client/src-tauri/src/voice/audio_mixer.rs`, `client/src-tauri/src/webrtc/mod.rs` (ICE buffering), `client/src-tauri/src/commands/voice.rs` (device switch, per-user volume commands), `client/src/stores/websocket/index.ts` + `client/src/lib/webrtc/tauri.ts` (one `voice:*` listener set — keep the adapter's), `client/src/stores/voice.ts` (show audio errors); optionally `client/src-tauri/Cargo.toml` (`rubato`, MIT); tests in-module; `CHANGELOG.md`.

**Interfaces:**
- `pub struct CaptureFormat { pub sample_rate: u32, pub channels: u16 }`; `pub fn negotiate_capture_format(supported: &[(u32, u32, u16)] /* (min_rate, max_rate, channels) */) -> CaptureFormat` — 48 kHz mono, else 48 kHz any channels, else the device default.
- `pub fn downmix_to_mono(interleaved: &[f32], channels: u16) -> Vec<f32>` (average); a resampler to 48 kHz between capture and RNNoise/Opus.
- `AudioHandle::start_capture(...) -> Result<(), AudioError>` propagates stream-build errors; runtime stream errors emit Tauri event `voice:audio_error` `{ kind: "capture_failed" | "playback_failed", device: string }` → toast `voice.audio.captureFailed` = "Microphone stopped working. Check your audio settings."
- `AudioHandle::switch_capture_device(name: &str) -> Result<(), AudioError>` (mid-call); `AudioMixer::set_user_gain(user_id: Uuid, gain: f32)` (0.0 = muted) applied per remote track.
- `WebRtcClient::add_ice_candidate` queues candidates until the remote description is set and flushes them after `set_remote_description`.

- [ ] **Step 1: Failing tests:** `negotiates_48k_mono_when_available`; `falls_back_to_device_default_and_resamples` (44.1 kHz stereo → 48 kHz mono frames of 960 samples); `downmix_stereo_to_mono_averages`; `capture_build_error_is_returned`; `capture_stream_error_emits_audio_error`; `mixer_applies_user_gain`, `zero_gain_is_silent`; `candidates_before_remote_description_are_applied_after`; TS `voice events are handled once per event`.
- [ ] **Step 2: Run** `cargo test -p vc-client --lib audio voice webrtc` → FAIL.
- [ ] **Step 3: Implement**; `cargo deny check licenses` if `rubato` is added.
- [ ] **Step 4: Run** desktop gates → PASS; add checklist rows "44.1 kHz headset", "unplug mic mid-call", "switch mic mid-call", "per-person volume".
- [ ] **Step 5: CHANGELOG** `### Fixed`: "Desktop voice works with headsets that don't offer 48 kHz mono, tells you when the microphone stops working, can switch microphones during a call, and per-person volume works."
- [ ] **Step 6: Commit** `fix(voice): negotiate device formats and surface native audio errors`

---

### Task 16: Native screen and webcam capture with any frame size (D-13) · **Opus** · cut 4th

Branch `fix/native-capture-frame-size`. After #665.

**Files:** Modify `client/src-tauri/src/capture/source.rs`, `capture/capturer.rs`, `capture/convert.rs`, `capture/webcam.rs`, `client/src-tauri/src/video/encoder.rs`; tests in-module; `CHANGELOG.md`.

**Interfaces:**
- `pub fn encode_dimensions(src_w: u32, src_h: u32, max_w: u32, max_h: u32) -> (u32, u32)` — keeps aspect ratio, never upscales, rounds to even.
- Conversion uses the frame's actual width, height and row stride, scaling to `encode_dimensions` before encoding; the encoder is re-created when the dimensions change.

- [ ] **Step 1: Failing tests:** `small_window_is_not_upscaled_and_even`; `convert_respects_row_stride`; `convert_handles_frame_smaller_than_tier` (no panic, correct buffer size); `encoder_recreated_on_size_change`.
- [ ] **Step 2: Run** `cargo test -p vc-client --lib capture video` → FAIL. **Step 3: Implement.** **Step 4: Run** → PASS; checklist row "share a small game window".
- [ ] **Step 5: CHANGELOG** `### Fixed`: "Sharing a small window or a webcam with an unusual resolution no longer crashes the desktop app."
- [ ] **Step 6: Commit** `fix(voice): handle any capture frame size in native sharing`

---

### Batch E — Android UI/UX

### Task 17: First run without dead ends (A-10, A-11, U-09 Android) · **Sonnet**

Branch `fix/android-first-run`. After #660/#662 (Android P1 Task 10 preferred). Coordinate with i18n Task 11 (same screens; later PR rebases).

**Files:** Create `domain/ServerAddress.kt`, `data/api/ServerProbe.kt`; modify `app/build.gradle.kts` (`buildConfigField("String", "DEFAULT_SERVER_URL", …)` from Gradle property `kaiku.defaultServerUrl`, default `"https://kaiku.pmind.de"`), `ui/auth/ServerUrlScreen.kt`, `ui/auth/LoginScreen.kt` + `LoginViewModel.kt` ("Change server", Register only when open), `ui/KaikuNavGraph.kt` (start at Login with the default URL when none is stored; change-server route without Clear Data), `ui/home/HomeViewModel.kt` (auto-select), `ui/home/HomeScreen.kt` (empty state), `res/values/strings.xml`; tests under `app/src/test/...`; `CHANGELOG.md`.

**Interfaces:**
- `fun normalizeServerAddress(input: String): Result<String>` — trims, adds `https://` to a bare host, rejects `http://`, strips path/query/trailing slash, rejects blank.
- `sealed interface ProbeResult { data class Ok(val registrationOpen: Boolean) : ProbeResult; data object NotKaiku : ProbeResult; data object Unreachable : ProbeResult }`; `class ServerProbe(client: HttpClient) { suspend fun probe(baseUrl: String): ProbeResult }` — `GET /health` 2xx, then `GET /api/settings` for `registration_policy`.
- `HomeViewModel` selects `guilds.first()` when nothing is selected; with no guilds the empty state says `home_no_servers_hint` = "You're not on a server yet. Ask the person who runs Kaiku to add you."

- [ ] **Step 1: Failing tests:** `bare host gets https`, `http is rejected`, `path is stripped`, `blank is rejected`; `ServerProbeTest` (MockEngine: 200 + settings → `Ok(false)`; 404 → `NotKaiku`; IOException → `Unreachable`); `register hidden when registration is closed`; `change server returns to server screen keeping nothing stale`; `auto-selects the only server`.
- [ ] **Step 2: Push** → CI shows them failing. **Step 3: Implement** (strings with C15 names). **Step 4: Push** → CI green, tests passed in the log.
- [ ] **Step 5: CHANGELOG** "Android: the app starts with the family server filled in, accepts a plain address, checks it before sign-in, lets you change the server, hides Register when it is closed, and opens your server automatically."
- [ ] **Step 6: Commit** `fix(client): Android first run without dead ends`

---

### Task 18: Unread state that matches reality on Android (A-12 rest, A-13) · **Sonnet**

Branch `fix/android-unread`. After #662 and Android P1 Task 5.

**Files:** Modify `data/api/ChannelApi.kt` (`markAsRead`), `data/repository/GuildRepository.kt` (`clearActiveChannel`, read on open/new message), `data/repository/DmRepository.kt` (observe `MessageNew`), `ui/channel/TextChannelViewModel.kt` (`onCleared` → `clearActiveChannel`); tests `GuildRepositoryTest.kt`, `DmRepositoryTest.kt`, `ChannelApiTest.kt`; `CHANGELOG.md`.

**Interfaces:**
- `ChannelApi.markAsRead(channelId: String, lastReadMessageId: String)` → `POST /api/channels/{id}/read` body `{"last_read_message_id": …}`.
- `GuildRepository.clearActiveChannel(channelId: String)` resets the active id only if it still matches. While a channel is open, a new message marks it read, debounced 1 s.
- `DmRepository` collects `ServerEvent.MessageNew` for DM channels: updates `lastMessage`, increments `unreadCount` unless that DM is active (then marks read).

- [ ] **Step 1: Failing tests:** `leaving a channel re-enables unread`; `opening a channel posts read with the last message id`; `messages while open are marked read once per second at most`; `message in an inactive DM increments unread and updates the preview`; `message in the active DM marks read`; `mark read sends last_read_message_id`.
- [ ] **Step 2–4:** push → fail → implement → green.
- [ ] **Step 5: CHANGELOG** "Android: unread badges appear for channels you left, clear on the server when you read them, and new direct messages update the list live."
- [ ] **Step 6: Commit** `fix(client): Android unread state for channels and DMs`

---

### Task 19: Logout really signs out; calls survive navigation (A-14, A-07, A-21) · *concurrency* · **Opus**

Branch `fix/android-logout-voice-lifetime`. After #662; coordinate with push Task 7 (it adds `pushRegistrar.unregisterForLogout()` as the first logout step — keep it first).

**Files:** Create `data/session/SessionTeardown.kt`, `di/SessionTeardownModule.kt`; modify `data/repository/AuthRepository.kt` (`logout` order), `data/api/AuthApi.kt` (`logout(refreshToken)` with body), `data/repository/{VoiceRepository,ChatRepository,GuildRepository,DmRepository}.kt` (teardown), `data/ws/RealtimeConnectionManager.kt` (teardown), `ui/voice/VoiceViewModel.kt` (no leave in `onCleared`), `ui/KaikuNavGraph.kt` + `ui/voice/VoiceOverlay.kt` (overlay while in a call and not on the voice screen; tap navigates), `data/voice/WebRtcManager.kt` (factory reuse); tests; `CHANGELOG.md`.

**Interfaces:**
- `interface SessionTeardown { val order: Int; suspend fun onLogout() }`, Hilt `@IntoSet` bindings: voice leave (10), realtime disconnect (20), chat/guild/DM caches clear (30).
- `AuthRepository.logout()`: (push unregister, push plan) → teardowns by `order`, each with a 3 s timeout and errors logged → `authApi.logout(refreshToken)` (C25 also accepts bearer-only) → `tokenStorage.clear()` → `setLoggedOut()`. The same teardown runs when the session is rejected (`RefreshResult` auth failure).
- `VoiceViewModel.onCleared()` no longer leaves; leaving is only the Leave button or the notification action.
- `interface PeerConnectionFactoryProvider { fun get(): PeerConnectionFactory }` — one factory and audio device module per process; `dispose()` releases peer connections, tracks and sources only.

- [ ] **Step 1: Failing tests:** `logout runs teardowns in order before server logout and token clear`; `failing teardown does not block logout`; `logout sends the refresh token`; `auth rejected logout clears repositories` (Review Focus 1); `clearing the voice view model keeps the call`; `overlay shown while in a call off the voice screen`; `factory created once across two joins` (fake provider counts).
- [ ] **Step 2–4:** push → fail → implement → green.
- [ ] **Step 5: CHANGELOG** "Android: signing out ends the session, leaves voice and removes your data from the screen; going back from a call keeps it running with a bar to return to it."
- [ ] **Step 6: Commit** `fix(client): Android logout teardown and persistent voice calls`

---

### Task 20: Daily-chat fixes and icon labels (A-19 rest, A-17, Android a11y basics) · **Sonnet**

Branch `fix/android-daily-chat`. After #662. (A-17 is independent enough to ride here: same reviewer, small.)

**Files:** Create `domain/MessageTime.kt`, `domain/ReactionSummary.kt`; modify `ui/channel/MessageItem.kt` (time, reactions row via existing `Reactions.kt`), `data/repository/ChatRepository.kt` (typing expiry), `ui/channel/MessageInput.kt` or the typing UI (names), icon buttons in `TextChannelScreen.kt`, `MessageInput.kt`, `VoiceChannelScreen.kt`, `VoiceOverlay.kt` (`contentDescription`), `data/local/TokenStorage.kt` + `di/StorageModule.kt` + `ui/MainActivity.kt` (read-time recovery), `res/values/strings.xml`; tests; `CHANGELOG.md`.

**Interfaces:**
- `fun formatMessageTime(iso: String, zone: ZoneId, now: Instant): String` — today → localized short time; otherwise localized short date + time (`java.time`, minSdk 26 OK).
- Typing: `Map<userId, expiresAtMs>`, entries expire 8 s after the last typing event; `fun typingNames(ids: Set<String>, members: Map<String, String>): List<String>` never returns ids (unknown → `chat_typing_someone`).
- `data class ReactionSummary(val emoji: String, val count: Int, val mine: Boolean)`; `fun reactionSummaries(message: Message, currentUserId: String?): List<ReactionSummary>`; the row is shown under messages, tapping a chip toggles your own reaction through the existing repository call if present (otherwise display-only).
- `TokenStorage` wraps every read in try/catch (`GeneralSecurityException`, `IOException`, `SecurityException`): reset storage, set a flag, return null; `fun consumeStorageResetNotice(): Boolean` → `MainActivity` shows `auth_storage_reset` = "Please sign in again." once.

- [ ] **Step 1: Failing tests:** `utc time shown in local zone` (`"2026-11-12T18:30:00Z"`, `Europe/Berlin` → `19:30` when same day); `older message shows date`; `typing expires after 8 s`; `typing shows names not ids`; `reaction summaries count and mark mine`; `read failure resets storage and reports once`.
- [ ] **Step 2–4:** push → fail → implement → green; `lintDebug` reports no new `ContentDescription` warnings in the touched files.
- [ ] **Step 5: CHANGELOG** "Android: message times use your time zone, typing shows names and disappears, reactions are visible, buttons are labelled for screen readers, and a damaged sign-in store asks you to sign in again instead of crashing."
- [ ] **Step 6: Commit** `fix(client): Android message times, typing, reactions and storage recovery`

---

### Task 21: Send photos from the phone (A-20, spec item 6) · **Sonnet** · cut 2nd

Branch `feature/android-photo-upload`. After #662.

**Files:** Modify `data/api/MessageApi.kt` (multipart upload, upload limits), `data/repository/ChatRepository.kt` (`sendAttachment`), `ui/channel/MessageInput.kt` (attach button, `PickVisualMedia`), `ui/channel/TextChannelViewModel.kt` (state + error), `res/values/strings.xml`; tests `MessageApiUploadTest.kt`, `TextChannelViewModelTest.kt`; `CHANGELOG.md`.

**Interfaces:**
- `MessageApi.uploadMessageWithFile(channelId: String, bytes: ByteArray, fileName: String, mimeType: String, content: String?): Message` → `POST /api/messages/channel/{id}/upload`, multipart fields `file` and `content`.
- `MessageApi.getUploadLimits(): UploadLimits` (`GET /api/config/upload-limits`), cached per session.
- `ChatRepository.sendAttachment(channelId: String, uri: Uri, caption: String?): Result<Message>` — reads via `ContentResolver`, rejects files above the limit before uploading (`chat_upload_too_large`), maps failures to `chat_upload_failed`.

- [ ] **Step 1: Failing tests:** `upload sends multipart with file and content`; `oversized file rejected before upload`; `upload failure exposes chat_upload_failed`.
- [ ] **Step 2–4:** push → fail → implement → green.
- [ ] **Step 5: CHANGELOG** "Android: you can send photos and videos from your phone."
- [ ] **Step 6: Commit** `feat(client): Android photo upload`

---

### Batch F — Ops and supply chain

### Task 22: Frontend advisories and a blocking audit gate (S-12) · *security* · **Sonnet**

Branch `fix/client-advisories`. Independent (rebase over i18n/desktop `bun.lock` changes).

**Files:** Modify `client/package.json` (`dompurify` and its `overrides` entry to the first fixed release ≥3.4.12, `mermaid` ≥11.16.1, `overrides` for `seroval` ≥1.5.3 and `katex` ≥0.18.2; dev: `vitest` ≥4.1.11, `vite`, `postcss`, `nanoid`, `undici`, `browserslist` to fixed versions where no major bump is needed), `client/bun.lock`, `.github/workflows/security.yml` (replace the non-gating `bun pm scan` step), `CHANGELOG.md`.

**Interfaces / decisions:**
- New blocking CI step in `security.yml` (and the PR `frontend` job of `ci.yml`): `cd client && bun audit --prod --audit-level=moderate`. Dev-only advisories are reported by a non-blocking `bun audit --audit-level=high` step. Any justified exception uses `--ignore=<GHSA id>` with a dated comment.
- Verify the sanitizer behaviour is unchanged: `client/src/lib/__tests__/sanitize.test.ts`, `spoiler.test.ts` and the mermaid render path in `MarkdownPreview.tsx` (`securityLevel: "strict"` kept).

- [ ] **Step 1: Run** `cd client && bun audit --prod` → lists dompurify, seroval, katex, mermaid (baseline, record in the PR).
- [ ] **Step 2: Bump** and `bun install`; **run** `bun audit --prod --audit-level=moderate` → exit 0.
- [ ] **Step 3: Run** desktop gates + `bun run build` → PASS (mermaid pages render in a dev check).
- [ ] **Step 4: Wire the gate**; confirm with `python3 -c "import yaml;yaml.safe_load(open('.github/workflows/security.yml'))"`.
- [ ] **Step 5: CHANGELOG** `### Security`: "Updated the Markdown sanitizer, diagram renderer and UI runtime libraries to versions without known vulnerabilities."
- [ ] **Step 6: Commit** `fix(client): update sanitizer and diagram deps, gate bun audit`

---

### Task 23: CI least privilege, pinned actions, blocking gates (S-10 rest, R-13, R-14) · *security* · **Sonnet**

Branch `fix/ci-hygiene`. After #658 (merged); rebase over sibling `ci.yml` edits.

**Files:** Modify `.github/workflows/{ci,security,tauri-build,release-notes-on-merge}.yml` (top-level `permissions: contents: read`, per-job grants only where needed; every third-party `uses:` pinned to a full commit SHA with a `# vX.Y.Z` comment, `dtolnay/rust-toolchain@master` and `jlumbroso/free-disk-space@main` included; gitleaks without `continue-on-error`; Android unit tests without `continue-on-error` — tests that need native WebRTC get `@Ignore("needs native WebRTC")` with an issue link; Windows Tauri job `allow_failure: false`), `scripts/check_ci_guardrails.py` + `scripts/tests/test_check_ci_guardrails.py`; create `.github/dependabot.yml` (`github-actions`, weekly, grouped); create `.gitleaksignore` only if the blocking scan reports false positives (each line commented); `CHANGELOG.md` not needed (internal).

**Interfaces:**
- Guardrail functions: `check_workflow_permissions(workflows: dict[str, dict], errors: list[str])` (every workflow has a top-level `permissions` and no job grants `write-all`); `check_actions_pinned(workflow_texts: dict[str, str], errors: list[str])` (every `uses:` of a non-local action matches `@[0-9a-f]{40}`); `check_no_advisory_gates(ci: dict, errors: list[str])` (no `continue-on-error: true` on the gitleaks, Android unit-test or Windows Tauri jobs/steps).

- [ ] **Step 1: Failing tests** in `test_check_ci_guardrails.py`: `test_workflow_without_permissions_is_rejected`, `test_unpinned_action_is_rejected`, `test_sha_pinned_action_passes`, `test_advisory_android_tests_rejected`.
- [ ] **Step 2: Run** `python3 -m unittest discover -s scripts/tests -v` → FAIL; `python3 scripts/check_ci_guardrails.py` → lists current violations.
- [ ] **Step 3: Fix the workflows** as listed; resolve SHAs with `gh api repos/<owner>/<repo>/commits/<tag> -q .sha`.
- [ ] **Step 4: Run** both commands → PASS; push and require the full CI run green, including Windows and Android tests shown as passed.
- [ ] **Step 5: Commit** `fix(infra): least-privilege workflows, pinned actions, blocking gates`

---

## 4. Maintainer actions (none block merging)

1. **Confirm the July-feature decision** (Task 4): forum/announcement channels, events and screening stay **off** on the family server (no env var needed). Veto before Task 4 merges if any should stay on.
2. **Deploy approvals** on athene for the server batch (Tasks 1–8), with a backup first (athene runbook).
3. **Release M1 is still pending** (from #658): move `TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)` into the `release` Environment and delete the repo-level copies.
4. **Repo settings** after Task 23: enable "Dependabot alerts" and "Dependabot security updates" (Settings → Code security). Optional: branch protection requiring the Windows and Android jobs.
5. **Per-OS manual checks** for Tasks 13–16 on one Windows, Linux and macOS PC using `docs/developer-guide/testing/desktop-manual-checklist.md` (new rows added by those tasks), and the Android rows for Tasks 17–21 on one phone.
6. **Code-signing cost decision** (R-12) is unchanged — still open from the review.

## 5. Out of scope here (owned elsewhere or post-1.0)

- Push for DMs/mentions/photos, notification channels, device subscriptions and their cap → push plan. Call ringing by push (B-09 push half) → post-1.0.
- String extraction and the i18n runtime → i18n plan. Tasks here only add strings in the catalog format.
- Backups, alerting, image pins, live configuration, restore drill → athene plan.
- Post-1.0: CSP tightening, non-consuming global PTT hook, event reminder fixes, Android invite-code dialog, security ignore-list unification, QR-redeem value binding.

## Self-review notes

- Coverage: all 70 *Before rollout* IDs of both consolidated files appear in §1 with a status; every **keep** row names a task; the caller's named items map to Tasks 9 (forum/announcement hidden), 10 (Solarized), 11 (guild/server wording), 11b (a11y basics), 2 (admin re-auth), 22 (bun audit, mermaid/DOMPurify), 23 (updater/secret hygiene leftovers); the dead Edit item is already fixed in #662.
- Type consistency: `AuthUser.session_id` (Task 1) is consumed by Task 2; `OptionalFeatures`/`features` JSON (Task 4) matches `OptionalFeatureKey` (Task 9); `ReauthRequest` (C24) is the same shape on server (Task 2) and client (Task 12); `createDialog` (Task 11b) is used by `ReauthDialog` (Task 12); `guild_ids` (C23) in Tasks 5 and 12.
- Review Focus items each have a pinned test in the owning task (Tasks 1/19, 4/9, 2, 15, 14).
