# v1.0 Desktop / Web Client — Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The installed desktop app (Windows/Linux/macOS) runs in native mode with a session that survives restarts and token expiry, realtime that recovers on its own, no misleading E2EE UI, and the maintainer can create family accounts — while the web client gains the same reconnect/catch-up behaviour.

**Architecture:** Decision "Option A — native desktop" (review "Design inputs", contract C6). One `isTauri()` helper drives mode detection. All REST calls go through the TypeScript `httpRequest` in both modes; in native mode the base URL and bearer come from Rust (`get_auth_info`), and Rust alone holds the refresh token (keyring), refreshes it, and owns the WebSocket (needed by native voice signalling). Server events from the Rust socket are forwarded raw to the same `handleServerEvent` the browser path uses, so one event handler and one connection state machine serve both modes.

**Tech Stack:** Solid.js + TypeScript (vitest, ESLint flat config, Playwright), Tauri 2 (Rust: reqwest 0.13, tokio-tungstenite 0.29, keyring 4), `@solid-primitives/i18n` (MIT), `wiremock` (MIT/Apache-2.0, dev-dependency only).

**Spec:** `docs/superpowers/specs/2026-10-20-v1-family-release-goal-design.md` · Review: `docs/reviews/2026-10-22-v1-family-release-readiness.md` · Contracts: `docs/superpowers/plans/2026-10-22-v1-family-contracts.md` (C1, C2, C3, C4, C6 used here).

## Global Constraints

- Repo rules from `CLAUDE.md` apply: never commit to `main`; branch `feature/…`, `fix/…`, `refactor/…`, `docs/…`; one worktree per task; squash-merge PRs.
- Commit format `type(scope): subject`, max 72 chars, imperative; scopes only `auth, voice, chat, db, api, ws, ratelimit, infra, client, crypto`.
- Every user-visible change gets a `CHANGELOG.md` entry under `[Unreleased]` (Keep a Changelog 1.1.0), written from the user's perspective.
- Pre-push gates per task: `cd client && bunx tsc --noEmit && bun run lint && bun run test:run`; for Rust changes also `cargo fmt --check` and `SQLX_OFFLINE=true cargo clippy -p vc-client -- -D warnings` and `cargo test -p vc-client --lib`.
- License policy: only MIT, Apache-2.0, BSD-2/3, ISC, Zlib, MPL-2.0 (and the other allowed ones in `CLAUDE.md`); run `cargo deny check licenses` whenever a crate or crate feature is added.
- UI contrast rules (`CLAUDE.md` → "UI Contrast Rules"): readable text uses `text-text-primary`/`text-text-secondary`; never `text-accent-primary` on `bg-accent-primary/*`; no text opacity below 50%.
- Access tokens live 15 min (`CLAUDE.md`); refresh 60 s before expiry; the server rotates the refresh token on every use.
- **i18n-ready:** English only for v1.0, but every user-facing string in a file this plan *creates* goes through `t()`; files this plan *rewrites the UI of* are fully migrated and added to the ESLint guard list. In large files that only get a small change (`UsersPanel.tsx`, `SecuritySettings.tsx`, `AuthGuard.tsx`, `Login.tsx`), only the new/changed strings use `t()` and the file is **not** added to the guard (Phase 2 extracts the rest).
- Refresh token never reaches the webview in native mode (no `X-Refresh-Token` from TypeScript, no refresh token in any command result).
- The repo is public: security-relevant changes are described by behaviour and test, never by how to exploit.
- No task needs a live server. Verification is unit tests, the local dev stack (`docker-compose.dev.yml` + `make dev`, server on `http://localhost:8080`) and CI. Packaged-build checks follow the checklist from Task 11.
- Review gate for every task: Fable 5.1 + one external model (Gemini 4 Argon or Codex), per `docs/developer-guide/agents/multi-model-workflow.md`.
- Performance targets from `CLAUDE.md`: idle client RAM < 80 MB, idle CPU < 1 % — timers added here (ping 20 s, watchdog) must not wake more often than every 5 s.

## Review Focus

1. **Wake after > 15 min sleep** — the access token has expired when the socket reconnects; expected: one refresh, then a clean reconnect, never an endless "Reconnecting…" loop or a logout. Pinned by `reconnect refreshes an expired token before reopening` (Task 2) and `reconnect_uses_refreshed_token` (Task 8).
2. **Concurrent refreshes** (Rust scheduler + a 401 retry + a WebSocket reconnect at the same moment) — rotation must not invalidate the session. Pinned by `concurrent_refresh_calls_hit_server_once` (Task 6).
3. **Server restart while the user is reading a channel** — missed messages appear once, in order, without duplicates and without jumping the scroll position. Pinned by `catchUpChannel appends only newer messages, deduplicated` (Task 3).
4. **Linux PC without a Secret Service keyring** — login must still work for the session and the user is told the login won't be remembered, instead of a silent logout on next start. Pinned by `keyring_failure_still_logs_in_with_persisted_false` (Task 6) and the Linux checklist row (Task 11).
5. **New user restarts the app before changing the temporary password** — they land on the change-password screen again, not on an app full of 403 errors. Pinned by `PASSWORD_CHANGE_REQUIRED from any request switches to the change screen` (Task 13).

---

## File Structure

| Path | Responsibility | Task |
|---|---|---|
| `client/src/i18n/en.ts` (new) | Flat English catalog, `MessageKey` type | 1 (+ keys from later tasks) |
| `client/src/i18n/index.ts` (new) | `t()`, `plural()` | 1 |
| `client/eslint.i18n.mjs` (new) | `I18N_GUARDED_FILES` list + shared restricted-syntax selectors | 1, 9 |
| `client/src/stores/websocket/connection.ts` (new) | Reconnect supervisor: backoff, triggers, ping watchdog | 2, 8 |
| `client/src/components/layout/ConnectionIndicator.tsx` (new) | Single connection indicator | 2 |
| `client/src/lib/qrLogin.ts` (new) | `buildQrLoginUri()` | 4 |
| `client/src/lib/featureFlags.ts` (new) | `isE2EEEnabled()` | 5 |
| `client/src-tauri/src/session/{mod.rs,store.rs}` (new) | Rust-owned session: login, refresh, keyring, allowlisted session requests | 6 |
| `client/scripts/check-tauri-commands.ts` (new) | CI: every `invoke("…")` is registered; no uncalled `isTauri` | 7, 9 |
| `client/src-tauri/src/network/websocket.rs` | Token-aware reconnect, raw event passthrough, watchdog | 8 |
| `client/src-tauri/Info.plist`, `Entitlements.plist` (new) | macOS usage strings | 10 |
| `client/src-tauri/src/commands/voice.rs` | ICE servers from the server for native voice | 11 |
| `docs/developer-guide/testing/desktop-manual-checklist.md` (new) | Per-OS manual test on packaged builds | 11 |
| `client/src/lib/passwords.ts` (new) | `generateTemporaryPassword()` | 12 |
| `client/src/components/admin/CreateAccountModal.tsx` (new) | Admin "Create account" | 12 |
| `client/src/views/ForcedPasswordChange.tsx` (new) | Forced password change screen | 13 |

## Task order and cross-stream dependencies

| Task | Branch | Depends on |
|---|---|---|
| 1 i18n foundation | `feature/i18n-foundation` | — |
| 2 WebSocket reconnect (browser) + indicator | `fix/ws-reconnect` | 1 |
| 3 Catch-up + live DM creation | `feature/ws-catch-up` | 2; **server C1, C2, C3** |
| 4 QR login server URL | `fix/qr-login-server-url` | 1 |
| 5 E2EE off for v1.0 | `fix/e2ee-v1-gate` | 1 |
| 6 Rust-owned native session | `feature/desktop-native-session` | — |
| 7 REST through `httpRequest` + invoke CI check | `refactor/client-rest-transport` | 6 |
| 8 Native WebSocket manager | `fix/desktop-ws-manager` | 2, 6 |
| 9 Activate native mode (`isTauri()`) | `fix/desktop-native-mode` | 5, 6, 7, 8 (U-01 gate must be merged before the flip) |
| 10 macOS usage strings | `fix/macos-usage-strings` | — |
| 11 Native voice ICE + manual checklist | `feature/desktop-voice-checklist` | 9 |
| 12 Admin "Create account" | `feature/admin-create-account` | 1; **server C4** |
| 13 Forced password change | `feature/forced-password-change` | 6, 12; **server C4** |

