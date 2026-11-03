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

## Task 14 — Android

- `PasskeyAuth` (sign-in) and `PasskeyStepUp` (LP's `StepUpProvider`) live in `data/auth/`;
  `AuthRepository` gains only `installNativeSignIn` (the single install point), so its
  constructor (used by seven test files) is unchanged. The plan's
  `AuthRepository.loginWithPasskey` is `PasskeyAuth.loginWithPasskey`.
- `StepUpProvider` has L2b's exact signature and package (`data/auth/StepUpProvider.kt`);
  `StepUpGrant` is L2b's model (`domain/model/StepUp.kt`). The key device's provider is an
  optional binding (`@BindsOptionalOf @DeviceStepUp`); when L2 binds it, Settings uses it to add a
  passkey (the first one, DC1), else a passkey step-up when the account has an active passkey.
- String names mirror the web keys (`passkeys_error_removed`, …); the two Android-only texts are
  `passkeys_android_none_on_device` / `passkeys_android_not_set_up_for_app`. `values-de` stays
  empty (the i18n check wants each locale folder empty or complete).
- Credential Manager signals (`SignalUnknownCredentialRequest`,
  `SignalAllAcceptedCredentialIdsRequest`) are sent best effort.
- The Android tests were written together with the code (no failing run observed first).
  `:app:lintDebug` and `:app:assembleDebug` were not run.

## Task 15 — gate results (2026-11-03)

- `cargo test --no-fail-fast -p vc-server --lib --bins --tests` (Postgres and Valkey in local
  containers, lib Valkey tests on `127.0.0.1:6379`): lib 692 passed; `--test integration`
  1154 passed, 11 failed, 33 ignored. The 11 (pending-member/screening family, optional-features
  screening cases, `dm_calls::test_recall_after_decline_succeeds_immediately`,
  `oidc_mock::test_login_flow_does_not_force_a_fresh_login`,
  `session_revocation::test_ban_closes_socket_without_session_rows`) pass on rerun (6 together, the
  other 5 with `--test-threads=1`); none touches passkey code. The passkey suites
  (`passkeys_{profile,schema,step_up,register,login,manage,desktop,assetlinks}`, 122 tests) pass.
- `cargo +nightly fmt --check` clean; `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
  clean; `cargo deny check`: `advisories ok, bans ok, licenses ok, sources ok`; no `query!` macros
  were added, so `.sqlx` is unchanged.
- `cargo test -p vc-client --lib`: 268 passed, 2 ignored. `cargo clippy -p vc-client`: LP's files
  clean; the crate still fails on the pre-existing `voice/` warnings (as on L1a).
- Client: `bun run test:run` 148 files / 1586 tests passed; `bunx tsc --noEmit` clean;
  `bun run i18n:check` ok; `bun run lint` 0 errors (141 warnings, pre-existing kinds);
  `bun run lint:tauri-commands` ok (82 commands).
- Android: `./gradlew :app:testDebugUnitTest` 889 tests, 0 failures.
- Not done here: the Chromium hand check of `ceremony.js` (Task 9 Step 3), the spike B browser
  matrix, D1–D6, the external reviews (Fable, Codex, Gemini Argon), the PR.
- Size: `git diff --stat c07f402e..HEAD` ≈ 13.4k insertions (server ≈ 8.2k, clients ≈ 4.7k), above
  the 7k threshold: the split point is the commit marked "LP-1 boundary" (Tasks 1–10, 28e4ee38).

## Review round 1 (2026-11-03)

Reviews: Codex (REQUEST CHANGES), Gemini Argon client, Argon-a (Android, infra, desktop flow),
Argon-b (server rest), Fable (all APPROVE WITH FIXES). Branch rebased by the coordinator onto L1a
267e4c78 (6fd3ab54); the fixes sit on top. Every finding was checked against the code first.

### C29 addition (coordinator, 2026-11-03)

`POST /auth/passkeys/desktop/flows` with `purpose: "sign_in"` takes an optional
`username: string|null` (trimmed, at most 64 characters, refused for `step_up`). It is kept in
the flow state and compared case-insensitively after verification in `desktop::finish`, as
`login/finish` does. A mismatch deletes the flow and hands back `error=PASSKEY_USER_MISMATCH`, no
session. The desktop app passes the typed name (`passkey_sign_in` → `run_flow` → start body) and,
on redemption, adopts the tokens only when `/auth/me` is the redeemed `user_id`
(`adopt_tokens_for`, `ACCOUNT_MISMATCH` otherwise).

### Signature counter (coordinator decision 2: the spec wins over DC4)

B.1: the counter is a signal, never a lockout. webauthn-rs-core 0.5.5 hardcodes
`require_valid_counter_value: true`, so `rp.rs` hands the crate a working copy of the stored
passkey with counter 0 (`without_counter`, needs `danger-credential-internals`). With a stored
counter of 0 the crate skips only the counter comparison. It still checks the challenge, origin,
RP ID hash, UV, BE and the signature over `authenticatorData`. The copy is never stored. Why the
danger feature is safe here: it only exposes the `Credential` fields to this module; the copy
exists for one verification call; LP compares the received counter with the stored one itself
(`counter_regressed`: stored > 0 and received <= stored) and writes the regression as an audit
signal (`passkey_counter_regressed`, label, stored, received); `record_use` persists
`GREATEST(stored, received)` in one `UPDATE` (no read-modify-write, so the order in which two
uses complete does not matter). Tests: regressed counter signs in / steps up and is audited, a
tampered signature still fails, the stored counter never decreases (reversed completion order).

### Findings

| Finding | Reviewer | Result |
|---|---|---|
| M1 desktop sign-in drops the typed user name | Fable | fixed (C29 addition above), 4c83866e, 549b9afd |
| m1 `user_handle` writes on every read | Fable | fixed: SELECT first, `UPDATE … IS NULL` only when unset (217de2d2) |
| m2 desktop ceremonies not bound to their flow | Fable | fixed: `flow_id` in `CeremonyKind::{SignIn, StepUp}`, web routes refuse flow ceremonies, `finish` needs the flow's current ceremony (4c83866e) |
| m3 bidi/format characters in labels | Fable | fixed: general category Cf refused (217de2d2) |
| m4 loopback error code passed verbatim | Fable | fixed: `[A-Z0-9_]` filter, else `PASSKEY_FAILED` (549b9afd) |
| m5 one 30/min bucket for login page and desktop hand-off | Fable | fixed: ceremony page GET outside the bucket, default 60/60 (217de2d2) |
| m6 no `ready_at` on the desktop hand-off | Fable | deferred to L1b (hand-off page shared with OIDC delivery) |
| m7 Android string-matches provider messages | Fable | fixed: DOM error type only (49d48e51) |
| Condition: one real browser run | Fable | done, see "Browser run" (b18a0259) |
| 1 (Major) stale web ceremony replaces the session | Codex | fixed: attempt generation checked after every await, stale success ends its own session (6a6119b8) |
| 2 (Major) counter regression refuses sign-in | Codex | fixed (decision 2, 1f0b430e) |
| 3 counter can roll back | Codex | fixed: `GREATEST` in one `UPDATE` (1f0b430e) |
| 4 desktop step-up page vague | Codex | fixed: every action has an en/de sentence with an owner-scoped label; an action without one is refused at start (e5eda80b) |
| 5 desktop sign-in drops the user name | Codex | fixed (= Fable M1) |
| M1 desktop user name, redeemed user not checked, conditional name | Argon | fixed: C29 addition, `adopt_tokens_for`, name read when the passkey is picked |
| M2 loopback listener serves connections one by one | Argon | fixed: `JoinSet`, at most 16 at once (549b9afd) |
| M3 desktop flow read-modify-write races | Argon | fixed: Lua compare-and-set for options/finish/cancel (4c83866e) |
| M4 conditional mediation sets the server early, stops after errors | Argon | fixed: server state set after success only, restart on server change, backoff restart (6a6119b8) |
| m1 desktop step-up grant/session not checked | Argon | fixed: same user, unchanged session identity, grant action and target match (549b9afd) |
| m2 `signalUnknown` / `ready_at` missing outside the login page | Argon | fixed: `serverError` signals, `formatPasskeyError` everywhere (6a6119b8) |
| m3 Android checks sign-in state after `loginFinish` | Argon | fixed: before start and again before finish (49d48e51) |
| m4 Android `InvalidStateError` / `NotAllowedError` | Argon | fixed (49d48e51) |
| m5 Android PATCH missing, id not encoded | Argon | fixed (49d48e51) |
| m6 duplicate `Host` / `Content-Type` accepted | Argon | fixed (549b9afd) |
| m7 `.env.example` Android comment | Argon | fixed (3690fa93) |
| m8 no `isWebAuthnAvailable` guards | Argon | fixed (6a6119b8) |
| M1 pending result TTL (KEEPTTL) and options/finish race | Argon-a | fixed: compare-and-set; the pending result gets exactly 60 s, also near the flow's end (4c83866e, f123eaf9) |
| M2 Android `mapDom` misses `TYPE_SECURITY_ERROR`, `InvalidStateError` | Argon-a | fixed: `mapDomError(e.domError)` (49d48e51) |
| m1 Android passkey sign-in mints a session while signed in | Argon-a | fixed (= Argon m3) |
| m2 Android `ready_at` unformatted, no reload after removal | Argon-a | fixed (49d48e51) |
| m3 ceremony page reload loop, CSP silently dropped | Argon-a | fixed: options end a flow that cannot continue, reload only on 400/404, CSP `expect` (4c83866e) |
| m4 desktop user name | Argon-a | fixed (= Fable M1) |
| M1 `strong::is_active` races revocation | Argon-b | fixed: `FOR SHARE` (217de2d2) |
| M2 `must_change_password` sessions cannot step up for `password.set` | Argon-b | rejected, see below |
| M3 `user_handle` writes on read | Argon-b | fixed (= Fable m1) |
| m1 step-up leaks foreign credential existence | Argon-b | fixed: origin first, revoked rows are unknown; residual below |
| m2 registration checks `credProps` before verification, no UV check | Argon-b | fixed (217de2d2) |
| m3 `activate` ignores the limit; `revoke_all_live` outside the locked users | Argon-b | fixed (217de2d2) |
| m4 `passkey.remove` label for non-active passkeys | Argon-b | fixed (217de2d2) |

Counts: 39 findings, 37 fixed, 1 rejected, 1 deferred.

**Rejected, Argon-b M2.** On this branch `POST /auth/me/password` checks the current password,
not a `password.set` grant (`profile::update_password`), and it runs behind
`require_auth_allow_password_change`. C4 allows only the password change, `/auth/me`, refresh
and logout while `must_change_password` is set; passkey routes are not on that list, so refusing
them is the contract. When `/auth/me/password` moves onto a `password.set` grant (F7, L1b), that
package must decide how a forced change gets its grant; noted for L1b.

### Earlier deferred minors (final review m1–m14)

| Minor | Result |
|---|---|
| m1 admin guide wrong about QR PCs | fixed (passkeys.md) |
| m2 autofill stops after a failure | fixed: backoff restart; also after a failed password sign-in (de92e56a) |
| m3 removing the sign-in passkey signs out unannounced | fixed: the confirmation says so (web en/de, Android) (a397ea23) |
| m4 Safari may refuse `create()` after the step-up | deferred: needs the Safari device check (D3); a second button is the fallback |
| m5 no refresh on 401 for desktop passkey requests | fixed: one refresh and retry; the server checks the bearer before it takes the flow (test `an_expired_bearer_leaves_the_step_up_unspent`) (bbdbd300) |
| m6 cancel can lose to redemption | deferred: the session that results is real, announced by the sign-in notice and listed under sessions; a fix needs a cancel protocol in the shared `run_cancellable` (OIDC uses it too) |
| m7 `Debug` carries the callback path / tokens | fixed: redacting `Debug` for `LoopbackTarget`, `DesktopFlowRequest`, `DesktopSignInRedeemed` (03e9b114) |
| m8 hand-off drops `ready_at`, no unknown signal | deferred to L1b (= Fable m6) |
| m9 vague step-up page | fixed (= Codex 4) |
| m10 browser setting writes stale settings | fixed: writes onto the stored settings, shows the stored choice after a failed save (b13b43d5) |
| m11 Valkey race | fixed (= Argon M3) |
| m12 no release certificate | deferred: maintainer data (Q1); `.env.example` now says it must be set |
| m13 admin guide overstates shared-IP protection | fixed (docs, 60/min default) |
| m14 regenerated Tauri schema churn | fixed again: a build in this round regenerated it (a6cfad1e) |

### Browser run (Fable's condition)

`scripts/passkey-browser-check.sh` (opt-in: `KAIKU_PASSKEY_BROWSER=1`, a throwaway
`DATABASE_URL`, `REDIS_URL`; not in CI) starts vc-server (`PUBLIC_URL=https://localhost:5173`,
fresh Ed25519 JWT keys) and the web dev server, then `client/scripts/passkey-browser-check.mjs`
drives Chromium (Playwright) with a CDP virtual authenticator (CTAP 2.1, internal, resident keys,
UV). Steps: web autofill sign-in with a seeded passkey (fixture like `seed_passkey`: the product
path to a first passkey is a key phone); "Add a passkey" refused while the authenticator holds
the account's passkey (`excludeCredentials` → "This passkey is already added."); "Add a passkey"
(step-up, then a new discoverable credential with the account's user handle); button sign-in
with only the new passkey (the pending autofill request is aborted); desktop sign-in through the
ceremony page, the auto-submitted hand-off from the `https` page to a test listener on
`http://127.0.0.1:P/callback/<path>` (method, path, `Host`, content type checked), and
redemption with the verifier (`/auth/me` is the test user; the typed name was upper case);
desktop step-up (`passkey.remove`, the page names the passkey, the grant matches).

Result (fresh database): all six steps pass with Chrome for Testing 145 and Chromium 156.
Found and fixed while writing it: nothing in the product code. Limits: the page origin is
loopback (dev proxy), so Chrome's rules for a public origin posting to 127.0.0.1 (Local Network
Access) are not exercised; the virtual authenticator answers conditional requests at once, so
"the user picks the autofill entry" is simulated; Playwright accepts the dev certificate. D4/D6
on real devices stay the RC gate.

