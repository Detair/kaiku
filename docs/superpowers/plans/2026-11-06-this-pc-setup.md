# Set up this PC Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A person with a chat-only PC session gets a passkey on that PC by scanning a QR code with their key phone, confirming with the screen lock and typing the code the phone shows.

**Architecture:** A new attempt object (SM5) in `server/src/approvals/pc_setup/` sits on the existing approvals core and creates an SM2 enrolment of variant `pc_setup` when the right code is entered. Registration has no setup token: on the web it runs on session-authenticated routes; on the desktop it runs through a new purpose of the F1 desktop flow, with the code typed in the system browser, which earns that browser an HttpOnly secret.

**Tech Stack:** Rust (axum, sqlx, fred), Solid.js, Tauri, Kotlin/Compose.

**Spec:** `docs/superpowers/specs/2026-11-06-this-pc-setup-design.md` (revision 4). It amends `docs/superpowers/specs/2026-11-02-phone-key-login-and-simple-app-design.md`. Read the spec's Step 3, Step 5, Locking and SM5 before any server task; they are normative.

## Global Constraints

- One PR. An auth PR: merge needs the maintainer's explicit OK; reviews by Codex, Flash and Fable.
- QR token 256-bit, rotates every 60 s while `waiting`; attempt ≤10 min in `waiting`/`claimed`; challenge TTL 2 min; code 6 digits, 5 min, 3 tries; enrolment 10 min.
- Only keyed hashes of the token, code and browser secret are stored. No secret in a URL query, log or audit row.
- Every attempt transition: account lock → requesting session row → attempt → enrolment, session lock held through commit (spec, Locking 3). Session-ending paths without the account lock never touch attempts.
- No push: every prompt starts with a scan, and PC and phone learn state by polling, at most once a second.
- Text colours per CLAUDE.md's contrast rules; all strings in the i18n files or `strings.xml`.
- `wire` fields are `snake_case`. New `.sqlx/*.json` committed. `CHANGELOG.md` under `[Unreleased]`.
- No private hostnames or IPs in the repository.

## Review Focus

1. Reloading the tab or restarting the desktop app mid-flow: the PC finds its live attempt and continues, instead of stranding it. Test in Task 3 and Task 6.
2. Account already at `MAX_ACTIVE_PASSKEYS`, or the authenticator already registered: the PC shows the existing passkey error and can leave; the phone does not say "Done". Test in Task 3.
3. Phone offline or server unreachable while the code is shown: the code stays on screen and polling resumes. Test in Task 7.
4. A cancelled browser ceremony on the desktop (Firefox on Linux with no key at hand): retrying in the same browser works without a second code. Test in Task 4.
5. Two PCs of one account running attempts at once: each is independent, and a report on one does not touch the other. Test in Task 2.

---

### Task 1: Data model, kind and golden vectors

**Files:**
- Create: `server/migrations/20261126000000_pc_setup.sql`
- Create: `server/src/approvals/pc_setup/mod.rs`
- Modify: `server/src/approvals/mod.rs`, `signing.rs`, `context.rs`, `secrets.rs`, `golden_tests.rs`, `enrollments/mod.rs`, `core.rs`
- Modify: `server/src/security_events/types.rs`
- Test: `server/src/approvals/golden_tests.rs`, `server/tests/integration/pc_setup.rs` (register in `server/tests/integration/main.rs`)
- Test: `mobile/android/app/src/test/resources/contract/` (new vector files)

**Interfaces:**
- Produces: table `pc_setup_attempts (id, user_id, session_id → sessions, state, reason, qr_token_hash, qr_rotates_at, challenge_id, device_id → approval_devices, code_hash, code_expires_at, tries, enrollment_id, created_at, expires_at)`.
- Produces: `Kind::PcSetup = 6`; `CanonicalContext::PcSetup(PcSetupContext { attempt_id, sid_hash, client_description, session_created_at, place })`; `SecretPurpose::{PcSetupToken, PcSetupCode, PcSetupBrowser}`; `EnrollVariant::PcSetup`; `issuer_kind = 'pc_setup'`.
- Produces: `enum AttemptState { Waiting, Claimed, Code, Enrolled, Completed, Refused, Reported, Failed, Cancelled, Expired }`, `EventType::{PcSetupReported, PcSetupFailed}`.