Production login from the packaged app additionally needs the server/infra stream's D-02 fix (CORS for `tauri://localhost` and `http://tauri.localhost`); the dev stack's default `CORS_ALLOWED_ORIGINS=*` (no credentials) is enough for every test here because native mode authenticates with a bearer token only.

---

### Task 1: i18n foundation

**Files:**
- Create: `client/src/i18n/en.ts`, `client/src/i18n/index.ts`, `client/eslint.i18n.mjs`
- Modify: `client/eslint.config.mjs`, `client/package.json` (+ `bun.lock`), `client/src/AGENTS.md` (one paragraph: "new UI strings go through `t()`")
- Test: `client/src/i18n/__tests__/i18n.test.ts`

**Interfaces:**
- Produces:
  - `export const en: Record<string, string>` (declared `as const`), `export type MessageKey = keyof typeof en` in `en.ts`.
  - `export function t(key: MessageKey, params?: Record<string, string | number>): string` — `{{name}}` placeholders, via `@solid-primitives/i18n` `flatten` + `translator` + `resolveTemplate`; locale fixed to `"en"`.
  - `export function plural(count: number, forms: { one: MessageKey; other: MessageKey }, params?: Record<string, string | number>): string` — chooses via `new Intl.PluralRules("en").select(count)`, passes `count` as a param.
  - `eslint.i18n.mjs`: `export const I18N_GUARDED_FILES: string[]` (starts as `["src/i18n/**/*.{ts,tsx}"]`) and `export const I18N_SELECTORS` — two `no-restricted-syntax` entries: `JSXText[value=/[A-Za-z]/]` and `JSXAttribute[name.name=/^(placeholder|title|aria-label|alt|label)$/] > Literal`, message `"User-facing text must use t() from @/i18n (see client/src/AGENTS.md)."`.
- Key convention (Android mirrors it in `strings.xml` with `.` → `_`): `area.screen.purpose`, whole sentences per key, e.g. `"connection.reconnecting": "Reconnecting…"`. Seed keys: `common.cancel`, `common.close`, `common.save`, `common.retry`, `common.copy`, `common.copied`, `connection.attempt.one`/`connection.attempt.other` (both `"Attempt {{count}}"`).

- [ ] **Step 1: Write the failing tests** in `i18n.test.ts`:
  - `t returns the English text for a key` → `expect(t("common.retry")).toBe("Retry")`
  - `t fills placeholders` → `expect(t("connection.attempt.other", { count: 3 })).toBe("Attempt 3")`
  - `plural picks one/other` → `plural(1, { one: "connection.attempt.one", other: "connection.attempt.other" })` is `"Attempt 1"`, `plural(3, …)` is `"Attempt 3"` (forms identical in English but separate keys, so German can differ later).
  - `lint rejects JSX text literals in guarded files` — `// @vitest-environment node`; `new ESLint({ cwd: <client dir> })`, `lintText('export const X = () => <p>Hello</p>;', { filePath: "src/i18n/__fixtures__/x.tsx" })` → one message whose `ruleId === "no-restricted-syntax"`. (No file is written; `lintText` only uses the path to select the config block.)
- [ ] **Step 2: Run** `cd client && bunx vitest run src/i18n` → FAIL (module not found).
- [ ] **Step 3: Install** `bun add @solid-primitives/i18n@^2` and implement `en.ts`, `index.ts`, `eslint.i18n.mjs`; in `eslint.config.mjs` add a block `{ files: I18N_GUARDED_FILES, rules: { "no-restricted-syntax": ["error", ...I18N_SELECTORS] } }` after the existing `src/**` block.
- [ ] **Step 4: Run** `bunx vitest run src/i18n && bun run lint && bunx tsc --noEmit` → PASS, and `t("nope")` in a scratch line fails `tsc` (remove the line).
- [ ] **Step 5: Commit** (no CHANGELOG — internal)
```bash
git add client/src/i18n client/eslint.i18n.mjs client/eslint.config.mjs client/package.json client/bun.lock client/src/AGENTS.md
git commit -m "feat(client): add i18n catalog layer and JSX literal lint guard"
```

---

### Task 2: WebSocket reconnect, watchdog and connection indicator (D-04, browser mode)

Kimi correction applies: `handleReconnect()` (`stores/websocket/index.ts` ~1509) already re-subscribes, reloads cached channels and refreshes unread state. The gap is that nothing triggers it in browser mode. This task adds the trigger and the state machine; Task 3 replaces the full reload with catch-up.

**Files:**
- Create: `client/src/stores/websocket/connection.ts`, `client/src/components/layout/ConnectionIndicator.tsx`
- Modify: `client/src/lib/tauri/websocket.ts` (close/frame hooks, intentional-close flag), `client/src/stores/websocket/index.ts` (use supervisor; drop the `ws-reconnect` toast and the `ws-reconnected` re-attach dance), `client/src/stores/auth.ts` (start/stop supervisor where `wsConnect`/`wsDisconnect` are called), `client/src/components/layout/AppShell.tsx` (mount indicator), `client/src/i18n/en.ts`, `client/eslint.i18n.mjs` (add `ConnectionIndicator.tsx`)
- Test: `client/src/stores/__tests__/websocketReconnect.test.ts`, extend `client/src/stores/__tests__/websocket.test.ts`

**Interfaces:**
- Produces (in `connection.ts`):
  - `export function nextBackoffMs(attempt: number, random: () => number = Math.random): number` — `min(1000·2^(attempt-1), 30000)` scaled by `1 + (random()-0.5)·0.4` (±20 %).
  - `export interface ConnectionDriver { open(): Promise<void>; close(): void; sendPing(): void; reconnectNow?(): Promise<void> }`
  - `export interface SupervisorOptions { driver: ConnectionDriver; mode: "managed" | "external"; onStatus(status: ConnectionState, attempt: number): void; onRecovered(): Promise<void>; ensureFreshToken(): Promise<boolean>; timings?: { pingEveryMs: number; silenceLimitMs: number } }` — defaults `pingEveryMs: 20000`, `silenceLimitMs: 45000`.
  - `export function createConnectionSupervisor(opts: SupervisorOptions): { start(): Promise<void>; stop(): void; notifyOpened(): void; notifyClosed(intentional: boolean): void; notifyFrame(): void; triggerNow(reason: "online" | "visible" | "user"): void }`.
  - `mode: "managed"` = the supervisor schedules retries (browser). `mode: "external"` = something else retries (Rust, Task 8); the supervisor only maps status, runs `onRecovered`, and turns `triggerNow` into `driver.reconnectNow()`.
  - `ConnectionState` is the existing union in `stores/websocket/index.ts` (`"disconnected" | "connecting" | "connected" | "reconnecting"`) — move it to `connection.ts` and re-export.