### Residuals

- Argon-b m1: a foreign *active* credential still answers `PASSKEY_USER_MISMATCH` (an unknown or
  revoked one answers `PASSKEY_UNKNOWN`). Answering "unknown" would make the client tell a
  shared authenticator to drop another person's passkey. The answer reveals existence only, and
  credential ids are random and not guessable.
- Commit 4c83866e carries three interleaved `desktop.rs` fixes (user name, flow-bound
  ceremonies, compare-and-set); one commit, not three.
- The desktop step-up "session replaced meanwhile" check (`identity_generation`) has no test of
  its own; the grant/user checks next to it do.
- Android `minSdk` 26: lint warns that passkeys need API 28 (`PublicKeyCredential`); on Android 8
  the sheet fails with the generic error. Hiding passkeys below API 28 is left for later.
- `cargo clippy --all-targets` reports pedantic test-only lints in LP's integration tests (same
  kinds as the existing suites); the gate (`-p vc-server` without test targets) is clean.

### Gates (review round 1)

- `cargo test --no-fail-fast -p vc-server --lib --bins --tests`: lib 694 passed; integration
  1175 passed, 7 failed, 33 ignored. The 7 (`dm_calls` ×2, `optional_features` ×4,
  `session_revocation::test_ban_closes_socket_without_session_rows`, the known flaky family of
  Task 15) pass on rerun with `--test-threads=1`; the passkey suites (138 tests) pass.
