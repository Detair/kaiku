# L2a (desktop/web) implementation notes — 2026-11-03

Branch `feat/v1.0-login-l2a`, based on `origin/main` 7a9e7677; since 2026-11-03 rebased onto
LP `origin/feat/v1.0-login-lp` 203b822c (on L1a 267e4c78), see "LP rebase" at the end. Plan:
`docs/superpowers/plans/2026-11-03-v1.0-login-clients.md` section A (A1–A14). Spec rev 5 and
contracts C26–C28 win where the plan differs. L1a, LP and L1b are not on main: everything is
built against the frozen contracts with mocked `fetch` / Tauri `invoke` / wiremock.

Commit `96918eb6` ("set up a phone …") carries the line **L2a-1 boundary** (A1–A7). A8–A13
follow it (L2a-2).

## Task status

| Task | Status |
|---|---|
| A1 wire types, API wrappers | done |
| A2 desktop Rust: attempts, pre-session secrets, refresh reasons | done; OIDC loopback delivery (C29 step 8) on LP's listener since the LP rebase |
| A3 sign-in screen (phone QR, confirmation, TOTP, neutral answers) | done; LP's PasskeyButton first since the LP rebase |
| A4 probation banner, full-session dialog, Web Locks, SESSION_REUSED | done; passkey sign-in through LP's attempt logic |
| A5 step-up | done at the LP rebase (ReauthDialog and C24 re-auth removed, elevation, linked accounts, Linux hint, empty passkey list) |
| A6 security settings | done (+ follow-up: AccountSettings uses `PasswordSection`, old `ChangePasswordModal` removed) |
| A7 set up a phone, CodeEntry | done |
| A8 `/setup` page | done |
| A9 Kaiku DM, notices, security log | done |
| A10 open-request modal, prompts | done |
| A11 recovery, "Secure my account", request client | done |
| A12 registration ticket, bind step | done |
| A13 admin per-user security, passwordless create, open requests | done |
| A14 gate | partly: CHANGELOG lines added per task; rebased onto LP (on L1a); the rebase onto main after L1a, LP and L1b merge and the browser/desktop checks are still open |

## LP seams (replaced at the LP rebase, 2026-11-03; kept as the record)

1. `client/src/lib/passkeySeam.ts` — `registerPasskeyCeremonies` / `passkeyCeremonies()`:
   - `signIn` → LP `stores/auth.ts::loginWithPasskey(serverUrl, {mediation, signal})`
     (used by `replaceSessionWithPasskey`; if LP's function already calls `completeSignIn`, drop
     that call);
   - `stepUp` → LP `lib/tauri/passkeys.ts::stepUpWithPasskey(action, target, signal)`
     (used by `lib/stepUp.ts`);
   - `create` → LP `lib/webauthn.ts` (`creationOptionsFromJSON` + `credentialToJSON`), used by
     `/setup`, the request passkey and the registration passkey. Fallback today: WebAuthn L3
     `PublicKeyCredential.parseCreationOptionsFromJSON` / `toJSON()`.
   - While nothing is registered, every "Sign in with a passkey" / "Confirm with a passkey" button
     is hidden and the step-up dialog shows "Confirm this in the Kaiku app on your phone."
   Then delete the module and import LP's functions directly.
2. `client/src/views/Login.tsx` — `TODO(LP rebase)`: LP's `PasskeyButton` goes first, above
   "Sign in with your phone"; pass LP's sign-in as `QrLoginPanel.onUsePasskey`.