- Produces (in `lib/tauri/websocket.ts`): `export function setBrowserSocketHooks(hooks: { onClose(intentional: boolean): void; onFrame(): void }): void`; `wsDisconnect()` marks the close as intentional.
- Behaviour decisions:
  - Before every retry: `ensureFreshToken()` → `refreshAccessToken()` when `tokenExpiresAt - now < 60 s`; `false` → `stop()` + dispatch `kaiku:session-expired` (existing handler shows the modal).
  - `window` `online` → `triggerNow("online")`; `visibilitychange` to visible → if connected `driver.sendPing()`, else `triggerNow("visible")`.
  - Watchdog: every `pingEveryMs` send `{type:"ping"}`; if no inbound frame for `silenceLimitMs` → `driver.close()` and treat as unintentional close.
  - Exactly one message listener per socket: the supervisor re-attaches the handler in `notifyOpened`; remove `reinitWebSocketListeners` and the `ws-reconnected` window event (and its listener in `stores/auth.ts`).
  - Indicator: hidden while `connected`; shown only after the state has been non-connected for 2 s; text `t("connection.reconnecting")` = "Reconnecting…", when `navigator.onLine === false` `t("connection.offline")` = "You're offline. New messages will appear when you're back online."; button `t("connection.retryNow")` = "Retry now" → `triggerNow("user")`. Colours: `bg-status-warning/15` container, `text-text-primary` text. `role="status"`, `aria-live="polite"`.

- [ ] **Step 1: Write failing tests** in `websocketReconnect.test.ts` (fake timers, mock driver):
  - `nextBackoffMs grows to a 30s cap` → `[1,2,3,4,5,6,7].map(a => nextBackoffMs(a, () => 0.5))` equals `[1000,2000,4000,8000,16000,30000,30000]`.
  - `reconnects after an unintentional close` → `notifyClosed(false)`; advance 1000 ms; `driver.open` called twice in total; `onStatus` saw `("reconnecting",1)` then `("connected",0)`; `onRecovered` called once.
  - `does not reconnect after an intentional close` → `notifyClosed(true)`; advance 60 s; `open` called once.
  - `online event triggers an immediate attempt` → after a close, `window.dispatchEvent(new Event("online"))` → `open` called without advancing timers.
  - `watchdog closes a silent socket` → no `notifyFrame` for 45 s → `driver.close` called, then a retry is scheduled.
  - `reconnect refreshes an expired token before reopening` → `ensureFreshToken` resolves `true` → called before the 2nd `open`; when it resolves `false` → no 2nd `open`, a `kaiku:session-expired` event is dispatched.
  - `external mode never schedules retries itself` → `mode:"external"`, `notifyClosed(false)`, advance 60 s → `open` called once; `triggerNow("online")` → `driver.reconnectNow` called.
- [ ] **Step 2: Extend `websocket.test.ts`:** `isConnected() is false after the browser socket closes` and `one inbound frame after two reconnects is handled once` (export `handleServerEvent` for tests and spy on it).
- [ ] **Step 3: Run** `bunx vitest run src/stores/__tests__/websocketReconnect.test.ts src/stores/__tests__/websocket.test.ts` → FAIL.
- [ ] **Step 4: Implement** `connection.ts`, the hooks in `lib/tauri/websocket.ts`, wire the supervisor into `stores/websocket/index.ts` (`connect()`/`disconnect()` start/stop it; `onRecovered` = existing `handleReconnect`), `ConnectionIndicator.tsx` mounted once in `AppShell.tsx` (covers desktop and mobile layouts), catalog keys.
- [ ] **Step 5: Run** the two test files, then `bunx tsc --noEmit && bun run lint && bun run test:run` → PASS.
- [ ] **Step 6: Manual check on the dev stack** (`make dev`, `bun run dev`): log in, stop the server for 20 s, start it → indicator appears after 2 s, disappears after reconnect, a message sent from a second browser during the outage is visible after Task 3 (for now: after reload of the cached channel).
- [ ] **Step 7: CHANGELOG** `### Fixed`: "Realtime updates now recover by themselves after a network drop, sleep or server restart, and a small indicator shows while the app reconnects."
- [ ] **Step 8: Commit** `fix(ws): reconnect with backoff, watchdog and connection indicator`

---

### Task 3: Message catch-up and live DM creation (C1, C2, C3)

**Requires:** server stream merged C1 (`?after=`), C2 (`Ready.server_time`), C3 (`dm_created`). Verify against the dev stack before merging.

**Files:**
- Modify: `client/src/lib/tauri/messages.ts`, `client/src/stores/messages.ts`, `client/src/stores/websocket/index.ts` (`handleReconnect`, `handleServerEvent` cases `ready`, `dm_created`), `client/src/stores/dms.ts`, `client/src/lib/types/events.ts`
- Test: `client/src/stores/__tests__/messages.test.ts`, `client/src/stores/__tests__/dms.test.ts`, `client/src/stores/__tests__/websocket.test.ts`

**Interfaces:**
- Consumes: C1 `GET /api/messages/channel/{id}?after={message_id}&limit={n}` → `PaginatedMessages` ascending; C2 `{ type: "ready", user_id, server_time }`; C3 `{ type: "dm_created", channel: DMListItem }`.
- Produces:
  - `lib/tauri/messages.ts`: `export async function getMessagesAfter(channelId: string, afterId: string, limit = 100): Promise<PaginatedMessages>` (always `httpRequest`, no native branch).
  - `stores/messages.ts`: `export async function catchUpChannel(channelId: string, maxPages = 5): Promise<"caught_up" | "reloaded">` — anchor = id of the last element of `messagesState.byChannel[channelId]` (the list is ascending, newest last); pass each page through `decryptMessageIfNeeded` like `loadMessages` does; loop pages while `has_more`, appending items not already present (dedupe by `id`) in ascending order; still `has_more` after `maxPages` → `loadInitialMessages(channelId)` and return `"reloaded"`; empty cache → `loadInitialMessages`.
  - `stores/websocket/index.ts`: `export function getResyncWatermark(): string | null` (last `Ready.server_time`). `handleReconnect()` changes: the currently open channel (`channelsState.selectedChannelId`, or `dmsState.selectedDMId` when a DM is shown) → `catchUpChannel`; every other cached channel → `clearChannelMessages(id)` (reloads lazily when opened, which also picks up edits/deletes from the gap); if the new `server_time` minus the previous watermark exceeds 7 days → `loadInitialMessages` for the open channel instead of catch-up. `handleReconnect` waits up to 3 s for the `ready` event before comparing.
  - `stores/dms.ts`: `export function handleDmCreated(channel: DMListItem): void` — insert at the top if the id is unknown (idempotent), then `subscribeChannel(channel.id)` so reconnects re-subscribe it.
  - `lib/types/events.ts`: `ready` gains `server_time: string`; add `{ type: "dm_created"; channel: DMListItem }`.

- [ ] **Step 1: Write failing tests:**
  - messages: `catchUpChannel appends only newer messages, deduplicated` — cache `[m1, m2]`; mock `getMessagesAfter` → page 1 `{items:[m2,m3], has_more:true}`, page 2 `{items:[m4], has_more:false}` → cache ids `["m1","m2","m3","m4"]`, returns `"caught_up"`, first call's `afterId === "m2"`.
  - messages: `catchUpChannel reloads after maxPages` — every page `has_more:true` → `loadInitialMessages` spy called once, returns `"reloaded"`, `getMessagesAfter` called exactly 5 times.
  - dms: `handleDmCreated adds a new DM once` → called twice with the same channel → `dmsState.dms` contains it once, at index 0.
  - websocket: `handleReconnect catches up the open channel and clears the others` — cache for `c-open` and `c-other`, `selectedChannelId = "c-open"` → `catchUpChannel("c-open")` called, `getChannelMessages("c-other")` is `[]`.
  - websocket: `ready stores server_time as watermark` → after handling `{type:"ready", user_id:"u", server_time:"2026-11-01T10:00:00Z"}`, `getResyncWatermark()` returns that string.
  - websocket: `dm_created routes to handleDmCreated`.
- [ ] **Step 2: Run** `bunx vitest run src/stores/__tests__/messages.test.ts src/stores/__tests__/dms.test.ts src/stores/__tests__/websocket.test.ts` → FAIL.
- [ ] **Step 3: Implement** the functions above.
- [ ] **Step 4: Run** the three files, then the full gate → PASS.
- [ ] **Step 5: Dev-stack check:** user A has a channel open; stop A's network 30 s; B sends 3 messages and starts a brand-new DM with A; restore → the 3 messages appear in order without a scroll jump, the new DM is in A's list without reload.
- [ ] **Step 6: CHANGELOG** `### Fixed`: "Messages sent while you were disconnected now appear after reconnecting, and a new direct message shows up immediately for the recipient."
- [ ] **Step 7: Commit** `feat(ws): catch up missed messages and handle dm_created`