- `cargo +nightly fmt --all -- --check` clean; `SQLX_OFFLINE=true cargo clippy -p vc-server --
  -D warnings` clean; `cargo deny check`: advisories, bans, licenses, sources ok.
- `cargo test -p vc-client --lib`: 276 passed, 2 ignored.
- Client: vitest 149 files / 1598 tests passed; `tsc --noEmit` clean; `i18n:check` ok (874 web
  keys, 235 Android strings); `lint:tauri-commands` ok (82); `bun run lint` 0 errors.
- Android: `:app:testDebugUnitTest` 893 tests, 0 failures; `lintDebug` passes (the
  `UnusedResources` error is gone).
- Browser run: passed (see above).

## Review round 2 (2026-11-03)

Confirmation reviews of `6fd3ab54..eefe584a`: Codex REQUEST CHANGES, Fable and Argon A/B APPROVE
WITH FIXES, Argon C APPROVE; no blockers. Every item below has a test that failed before its fix.

| Finding | Reviewer | Commit | Test |
|---|---|---|---|
| Non-admins could start `admin.*` step-ups and read foreign authenticator labels or user names (desktop page, key-device `target_label`) | Codex 2.1, Fable R1 | 6c8a4444 | `passkeys_step_up::admin_step_ups_need_an_admin`, `passkeys_step_up::device_admin_step_up_needs_an_admin`, `passkeys_desktop::admin_step_up_pages_are_for_admins_only` |
| Desktop `finish`: a CAS loser posts an error to the shared listener; a foreign or old ceremony id is taken before the check | Codex 2.2, Argon A R1 | 02837bbf | `passkeys_desktop::a_finish_that_loses_the_race_hands_back_nothing`, `passkeys_desktop::a_foreign_ceremony_id_consumes_nothing` |
| Web passkey sign-in: the attempt signal aborts an in-flight finish (no `endStaleSession`); state installed before `/auth/me` | Codex 1.1, Argon B R1 | c69a423f | `lib/__tests__/passkeySignInRound2.test.ts` (invalidated during finish, during `/auth/me`, current attempt) |
| An already aborted signal still starts a sign-in | Codex 2.3 | 82bc72fc | `stores/__tests__/authPasskey.test.ts` |
| Autofill restart on server change: no debounce, overlapping starts across the availability check, backoff never reset | Argon B R2 | 215694af | `Login.passkey.test.tsx`: "a server change restarts autofill once, after a pause, with a fresh backoff", "an autofill start waiting for availability never runs beside the button's ceremony" |
| Loopback listener answers connection 17 with 204 at the cap | Argon B R3 | f8b81e61 | `loopback_post::connections_over_the_cap_wait_for_a_slot` |
| Dead `PasskeyFailure::CounterRegressed` with a "refused" doc | Fable (cosmetic) | 256367fc | `rp::tests::a_crate_counter_refusal_is_plain_invalid` |