3. `client/src/components/auth/StepUpDialog.tsx`, `FullSessionRequiredDialog.tsx` —
   `TODO(LP rebase)`: mount `LinuxPasskeyHint` (to be extracted from LP's `PasskeyButton` with
   LP's text key).
4. `client/src/components/settings/SecuritySettings.tsx` — `TODO(LP rebase)`: LP's
   `PasskeysSection` above the TOTP section; its empty state gains "Set up a phone" and
   "Secure my account".
5. `client/src/components/security/SecurityPromptBanner.tsx` — `add_passkey` opens Security
   settings until LP's "Add" can be opened directly.
6. `client/src-tauri/src/commands/auth.rs::oidc_authorize` — `TODO(LP rebase, C29 step 8)`: bind
   with `loopback_post::bind()`, send `loopback_port` + `callback_path` instead of
   `redirect_uri`, open with `open_browser(app, url, settings.passkey_browser)`, wait with
   `wait_for_post`, delete the GET hand-back. The exchange already returns the `LoginStep` and
   keeps the verifier as the attempt's collection secret.
7. `client/src-tauri/src/commands/pre_session.rs::request_oidc_factor` — refuses with
   `OIDC_UNAVAILABLE` until the same LP delivery exists (keep the verifier with
   `SessionManager::keep_request_oidc_verifier`).

Deferred A5 edits on LP-owned files (do them in the rebase commit): delete `ReauthDialog` and
the C24 password/OIDC paths in `lib/reauth.ts`, `startOidcReauth` and `commands/reauth.rs`
`oidc_reauth*`; `ElevateDialog` → LP `elevateWithStepUp(reason)` behind `withStepUp`;
`LinkedAccountsSection` → `identity.link` / `identity.unlink` grants; LP's narrow
`StepUpAction` / `StepUpGrant` import the full table from `lib/types/login.ts`; the A5 CHANGELOG
line ("Sensitive actions … Password and single sign-on re-authentication are gone.").

## Deviations and rulings

- `GET /auth/me` session type is `AuthSession` (plan: `SessionInfo`, which already names the
  session-list row).
- Native bare codes may carry `:<seconds>` (`TOO_MANY_ATTEMPTS:120`) so the desktop keeps
  `Retry-After`. `HttpError` gains `details` (extra error-body fields) and `retryAfter`.
- New helper module `lib/tauri/preSession.ts` (pre-session request routing, browser token
  adoption).
- Rust attempt and pre-session logic lives in `session/attempts.rs` and
  `session/pre_session.rs` (impl blocks on `SessionManager`); commands stay thin. The named
  pre-session secrets use new `TokenStore::{load,save,clear}_secret` methods (keyring entries
  `account_request:<base>` + `account_request_server`).
- `pre_session_request` also checks the HTTP method and refuses any path with a query.
- `completeSignIn(serverUrl, result)` is the one session start in the auth store; `login()`
  returns the `LoginStep` and keeps the typed password only until `completeSignIn` (C4
  temporary-password reuse kept).
- Polls are never more often than once per second; a client-side timer ends an attempt at its
  `expires_at` (capped below the `setTimeout` overflow).
- Probation banner and prompt banner are fixed overlays (the app shell owns the full height).
- `UserPicker` lists the user's friends: the server has no general user search route.
- CodeEntry adds a large on-screen keypad next to the numeric field.
- Notice texts: one sentence per event type with `{{subject}}` ("your account" / "Oma's
  account") and a "(N times)" wrapper; push texts are short headlines.
- Security OS notifications bypass the per-conversation settings (C28 push rule).
- Admin "immediate actions hidden for another admin" reads an optional `is_admin` on the admin
  user view (not in C27's field list; otherwise the server's 403 `ADMIN_TARGET_IS_ADMIN` is
  shown).
- Register's SSO sign-up applies the `LoginStep` and sends the user to `/login` for the
  confirmation.
- Removed: password reset views and routes, the QR-login modal, backup-code UI, the `qr` i18n
  area, `lib/passwords.ts`, `ChangePasswordModal` (C24-era current-password change).

## Assumptions for the server owners

- `oidc-callback` postMessage carries `{type, status, attempt}`; an older token-style message is
  ignored.
- The request OIDC popup posts `oidc-request-callback {oidc_code}` (C27).
- `/auth/mfa/disable` still accepts the code body next to the `totp.remove` grant.
- Admin user detail (`GET /api/admin/users/{id}`) carries the C27 fields as optional
  `UserDetailsResponse` members.

## Gates (this branch, before the rebase)

- `bun run test:run`: all files pass; the one unhandled `EnvironmentTeardownError` from
  `storedServerUrl.test.ts` is pre-existing on main.
- `bunx tsc --noEmit`, `bun run i18n:check`, `bun run lint` (0 errors), `bun run
  lint:tauri-commands`: clean.
- `cargo test -p vc-client --lib`: pass. `cargo fmt --check`: clean.
  `SQLX_OFFLINE=true cargo clippy -p vc-client -- -D warnings` fails only in files this branch
  does not touch (`voice/video_decoder.rs`, `voice/connection_stats.rs`, `commands/crypto.rs`,
  `crypto/*`: lints of the current toolchain); no finding in the touched files.
- Not run: Android (section B), the rebase onto L1a → LP → L1b, the A14 browser/desktop checks.

## Final review (internal, before the PR reviews)

One fresh reviewer (Sonnet) on the whole branch: 0 Critical, 8 Important (all fixed in
`7539c48b`, each with a test that failed first), 10 Minor deferred:

- `withStepUp`: a second request replaces a pending one without settling it.
- `forgetStepPassword` is never called for an abandoned password step.
- `refresh_session` returning `None` can win against `auth:session_expired {reason: "reused"}`.
- Attempt expiry uses the PC clock without a grace period.
- `navigator.locks.request` rejections are not caught.
- `adopt_tokens` awaits the old session's `/auth/logout` (up to the 30 s client timeout).
- Request OIDC popup: no close grace; the provider URL is not checked for `https:`.
- `adoptBrowserAuth` keeps the token when `/auth/me` fails.
- An in-flight `refreshMeSecurity` can write after `resetMeSecurity`.
- Sentry `beforeSend` strips queries but not fragments (`/setup#…` in the desktop webview).

Native error strings are now `CODE`, `CODE:<secs>` or `CODE:<secs?>:{json}` (extra body fields),
also for `session_request`; `Server error (NNN)` maps to that status.

## Size

~12.9k insertions incl. ~3.5k test lines (estimate was ~7.7k), Rust ~1.7k.

## Review round 1 (2026-11-03, Codex + Argon)

Reviews: `kaiku-reviews-private/goals/work/codereview-l2a-{codex,argon}.md`. Every finding was
checked against the code first; fixes are test-first (each new test failed before its fix).

| Finding | Outcome |
|---|---|
| Codex M1: part-2 and step-up confirmations render behind the open-request prompt (z 70 under 80) | fixed `15f4d8a4`: `lib/a11y/modalLayer.ts` (`createModalLayer`), used by `DialogShell` and `OpenRequestModal`; a modal opens above every open one. Checked by z-index in jsdom, not in a real browser |
| Codex M2: a superseded login installs its session and stops the newer flow | fixed `3d586522`: login runs are objects with a fixed server (`applyLoginStep(step, from, serverUrl?)`); browser wrappers take `stillWanted` and check it before and after `/auth/me`, loaded from the collect's own server; Rust `adopt_tokens_if` checks the attempt under the token lock and ends an unwanted pair (`LOGIN_ATTEMPT_ENDED {state: cancelled}`); `cancel_attempt` takes the verifier before the request; the store ends a session that slipped through (`logout`) |
| Codex M3: the previous account's overview lands under the next account | fixed `15f4d8a4`: `stores/security.ts` generation; a reset detaches the flight, drops its answer, an old `finally` cannot clear a newer flight (also the deferred minor "in-flight `refreshMeSecurity` after `resetMeSecurity`") |
| Codex m4: expired desktop verifiers kept until the next attempt call | fixed `3d586522`: each verifier has its own 10-min timer (`expire_later`, seq-checked); the UI cancels an attempt that runs out on the PC's clock |
| Argon M1: collect drops the verifier before checking the outcome | fixed `3d586522`: only success (adopted or ended), 404 and 410 drop it; the store retries a transient collect failure within the 60-s window and goes back to polling on `LOGIN_ATTEMPT_NOT_READY` |
| Argon M2: `oidc_reauth` / `oidc_reauth_cancel` still registered | fixed `f3dc85ca`: both commands and their loopback listener removed; `commands/reauth.rs` keeps only `Attempt` / `registry` / `run_cancellable`, which LP's `passkey.rs` reuses (LP plan Task 11; `allow(dead_code)` until then). `capabilities/default.json` lists no app commands, so nothing to remove there. Desktop `startOidcReauth` now refuses with `REAUTH_UNAVAILABLE` without an invoke; the C24 browser path in `ReauthDialog` / `lib/reauth.ts` still goes in the LP rebase (A5) |
| Argon M3: stale pre-session secrets | fixed `f087474b`, narrowed: 401/404/410 forget the secret only on the resource route (`GET`/`DELETE …/current`, `GET /auth/registration`). Not on any path as proposed: a wrong code on `…/totp/verify` is 401 `INVALID_MFA` and a 404 on `…/enrollments/{id}` is about the enrolment, so wiping there would strand a live request. The ticket also goes after `totp/verify` or an enrolment answer with `state: done`; the keyring probe runs before the single-use OIDC verifier is taken; a new request on another server clears the old server's secret |
| Argon M4: register maps 409 to an English sentence | fixed `f087474b` (pre-existing from main): 409 passes through `code_error`; `USER_EXISTS` / `EMAIL_TAKEN` map to 409 in `NATIVE_CODE_STATUS`; the webview's legacy string mapping is gone |
| Argon M5: admin panel lacks TOTP state, recovery contact; `may_approve` on admin targets; `meId` unused | fixed `0e3b5a42`: TOTP "Mark as unsafe" / "Allow again" (`admin.factor.set_state`), recovery-contact picker for non-admin accounts (`admin.recovery_contact.set`), `isAdminTarget` = another admin (own account keeps immediate actions), `may_approve` toggle disabled for another admin |
| Argon M6: create-account contact list offers admins and the system account | fixed `0e3b5a42`: `components/admin/recoveryContacts.ts`; `UserSummary` gains optional `is_system` (C28) and `is_admin` (not in C27; same deviation as the detail view); the creating admin is left out via `meId`. The server's 400 `RECOVERY_CONTACT_INVALID` stays the boundary |
| Argon M7: probation banner only refreshes on mount; full-session dialog offers passkeys without one | fixed `15f4d8a4`: `createEffect` refreshes whenever the session turns chat-only and reopens the banner for a new chat-only session; the dialog shows "Sign in with a passkey" only with `passkeys > 0` and loads the overview when opened without one |
| Argon m8: admin requests ignore `account_request_updated`; 409 `ACCOUNT_REQUEST_COMPLETED` offers nothing | fixed `0e3b5a42`: `noteAccountRequestHint` / `accountRequestHints` (websocket) drive the list; the too-late text offers "Open {{name}}" (user panel, where "Remove device now" is); the user panel reloads the account with a toast |
| Argon m9: raw `a.state`; new views not i18n-guarded | fixed `0e3b5a42`: states through `security.keyDevices.state.*`; `eslint.i18n.mjs` guards the new views, `components/auth` (new files), `auth/login`, `security`, `settings/security`, the two admin panels (no violations); stale `QrLoginModal` / `ChangePasswordModal` entries dropped |
| Argon m10: chip below 44 px; recovery code field not regrouped | fixed `15f4d8a4`: `min-h-11 text-base`; `RecoveryCodeInput` writes the grouped value back like `CodeEntry` |

Rejected: none. Argon M3 was narrowed as described above. Also closed by these fixes: the
deferred minor "`adoptBrowserAuth` keeps the token when `/auth/me` fails" (the new session is now
ended instead). CHANGELOG unchanged: these are fixes inside unreleased L2a features; the A5 line
on removed password/SSO re-authentication still waits for the LP rebase.

Gates after round 1: `bun run test:run` 176 files / 1742 tests passed; `bunx tsc --noEmit` clean;
`bun run i18n:check` passes (1191 web keys, 208 Android strings; duplicate-text warnings only);
`bun run lint` 0 errors (145 warnings); `bun run lint:tauri-commands` 85 commands, all registered;
`cargo test -p vc-client --lib` 268 passed, 2 ignored; `cargo fmt --check` clean;
`SQLX_OFFLINE=true cargo clippy -p vc-client --all-targets`: no finding in the files this round
touches (the known findings in untouched files remain).

## LP rebase (2026-11-03)

Safety ref `l2a-pre-lp-rebase` = 7febc1f5 (the branch before). `git rebase --onto
origin/feat/v1.0-login-lp 7a9e7677`: 24 commits replayed, 16 of them with conflicts, 26 file
conflicts in all; 13 were `CHANGELOG.md` (both sides kept, LP's lines first) and
`locales/de.json` (key-level three-way merge, then `i18n:sync`). LP's review fixes are unchanged:
web adoption after `/auth/me` with `live()` and `endStaleSession`, attempt generations and
already-aborted signals, the debounced autofill restart and `stopConditional` clearing
`serverChangeTimer`, `ServerMismatch`, `ceremony_url`, `bare_code`, the listener's cap and
backlog.

| Replayed commit | File | Resolution |
|---|---|---|
| wire types (A1) | `lib/tauri/common.ts` | L2a's `HttpError(status, message, code, details = {}, retryAfter)` (details without `error`/`message`; LP only reads `details.ready_at`) and `isPreSessionPath`, which already lists `/auth/passkeys/login` (LP's two `AUTH_ENDPOINTS` entries) |
| wire types | `lib/tauri/index.ts` | both export lists |
| desktop Rust (A2) | `commands/mod.rs` | both module lists |
| desktop Rust | `session/mod.rs` | L2a's move of `login`/`register`/`exchange_oidc_code` to `session/attempts.rs`; LP's `passkey_flow_post`, `start_passkey_flow`, `redeem_passkey_flow`, `identity_generation`, `current_user_id`, `ServerMismatch` kept |
| sign-in (A3) | `stores/auth.ts` | **hard:** LP's `loginWithPasskey` and generations kept beside L2a's step `login` and `completeSignIn`; `invalidatePasskeySignIns` in `login`, `completeSignIn` and `logout` (LP had it in `login`, `completeNativeOidcLogin`, `loginWithOidc`, which L2a removed); `mfaRequired` dropped |
| sign-in | `views/Login.tsx` | **hard:** LP's autofill, button and desktop box moved into L2a's `form` step, `PasskeyButton` first, LP's divider dropped (the phone button follows); autofill stops when a step replaces the form and starts again when it comes back (new; LP stopped it only for the password submit) |
| sign-in | `lib/authErrors.ts` | both (`PASSKEY_ERRORS`, `formatPasskeyError`, `loginErrorText`) |
| step-up (A5 part) | `i18n/en.ts` | both areas |
| settings (A6, A7) | `SecuritySettings.tsx` | LP's `PasskeysSection` + `PasskeyBrowserSetting` after the key devices, before the password |
| fixes (round 0) | `views/Login.tsx` | both imports |
| round 1 M2 | `session/mod.rs` | **hard:** LP's `adopt_tokens_for` (account check) and L2a's `adopt_tokens_if` (`wanted()` checks) share `adopt_tokens_checked`; `adopt_tokens` → `adopt_tokens_for(None)` |
| round 1 (Argon M2) | `lib.rs` | LP's `passkey_*` commands kept, `oidc_reauth*` removed; `reauth.rs` loses its `dead_code` allow (LP uses the registry) |

After the replay (`e0313e00`): LP's `passkey.rs::code_of` matched `SessionError::AccountBanned`,
which L2a's bare codes removed (one line in an LP file; a banned account stays
`ACCOUNT_BANNED` through `Server(403, code)`), and LP's login-page tests drove the C24
password sign-in. The replayed commits between A2 and `e0313e00` do not build the Rust crate.

### Seams replaced

| Seam | Now |
|---|---|
| `lib/passkeySeam.ts` | deleted. `replaceSessionWithPasskey` runs LP's attempt logic (`runPasskeySignIn`, shared with `loginWithPasskey`); `withStepUp` → `stepUpWithPasskey`; setup links, requests, tickets → `createPasskeyCredential` (new in `lib/tauri/passkeys.ts`, LP's WebAuthn helpers; `registerPasskey` uses the same `createCredential`); offers depend on `canUsePasskeys()` (desktop or WebAuthn) |
| `Login.tsx` | `PasskeyButton` first; QR "Use a passkey instead" leaves the attempt and runs the button's ceremony |
| `LinuxPasskeyHint` | extracted with LP's key `passkeys.login.linuxHint` from **Login.tsx's** desktop box (LP has no hint in `PasskeyButton`); shown in the login box, `StepUpDialog`, `FullSessionRequiredDialog` and `ProbationBanner` while the desktop app waits |
| `SecuritySettings` | LP's `PasskeysSection` mounted (rebase); its empty state gets L2's first steps |
| `SecurityPromptBanner` `add_passkey` | still opens Security settings: LP's section has no direct "Add" and WebAuthn needs the click there |
| `oidc_authorize`, `request_oidc_factor` | `commands/oidc_delivery.rs`: `loopback_post::bind()` first, start with `loopback_port` + `callback_path` + `client_challenge`, `open_browser_with(app, url, settings.passkey_browser)`, `wait_for_post` (5 min); sign-in → exchange → `LoginStep`, request factor → `{oidc_code}` with the verifier kept; the GET sign-in hand-back is gone (`LoginCallback` lives on only as a test parser in `oidc_loopback`) |