---

### Task 4: QR login keeps the server address after a restart (U-03)

**Files:**
- Create: `client/src/lib/qrLogin.ts`
- Modify: `client/src/stores/auth.ts` (`initAuth`, `loginWithOidc`: set `serverUrl`), `client/src/lib/tauri/common.ts` (`getServerUrl` fallback), `client/src/components/settings/QrLoginModal.tsx` (use `buildQrLoginUri`, error state; full string migration), `client/src/i18n/en.ts`, `client/eslint.i18n.mjs` (add `QrLoginModal.tsx`)
- Test: `client/src/lib/__tests__/qrLogin.test.ts`, `client/src/stores/__tests__/auth.test.ts`

**Interfaces:**
- Produces: `export function buildQrLoginUri(serverUrl: string, token: string): string` → `kaiku://qr/login?server=<encoded>&token=<encoded>` (format unchanged, Android parses it); throws `Error("QR_NO_SERVER")` unless `serverUrl` is an absolute `http(s)` URL.
- `getServerUrl(): string` returns `browserState.serverUrl`, falling back to `window.location.origin` only in browser mode (never `tauri://…`). Native mode gets the URL from Rust in Task 6.
- `initAuth()` success sets `authState.serverUrl = getServerUrl()`.
- `QrLoginModal`: when `buildQrLoginUri` throws, show `t("qr.noServer")` = "Can't create a login code because the server address is unknown. Sign out and sign in again." and do not call `qrLoginCreate()`.

- [ ] **Step 1: Write failing tests:** `buildQrLoginUri encodes server and token` (`("https://kaiku.example", "a+b")` → `"kaiku://qr/login?server=https%3A%2F%2Fkaiku.example&token=a%2Bb"`); `buildQrLoginUri rejects an empty server` (`""` and `"tauri://localhost"` throw `QR_NO_SERVER`); auth: `initAuth restores serverUrl from the stored server address` (localStorage `serverUrl` = `"https://kaiku.example"`, `getCurrentUser` mocked to return a user → `authState.serverUrl === "https://kaiku.example"`).
- [ ] **Step 2: Run** `bunx vitest run src/lib/__tests__/qrLogin.test.ts src/stores/__tests__/auth.test.ts` → FAIL.
- [ ] **Step 3: Implement**; migrate all `QrLoginModal.tsx` strings to `qr.*` keys.
- [ ] **Step 4: Run** the files + full gate → PASS.
- [ ] **Step 5: CHANGELOG** `### Fixed`: "The 'Log in on phone' QR code works again after restarting the app or reloading the page."
- [ ] **Step 6: Commit** `fix(auth): keep server address for QR login after restart`

---

### Task 5: E2EE off for v1.0 (U-01)

Today inert in the packaged app (detection broken); it becomes live with Task 9, so this must merge first.

**Files:**
- Create: `client/src/lib/featureFlags.ts`
- Modify: `client/src/App.tsx` (mount `E2EESetupPrompt` only when enabled), `client/src/components/settings/SecuritySettings.tsx` and `SettingsModal.tsx` (hide encryption-backup section/actions), the mount site of `BackupReminderBanner` (gate), `client/src/components/home/DMConversation.tsx` (remove padlock, tooltip, `isEncrypted`, `isE2EE` prop; migrate all strings), `client/src/stores/messages.ts` (delete `sendDMMessage`, `sendEncryptedDM`, `sendEncryptedGroupDM` and helpers only they use; reword the decrypt placeholder), `client/src/i18n/en.ts`, `client/eslint.i18n.mjs` (add `DMConversation.tsx`), `client/.env.example` (document `VITE_FEATURE_E2EE=false`)
- Test: `client/src/lib/__tests__/featureFlags.test.ts`, `client/src/stores/__tests__/messages.test.ts`, `client/e2e/dms.spec.ts`

**Interfaces:**
- Produces: `export function isE2EEEnabled(): boolean` → `import.meta.env.VITE_FEATURE_E2EE === "true"` (default off). Decision: build-time flag, no user toggle — E2EE returns post-1.0 with Android support.
- Decrypt placeholder text for legacy encrypted messages: `t("chat.encryptedPlaceholder")` = "Encrypted message — it can't be shown on this device."
- Decision: delete the dead send path rather than keep it behind the flag (no caller exists; post-1.0 E2EE will be redesigned together with Android).

- [ ] **Step 1: Write failing tests:** `isE2EEEnabled is false by default` and `true only for the exact string "true"` (`vi.stubEnv("VITE_FEATURE_E2EE", …)`); messages: `encrypted messages render the placeholder when E2EE is off`; messages: `no encrypted DM send path is exported` (`expect(Object.keys(await import("../messages"))).not.toContain("sendDMMessage")`, same for `sendEncryptedDM`, `sendEncryptedGroupDM`).
- [ ] **Step 2: Add e2e** in `e2e/dms.spec.ts`: `DM header shows no encryption indicator` (no element with `aria-label` matching `/encryption/i` in the DM header) and `no recovery-key prompt after login` (the E2EE setup dialog title is absent 3 s after login).
- [ ] **Step 3: Run** `bunx vitest run src/lib/__tests__/featureFlags.test.ts src/stores/__tests__/messages.test.ts` → FAIL.
- [ ] **Step 4: Implement**; remove any now-unused imports (`Lock`, `Unlock`, `e2eeStatus` in `DMConversation.tsx`).
- [ ] **Step 5: Run** unit gate → PASS; `bun run test:e2e -- dms.spec.ts` against the dev stack → PASS.
- [ ] **Step 6: CHANGELOG** `### Changed`: "End-to-end encryption setup is no longer offered and the DM padlock is removed; direct messages are protected in transit (TLS) and on the server, and end-to-end encryption will return in a later release."
- [ ] **Step 7: Commit** `fix(crypto): disable E2EE prompts and padlock for v1.0`

---

### Task 6: Rust-owned native session (D-03 client half, D-01 login/register mismatch)

Native mode stays dormant until Task 9; tests drive it directly.

**Files:**
- Create: `client/src-tauri/src/session/mod.rs`, `client/src-tauri/src/session/store.rs`
- Modify: `client/src-tauri/src/lib.rs` (`mod session;`, `AppState.session: Arc<SessionManager>`, start refresh scheduler in `setup`, register new commands), `client/src-tauri/src/commands/auth.rs` (thin wrappers; remove duplicated token/keyring code), `client/src-tauri/Cargo.toml` (`[dev-dependencies] wiremock = "0.6"`), `client/src/lib/tauri/common.ts`, `client/src/lib/tauri/auth.ts`, `client/src/views/Login.tsx` + `Register.tsx` (native OIDC result shape only), `client/src/stores/auth.ts` (show "not remembered" toast; subscribe to native auth events)
- Test: Rust `#[cfg(test)] mod tests` in `session/mod.rs`; TS `client/src/lib/__tests__/nativeAuth.test.ts`