- [ ] **Step 1: Write failing tests:** golden vectors `pc_setup_approve_vector` and `pc_setup_report_vector` (fixed bytes for the B.2 signed format, kind byte 6, `response` = the 32 token bytes); `Kind::from_byte(6) == Some(PcSetup)`; integration `schema_allows_pc_setup_enrolment_without_a_token`, `one_live_attempt_per_session`.
- [ ] **Step 2: Run:** `SQLX_OFFLINE=true cargo test -p vc-server pc_setup` → FAIL, not defined.
- [ ] **Step 3: Implement.** Migration: the table above with a partial unique index on `session_id` for live states; extend `enrollments` CHECKs for `variant` and `issuer_kind`; make `setup_token_hash` nullable with `CHECK (variant = 'pc_setup' OR setup_token_hash IS NOT NULL)`, `CHECK (variant <> 'pc_setup' OR authenticator_type = 'passkey')`. Add `pc_setup_attempt_id` to `approvals`' requester columns following `Requester` in `core.rs`. `SM2 confirm_tx`, the open-count query and `issued_by` get match arms that refuse `pc_setup`; no claim or confirm route can act on such a row.
- [ ] **Step 4: Run** the same tests and the existing enrolment suite (`--test integration enrollment`) → PASS. Regenerate and commit the `.sqlx` query cache.
- [ ] **Step 5: Commit** `feat(auth): add the pc setup attempt model and challenge kind`.

### Task 2: Attempt lifecycle on the PC and the phone

**Files:**
- Create: `server/src/approvals/pc_setup/handlers.rs`, `server/src/approvals/pc_setup/transition.rs`
- Modify: `server/src/approvals/mod.rs` (router), `server/src/approvals/cascade.rs` (enrolment only), sweep caller of `expire_due`
- Modify: `/api/me/security` handler and its type: `can_set_up_this_pc: bool`
- Test: `server/tests/integration/pc_setup.rs`

**Interfaces:**
- PC, all sid-bound (another session → 404): `POST /auth/pc-setups` → 201 `{attempt_id, qr_payload, rotates_at}`, 403 `PC_SETUP_UNAVAILABLE` without an eligible key device or from a full session; `GET /auth/pc-setups/current`; `GET /auth/pc-setups/{id}?wait=` → `{state, reason, qr_payload?, enrollment_id?}`; `DELETE /auth/pc-setups/{id}`.
- Phone: `POST /api/pc-setups/claim {token}` → `ApprovalChallenge` with context, 409 `PC_SETUP_OTHER_ACCOUNT` (no name), or `LoginCodeStale` for a rotated, unknown or dead token. `POST /api/approvals/{id}/release` works as for `login`. `POST /api/pc-setups/{challenge_id}/answer {signature, decision}` → for approve `{attempt_id, code, code_expires_at}`, for report `{attempt_id}`. `POST /api/pc-setups/{challenge_id}/refuse`. `GET /api/pc-setups/{attempt_id}?wait=` → `{state, reason}`, the scanning device only.
- Produces: `pub async fn lock_attempt(tx, attempt_id) -> Result<LockedAttempt, ApprovalError>`: takes account, session row and attempt in the Locking 3 order, and moves a stale attempt to `expired` with a reason. Every transition starts here.
- QR payload: `kaiku://pcsetup/v1?o=<percent-encoded origin>&t=<43-char b64u>`.

- [ ] **Step 1: Write failing tests** in `pc_setup.rs`: `pc_setup_needs_a_live_probation_session`, `start_refused_without_an_eligible_key_device`, `can_set_up_this_pc_follows_may_approve`, `scan_by_other_account_gets_its_own_answer_without_a_name`, `may_approve_off_refuses_scan_and_approval`, `other_session_of_the_account_gets_404_on_every_attempt_route`, `token_rotates_while_waiting_and_pauses_while_claimed`, `challenge_expiry_returns_to_waiting`, `code_is_returned_to_the_device_only` (no PC response body contains it; capture tracing output), `report_needs_uv_and_signature`, `no_needs_no_signature_and_only_ends_the_attempt`, `report_ends_only_the_requesting_session_and_marks_no_credential`, `two_sessions_run_independent_attempts` (Review Focus 5), `session_ended_or_device_revoked_stops_every_transition` (before any sweep), `context_comes_from_the_session_row_not_the_request`, `no_route_returns_a_setup_token_for_pc_setup`.
- [ ] **Step 2: Run:** `SQLX_OFFLINE=true cargo test -p vc-server --test integration pc_setup` → FAIL.
- [ ] **Step 3: Implement** the handlers and SM5's rows up to `code`. Sign with the existing `verify`, then `set_state`; a report's `end_sessions_tx` runs in the answer transaction after the challenge and attempt locks. Report records `PcSetupReported` and delivers after commit via `spawn_delivery`. Add the attempt sweep beside `expire_due`.
- [ ] **Step 4: Run** the tests plus `--test integration approvals_core` → PASS.
- [ ] **Step 5: Commit** `feat(auth): add the pc setup attempt lifecycle`.

