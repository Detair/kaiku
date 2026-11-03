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