**Interfaces:**
- Rust (`session`):
  - `pub struct StoredSession { pub server_url: String, pub refresh_token: String }`
  - `pub trait TokenStore: Send + Sync { fn load(&self) -> Result<Option<StoredSession>, StoreError>; fn save(&self, s: &StoredSession) -> Result<(), StoreError>; fn clear(&self, server_url: &str) -> Result<(), StoreError>; }` with `KeyringTokenStore` (existing keyring keys: service + `server_url` entry + per-server token entry) and `MemoryTokenStore` (tests; can be told to fail `save`).
  - `#[derive(Serialize)] pub struct AuthInfo { pub server_url: String, pub access_token: String, pub expires_at_ms: u64 }`
  - `#[derive(Serialize)] pub struct AuthResult { pub user: serde_json::Value, pub setup_required: bool, pub password_change_required: bool, pub persisted: bool }` — `user` is the raw `/auth/me` JSON so the webview gets every field; the internal `crate::User` is parsed from the same value for `AuthState.user` (used by `commands/crypto.rs`).
  - `#[derive(Serialize)] pub struct SessionRestoreResult { pub user: Option<serde_json::Value>, pub error_reason: Option<String>, pub password_change_required: bool }`
  - `pub enum SessionError { Network(String), InvalidCredentials, MfaRequired, Expired, Server(u16, String) }`; `Display` of `MfaRequired` is exactly `"MFA_REQUIRED"` (TS relies on it).
  - `impl SessionManager`: `login(&self, server_url: &str, username: &str, password: &str, mfa_code: Option<String>) -> Result<AuthResult, SessionError>`, `register(&self, req: RegisterRequest) -> Result<AuthResult, SessionError>`, `adopt_tokens(&self, server_url: &str, tokens: TokenResponse) -> Result<AuthResult, SessionError>` (shared by login/register/OIDC), `restore(&self) -> SessionRestoreResult`, `refresh(&self) -> Result<AuthInfo, SessionError>` (single-flight: holds a `tokio::sync::Mutex`; if the access token changed while waiting, returns the current one without calling the server), `logout(&self) -> Result<(), SessionError>`, `auth_info(&self) -> Option<AuthInfo>`, `session_request(&self, method: &str, path: &str, body: Option<serde_json::Value>) -> Result<serde_json::Value, SessionError>`.
  - `TokenResponse` gains `#[serde(default)] setup_required: bool` and `#[serde(default)] password_change_required: bool` (C4).
  - `session_request` allowlist (exact match after parsing; anything else → error without any HTTP call): `GET /auth/sessions`, `DELETE /auth/sessions`, `DELETE /auth/sessions/{uuid}`, `POST /auth/me/password`. Sends bearer + `X-Refresh-Token` (server identifies the current session from it, `server/src/auth/helpers.rs`).
  - `pub fn spawn_refresh_scheduler(app: AppHandle, mgr: Arc<SessionManager>)` — refreshes at `expires_at - 60 s`, emits `auth:token_refreshed` (payload `AuthInfo`) or `auth:session_expired` (no payload).
- Tauri commands (names from C6 kept): `login(request) -> Result<AuthResult, String>`, `register(request) -> Result<AuthResult, String>`, `get_current_user() -> Result<SessionRestoreResult, String>`, `get_auth_info() -> Result<Option<AuthInfo>, String>` (struct replaces the old tuple), new `refresh_session() -> Result<Option<AuthInfo>, String>` (`None` = expired), new `session_request(method: String, path: String, body: Option<Value>) -> Result<Value, String>`, `logout()`, `oidc_authorize(...) -> Result<AuthResult, String>` (no tokens leave Rust).
- TypeScript (`common.ts`):
  - `export interface NativeAuthInfo { server_url: string; access_token: string; expires_at_ms: number }`
  - `export interface AuthContext { baseUrl: string; accessToken: string | null }`; `export async function getAuthContext(): Promise<AuthContext>` — browser: `browserState`; native: cached `NativeAuthInfo` (set by `export function setNativeAuthInfo(info: NativeAuthInfo | null): void`), else `invoke("get_auth_info")`.
  - `httpRequest` uses `getAuthContext()`, never sends `X-Refresh-Token`, sends `credentials: "include"` only in browser mode, and on a `401` from a non-auth endpoint calls `refreshAccessToken()` once and retries once.
  - `refreshAccessToken()` native branch: `invoke("refresh_session")` → cache result; `null` → `setNativeAuthInfo(null)` + dispatch `kaiku:session-expired`.
  - `export async function initNativeAuthEvents(): Promise<void>` — listens to `auth:token_refreshed` → `setNativeAuthInfo`, `auth:session_expired` → `kaiku:session-expired`; called once from `initAuth`/`login` in native mode.
  - Remove `browserState.refreshToken` and every `if (isTauri)` refresh-token branch; `AuthResult` gains `password_change_required: boolean` and `persisted?: boolean`; `getUploadAuth()` becomes `getAuthContext()`-based.
  - `auth.ts` native branches: `listSessions`, `revokeSession`, `revokeAllOtherSessions`, `updatePassword` → `invoke("session_request", …)`; `login`/`register` → `invoke` then `setNativeAuthInfo(await invoke("get_auth_info"))` and `browserState.serverUrl = info.server_url`.
  - `persisted === false` → toast `t("auth.notRemembered")` = "Kaiku couldn't save your login on this computer, so you'll need to sign in again next time."

- [ ] **Step 1: Write failing Rust tests** (wiremock server, `MemoryTokenStore`):
  - `login_returns_full_user_and_flags` — `/auth/login` returns tokens with `setup_required:true, password_change_required:true`; `/auth/me` returns JSON with an extra field `"is_admin": true` → `result.user["is_admin"] == true`, both flags true, `persisted == true`, store holds the refresh token.
  - `login_maps_mfa_required` — 403 `{"error":"MFA_REQUIRED"}` → `Err(SessionError::MfaRequired)`, `to_string() == "MFA_REQUIRED"`.
  - `refresh_rotates_and_persists` — new refresh token is in the store, `auth_info().access_token` is the new one.
  - `concurrent_refresh_calls_hit_server_once` — `tokio::join!(m.refresh(), m.refresh())` both `Ok`, wiremock mock `.expect(1)`.
  - `refresh_401_clears_store_and_returns_expired`.
  - `restore_without_stored_session_returns_none` → `user: None, error_reason: None`.
  - `restore_network_error_keeps_stored_token` → server unreachable → `error_reason == Some("network_error")`, store unchanged.
  - `keyring_failure_still_logs_in_with_persisted_false`.
  - `session_request_rejects_paths_outside_allowlist` — `("GET","/api/guilds")`, `("DELETE","/auth/sessions/../x")`, `("POST","/auth/sessions")` → `Err`, wiremock received 0 requests.
  - `session_request_sends_refresh_header` — `DELETE /auth/sessions` carries `x-refresh-token` and bearer.
- [ ] **Step 2: Run** `cargo test -p vc-client --lib session::` → FAIL (module missing).
- [ ] **Step 3: Implement** `session/` and rewire `commands/auth.rs`, `lib.rs`.
- [ ] **Step 4: Run** `cargo test -p vc-client --lib session:: && cargo fmt --check && SQLX_OFFLINE=true cargo clippy -p vc-client -- -D warnings && cargo deny check licenses` → PASS.
- [ ] **Step 5: Write failing TS tests** in `nativeAuth.test.ts` (set `window.__TAURI__ = {}` then `vi.resetModules()` + dynamic import — Task 9 switches these to mocking `@tauri-apps/api/core`; mock `invoke`, `listen`, `fetch`):
  - `login in native mode returns AuthResult and caches auth info` → `invoke` called with `"login"`, then `"get_auth_info"`; `getServerUrl() === "https://srv"`.
  - `httpRequest in native mode uses the Rust base URL and bearer` → `fetch` URL starts `https://srv/api/`, header `Authorization: Bearer tok`, no `X-Refresh-Token`, `credentials` not `"include"`.
  - `httpRequest retries once after 401 via refresh_session` → first fetch 401, `invoke("refresh_session")` returns new info, second fetch uses the new bearer; a second 401 is thrown, not retried again.
  - `refresh_session returning null dispatches kaiku:session-expired`.
  - `listSessions in native mode goes through session_request`.
  - browser regression: `httpRequest in browser mode still sends credentials and no X-Refresh-Token`.