### Task 3: Code entry, enrolment and web registration

**Files:**
- Modify: `server/src/approvals/pc_setup/handlers.rs`, `server/src/approvals/pc_setup/transition.rs`, `server/src/auth/repudiate.rs`
- Test: `server/tests/integration/pc_setup.rs`

**Interfaces:**
- Consumes: `lock_attempt`; `register::{begin, complete_recorded_by}`.
- Produces: `pub async fn enter_code(tx, &LockedAttempt, code: &str) -> Result<CodeOutcome, ApprovalError>` where `CodeOutcome = Enrolled { enrollment_id } | Wrong { tries_left } | Failed`, shared with Task 4.
- Produces: `pub async fn complete_enrolment(tx, &LockedAttempt, credential) -> Result<Passkey, ApprovalError>`: re-checks, creates the `active` passkey with `enrollment_id`, sets enrolment `done` and attempt `completed`, records `authenticator_added` with `via: pc_setup` and `requesting_sid`. Shared with Task 4.
- Routes, sid-bound: `POST /auth/pc-setups/{id}/code {code}` → `{state, tries_left?}`; `POST /auth/pc-setups/{id}/passkey/start`; `POST /auth/pc-setups/{id}/passkey/finish {ceremony_id, credential, label|null}`.

- [ ] **Step 1: Write failing tests:** `third_wrong_code_ends_the_attempt` (and records `PcSetupFailed`), `expired_code_refused`, `registration_needs_the_requesting_session` (other session and no session refused), `start_refused_before_the_right_code`, `lost_code_entry_response_recovers_by_poll`, `ending_the_session_or_revoking_the_device_voids_the_enrolment`, `restart_cancels_the_previous_enrolment_and_its_ceremonies`, `restart_racing_finish_has_one_winner`, `that_wasnt_me_revokes_the_passkey_and_ends_the_requesting_session`, `that_wasnt_me_marks_no_credential`, `notice_names_pc_and_phone`, `passkey_limit_at_finish_leaves_a_readable_state` (Review Focus 2), `current_attempt_resumes_after_reload` (Review Focus 1), and lock-order tests `logout_against_report`, `logout_against_finish`, `refresh_reuse_against_code_entry`, `device_removal_against_finish` (concurrent tasks; no deadlock error; a passkey never survives a session end that committed first).
- [ ] **Step 2: Run** `--test integration pc_setup` → FAIL.
- [ ] **Step 3: Implement.** A `pc_setup` enrolment rides `on_sessions_ended` and `on_authenticator_revoked` through its two issuer columns. `repudiate::effects` for `authenticator_added` with `via = pc_setup` adds `end_sessions_tx` for `requesting_sid`. A fresh start while an attempt is live is a restart (spec, Locking 6).
- [ ] **Step 4: Run** the tests plus `--test integration enrollments` and the repudiation suite → PASS.
- [ ] **Step 5: Commit** `feat(auth): enrol a passkey from a pc setup attempt`.

### Task 4: Desktop flow purpose

**Files:**
- Modify: `server/src/auth/passkeys/desktop.rs`, `desktop_page.rs`, `ceremony.rs`, `assets/ceremony.js`, `assets/ceremony.css`
- Test: `server/tests/integration/passkeys_desktop.rs`