`grep -rn "TODO(LP rebase" client/` is empty.

### A5 done

- `ReauthDialog` and its test deleted; `ElevateDialog` → `withStepUp({action: "admin.elevate"})` →
  LP's `elevateWithStepUp(null)`; on a PC without a passkey the dialog points to the phone.
- C24 paths deleted: `lib/reauth.ts` keeps `reauthErrorText` (NOT_ADMIN, FULL_SESSION_REQUIRED,
  STEP_UP_REQUIRED, LP's passkey texts); `startOidcReauth`, `getReauthMethods`,
  `adminElevate(password)`, `elevateSession`, `confirmElevation`, `ReauthRequest`,
  `ReauthMethodsResponse`, 26 `admin.reauth.*` keys and LP's `passkeys.reauth.method` removed.
- `LinkedAccountsSection`: `identity.link` step-up before "Link now" (the popup still opens in
  that click; ELEVATION_REQUIRED comes before the grant is spent on the server), `identity.unlink`
  for that identity; STEP_UP_REQUIRED asks to confirm again.
- LP's `StepUpAction` / `StepUpGrant` (already the full 22 actions in LP, not narrow) come from
  `lib/types/login.ts`; `stepUpActions.test.ts` compares `STEP_UP_ACTIONS` with the server's
  `StepUpAction::ALL` read from `server/src/auth/step_up.rs`. NOT_ADMIN → "Only a server admin
  can do this." in `StepUpDialog` and `reauthErrorText`.
- Empty passkey list: "Set up a phone" (full session) and "Secure my account"
  (`PasskeyFirstSteps`, passed as `PasskeysSection`'s `empty` slot).
- Tests from Step 1: elevation posts only the reason after the step-up, no password/SSO
  re-auth, the empty state, the Linux hint (step-up dialog and login), plus NOT_ADMIN, the
  link/unlink grants and unmount-cancel. CHANGELOG `### Changed` line added (also one for the
  desktop SSO browser).

### Interfaces that differ from the plan (LP's real ones used)

- `commands::auth::open_browser_with(app, url, PasskeyBrowser)` (plan: `open_browser`);
  `loopback_post::wait_for_post(listener, cb, flow_id, deadline, read_deadline)` without
  `cors_origin` (LP shipped the form POST, no `fetch()` fallback).
- The OIDC start's `flow_id`: C29 step 8 does not name the field; L1b's branch answers desktop
  starts with `{url, flow_id}`, which L2a reads. **For L1b:** the request OIDC factor start
  (`POST …/oidc/authorize/{provider_id}` with `{loopback_port, callback_path,
  client_challenge}`) is assumed to answer the same way.

### Left / residuals

- A14 step 1 (rebase onto main after L1a, LP, L1b) and step 3 (browser/desktop checks).
- The desktop OIDC wait cannot be cancelled from the webview (as before; it ends after 5 min).
- LP's `adopt_tokens_for` leaves an `ACCOUNT_MISMATCH` session to lapse on its own (LP
  behaviour, unchanged).
- Task V greps: client hits are test assertions and one comment; the Android hits
  (`qr/redeem`, `mfa_code`, `kaiku://qr/login`) are Section B (L2b).

### Gates (after the LP rebase, `e9b29f38`)

- `bun run test:run`: 184 files / 1763 tests passed. `bunx tsc --noEmit`: clean.
  `bun run i18n:check`: 1215 web keys, 235 Android strings (duplicate-text warnings only).
  `bun run lint`: 0 errors, 140 warnings (none new in the touched files; 145 before the rebase).
  `bun run lint:tauri-commands`: 88 commands, all registered.
- `cargo fmt --check` and `cargo +nightly fmt --check -p vc-client`: clean.
  `SQLX_OFFLINE=true cargo clippy -p vc-client -- -D warnings`: fails only in
  `voice/video_decoder.rs` and `voice/connection_stats.rs` (untouched by LP and L2a; known).
  `cargo test -p vc-client --lib`: 304 passed, 2 ignored. `cargo test -p vc-client` stops at
  `examples/capture_vp8_fixture.rs` (vpx-encode API; unchanged since main, pre-existing).
- `git diff origin/feat/v1.0-login-lp --stat -- server/`: empty.
