# L2a (desktop/web) implementation notes — 2026-11-03

Branch `feat/v1.0-login-l2a`, based on `origin/main` 7a9e7677. Plan:
`docs/superpowers/plans/2026-11-03-v1.0-login-clients.md` section A (A1–A14). Spec rev 5 and
contracts C26–C28 win where the plan differs. L1a, LP and L1b are not on main: everything is
built against the frozen contracts with mocked `fetch` / Tauri `invoke` / wiremock.

Commit `96918eb6` ("set up a phone …") carries the line **L2a-1 boundary** (A1–A7). A8–A13
follow it (L2a-2).

## Task status

| Task | Status |
|---|---|
| A1 wire types, API wrappers | done |
| A2 desktop Rust: attempts, pre-session secrets, refresh reasons | done except the OIDC loopback delivery (LP seam) |
| A3 sign-in screen (phone QR, confirmation, TOTP, neutral answers) | done; PasskeyButton mount is an LP seam |
| A4 probation banner, full-session dialog, Web Locks, SESSION_REUSED | done; passkey sign-in through the seam |
| A5 step-up | partly: `withStepUp`, `StepUpDialog`, action table done; the LP-file edits wait for the LP rebase (below) |
| A6 security settings | done (+ follow-up: AccountSettings uses `PasswordSection`, old `ChangePasswordModal` removed) |
| A7 set up a phone, CodeEntry | done |
| A8 `/setup` page | done |
| A9 Kaiku DM, notices, security log | done |
| A10 open-request modal, prompts | done |
| A11 recovery, "Secure my account", request client | done |
| A12 registration ticket, bind step | done |
| A13 admin per-user security, passwordless create, open requests | done |
| A14 gate | partly: CHANGELOG lines added per task; the rebase onto L1a → LP → L1b and the browser/desktop checks cannot run yet |

## LP seams (replace at the LP rebase)

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