**Interfaces:**
- Consumes: `enter_code`, `complete_enrolment`.
- Produces: `FlowPurpose::PcSetup { user_id, session_id, attempt_id }`; request `purpose = "pc_setup"` with `attempt_id`, bearer required (401 otherwise) and sid must match the attempt; `CeremonyKind::PcSetup { user_id, session_id, attempt_id, flow_id }`.
- Produces: `POST /auth/passkeys/desktop/flows/{flow_id}/code {code}` → 200 with `Set-Cookie: __Secure-kaiku_pcsetup=…; HttpOnly; Secure; SameSite=Strict; Path=/auth/passkeys/desktop/flows/{flow_id}`; refused once the flow has entered a code.
- `options` and `finish` for this purpose require the cookie (compare with `ct_eq` over `SecretPurpose::PcSetupBrowser`'s keyed hash in the flow record). `finish` verifies the credential and holds it in the pending result; `redeem` with code, verifier and the starting session's bearer calls `complete_enrolment`. The redeem response carries no tokens.

- [ ] **Step 1: Write failing tests:** `desktop_flow_options_refused_before_the_right_code`, `desktop_flow_needs_the_starting_session`, `leaked_flow_url_cannot_race_after_the_right_code` (no cookie → refused on `options` and `finish`; second code entry refused), `relayed_handoff_from_a_leaked_flow_url_is_impossible`, `desktop_redeem_needs_code_verifier_and_the_starting_session`, `nothing_is_stored_before_redeem` (no `passkeys` row after `finish`), `ceremony_retry_after_cancel_keeps_browser_secret` (Review Focus 4), `wrong_codes_in_the_browser_count_on_the_attempt`, `set_cookie_has_the_exact_attributes`, and a page test that the code never appears in the rendered hand-off.
- [ ] **Step 2: Run** `--test integration passkeys_desktop` → FAIL for the new tests.
- [ ] **Step 3: Implement.** The server page shows a keypad first when the flow's purpose is `pc_setup`, in the existing page's no-framework style under its CSP; existing `sign_in`/`step_up` behaviour is unchanged. Pending result TTL: `PENDING_RESULT_TTL_SECS`, as in F1.
- [ ] **Step 4: Run** the whole `passkeys_` integration group → PASS.
- [ ] **Step 5: Commit** `feat(auth): register a passkey through the desktop flow`.

### Task 5: Web client

**Files:**
- Create: `client/src/lib/tauri/pcSetup.ts`, `client/src/components/auth/SetUpThisPc.tsx`, `client/src/components/auth/__tests__/SetUpThisPc.test.tsx`
- Modify: `client/src/components/auth/ProbationBanner.tsx`, `client/src/components/security/SecurityPromptBanner.tsx`, `client/src/components/settings/security/PasskeyFirstSteps.tsx`, `client/src/i18n/en/auth.ts` and sibling locales, `client/src/lib/types`

**Interfaces:**
- Produces: `startPcSetup()`, `currentPcSetup()`, `pollPcSetup(id, wait)`, `cancelPcSetup(id)`, `enterPcSetupCode(id, code)`, `startPcSetupPasskey(id)`, `finishPcSetupPasskey(id, ceremonyId, credential, label)`.
- Produces: `<SetUpThisPc onClose />`, a dialog using `DialogShell`, `QrCodeImage` and `CodeEntry`.

- [ ] **Step 1: Write failing tests:** button shown only with `can_set_up_this_pc` and probation; with an active passkey "Sign in with a passkey" comes first; hint text "Ask your admin" when not eligible; each PC row of the spec's error table; Start again visible in every state; resumes via `currentPcSetup` on mount; finish offers "Sign in with your passkey", which calls `replaceSessionWithPasskey`; no poll closer than 1 s even if the server answers at once.
- [ ] **Step 2: Run:** `cd client && bun run test:run SetUpThisPc` → FAIL.
- [ ] **Step 3: Implement.** Replace the banner's `addPasskeyHint` fallback with the button or the admin hint; shorten `addPasskeyHint` to the no-eligible-phone case. Copy from the spec's error table, verbatim. Ignore the known `EnvironmentTeardownError` exit if every test passes, and note it.
- [ ] **Step 4: Run** the same, then `bun run test:run` → all tests pass.
- [ ] **Step 5: Commit** `feat(client): add Set up this PC on the web`.

### Task 6: Desktop client

**Files:**
- Modify: `client/src-tauri/src/commands/passkey.rs`, `client/src-tauri/src/session/mod.rs`, `client/src/components/auth/SetUpThisPc.tsx`, `client/src/lib/tauri/passkeys.ts`
- Test: tests in `passkey.rs`, `SetUpThisPc.test.tsx`

**Interfaces:**
- Consumes: `run_flow`.
- Produces: Tauri command `passkey_pc_setup(attempt_id: String) -> Result<(), String>`: binds the loopback listener, starts the `pc_setup` flow with the bearer, opens the browser, redeems. Cancel via the existing `passkey_cancel`.

- [ ] **Step 1: Write failing tests:** Rust `pc_setup_flow_sends_attempt_id_and_bearer`, `pc_setup_redeem_returns_no_session`; client: in Tauri no keypad is rendered and "Type the code in your browser" is shown with `LinuxPasskeyHint`; `currentPcSetup` after an app restart in `code` or `enrolled` state offers Start again, since the verifier died with the app (Review Focus 1).
- [ ] **Step 2: Run:** `cargo test -p vc-client passkey` and `bun run test:run SetUpThisPc` → FAIL.
- [ ] **Step 3: Implement.** Generalise `run_flow`'s redeem handling so `pc_setup` expects an empty result. The verifier stays in Rust memory, as for F1.
- [ ] **Step 4: Run** both → PASS.
- [ ] **Step 5: Commit** `feat(client): add Set up this PC on the desktop`.

### Task 7: Android

**Files:**
- Create: `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/approval/PcSetupApprovalScreen.kt`, `PcSetupApprovalViewModel.kt`
- Modify: `domain/KaikuPayloads.kt`, `ui/scan/ScanViewModel.kt`, `navigation/ScanHandoff.kt`, `ui/KaikuNavGraph.kt`, `data/api/ApprovalApi.kt`, `res/values/strings.xml` and locales
- Test: `KaikuPayloadsTest`, `ScanViewModelTest`, new `PcSetupApprovalViewModelTest`, contract vectors from Task 1

**Interfaces:**
- Consumes: the Phone routes of Task 2; the signing path of `LoginApprovalViewModel`.
- Produces: `ScanPayload.PcSetup(origin, tokenB64u)`, `ScanUi.OpenPcSetup(payload)`.

- [ ] **Step 1: Write failing tests:** payload parse and redacted `toString`; refused without a key device or with `may_approve` off; the signed bytes equal the Task 1 vectors for approve and report; context hash mismatch refuses; the screen uses the spec's exact wording ("Give this computer full access to your account?", "Only continue if you started this on a computer in front of you, and nobody asked you to."); "I didn't start this" asks "End that computer's session?" first; the code appears only after UV; `code_survives_lifecycle_stop`; `network_loss_keeps_the_code_and_resumes_polling` (Review Focus 3); foreign account shows "This computer is signed in with a different account."; polling at most once a second.
- [ ] **Step 2: Run:** `cd mobile/android && ./gradlew :app:testDebugUnitTest --tests '*PcSetup*' --tests '*KaikuPayloads*'` → FAIL.
- [ ] **Step 3: Implement.** The code lives in the ViewModel, not in saved state, so process death loses it (spec, Step 4). Size and contrast follow the existing setup code screen.
- [ ] **Step 4: Run** `./gradlew :app:testDebugUnitTest` → PASS.
- [ ] **Step 5: Commit** `feat(client): approve Set up this PC on Android`.

### Task 8: Spec, contracts, docs and final gates

**Files:**
- Modify: `docs/superpowers/specs/2026-11-02-phone-key-login-and-simple-app-design.md` (revision 6, the spec's "Changes to the login spec", open question 5)
- Modify: `docs/superpowers/plans/2026-10-22-v1-family-contracts.md` (C26, C27, C29 additive)
- Modify: `docs/admin-guide/features/login/user-guide.md`, `admin-guide.md`, `technical-reference.md`, `CHANGELOG.md`
- Test: `docs/` link check where the repo has one

- [ ] **Step 1: Write** the amendments, including the residual risk in B.6 and SM5 with the Locking rules in B.7.
- [ ] **Step 2: Run the gates:** `cargo fmt --check`, `SQLX_OFFLINE=true cargo clippy -- -D warnings`, `cargo test`, `cargo deny check licenses`, `bun run test:run`, `./gradlew :app:testDebugUnitTest`. Expected: all pass, or only failures already known on `main`, listed by name.
- [ ] **Step 3: Device checks for the maintainer, listed in the PR:** one full run on Windows, macOS, Linux with Chromium, Linux with Firefox and a security key, browser only, desktop app, and a server without FCM.
- [ ] **Step 4: Commit** `docs(auth): document Set up this PC` and open the PR for review. Do not merge without the maintainer's OK.