- [ ] **Step 6: Run** `bunx vitest run src/lib/__tests__/nativeAuth.test.ts` → FAIL; implement TS; run again + full gate → PASS.
- [ ] **Step 7: Commit** (no CHANGELOG yet — dormant until Task 9) `feat(auth): move desktop tokens and refresh into Rust session manager`

---

### Task 7: All REST through `httpRequest` + invoke registration check (U-02, D-01)

Decision: collapse **every** pure-REST `invoke` (registered or not) onto `httpRequest`, not only the 48 unregistered ones. Reason: the registered Rust REST proxies re-type responses with reduced structs (e.g. Rust `User` drops fields) and have never run in a shipped build; one REST path, the one CI tests, is safer. Native commands remain only for: session/auth (`login`, `register`, `logout`, `get_current_user`, `get_auth_info`, `refresh_session`, `session_request`, `oidc_authorize`, `oidc_link_identity`), `ws_*`, voice/screen-share/webcam/video, tray, local settings/UI state, crypto/E2EE, presence scanning, sound, clipboard, calls only where they touch native voice.

**Files:**
- Create: `client/scripts/check-tauri-commands.ts`
- Modify: `client/src/lib/tauri/{admin,auth,channels,dms,guilds,messages,pages}.ts`, `client/src/stores/{preferences,favorites,presence}.ts`, `client/src/components/SetupWizard.tsx` (`get_access_token` → `getAuthContext()`), `client/package.json` (`"lint:tauri-commands": "bun scripts/check-tauri-commands.ts"`), `.github/workflows/ci.yml` (Frontend job: run it after Lint), `client/src-tauri/src/lib.rs` + `client/src-tauri/src/commands/{chat,pages,roles,admin,preferences,channel_pins,pins,favorites}.rs` and `auth.rs` MFA/QR commands (delete Rust REST proxies no TS code invokes any more; delete a whole module only when nothing in Rust uses it)
- Test: `client/src/lib/__tests__/restTransport.test.ts`, `client/scripts/__tests__/check-tauri-commands.test.ts`

**Interfaces:**
- Consumes: `getAuthContext()`, `httpRequest()` (Task 6).
- Produces: `check-tauri-commands.ts` exports `export function findInvokedCommands(srcFiles: Map<string, string>): Set<string>` (matches `invoke(<…>)?("name"` across `client/src/**/*.{ts,tsx}`, excluding `__tests__`), `export function findRegisteredCommands(libRs: string): Set<string>` (identifiers inside `generate_handler![…]`), `export function findUncalledIsTauri(srcFiles: Map<string,string>): string[]` (Task 9 enables it); CLI prints each unregistered name with its file and exits 1. Also fails on any `invoke(` whose first argument is not a string literal.

- [ ] **Step 1: Write failing tests:**
  - `check-tauri-commands.test.ts`: `reports an invoke name missing from generate_handler` (fixture `invoke("get_guilds")` vs handler without it → `["get_guilds"]`); `accepts registered names with type params` (`invoke<Foo>("login", …)`); `rejects non-literal invoke names`.
  - `restTransport.test.ts` (native mode as in Task 6), table-driven over `getGuilds()`, `getDMList()`, `deleteMessage("m")`, `addReaction("c","m","👍")`, `markChannelAsRead("c","m")`, `adminListUsers()`, `getMessages("c")`, `mfaSetup()`, `qrLoginCreate()`: each issues exactly one `fetch` to the expected method + path with the bearer, and `invoke` is never called with the old command name.
- [ ] **Step 2: Run** `bunx vitest run src/lib/__tests__/restTransport.test.ts scripts/__tests__` → FAIL (extend `vitest.config.ts` `include` with `scripts/**/*.test.ts`).
- [ ] **Step 3: Implement** the collapse (delete each `if (isTauri) { invoke(...) }` REST branch), the script, CI step, and the Rust deletions.
- [ ] **Step 4: Run** `bun run lint:tauri-commands` → exit 0 with zero unregistered names; full TS gate; `cargo build -p vc-client && SQLX_OFFLINE=true cargo clippy -p vc-client -- -D warnings` → PASS.
- [ ] **Step 5: Commit** (internal, no CHANGELOG) `refactor(client): route desktop REST through httpRequest, check invokes`

---

### Task 8: Native WebSocket manager (token-aware reconnect, one event path)

**Files:**
- Modify: `client/src-tauri/Cargo.toml` (`tokio-tungstenite = { workspace = true, features = ["rustls-tls-native-roots"] }` — today the client crate has no TLS feature, so `wss://` cannot connect at all), `client/src-tauri/src/lib.rs` (install the rustls crypto provider once at startup if the handshake test requires it; register `ws_send`, `ws_reconnect_now`), `client/src-tauri/src/network/websocket.rs`, `client/src-tauri/src/commands/websocket.rs`, `client/src/lib/tauri/websocket.ts`, `client/src/stores/websocket/index.ts` (native branch of `initWebSocket`)
- Test: Rust tests in `network/websocket.rs`; TS `client/src/stores/__tests__/websocket.test.ts`

**Interfaces:**
- Consumes: `SessionManager::{auth_info, refresh}` (Task 6); supervisor `mode: "external"` (Task 2).
- Produces (Rust):
  - `WebSocketManager::connect(app: AppHandle, session: Arc<SessionManager>, timings: WsTimings) -> Result<Self, String>`; `pub struct WsTimings { pub ping_every: Duration, pub silence_limit: Duration }` (default 20 s / 45 s).
  - Before each attempt read the current token from `session.auth_info()`; if it expires within 60 s, `session.refresh()` first. A handshake rejected with HTTP 401 → `refresh()` and retry immediately once; `SessionError::Expired` → emit `auth:session_expired` and `ws:disconnected`, exit the loop.
  - `pub fn next_backoff(attempt: u32, jitter: f64) -> Duration` (same curve as `nextBackoffMs`), `fn is_auth_rejection(err: &tungstenite::Error) -> bool`.
  - Outgoing queue `enum Outbound { Event(ClientEvent), Raw(String) }`; command `ws_send(message: String) -> Result<(), String>` accepts only a JSON object with a string `type`.
  - Incoming: every text frame is emitted once as `ws:server_event` with the parsed `serde_json::Value`; the typed `ServerEvent` enum and per-type `ws:<name>` emits are removed (they silently dropped unknown events). Status events `ws:connecting`, `ws:connected`, `ws:reconnecting` (attempt `u32`), `ws:disconnected` stay.
  - Watchdog: send `ClientEvent::Ping` every `ping_every`; no inbound frame for `silence_limit` → close and reconnect.
  - Command `ws_reconnect_now()` — wakes the backoff sleep via `tokio::sync::Notify`.
- Produces (TS): native `initWebSocket` registers one `listen("ws:server_event", e => handleServerEvent(e.payload))` plus the four status listeners mapped to `supervisor` (`ws:connected` after `ws:reconnecting` → `onRecovered`); native driver `{ open: invoke("ws_connect"), close: invoke("ws_disconnect"), sendPing: no-op (Rust pings), reconnectNow: invoke("ws_reconnect_now") }`; `wsSend()` native branch uses `ws_send`.

- [ ] **Step 1: Write failing Rust tests:**
  - `next_backoff_caps_at_30s` (`next_backoff(10, 0.0) == 30 s`, `next_backoff(1, 0.0) == 1 s`).
  - `wss_scheme_is_supported` — `connect_async("wss://127.0.0.1:9/ws")` returns an IO/connection error, **not** `Error::Url(UrlError::TlsFeatureNotEnabled)`.
  - `ws_send_rejects_non_object_json` (`"[]"`, `"{\"x\":1}"` → `Err`; `{"type":"typing","channel_id":"c"}` → `Ok`).
  - `auth_rejection_detected_for_http_401`.
  - `reconnect_uses_refreshed_token` — local `TcpListener` + `tokio_tungstenite::accept_hdr_async` that answers 401 for protocol `access_token.old` and accepts `access_token.new`; wiremock `/auth/refresh` returns `new` → second handshake carries `new`, `ws:connected` reached (use a test hook instead of `AppHandle` emits: make the emitter a `trait WsEvents` with a recording impl in tests).
  - `silent_server_triggers_watchdog_reconnect` — `tokio::time::pause()`, server accepts and never sends; after `silence_limit` a second connection attempt arrives.
  - `unknown_event_types_are_forwarded` — server sends `{"type":"forum_post_created", …}` → recorded `ws:server_event` with that `type`.