Counts: 7 fixed, 0 rejected, 0 deferred. Fable's side note (a user-name mismatch records the valid
assertion's use before `fail()`) is accepted as harmless; no change.

Notes on the fixes:

- **Admin gate.** `StepUpAction::is_admin` and `guards::require_admin_for_step_up` (new
  `AuthError::NotAdmin`, 403 `NOT_ADMIN` with the admin middleware's message): for an `admin.*`
  action the caller must be a system admin (`NOT_ADMIN` first, then `FULL_SESSION_REQUIRED`, as on
  `/api/admin/*`). `check_start` runs it before anything else, so the web step-up start and the
  desktop flow start both refuse; the desktop page checks admin again before it resolves a foreign
  label. **L1a code changed here** (L1a is not merged): `approvals/handlers.rs::step_up` runs the
  same gate before `target_label`, which covers `admin.authenticator.remove`
  (`strong::authenticator_label`) and the `admin.*` user-name lookups. L1a's `approvals_core`
  tests and LP tests that used `admin.elevate` as a convenient no-target action now make their
  user an admin (fixtures only).
- **Desktop finish.** Only the flow's current ceremony is taken; anything else (old tab, other
  flow, web ceremony, malformed id) gets the stale page and consumes nothing. `fail()` posts the
  terminal error only when its compare-and-set deletion succeeds; when the flow changed meanwhile
  it returns the stale page. A finish that loses the final compare-and-set returns the stale
  page. The remaining `fail()` callers are the flow's own ceremony failing verification or not
  being bound to the flow.
