# LP implementation notes (v1.0 passkeys, server and clients)

Plan: `docs/superpowers/plans/2026-11-03-v1.0-lp-passkeys.md` (re-aligned to spec rev 5 and C29,
merge order L1a → LP → L1b). Branch `feat/v1.0-login-lp`, stacked on `feat/v1.0-login-l1a`
(draft PR #743). Spec rev 5 and contracts C29 + the C26–C29 common rules win over the plan; every
deviation and judgment call is listed here, per task. "Coordinator" marks items that need a
decision.

## Task 1 — L1a's interfaces (checked against `feat/v1.0-login-l1a` at e7b340e3)

Every row of the plan's "Interfaces LP consumes from L1a" exists with the named signature, with
these differences (the code wins; LP follows it):

- `auth::session_issue`: `issue_session_tx` and `issue_session` exist; `SignInMethod::Passkey`
  yields `full` / `["passkey"]`; `NewSession.authenticator` is `Option<AuthenticatorRef>`, and
  a full session without exactly one authenticator is refused by `issue_session_tx`.
- `auth::step_up`: `StepUpAction::parse` returns `Option` (LP maps `None` to 400
  `VALIDATION_ERROR`); `StepUpTarget::parse(action, raw)` returns `AuthResult`. `grant_tx`
  refuses a grant whose producing authenticator is not active, `peek_tx` does not consume, and
  `consume_tx` re-checks the authenticator through `strong::is_active`.
- `auth::strong`: `AuthenticatorKind::Passkey` exists; `is_active`, `active_strong_count` and
  `authenticator_label` have the passkey arms to fill.
- `GET /auth/reauth/methods`: L1a computes `device` as `strong::active_strong_count(..) > 0`.
  Once LP adds passkeys to that count, `device` would turn true for a passkey-only account.
  **Ruling:** LP computes `device` from active key devices only and `passkey` from active
  passkeys (`reauth.rs`, the file the plan lets LP extend).
- `approvals::cascade::on_authenticator_revoked(conn, ctx, user_id, AuthenticatorRef)`,
  `approvals::transition::lock_accounts(conn, &[id])`, `auth::revocation::end_sessions_tx`,
  `security_events::{record, spawn_delivery, NewSecurityEvent, EventType, Actor}` exist as named.
  `record` returns `sqlx::Result<Uuid>`; sign-in handlers deliver the ids after commit with
  `spawn_delivery` (as `device_login`).
- `approvals::secrets::SecretPurpose` is append-only; LP appends `PasskeyHandoffCode`.
- `device_stepup` (`POST /api/approvals/step-up`) accepts every registry action, `passkey.add`
  included, and already labels `passkey.remove` targets through `strong::authenticator_label`.
- `auth::login::is_desktop_client` stays private: **Ruling:** LP uses
  `ClientClass::from_headers(..) == ClientClass::Desktop` (the same header test) instead of
  widening an L1a/OIDC file.
- Test helpers: `helpers::login::{TEST_ORIGIN, login_config, login_app}`,
  `create_session_with_token` / `create_probation_session_with_token`, `KeyDeviceFixture::insert`
  exist. Full test sessions carry a fake key-device authenticator (`test_authenticator_id`, no
  `approval_devices` row); grants record the passkey that produced them, so this does not matter
  for LP.
- `SQLX_OFFLINE=true cargo check -p vc-server --tests` on the branch: PASS.

## Spike A — crate decision

- `webauthn-rs 0.6.2` was published 2026-10-31. The plan's rule: 0.6.2 only if it is at least
  14 days old (on or after 2026-11-14) when LP merges; otherwise pin `=0.5.5`. Today is
  2026-11-03 (3 days). **Decision (coordinator instruction, plan rule):** pin `webauthn-rs =0.5.5`
  (features `conditional-ui`, `danger-allow-state-serialisation`, `danger-credential-internals`)
  and the dev-dependency `webauthn-authenticator-rs =0.5.5` (`softpasskey`). The API LP uses is
  the same in both versions (`start_passkey_registration`, `finish_passkey_registration`,
  `start_discoverable_authentication`, `identify_discoverable_authentication`,
  `finish_discoverable_authentication`, `start_passkey_authentication`,
  `finish_passkey_authentication`). Moving to 0.6.2 after 2026-11-14 is a version bump plus a
  re-run of the profile tests (`passkeys_profile.rs`), not a code change, unless 0.6.2 changes
  the soft-passkey test helper (it does: 0.6.2's soft passkey sets BE/BS flags).
- 0.5.5 verifies through `openssl` 0.10 / `openssl-sys` 0.9 (Apache-2.0 / MIT), already in the
  server tree through `native-tls`; the server image needs no change (builder `libssl-dev`,
  runtime `libssl3`).
- 0.5.5 behaviour the tests pin (same as the spike's 0.6.2 results): Android origins are compared
  by exact URL equality; UV=0 → `UserNotVerified`; the discoverable path refuses a BE change
  (`allow_backup_eligible_upgrade = false`) and the allow-list path accepts BE false→true, so LP
  compares BE itself on both paths; a counter regression → `CredentialPossibleCompromise`
  (no switch); registration defaults to `residentKey: discouraged`, which LP patches to
  `required`; `start_discoverable_authentication` forces `mediation: conditional`, which LP
  clears.
- **Exact origin strings:** the crate compares parsed URLs, which normalises the scheme's case.
  LP additionally checks the raw `clientDataJSON.origin` string against the allow-list (web
  origin, then each Android origin) before calling the crate, so only exact strings pass.

## Spike B — loopback delivery (research, 2026-11-03; device checks D4/D6 stay with the maintainer)

No browser was driven from a public `https` origin here (no test host and no Windows/macOS
machines in this session); the matrix below comes from specs, browser source and bug trackers.
Steps B1/B2 (the harness on a public `https` test subdomain and the browser matrix) remain for
the maintainer and are folded into D4/D6. "[D]" = documented or cited, "[I]" = inferred.

| Browser (stable, late 2026) | Form POST, auto-submitted | "Continue" without JS | `fetch()` + preflight |
|---|---|---|---|
| Chrome / Edge (Win, macOS, Linux) | delivered, no warning, no prompt (high) | same | one-time LNA prompt ("Apps on device"), then delivered (high) |
| Firefox (Win, macOS, Linux) | delivered, no warning, no prompt (medium-high) | same | one-time LNA prompt ("other apps and services on this device") |
| Safari (macOS) | delivered; at most a console warning (medium; unverified on a device) | same | blocked as mixed content, no prompt (high) |

- [D] Chromium's insecure-form interstitial (`InsecureFormNavigationThrottle`) fires only when the
  action URL is not potentially trustworthy; Secure Contexts makes `127.0.0.0/8` trustworthy, and
  Mixed Content §3.2/§7.1 exempt top-level navigations and trustworthy form actions. Edge shares it.
- [D] Chromium LNA (prompt since Chrome 142, loopback split as "Apps on device" since 145) gates
  `fetch()`, subresources and subframes, **not main-frame navigations** (blink-dev intent for 147:
  "Chrome does not currently enforce any LNA restrictions on main frame navigations"; WICG
  local-network-access issue 48: no timeline). A top-level form POST is a main-frame navigation [I].
  The `chrome://flags/#local-network-access-check` "Blocking" setting changes the subresource
  behaviour only [I].
- [D] Firefox: the secure-to-insecure form check returns early for loopback action URLs
  (`HTMLFormElement::DoSecureToInsecureSubmitCheck`); LNA is on for all users since 151, and
  top-level navigations skip it (`network.lna.allow_top_level_navigation`, default true, bug 1988525).
- [D] Safari/WebKit has no blocking mixed-form check in `FormSubmission.cpp`; `fetch()` to
  `http://127.0.0.1` from `https` is blocked as mixed content (WebKit bug 171934, open), so the
  `fetch()` variant cannot work in Safari at all.
- [D] CSP `form-action http://127.0.0.1:P/callback/<path>`: exact path match (no trailing `/`),
  explicit port must equal, the path is ignored after a redirect; all three engines match IP
  literals by string. The listener therefore answers 200 directly, never a redirect.

**Decision (rule B3):** the **form POST ships**. The `fetch()` variant fails the "works
everywhere" bar (prompts in Chrome and Firefox, hard-blocked in Safari), so Task 9 builds the
`form-action` CSP and Task 11 has no preflight answer (`Access-Control-Allow-Private-Network` is
not shipped). The admin guide names what a user may see: on Safari possibly a "not secure form"
notice (and the "warn before connecting over HTTP" setting), and a future Chromium interstitial if
main-frame LNA gating ever lands. **For the maintainer (D4/D6):** a 10-minute hand check of the
hand-off on Safari macOS (default settings, and with the HTTP warning on) and Firefox stable; these
two rows are not confirmed on a real browser.

## Rebase onto L1a review round 1

After Task 4 the branch was rebased onto `origin/feat/v1.0-login-l1a` at `c07f402e` (no
conflicts). The two LP items from L1a's "Review round 1" are done in Task 5: `grant_tx` checks
that the producing authenticator is the caller's (`strong::belongs_to`, Fable m7), and the
`passkey.remove` target label of a `device_stepup` challenge reads only the caller's passkeys
(Fable m1). Tests: `grant_needs_the_callers_own_authenticator`,
`device_step_up_label_is_scoped_to_the_caller`.

## Tasks 2–10 (server, LP-1)

- **Release certificate (Q1, coordinator/maintainer).** No signed release APK was available
  without a download this session needed permission for, so `OFFICIAL_RELEASE_CERT_SHA256` is
  `None`. Consequence: with both Android variables unset the server has no Android origin and
  `assetlinks.json` is `[]` (a startup warning says so); official APKs work once the constant is
  filled in or `PASSKEY_ANDROID_CERT_SHA256` is set. Explicit app ids without any certificate
  refuse startup. `none` in either variable turns Android off; `none` plus an explicit value in
  the other refuses startup.
- `deny.toml`: no change. 0.5.5 does not use `rsa`, so the planned RUSTSEC-2023-0071 note does
  not apply.
- RP-ID change (DC5) revokes **live** passkeys (pending and active): a pending one is bound to the
  old domain as well. The refusal counts live passkeys.
- `queries::activate` (pending → active, clears `ready_at`, marks the first activity) exists for
  L1b's "Finish"; `queries::insert` marks the first activity for an `active` row.
- The step-up handlers live in `passkeys/step_up.rs` (plan: `reauth.rs`, which would clash with
  `auth/reauth.rs` in reviews and logs).
- All LP `AuthError` variants were added in Task 5. `AuthError::status_and_code()` was extracted
  from `into_response` so the hand-off page can carry the bare code. `PASSKEY_PENDING` adds
  `ready_at` next to `error`.
- `AuthPasskey` counts as a v1.0 sign-in category: 429 answers `TOO_MANY_ATTEMPTS` with
  `Retry-After` (common rules), like `AuthFlow`.
- `record_use` runs before the grant/session transaction, so the counter is kept even when the
  grant then fails (session ended).
- `register::{begin, complete}` are `pub` (integration tests, L1b); `complete_recorded` also
  returns the `authenticator_added` event id for delivery after commit.
- Passkey sign-in: the refresh cookie only for web clients, the token only in the body for native
  ones (plan); `issue` re-checks the passkey `active` under the account lock `issue_session_tx`
  takes, so a concurrent removal cannot leave a fresh session behind.
- Removal response: `remaining_credential_ids` lists live credentials (pending too), so
  `signalAllAcceptedCredentials` never hides a pending passkey (B.1: never signalled unknown).
- Desktop: `auth::optional_auth` (new middleware, additive) authenticates a bearer when one is
  sent; `desktop/flows` (step_up) and `desktop/redeem` use it. A request with an invalid bearer is
  refused 401 before the redemption runs, so it does not spend the flow; it grants nothing.
- Desktop page: a stale assertion after a reload fails with `PASSKEY_INVALID` (it is verified
  against the flow's current ceremony and its challenge does not match), not
  `PASSKEY_CEREMONY_INVALID`; the flow ends either way. The ceremony starts on a button press
  ("Continue with passkey"), not on load, because Safari requires a user gesture for WebAuthn.
- Desktop flow updates (`options` and `finish`) are read-modify-write without a Valkey
  transaction; a reload racing a finish can at worst lose the result (the app times out and
  starts again), never create a second one (the ceremony is `GETDEL`).
- Task 9 Step 3 (hand check of `ceremony.js` in Chromium with a virtual authenticator) was not run
  in this session (no GUI browser with a running local server); `node --check` passes. It is part
  of D4.
- Caddy: `caddy adapt` (local `caddy:2` image) shows `/.well-known/*` → `server:8080` as the first
  route, before the web catch-all.

## Task 11 security fix (coordinator finding on ba824df6)

`SessionManager::passkey_flow_post` sent the session's bearer to the caller-supplied
`server_url` for `step_up`. Fixed: with `bearer`, the session snapshot is taken first and its own
`server_url` is the base; a request naming another server is refused with the new
`SessionError::ServerMismatch` (`SESSION_SERVER_MISMATCH`) before anything is sent. Start and
redeem share the path. Test `step_up_never_sends_the_bearer_to_another_server` (the other mock
server receives no request). The web/desktop step-up wrapper passes the session's own server URL
(`getAuthContext().baseUrl`), never a user-editable field (Task 12/13).

## Final review (fresh reviewer, server + desktop + web; before Task 14)

- **I1 fixed:** the browser passkey sign-in cleared the logout's session-restore block (and stored
  the server) before the ceremony; the login page's autofill would have re-enabled restoring a
  session that logged out while the server was unreachable. Both now happen only after a
  completed `login/finish`. Tests `lib/__tests__/passkeySignIn.test.ts`.
- **I2 fixed:** the desktop app opened the server-supplied ceremony `url` unvalidated (the OS
  launcher, or a browser's command line with "Browser for passkey sign-in"). Only the server's own
  ceremony page for the flow is opened now: same origin as the server, `https` (or `http` on a
  loopback host), path `/auth/passkeys/desktop/<flow_id>`, no query, fragment or userinfo; anything
  else answers `PASSKEY_FAILED` and opens nothing. Test `only_the_servers_own_ceremony_url_is_opened`.
  The pre-existing OIDC `open_browser` has the same pattern (not LP code; for L1b).
- **I3 (coordinator):** the desktop sign-in cannot carry the typed user name: C29's
  `desktop/flows` has no `username` field, so on a shared PC the desktop path does not refuse
  another account's passkey the way the web path does (`PASSKEY_USER_MISMATCH`). Proposal: an
  additive `username|null` on the `sign_in` start, checked at `finish`, answered with the hand-off
  `error=PASSKEY_USER_MISMATCH`.
- **I4 (maintainer):** the desktop browser path (`ceremony.js`, the hand-off auto-submit and the
  real listener together) has not run in a real browser here; spike B was research only and Task 9
  Step 3 was not run. Proposal: one headless Chromium run with a CDP virtual authenticator before
  merge, or keep D4/D6 as the gate.
- Minors deferred (reviewer m1–m14): see the final report.