- [ ] **Step 2: Run** `cargo test -p vc-client --lib network::` → FAIL.
- [ ] **Step 3: Implement**; `cargo deny check licenses` after the feature change.
- [ ] **Step 4: Write failing TS tests** in `websocket.test.ts` (native mode as in Task 6, `listen` mock capturing callbacks): `native ws:server_event is handled by the shared handler` (a `message_new` payload adds the message); `native ws:connected after ws:reconnecting runs handleReconnect`; `online event in native mode invokes ws_reconnect_now`.
- [ ] **Step 5: Run** TS tests → FAIL; implement; run Rust + TS gates → PASS.
- [ ] **Step 6: Commit** (dormant until Task 9) `fix(ws): token-aware native reconnect and raw event forwarding`

---

### Task 9: Activate native mode — `isTauri()` (D-01, C6, U-02 live)

**Files:**
- Modify: `client/src/lib/tauri/common.ts` (`isTauri` becomes a function), every detection site: `components/E2EESetupPrompt.tsx`, `components/SetupWizard.tsx`, `components/chat/E2EESetupModal.tsx`, `components/settings/PrivacySettings.tsx`, `components/voice/VoiceControls.tsx`, `components/voice/VoiceTile.tsx`, `lib/clipboard.ts`, `lib/notifications.ts`, `lib/pttManager.ts`, `lib/trayBadge.ts`, `lib/updater.ts`, `lib/sentry.ts`, `lib/sound/index.ts`, `lib/webrtc/index.ts`, `stores/favorites.ts`, `stores/preferences.ts`, `stores/presence.ts`, `stores/voice.ts`, `stores/websocket/index.ts`, `views/ForgotPassword.tsx`, `views/ResetPassword.tsx`, all `lib/tauri/*.ts` users of `isTauri`; `client/eslint.config.mjs` + `client/eslint.i18n.mjs` (ban); `client/scripts/check-tauri-commands.ts` (enable `findUncalledIsTauri`); `client/src/AGENTS.md`, `client/src/lib/AGENTS.md` (replace the `__TAURI__` guidance); `.github/workflows/ci.yml` (Tauri ubuntu job: `cargo test -p vc-client --lib` — coordinate with the release stream, which also edits this workflow); tests from Tasks 6–8 (switch to mocking `@tauri-apps/api/core`)
- Test: `client/src/lib/__tests__/tauriDetection.test.ts`

**Interfaces:**
- Produces (C6): `export function isTauri(): boolean` in `client/src/lib/tauri/common.ts`, implemented as `import { isTauri as coreIsTauri } from "@tauri-apps/api/core"` → `return coreIsTauri()`. No other module defines its own detection.
- Lint: global `no-restricted-syntax` entries `Literal[value='__TAURI__']` and `MemberExpression[property.name='__TAURI__']`, message `"Use isTauri() from @/lib/tauri/common (contract C6)."`; the same selectors are added to `I18N_SELECTORS` in `eslint.i18n.mjs` because a file-specific `no-restricted-syntax` block replaces the global one.
- `findUncalledIsTauri` flags any `isTauri` reference not followed by `(` outside import/export lines (an uncalled function is always truthy).
- Native voice path (D-06): `createVoiceAdapter()` returns `TauriVoiceAdapter` when `isTauri()` — on Linux this is the only voice path that does not depend on WebKitGTK WebRTC.

- [ ] **Step 1: Write failing tests** in `tauriDetection.test.ts`: `isTauri() is true when the Tauri core reports it, without a __TAURI__ global` (mock core `isTauri: () => true`, `"__TAURI__" in window === false`); `isTauri() is false in a plain browser`; `createVoiceAdapter returns the Tauri adapter in native mode` (`instanceof TauriVoiceAdapter`); `lint rejects the __TAURI__ literal` (ESLint `lintText('const a = "__TAURI__" in window;', { filePath: "src/lib/x.ts" })` → `no-restricted-syntax`); in `check-tauri-commands.test.ts`: `flags an uncalled isTauri`.
- [ ] **Step 2: Run** `bunx vitest run src/lib/__tests__/tauriDetection.test.ts scripts/__tests__` → FAIL.
- [ ] **Step 3: Implement**; `rg -n '__TAURI__' client/src` must return only AGENTS.md history lines (remove those too).
- [ ] **Step 4: Run** full TS gate + `bun run lint:tauri-commands` + `cargo test -p vc-client --lib` → PASS.
- [ ] **Step 5: Smoke test** `cd client && bun run tauri dev` against the dev stack: devtools console shows `[VoiceAdapter] Creating Tauri adapter` on voice join; login → quit → restart → still logged in.
- [ ] **Step 6: CHANGELOG** `### Fixed`: "The desktop app now runs as a real desktop app again: your login is remembered securely across restarts, voice uses the built-in native engine, and tray, notifications and updates are active."
- [ ] **Step 7: Commit** `fix(client): detect Tauri via core isTauri and enable native mode`

---

### Task 10: macOS microphone/camera usage strings (D-05)

**Files:**
- Create: `client/src-tauri/Info.plist` (`NSMicrophoneUsageDescription` = "Kaiku uses the microphone for voice channels and calls.", `NSCameraUsageDescription` = "Kaiku uses the camera when you turn on your webcam in a voice channel."), `client/src-tauri/Entitlements.plist` (stub for hardened runtime/notarisation: `com.apple.security.device.audio-input` and `com.apple.security.device.camera` = true)
- Modify: `client/src-tauri/tauri.conf.json` (`bundle.macOS.entitlements: "./Entitlements.plist"`; Tauri merges `Info.plist` automatically), `scripts/check_ci_guardrails.py` (new check)
- Test: the guardrail itself

**Interfaces:** `check_macos_bundle(errors: list[str]) -> None` in `scripts/check_ci_guardrails.py`: both usage keys present with non-empty strings, entitlements file referenced in `tauri.conf.json` exists. (OS-level strings stay in the plist; localisation is `InfoPlist.strings` later, not the web catalog.)

- [ ] **Step 1: Add `check_macos_bundle`** and run `python3 scripts/check_ci_guardrails.py` → FAIL (files missing).
- [ ] **Step 2: Create the two plists and the config entry**; rerun → PASS.
- [ ] **Step 3: CHANGELOG** `### Fixed`: "macOS: the app no longer quits the first time it uses the microphone or camera; macOS now asks for permission."
- [ ] **Step 4: Commit** `fix(client): add macOS microphone and camera usage strings`

---

### Task 11: Native voice ICE servers + per-OS manual checklist (D-06)

**Files:**
- Modify: `client/src-tauri/src/commands/voice.rs` (`join_voice` uses server ICE config), `client/src-tauri/src/commands/settings.rs` (new command), `client/src-tauri/src/lib.rs` (register), `client/src/views/Login.tsx` (native mode: default server from `get_default_server_url` when set)
- Create: `docs/developer-guide/testing/desktop-manual-checklist.md`
- Test: Rust tests in `commands/voice.rs` (or a `voice/ice.rs` helper)