- **Web adoption.** `/auth/passkeys/login/finish` no longer gets the attempt's signal;
  `GET /auth/me` goes directly to `serverUrl` with the new token (`credentials: "omit"`); only
  after it and a final `live()` check are the token, `browserState.serverUrl`, the stored server
  and the restore block changed. A stale attempt or a failed `/auth/me` ends the minted session.
- **Residual (web, needs a server change):** a stale finish that lands after a newer sign-in sets
  its refresh cookie over the newer one's; `endStaleSession` revokes the stale session, so the
  newer session cannot refresh and asks for a new sign-in when its access token expires
  (≤ 15 min). Only with two sign-ins racing in one browser.
- **Loopback.** The accept branch is disabled at 16 open connections; extra connections wait in
  the kernel backlog until an idle one's read deadline (2 s) frees a slot.

Browser run after round 2 (`scripts/passkey-browser-check.sh`, fresh database): all six steps
pass with Chrome for Testing 145 and Chromium 156. Its listener is the Node test listener; the
Rust listener change is covered by its unit test.

Gates (review round 2):

- `cargo test --no-fail-fast -p vc-server --lib --bins --tests`: lib 695 passed; integration
  1183 passed, 4 failed, 33 ignored. The 4 (`optional_features` pending member, `screening_http`,
  `session_end::sockets_close_through_the_listener`, `unread::test_viewable_filter_…`, the known
  screening/socket flakes) pass on rerun with `--test-threads=1`. The passkey, approvals and
  step-up suites (170 tests) pass.
- `cargo +nightly fmt --all -- --check` clean; `SQLX_OFFLINE=true cargo clippy -p vc-server --
  -D warnings` clean; `cargo deny check`: advisories, bans, licenses, sources ok.
- `cargo test -p vc-client --lib`: 277 passed, 2 ignored.
- Client: vitest 151 files / 1604 tests passed; `tsc --noEmit` clean; `i18n:check` ok;
  `lint:tauri-commands` ok (82); `bun run lint` 0 errors. Android not touched.

## Review round 3 (2026-11-03)

Verdicts: Argon APPROVE, Fable APPROVE (2 nits), Codex APPROVE WITH FIXES (2 items). All three
accept the refresh-cookie residual for v1.0.

| Finding | Reviewer | Commit | Test |
|---|---|---|---|
| A Valkey error in `fail()` or the final result CAS still posts a terminal loopback callback | Codex | 0d13a583 | `passkeys_desktop::a_valkey_error_at_the_result_hands_back_nothing`, `passkeys_desktop::a_valkey_error_when_ending_a_flow_hands_back_nothing` |
| Key-device step-up: `KEY_DEVICE_REQUIRED` before `NOT_ADMIN` | Codex | e03bdb27 | `passkeys_step_up::device_admin_step_up_needs_an_admin` (non-admin without a key device) |
| A pending server-change timer can start autofill during a password sign-in | Fable | 47cde0ea | `Login.passkey.test.tsx`: "a password sign-in cancels a pending autofill restart" |
| `is_admin` must cover every `admin.*` action | Fable | e03bdb27 | `auth::step_up::tests::every_admin_action_is_gated` (fails when an arm is removed) |

On a Valkey error the finish now shows `unavailable_page` (503, en/de: Kaiku is not reachable,
go back to the app and try again) with no hand-off form; the app keeps waiting until
`expires_at` or a cancel. The tests make the compare-and-set fail with a real Valkey error: the
finish is held at the passkey row lock (as in round 2) while the flow key is replaced by a list,
so the script's `GET` fails with `WRONGTYPE`. The hand-off page template is unchanged, so the
browser run was not repeated.

Gates: server lib 696 passed; passkey, approvals and step-up integration suites 173 passed; fmt
and `clippy -p vc-server` clean; vitest 151 files / 1605 tests; `tsc` clean; lint 0 errors.

## Merge of L1a review round 3 (2026-11-03)

`origin/feat/v1.0-login-l1a` at `690df404` (L1a review round 3, Argon coverage,
`267e4c78..690df404`, 23 commits) is merged into LP with `--no-ff` (`1d3913be`); no rebase, no
force push. Safety ref: local branch `lp-pre-l1a-r3-merge` at `203b822c`.

### Conflicts

Four files changed on both sides; one textual conflict:

| File | Resolution |
|---|---|
| `server/src/auth/reauth.rs` (`get_reauth_methods`) | conflict: both sides replaced `active_strong_count` with the same `approval_devices … state = 'active'` query (L1a `62143ccf`, LP Task 5). Kept L1a's comment and LP's `passkey` (`passkeys::queries::active_count`) |
| `server/src/auth/step_up.rs` | auto-merged: L1a's account lock in `lock_full_session` (`9b41b012`) and LP's `strong::belongs_to` check in `grant_tx`, `is_admin` and its test sit side by side |
| `server/src/auth/AGENTS.md` | auto-merged: L1a's grant lock order and LP's passkey section |
| `server/tests/integration/approvals_core.rs` | auto-merged: LP's `make_admin` calls and L1a's `elevation_and_revocation_never_deadlock` (which calls `make_admin` itself) |