**Interfaces:**
- `pub async fn fetch_ice_servers(http: &reqwest::Client, server_url: &str, token: &str) -> Vec<IceServerConfig>` — `GET /api/voice/ice-servers`, reads `ice_servers[].{urls, username, credential}` (same shape the browser adapter uses), returns `vec![IceServerConfig::default()]` on any error or an empty list; logs a warning naming TURN as unavailable.
- `#[command] pub fn get_default_server_url() -> Option<String>` — reads env `KAIKU_SERVER_URL` (tester override so CI-built packages can point at the dev stack; invisible to family users). Login in native mode prefers it over `VITE_SERVER_URL`.
- Checklist content (decisions): test against the dev stack served over HTTPS by Caddy `tls internal` on the dev host (root CA installed on each test PC — the bundled CSP allows only `https:`/`wss:`), packages from the PR's CI artifacts (`tauri-linux`, `tauri-windows`, `tauri-macos`), test users from `scripts/create-test-users.sh`, second participant in a browser. One section per OS (Windows, Linux, macOS), each with checkbox rows: install (SmartScreen/Gatekeeper path noted), login, quit + restart still logged in, DM both directions, new DM appears live, network off 30 s → indicator → missed messages appear, sleep ≥ 20 min → recovers without login, server restart → recovers, voice join uses native adapter (console line), two-way audio, mute/deafen, mic device switch, leave → no ghost in the browser participant list, macOS permission prompt shows the usage text and no crash, Linux with and without a Secret Service keyring (expect the "not remembered" toast without), tray icon + close-to-tray, QR code contains the server address, admin create account + forced change (after Tasks 12–13), logout → restart → login screen. Result table: date, build/commit, OS + version, pass/fail, notes.

- [ ] **Step 1: Write failing Rust tests:** `ice_servers_parsed_from_server` (wiremock returns one STUN + one TURN with credentials → 2 configs, TURN credentials kept); `ice_servers_fall_back_to_default_on_error` (500 → `[IceServerConfig::default()]`); `default_server_url_reads_env` (`KAIKU_SERVER_URL` set/unset).
- [ ] **Step 2: Run** `cargo test -p vc-client --lib ice_servers default_server_url` → FAIL; implement; rerun + clippy → PASS.
- [ ] **Step 3: Write the checklist doc**; run `python3 scripts/check_docs_governance.py` → PASS.
- [ ] **Step 4: CHANGELOG** `### Fixed`: "Desktop voice now uses the server's TURN relay, so voice also works on restrictive networks."
- [ ] **Step 5: Commit** `feat(voice): use server ICE config in native voice; add desktop checklist`

---

### Task 12: Admin "Create account" (C4)

**Requires:** server C4 `POST /api/admin/users` (elevated session). Contract says the response is "the admin user view" — this plan assumes the same `UserSummary` shape as list items (see report).

**Files:**
- Create: `client/src/lib/passwords.ts`, `client/src/components/admin/CreateAccountModal.tsx`
- Modify: `client/src/lib/tauri/admin.ts`, `client/src/lib/types/admin.ts`, `client/src/stores/admin.ts`, `client/src/components/admin/UsersPanel.tsx` (header button only), `client/src/i18n/en.ts`, `client/eslint.i18n.mjs` (add `CreateAccountModal.tsx`)
- Test: `client/src/lib/__tests__/passwords.test.ts`, `client/src/lib/__tests__/adminCreateUser.test.ts`

**Interfaces:**
- `export interface AdminCreateUserRequest { username: string; display_name: string | null; temporary_password: string }` (snake_case wire type).
- `export async function adminCreateUser(body: AdminCreateUserRequest): Promise<UserSummary>` → `httpRequest("POST", "/api/admin/users", body)`.
- `stores/admin.ts`: `export async function createUser(req: AdminCreateUserRequest): Promise<UserSummary>` — prepends to the loaded user list.
- `export function generateTemporaryPassword(length = 16): string` — `crypto.getRandomValues`, alphabet without look-alikes (`0 O o 1 l I`), rejection sampling (no modulo bias).
- Modal: username, display name (optional), temporary password (prefilled, "Generate new", "Copy"); disabled with `t("admin.createAccount.needsElevation")` unless the session is elevated (reuse UsersPanel's elevation state/button); client validation username non-empty and password 8–128 chars (server rule in `server/src/auth/login.rs`); 409 → `t("admin.createAccount.usernameTaken")` = "That username is already taken."; success view: server address, username, temporary password, `t("admin.createAccount.handover")` = "Give these to {{name}}. They will choose their own password the first time they sign in." and "Copy all".

- [ ] **Step 1: Write failing tests:** `generateTemporaryPassword has the requested length and only allowed characters`; `100 generated passwords are unique`; `adminCreateUser posts the contract body` (fetch called with `POST …/api/admin/users`, JSON body equals the request); `adminCreateUser surfaces 409 as HttpError` (`status === 409`).
- [ ] **Step 2: Run** `bunx vitest run src/lib/__tests__/passwords.test.ts src/lib/__tests__/adminCreateUser.test.ts` → FAIL; implement; rerun + full gate → PASS.
- [ ] **Step 3: CHANGELOG** `### Added`: "Admins can create accounts with a temporary password from the admin dashboard, so registration can stay closed."
- [ ] **Step 4: Commit** `feat(client): admin create-account dialog`

---

### Task 13: Forced password change screen (C4)

**Files:**
- Create: `client/src/views/ForcedPasswordChange.tsx`
- Modify: `client/src/stores/auth.ts`, `client/src/components/auth/AuthGuard.tsx`, `client/src/lib/tauri/common.ts` (`PASSWORD_CHANGE_REQUIRED` detection), `client/src/lib/tauri/auth.ts` (browser `AuthResponse.password_change_required?: boolean`), `client/src/i18n/en.ts`, `client/eslint.i18n.mjs` (add the new view), `client/e2e/admin.spec.ts`
- Test: `client/src/stores/__tests__/auth.test.ts`

**Interfaces:**
- `authState.passwordChangeRequired: boolean`.
- `async function startSessionServices(): Promise<void>` (extracted from the duplicated blocks in `initAuth`/`login`/`register`: WS listeners + connect, presence, preferences, idle detection) — not called while `passwordChangeRequired`.
- `httpRequest`: 403 with `error === "PASSWORD_CHANGE_REQUIRED"` → dispatch `kaiku:password-change-required` and throw `HttpError(403, "PASSWORD_CHANGE_REQUIRED")`; `stores/auth.ts` listens and sets the flag (covers restore after restart, where no login response carries the flag).
- `export async function completeForcedPasswordChange(currentPassword: string | null, newPassword: string): Promise<void>` — uses `currentPassword ?? pendingTemporaryPassword` (module variable set by `login()` only when the flag is true; memory only, cleared on success/logout); calls `updatePassword(current, new, true)`, then `refreshAccessToken()`, clears the flag, `startSessionServices()`.
- Screen: shows the temporary-password field only when `pendingTemporaryPassword` is null; new + confirm (8–128); "Sign out" button; strings `auth.forcedChange.*`, e.g. title "Choose your own password".

- [ ] **Step 1: Write failing tests** in `auth.test.ts`: `login with password_change_required sets the flag and does not connect the websocket`; `completeForcedPasswordChange uses the remembered temporary password, refreshes, clears the flag and connects` (`updatePassword` called with `("temp-pw", "new-pw-123", true)`); `PASSWORD_CHANGE_REQUIRED from any request switches to the change screen` (dispatch the event after a restored session → flag true, `pendingTemporaryPassword` null so the screen asks for it); `logout clears the remembered temporary password`.
- [ ] **Step 2: Add e2e** in `e2e/admin.spec.ts`: `admin creates an account and the new user must change the password` — admin elevates, creates user via the modal, signs out; new user signs in with the temporary password → change screen → sets a new one → main view loads; signing in again with the temporary password fails.
- [ ] **Step 3: Run** `bunx vitest run src/stores/__tests__/auth.test.ts` → FAIL; implement; rerun + full gate → PASS; `bun run test:e2e -- admin.spec.ts` on the dev stack (server C4 merged) → PASS.
- [ ] **Step 4: CHANGELOG** `### Added`: "People signing in with a temporary password are asked to choose their own password first."
- [ ] **Step 5: Commit** `feat(auth): forced password change after admin-created login`