Kept intact on the L1a side: the lock orders (`lock_scope` savepoint retries, notice delivery,
refresh rotation), the account lock first in `grant_tx`/`peek_tx`/`consume_tx`, and the
`pending`-claim refusal at `pop-challenge` and `confirm_tx`. On the LP side: the counter as a
signal (`GREATEST`), the CAS desktop flows (stale, loser and 503 pages without a loopback
callback), flow-bound ceremonies, the `admin.*` gate before `key_device_session`
(`6c8a4444`/`e03bdb27`; L1a's part 2 M2 "fixed in LP"), `strong::is_active` `FOR SHARE`,
`user_handle` written only when NULL, and the desktop `sign_in` user-name check. None of these
files changed on the L1a side.

### Adaptations to L1a's API changes

| L1a change | LP code | Result | Commit | Test |
|---|---|---|---|---|
| `grant_tx`/`peek_tx`/`consume_tx` lock the caller's account (`FOR UPDATE` on `users`) before the session row | `passkeys::step_up::grant` (web finish and desktop redeem) wrote its `auth.passkey.step_up` audit row first; its `actor_id` FK holds a key-share lock on the account, so two concurrent grants of one account each waited for the other's (40P01, 500). Compiled, wrong order | `grant` takes `lock_accounts` first | `0309dd40` | `passkeys_step_up::concurrent_passkey_step_ups_never_deadlock` (500 before) |
| same | `register::start` (`peek_tx` alone in its transaction), `register::finish` and `manage::remove` (`lock_accounts` before `consume_tx`), `passkeys::login::issue` (`issue_session_tx` locks the account) | already account first; unchanged | – | existing suites |
| `GET /auth/sessions` fills `authenticator.label` for key devices ("LP adds passkeys") | passkey sessions answered `label: null` | the list also joins the session's own passkey (`COALESCE`) | `c43e3212` | `passkeys_login::session_list_names_the_passkey` (null before) |
| `lock_scope_with`, `events_of_notices(conn, ids, per_notice)`, `revocation::revoke_session` removed, `notify_issuer`, `kaiku_dm_created_tx` refusing the system account, `notices::deliver` error | not used by LP | – | – | – |
| `/auth/reauth/methods` `device` = an active key device | LP had the same query | merged (above) | `1d3913be` | `passkeys_step_up::reauth_methods_report_passkey` |

L1a round 3 rows marked for LP: part 4 M1 "`grant_tx` does not check `by`'s owner" is LP's
`strong::belongs_to` (rebase onto round 1). Part 2 M2 "passkey fallback for
`admin.authenticator.remove`" was still open here; it is done in the next section (`8283f662`).

### Android fixes deferred to LP by the L2b review

- (a) `30551dc6` (Argon p3 m1): `installNativeSignIn` ends the session the server minted for a
  passkey result it does not install (a session already runs, the attempt was superseded by
  another sign-in, or the server changed), on the server that minted it, with its own tokens
  only; best effort, bounded by `SERVER_LOGOUT_TIMEOUT_MS`, `NonCancellable`. The private
  helper is `endUnused`, the same name and body as L2b's device-sign-in helper: when L2b and
  LP meet, keep one. Test: `AuthRepositoryNativeSignInTest` (3 of 4 red before).
- (b) `13f2c733` (Argon p5 #6): `PasskeySignInButton(busy, enabled)`: `busy` is the new
  `LoginUiState.passkeyLoading` (its label), `enabled` is `!isLoading` (any sign-in; L2b passes
  its combined `busy`). `onPasskeySignIn` starts nothing while a sign-in runs, so it never
  cancels a password or SSO sign-in and a double tap sends one ceremony; a password sign-in that
  replaces a passkey job clears its label. Tests in `LoginViewModelPasskeyTest`: "a passkey tap
  while a password sign-in runs starts nothing", "a second passkey tap while one runs starts no
  second ceremony", "a password sign-in that replaces a passkey one clears the passkey label"
  (all red before).

### Gates (merge of L1a review round 3)

- `cargo test -p vc-server --lib`: 702 passed (the Valkey lib tests, `redis_tests` and the voice
  `ws_handler_test`, hard-code `localhost:6379`: 18 failed for that reason only and passed through
  a temporary local forwarder to the L1a test Valkey, stopped afterwards).
- Passkey, approvals, approval devices, step-up grants, enrolment, device sign-in, session
  (assurance, end, revocation, refresh rotation), transition locks, Valkey outage, security
  events and notices, system account, CLI, elevation, guarded routes and re-auth suites:
  369 passed, 0 failed.
- Full `--test integration`: 1213 passed, 1 failed, 33 ignored. The one
  (`unread::test_viewable_filter_matches_require_channel_access`) is the known flaky
  pending-member family and passes 3/3 with `--test-threads=1`.
- `cargo +nightly fmt --all -- --check` clean; `SQLX_OFFLINE=true cargo clippy -p vc-server --
  -D warnings` clean; `cargo deny check`: advisories, bans, licenses, sources ok; `git diff
  --check` clean. No `query!` macro changed (the session list uses `query_as`), so `.sqlx` is
  unchanged.
- Client: vitest 151 files / 1605 tests passed; `tsc --noEmit` clean; `bun run lint` 0 errors
  (141 warnings, as before); `lint:tauri-commands` ok (82). `cargo test -p vc-client --lib`:
  277 passed, 2 ignored.
- Android: `./gradlew lintDebug testDebugUnitTest --no-daemon`: lint 0 errors (47 warnings),
  900 unit tests passed (88 classes).

## Merge of L1a review rounds 4 and 5 (2026-11-03)

Four `--no-ff` merges of `origin/feat/v1.0-login-l1a`, each pushed normally (no rebase, no
force): `8a18d06c` (round 4, `b2391e08`), `ae8b4df4` (round 5, `93baf530`), `9de25567` (notice
sweep cursor, `caa449e2`) and `e09642f7` (the sweep's advisory lock and the
`idx_security_events_undelivered (created_at, id)` change in the unreleased migration
20261120000000, `9ba364bf`).

**Conflicts: none.** Round 4 ports LP's `admin.*` step-up gate (`6c8a4444` + `e03bdb27`) into L1a
(`23450390`) with the same text, so `auth/step_up.rs`, `auth/guards.rs`, `auth/error.rs` and
`approvals/handlers.rs` merged to LP's version. Their remaining differences from L1a are LP's
own: `passkey.remove` labels read only the caller's active passkeys, and `grant_tx` checks
`strong::belongs_to`. L1a also added `device_admin_step_up_needs_an_admin` to
`approvals_core.rs` with LP's body. The merge commit drops LP's copy in `passkeys_step_up.rs`,
so the test exists once. LP's `admin_step_ups_need_an_admin` (the passkey start) stays.

**Attestation and enrolment changes.** `attestation::check` now binds the leaf SPKI to the
claimed key, and `ENROLLMENT_DEVICE_IN_USE` is answered at pop-challenge and confirm (also for
the issuer's own phone). Neither touches LP: passkeys use attestation `none`, and only L1a's
`enrollments.rs` builds chains (`helpers/attestation.rs`). The passkey suites pass unchanged.

**Label fallback (`8283f662`).** `target_label` for `admin.authenticator.remove` falls back from
the key device to the passkey with that id (C27: a key device or passkey id), as the desktop
ceremony page already did. Only system admins reach it, because the `admin.*` gate runs first.
Test: `passkeys_step_up::device_admin_step_up_names_a_passkey_target` (an empty label before;
it also checks a key-device target and an unknown id).

Gates on the final tree (`e09642f7`):

- `cargo test -p vc-server --lib`: 702 passed (the Valkey lib tests run through a temporary
  local forwarder to the L1a test Valkey, stopped afterwards).
- Passkey, approvals, approval devices, step-up grants, enrolment, device sign-in, session
  (assurance, end, revocation, refresh rotation), transition locks, Valkey outage, security
  events and notices, system account, CLI, elevation, guarded routes, re-auth and governance
  suites: 387 passed, 0 failed.
- Full `--test integration`: 1220 passed, 0 failed, 33 ignored.
- `cargo +nightly fmt --all -- --check` clean; `SQLX_OFFLINE=true cargo clippy -p vc-server --
  -D warnings` clean; `cargo deny check`: advisories, bans, licenses, sources ok. No `query!`
  macro changed (`.sqlx` unchanged). Client and Android were not touched in these merges, so
  their gates were not run again.

## Review round 5 (merge delta; Codex, Fable, Gemini Flash; 2026-11-03)

Codex APPROVE, Gemini Flash APPROVE (Argon stand-in until 2026-11-08), Fable APPROVE WITH FIXES (one item, fixed by the coordinator):

| Finding | Verdict | Commit | Test |
|---|---|---|---|
| Fable R1: `manage::rename` took the passkey row before the account (audit row key share); a step-up grant takes the account and then reads the passkey → 40P01, a 500 | fixed: `rename` takes `lock_accounts([auth.id])` first (B.7 rule 1) | see `git log` ("rename takes the account lock first") | `passkeys_step_up::renaming_a_passkey_during_its_step_up_never_deadlocks` (red before: the rename answered 500) |

Fable's other note (Android: a `GetMe` failure or a cancellation while suspended in `installNativeSignIn` leaves the minted session neither installed nor ended) is pre-existing and low (the UI makes it near-unreachable); recorded for the L2b merge.

Gates: `passkeys*` integration suites 148 passed; fmt and `clippy -p vc-server -D warnings` clean.
