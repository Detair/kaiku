# L2b (Android) implementation notes, 2026-11-03

Plan: `docs/superpowers/plans/2026-11-03-v1.0-login-clients.md`, Section B. Spec rev 5 and contracts
C26–C28 are binding; where the plan's interface sketch and the contracts differ, the contracts win.
Branch `feat/v1.0-login-l2b`, first built on `main` (`7a9e7677`) against fakes and mock engines, then
rebased onto LP (`origin/feat/v1.0-login-lp` `203b822c`, which sits on L1a `267e4c78`) on 2026-11-03
(see "LP rebase" below). L1b is still open: its C27 routes are consumed through the B4 models and
mock engines. No server file is changed.

## Status

| Task | Status |
|---|---|
| B1–B9 (L2b-1) | done; the last commit of L2b-1 carries the line "L2b-1 boundary" |
| B10–B13 | done |
| B14 | done after the LP rebase (see "LP rebase") |
| B15 | step 1 done (`KaikuSigGoldenFileTest` on L1a's `golden/kaiku_sig_v1.json`); step 2 gates run on the LP rebase; the rebase on `main` after L1a, LP and L1b merge is still to do. `KaikuSigGoldenTest` keeps the vectors from the contract text. Device checks D-B1…D-B11 are for the maintainer. |

## Deviations from the plan (all small; each also in the SDD ledger)

1. **LP seam** (resolved on the LP rebase). `data/auth/StepUpProvider.kt` is LP's file unchanged
   (interface + `@DeviceStepUp` qualifier); `StepUpFailure` and `DeviceStepUpProvider` moved to
   `data/auth/DeviceStepUpProvider.kt`, bound in `di/ApprovalModule` with LP's qualifier. `StepUpGrant`
   is LP's (same shape as L2b's); L2b's `AnswerResult` stays next to it in `domain/model/StepUp.kt`.
2. **New-device routes take an origin.** `EnrollmentApi.open/popChallenge/claim/status` have an optional
   `origin`: an unconfigured app must reach the setup code's server before the person confirms it, and
   these public routes never carry a bearer (the auth interceptor would otherwise attach the configured
   server's token to any absolute URL).
3. **Lapse is derived.** `KeyDeviceRepository.state` is computed from the stored phase and the current
   session: an `ACTIVE` key device without its session is `Lapsed` at once. `markLapsed()` stays as a
   no-op for E1. `KaikuHttpClient` only records the refresh error code (`SessionEndLog`) so "Confirm it's
   you" can say "A session was ended because its key was used twice".
4. **Sign-outs.** `signOut()` / `signOutAndRemovePhone()` live in `data/auth/KeyDeviceSignOut.kt`; the
   LP-shared `AuthRepository` only gains a setter-injected `KeyDevicePolicy` (a key device keeps its
   push subscription on a plain sign-out), `hasLiveSession()` and `installDeviceSession()`.
5. **Small type additions.** `DeviceSignInResult.KeyInvalidated`; `ApprovalKeyStore.keyUvClass()`;
   `SetupEntry.ticket`; `SetupUi.WaitingForCode/SignInPaused/Done` and `Ended.arg`;
   `IssuerUi.TypeCode.wrong`; `IssuerMode.Admin.displayName/newAccount`; `ApiException.fields`
   (`tries_left`, `ready_at`, `state`, `reason`, `recovery_codes_left`, `Retry-After`).
6. **Kaiku DM flag.** C28 puts `is_system` on `DMParticipant`, not on the DM list item, so
   `DmApiImpl.getDms()` sets `DmConversation.isSystem` from the participants' flags (never from an id
   or a name); the Kaiku DM is recognised by `isSystem && readOnly`.
7. **Extra checks the plan did not list.** The setup PoP context must carry the open's nonce, SHA-256 of
   this phone's SPKI and the subject's username; the issuer's `enroll` context must carry the shown
   label and the expected subject; the login scanner refuses a sign-in code client-side when this phone
   is not an active key.
8. **Passkeys section** (resolved on the LP rebase): Security settings mount LP's `PasskeysSection`
   unchanged; the general Settings screen no longer mounts it a second time.
9. **Admin user detail.** Read from `GET /api/admin/users/{id}/details` with the C27 fields. `is_admin`
   of the target is decoded when present (it is not a listed C27 field); otherwise the server's 403
   `ADMIN_TARGET_IS_ADMIN` is shown.
10. **Script location.** `scripts/android_check_app_links.sh` is in the repository's `scripts/` (next to
    `android_verify_apk.sh`); run it from `mobile/android` as `../../scripts/android_check_app_links.sh`.

## Assumptions to confirm with the server owners (no route changed)

- `POST /auth/enrollments/status` answers `{state, authenticator_state, ready_at}` as C26 says; a void
  or unknown enrolment is a 404/410 (the pending screen then says "Setup cancelled").
- `/api/me/security` `open_request` is set for `waiting` and `ready` requests (the prompt shows only those).
- The device step-up reads the current `sid` from `GET /auth/me` `session.id`.

## Follow-ups

- ~~B14 after the LP rebase; then B15's golden-file test after the L1a rebase.~~ Done on the LP rebase.
- Rebase on `main` once L1a, LP and L1b have merged (B15 step 1, coordinator order); then re-run the
  gates against L1b's real C27 routes and add parse tests for any server goldens L1b adds for the
  B4 types (`login_step`, `login_attempt`, `me_security`).
- ~~Count strings as `<plurals>`~~: done in review round 1 (`issuer_minutes_left`, `issuer_wrong_code`,
  `security_recovery_codes_left`). The C28 notice strings with `%4$d` stay `<string>`: their names and
  argument order are fixed by C28 and `NoticeText` returns one `@StringRes` per event type.

## Review round 1 (2026-11-03)

Reviews: `kaiku-reviews-private/goals/work/codereview-l2b-codex.md` (5 major, 3 minor) and
`codereview-l2b-argon.md` (6 major, 4 minor). Each finding was checked against the code first; every fix
has a test that failed before it (compile-red where a new seam was needed, behaviour-red otherwise),
except where noted.

| Finding | Result |
|---|---|
| Codex M1: device sign-in ignores logout, server change and the running session | **fixed** `3353dc87`: `AuthRepository.beginDeviceSignIn(keyOrigin)` captures the sign-in generation, server and session before the challenge; `installDeviceSession(auth, userId, start)` installs only if all three are unchanged (checked under the install lock, also for the renewal) and ends any session it does not install. Tests `AuthRepositoryDeviceSessionTest` (logout during `GET /auth/me`, sign-out during a renewal, server change). |
| Codex M2: an unclear claim failure deletes a possibly enrolled key | **fixed** `968bddb7`: new phase `CLAIMING` written before the claim; network errors, 5xx, 408/429 and unreadable answers keep key and record; "Try again" asks `KeyDeviceProbe` (device-login challenge) and only then resends the same signed claim. Only a definite 4xx refusal deletes the key. |
| Codex M3: a restored `setup_flow` spins forever | **fixed** `968bddb7`: `SetupFlowViewModel.resumeWithoutEntry()`: a stored setup continues on the pending screen (`SetupUi.Resume`), nothing stored → `SetupUi.Interrupted` with "Scan the setup code" (`onRescan`, nav → `scan_setup`). Tokens stay out of saved state. |
| Codex M4: the first setup screen never resumes a pending wait | **fixed** `968bddb7`: `SetupFlowScreen` switches to `SetupPendingScreen` (resume + 15-min checks) as soon as it waits. Compose only (no Robolectric here); the hand-over data path is covered by the pending tests. |
| Codex M5: a self-registration has no claim secret, so restart says "Setup cancelled" | **fixed** `968bddb7`: `done`+`active` is stored as an active key (`markEnrolled`, no secret needed; a restart opens "Confirm it's you"); waits without a secret are checked through `KeyDeviceProbe` (`KEY_DEVICE_PENDING {ready_at}` / challenge / unknown). |
| Codex m6: admin setup shows now + 24 h | **fixed** `b78384f9`: the ready time comes from the admin user view's `open_request.ready_at` (matched by `request_id`); unknown → "becomes active after the waiting time"; "(24 hours)" dropped from the copy (delay 24–72 h). `IssuerUi.Done(label, waits, readyAt)` replaces the plan's `Done(label, pending)`. |
| Codex m7: "Remove this device" after a too-late cancel is never shown | **fixed** `b78384f9`: `NoticeActionUi.Result(msg, removeDevice)`; the device is the event's authenticator, else the newest log entry of that request naming one; the dialog's button runs the usual step-up removal (`removeCompletedDevice`). |
| Codex m8: setup/issuer secondary buttons are 56 dp | **fixed** `b78384f9`: `KeyScreenSizes.secondaryButton` = 64 dp on all key screens; list rows and notice cards use `otherAction` (56 dp). |
| Argon M1: the challenge check pins to the saved server URL, not the key's origin | **fixed** `3353dc87`: `OriginPin.signingOrigin()` (key origin, else configured server) wired into `ChallengeVerifier`; `beginDeviceSignIn` restores the key's server when the saved one is missing and refuses while a session runs elsewhere. |
| Argon M2: screen-lock removal not mapped to `KeyInvalidated` | **fixed in part** `a3704860`: `uvPreflight` (no secure lock screen → invalidated, before any key lookup or prompt), a missing credential intent and `ERROR_NO_DEVICE_CREDENTIAL` → `KeyInvalidated`; keystore2's `UnrecoverableKeyException` for an invalidated key maps too (`isInvalidatedKeyFailure`, by cause/message; other Keystore failures stay failures). **Rejected:** `ERROR_NO_BIOMETRICS` → `KeyInvalidated` on API < 30: with `setInvalidatedByBiometricEnrollment(false)` the key is bound to the root SID and survives the removal of all fingerprints (it works again once one is added), so forgetting it would destroy a valid key. The Keystore mapping itself is device-only (D-B4). |
| Argon M3: device sign-in does not check `client_class` | **fixed** `3353dc87`: the verified `device_login` context must be `android` before the prompt (`CHALLENGE_CONTEXT_MISMATCH`). |
| Argon M4: remove-phone ends the session before push unregister; revoked/unknown not finished locally | **fixed in part** `5270ffe1`: `KEY_DEVICE_UNKNOWN`/`KEY_DEVICE_REVOKED` on the DELETE now forget the key and sign out. **Rejected:** unregistering push before the removal: plan B6 orders "removeKeyDevice → forget → push unregistered → tokens cleared" and C19 amendment 3 deletes the subscription with the revocation "after the last notice"; unregistering first would leave a key that failed to be removed without its subscription and lose the removal notice. The client DELETE afterwards stays best effort (bounded 3 s). |
| Argon M5: token migration can crash on Keystore errors or re-copy an old refresh token | **fixed** `5270ffe1`: the copy is guarded (false → "sign in again", the app starts); a target that already holds a session is never overwritten, the old file is just deleted. |
| Argon M6: a damaged key-device store crashes (non-GSE) and orphans `kaiku_approve_v1` | **fixed in part** `a3704860`: `PrefsKeyDeviceStore.load()` catches every exception (ProviderException, ClassCastException, bad times) → "no key device". **Rejected:** deleting the key at start when the store reads empty: setup already deletes an orphaned key before generating (`prepareKey`), so re-enrolment is not blocked, and deleting at start would turn a passing Keystore read failure into the permanent loss of a valid key. Clearing the damaged prefs is skipped for the same reason. |
| Argon m1: an installed-refused device session stays live | **fixed** `3353dc87` (with Codex M1): every not-installed device session is ended with its own tokens. |
| Argon m2: `RenewalGate` keeps a stale notification flag | **fixed** `5270ffe1`: `onIntent` assigns, and `MainActivity.onStop` clears a flag recorded while the app was already in front (the case the assignment alone does not cover). |
| Argon m3: unsynchronised `kaiku_probe` alias | **fixed** `a3704860`: the probe runs under one lock. Android-only, no JVM test. |
| Argon m4: count strings are `<string>` | **fixed** `b78384f9`: `<plurals>` + `pluralStringResource` (see Follow-ups). |

Interface changes from this round (small; the plan's shapes stay otherwise): `KeyDevicePhase.CLAIMING`,
`KeyDeviceRepository.saveClaiming/markEnrolled`, `data/keydevice/KeyDeviceProbe.kt` (`ServerKeyState`),
`SetupUi.Resume/Interrupted`, `SetupFlowScreen(…, onRescan)`, `SetupFlowViewModel.retry/resumeWithoutEntry`,
`AuthRepository.beginDeviceSignIn` + `DeviceSignInStart` (LP-shared file, additive),
`OriginPin.signingOrigin`, `RenewalGate.onStopped`, `IssuerUi.Done(label, waits, readyAt)`,
`NoticeActionUi.Result(msg, removeDevice)`, `KeyScreenSizes.otherAction`.

Assumptions added for the server owners: `POST /auth/device-login/challenge` is usable as the status of a
key device without a claim secret (challenge = active, 409 `KEY_DEVICE_PENDING {ready_at}`, 404/403 = none);
the admin user view's `open_request` right after an existing account's setup is the admin request that
setup started.

## LP rebase (2026-11-03)

`git rebase --onto origin/feat/v1.0-login-lp 7a9e7677` (safety ref `l2b-pre-lp-rebase` = `c274fd77`).
LP's branch carries L1a, so the golden file and L1a's server types came along. All 20 L2b commits
replayed; 1141 unit tests passed right after the replay, before any seam work.

**Conflicts (4), all on files both branches added or edited:**

| File | Resolution |
|---|---|
| `domain/model/StepUp.kt` (add/add) | LP's `StepUpGrant` (identical shape) + L2b's `AnswerResult`. |
| `data/auth/StepUpProvider.kt` (add/add) | LP's file verbatim. L2b's `StepUpFailure` and `DeviceStepUpProvider` moved to the new `data/auth/DeviceStepUpProvider.kt`, which imports `approval.DeviceStepUp` under an alias: LP's qualifier annotation has the same simple name in the same package. |
| `res/values/strings.xml` | both blocks kept (LP's passkey strings, then L2b's step-up strings); no duplicate names. |
| `ui/settings/SettingsScreen.kt` | both kept at replay time (LP's `PasskeysSection()` and L2b's "Security" entry); the seam commit then moves the section (below). |

Auto-merged and checked: `AuthRepository.kt` (LP's `canStartSignIn`/`installNativeSignIn` next to
L2b's device-session methods; disjoint hunks), `app/build.gradle.kts` and `LICENSE_COMPLIANCE.md`
(both dependency sets), `CHANGELOG.md`. LP's passkey internals are untouched by L2b: `git diff
origin/feat/v1.0-login-lp` is empty for `PasskeyCredentials.kt` (DOM-type `mapDomError`, the request
building), `PasskeyAuth.kt` (the sign-in guard before start and before finish), `PasskeyApi.kt` (PATCH
rename, encoded ids), `PasskeysSection.kt`/`PasskeysViewModel.kt` (`ready_at` formatting, reload),
`PasskeyLogin.kt`, `PasskeyModule.kt` and LP's tests. In `LoginViewModel`, LP's `onPasskeySignIn` and
`passkeyErrorRes` are unchanged; LP's passkey block on `LoginScreen` too.

**Seams replaced:**

- `StepUpProvider`: L2b's interim declaration is gone (LP's wins). `ApprovalModule` provides
  `@DeviceStepUp StepUpProvider` = `DeviceStepUpProvider(DeviceStepUp, fallback = PasskeyStepUp)`, which
  makes LP's `@BindsOptionalOf` present. Commit `feat(client): bind the key phone's step-up to LP's
  passkey seam`.
- **Deviation (DC1 vs LP's code):** LP's `PasskeysViewModel.addStepUp()` uses the device provider
  whenever it is bound, and the Hilt binding is static, so the provider itself decides: on the key
  phone (active key) it signs a `device_stepup`; on a phone that is not the key, or on 403
  `KEY_DEVICE_REQUIRED`, it falls back to LP's `PasskeyStepUp` (plan B6: "callers fall back to LP's
  Credential Manager step-up"). It returns LP's failure types where LP has words for them (UV cancel →
  `PasskeyFailure.Cancelled`, so no error shows; probation → `PasskeyFailure.Server("FULL_SESSION_REQUIRED")`);
  everything else stays `StepUpFailure(outcome)`. Side effect of LP's `canAdd = deviceStepUp.isPresent
  || …`: "Add a passkey" is now offered on every phone; on a phone without key or passkey the server's
  answer is shown (LP's mapping). Kept as LP's behaviour; LP's VM is not edited.
- Passkeys in Security settings: LP's `PasskeysSection()` mounted unchanged in place of L2b's count;
  `security_passkeys`/`security_passkeys_count` removed (UnusedResources). The CHANGELOG line of B11
  now names passkeys.
- `AuthRepository` (LP-shared): no seam left to replace; B14 below changes `login` there.

**B14 (done, test-first: the tests compiled red first).**

- `LoginAttemptViewModel` + `PhoneSignInScreen` (QR / TOTP / ended), `ProbationBanner` on home,
  `OidcHandler.handleCallback(uri): OidcCallback?` returning only `OidcCallback.Code` (any token
  parameter, a provider `error` or a missing code → null, nothing installed), `AuthApi.login` (with
  `mfa_code`) and `AuthApi.exchangeOidcCode` deleted with `MfaRequiredApiException`/`MfaRequiredException`,
  `AuthRepository.login`/`completeOidcLogin`/`exchangeOidcCode` replaced by
  `completeAttempt(serverUrl, typedPassword, collect)`: the old `login`'s install rules (latest sign-in
  only, same server, refused while signed in, C4 pending change for a password attempt) with the
  attempt's `collect`/`totp` call as the source of the session.
- Login order on `LoginScreen`: passkey (LP), "Sign in with your other phone", username/password, SSO
  (providers are now loaded from `/auth/oidc/providers`; before, the list was never filled), "Set up this
  phone with a setup code" (`scan_setup`), "I lost my phone / forgot my password" (`{server}/recover`)
  and "Secure my account" (`{server}/secure-account`) in a Custom Tab.
- The verifier: `AttemptSecret` (32 random bytes, b64u; `challenge` = S256) per attempt, held only by
  `LoginAttemptViewModel` (scoped to the `login` entry), never in state, saved state, logs or disk;
  leaving and `onCleared` cancel the attempt in the app scope (bounded 5 s). OIDC: the Custom Tab start
  sends `redirect_uri=kaiku://auth/callback&client_challenge=…` (no `state`, no `code_challenge`; the
  code is bound to the challenge); `MainActivity` hands the callback to `PendingOidcCallback` (memory),
  which the sign-in screen that holds the verifier takes; the activity-scoped `LoginViewModel` is gone.
  The PKCE values an older version wrote to disk are cleared at the next SSO start.
- Interface notes against the plan: `LoginAttemptViewModel` also owns the starts
  (`signInWithPassword`, `startOidc`, `onOidcCallback`), because only the verifier's owner may send it;
  `applyStep(step)` applies a start's `LoginStep` with that start's secret. `ShowQr` gains
  `totpAvailable`; `Ended` gains `via` (what "Try again" does: a new QR for F2, the form otherwise).
  The plan's test names live where the behaviour is: `password login shows the confirmation qr` and
  `neutral answer text for INVALID_CREDENTIALS` (which also pins the exact F3 copy from `strings.xml`)
  are in `LoginAttemptViewModelTest`; `probation session shows the banner` in `ProbationBannerTest`;
  `LoginViewModelTest` keeps the form, providers, recovery pages and server tests.
- Long-poll `wait` is 8 s (C27 allows 0..25): the shared OkHttp client keeps its default 10 s read
  timeout, so a 25 s wait would always time out. Network errors retry after 1 s, 2 s, then every 5 s
  while `expires_at` is ahead; a lost `collect` answer is asked again up to 3 times.
- `auth_login_invalid_credentials` now holds F3's neutral answer word for word; the SSO exchange's 401
  maps to it too. New: `CLIENT_OUTDATED`, `TOTP_UNAVAILABLE`, `TOO_MANY_ATTEMPTS` with `Retry-After`
  minutes (plural). `auth_login_mfa_code` removed.
- CHANGELOG: the plan's two lines (`### Changed`, `### Security`).

**B15 step 1 (done):** `approval/KaikuSigGoldenFileTest` reads `golden/kaiku_sig_v1.json` through
`testutil/Golden.kt` and, per vector, compares C, `context_hash`, the prefix built from the inputs (and
parsed back), M, verifies the signature with the file's key and runs `ChallengeVerifier` on the
server's bytes. Without the file all five tests fail (checked by moving it away once).

**Assumptions added for the server owners (L1b):** the Android OIDC start takes `redirect_uri=
kaiku://auth/callback` and `client_challenge` as query parameters of the Custom Tab URL (a browser
navigation cannot send `X-Kaiku-Client`), and the callback carries only `code`; a collected or TOTP
session may still carry `password_change_required` (then the C4 change runs with the typed password).

**Gates on the rebased branch** (after B14 and B15-1, `--rerun-tasks`):

- `cd mobile/android && ./gradlew :lint-rules:test lintDebug testDebugUnitTest assembleDebug --no-daemon`
  → BUILD SUCCESSFUL: `:app:testDebugUnitTest` 1161 tests in 134 classes, 0 failures (1141 before the
  seam work); `:lint-rules:test` 11, 0 failures; `lintDebug` 0 errors, 55 warnings (all pre-existing
  kinds, none in the B14 files); `assembleDebug` built (Hilt graph with the `@DeviceStepUp` binding).
- `cd client && bun run i18n:check` → no errors (874 web keys, 587 Android strings; 3 duplicate-text
  warnings, pre-existing).
- `git diff origin/feat/v1.0-login-lp --stat -- server/` → empty.
- `git grep -nE "access_token=" -- mobile/android/app/src/main` → none;
  `git grep -nE "kaiku://qr/login|qr/redeem|forgot-password|reset-password|mfa_code" -- mobile/android/app/src/main`
  → none (the legacy-QR refusal's comment was reworded); in Android tests only removal assertions
  (`login never sends mfa_code`, the old `kaiku://qr/login` code refused). The same grep over
  `client/src` still hits Section A's files: that is L2a's branch, not this one.
- Instrumented tests (`AndroidApprovalKeyStoreTest`) were not run: no device attached. Device checks
  D-B1…D-B11 (D-B10 now covers B14: password sign-in confirmed by the key phone, probation banner)
  stay with the maintainer.

**Left after this rebase:**

- Rebase on `main` after L1a, LP and L1b merge; re-check the C27 routes against L1b's real handlers
  (the Android OIDC start's query parameters above, `LoginStep`/`LoginAttempt` shapes).
- `TokenStorage` still has the pre-v1.0 OIDC PKCE getters/setters (unused by the app now; only
  `clearOidcPkceState` runs, at the SSO start and on the server screen). Remove them with the
  `security-crypto` migration in the release after v1.0, or earlier on request.
- External review of the B14 commits and the seam commits (Fable + Codex + Gemini Argon) before the PR.

## PR review round 1 (Fable, Codex, Argon full; PR #745, 2026-11-03)

Reviews: `kaiku-reviews-private/goals/work/codereview-l2b-codex-r2.md`, `codereview-l2b-fable.md`,
`codereview-argon-l2b-r2-part{1..9}.md`. Argon saw diff parts without tools: every claim was checked
against the code first. Each fix has a test that failed without it (compile-red where a new seam was
needed), except where the reason says why not.

**New seam:** `data/auth/AccountStepUp` (`KeyOrPasskeyStepUp` over the composite `@DeviceStepUp`
provider): the key on the key phone, else a passkey of the account, for exactly the action and target;
outcomes mapped from LP's failures and the server's codes (`toStepUpOutcome`). Used by notice actions,
the open-request prompt, Security settings and the admin views; issuing setup codes (own "Add a
phone", admin "Set up a phone") stays on `approval.DeviceStepUp`. The step-up label parameter is gone
from this API: `DeviceStepUp` shows only the server-signed `target_label` (it never showed it).
**LP touch (additive, allowed):** `PasskeysViewModel.addAvailable()` reads `StepUpAvailability` when the
bound provider has it (`DeviceStepUpProvider.available`: full session and the key, or a passkey).

| Finding | Verdict | Commit | Test / reason |
|---|---|---|---|
| Codex M1 / Argon p8 #1: `SecureScreen` lifted by the outgoing screen | fixed: ref count per window and per view | 517145f9 | `SecureScreensTest` (transition between two secure screens) |
| Codex M2 / Argon p7 #7 / Fable m5: passkey users cannot remediate; "Add a passkey" everywhere | fixed: `AccountStepUp` everywhere either authenticator works; FULL_SESSION_REQUIRED, KEY_DEVICE_REQUIRED, part-2 codes mapped; availability signal for LP's Add | 96db9cc0 | `NoticeActionsViewModelTest` (repudiate + cancel from a passkey session on a phone without a key; neither key nor passkey), `OpenRequestViewModelTest` (cancel with passkey, refusals), `SecuritySettingsViewModelTest`, `PasskeysAddAvailabilityTest` |
| Codex M3 / Argon p7 #8: open request read only at sign-in, only over home, survives users | fixed: refresh at every ON_START and account change, bound to the account, shown over any signed-in non-ceremony route, reset on sign-out/other user | b69d74a1 | `OpenRequestViewModelTest` (background → foreground into a chat, other account, sign-out), `OpenRequestRules` test; nav wiring is Compose only |
| Codex m4 / Argon p4 #2: rotation starts a second issuance | fixed: start job guard, generation (late code withdrawn), admin form + flow in `rememberSaveable` | a1991526 | `EnrollmentIssuerViewModelTest` (recreated screen, cancel during issuance); the rotation form state is Compose only |
| Fable M1: setup status `wait=25` vs 10 s read timeout | fixed: `LongPoll.WAIT_SECONDS = 8` shared with the sign-in poll; OkHttp read timeout set explicitly to `LongPoll.READ_TIMEOUT_SECONDS` | 8448ea43 | `the status long-poll waits less than the client's read timeout`, pending `long-polls within the read timeout` |
| Fable m1: probe not pinned to the key's origin | fixed: `deviceLoginChallenge(…, origin)`, `probe(userId, origin)` | 8448ea43 | `ApprovalApiTest` (changed saved server), pending test asserts the key's origin |
| Fable m2 / Argon p4 #1: enroll pre-UV check misses variant, claimant check, name, type, key hash | fixed: all pinned to the mode, the admin's record and the claim | a1991526 | `variant, claimant check, name, type and key must be what was recorded` |
| Fable m3: test gaps (PoP tamper, user id, issuer subject) | fixed (tests) | a1991526, 8448ea43 | `a PoP context that does not match …`, `a PoP prefix for another user id`, `a context for another subject is refused` |
| Fable m4 / Argon p6 #6: unsigned display name as headline / prompt | fixed: signed `subject_username` and the pinned host first, display and server name as hint | 8448ea43 | `the subject screen and the prompt show the signed name and the server first` |
| Fable m6 / Argon p3 m2: dead PKCE storage; OIDC keys migrated | fixed: removed, not in `STORED_KEYS` | 76adc1d9 | `old OIDC verifiers are not moved`; the API is gone (compile) |
| Fable m7: legacy store deleted on a transient write failure | fixed: kept for retry, bounded (3 starts, counter in `kaiku_app_state`) | 76adc1d9 | `a passing write failure is retried …`, `the retry is bounded` |
| Fable m8: `/setup` suffix accepted | fixed: exact path | 0d9d4ac5 | `a setup url needs exactly the setup path` |
| Argon p1 #1: `getCertificate` null | fixed: `KeyStoreException` | 01c61e19 | Android Keystore only (no JVM test) |
| Argon p1 #2: invalidation in another shape escapes | fixed in `DeviceSigner` | 01c61e19 | `an invalidated key reported in another shape …` |
| Argon p2 #1: `RegisterRequest.toString`, register not public | fixed | 01c61e19 | `register is a public route whose errors keep their fields`; `toString` is on a private class (not testable) |
| Argon p2 #2: `keyDeviceId` null blocks "remove this phone" | fixed: looked up from the server list, recorded | 01c61e19 | `KeyDeviceSignOutTest` |
| Argon p2 #3: non-IO failures escape | fixed: `DeviceSignIn`, `KeyDeviceSignOut`, `KeyDeviceProbe` | 01c61e19, 8448ea43 | `DeviceSignInTest` (SerializationException) |
| Argon p2 #4: `UserVerifier` catches | fixed (`NoApprovalKeyException`, `signNow` failures) | 01c61e19 | covered via `DeviceSignerTest`; the BiometricPrompt path is device-only |
| Argon p2 #5: `forget()` without finally | fixed | 01c61e19 | `a failing keystore delete still forgets the record` |
| Argon p2 #6: `ensureSuccess` swallows cancellation | fixed | 01c61e19 | not practical on the JVM (needs a cancel inside the body read) |
| Argon p2 #7: pin during CLAIMING/CLAIMED | **rejected** | — | an unresolved claim may be an enrolled key (lost answer): the app stays tied to that server until the waiting screen reconciles it, and a new setup link now goes there first (8448ea43); forgetting the claim unpins |
| Argon p3 #1: `beginDeviceSignIn` does not advance the generation | fixed; `completeAttempt` never clears under a running session | 01c61e19 | `a device sign-in supersedes a sign-in still in flight …`, `two overlapping device sign-ins never both install` |
| Argon p3 #2: `KeyInvalidation` and cancellation | fixed: NonCancellable, bounded report, forget in finally | 01c61e19 | `an invalidated key is forgotten even when its screen is left …` |
| Argon p3 m1: replaced-session logout cancellable; superseded collected session leaked | fixed for L2b: NonCancellable logout, `completeAttempt` ends unused sessions | 01c61e19 | `ending the replaced session survives a cancelled caller`, `a collected session that is not installed is ended …`, `AuthRepositoryLoginRaceTest` updated; **deferred to LP:** the same for LP's `installNativeSignIn` (LP's code) |
| Argon p4 m1 / p8 #3b: "your account" when the reader is unknown | fixed: key's account when signed out; unknown → the subject's name / "an account you help with" | 45d7f666 | `SecurityNoticePresenterTest` (lapsed key phone via the injected constructor, unknown reader) |
| Argon p4 m2: empty label for may-approve | fixed by removal (label never shown) | 96db9cc0 | API change (compile) |
| Argon p4 m3: admin double submit, search race, load resets, stale user id | fixed | 96db9cc0 | `AdminUserViewModelTest` (existing tests with the new guards); search race not tested |
| Argon p4 m4: "Remove now" on revoked | fixed: revoked not listed | 96db9cc0 | Compose only |
| Argon p4 m5: non-atomic take | fixed: `ScanHandoff`, `PendingSetupLinks`, `PendingOidcCallback` | 30aa3fa9 | `SetupLinksTest` |
| Argon p5 #1: Back during deny/report releases the claim | fixed: committed before sending, NonCancellable, Back off while Working | dec2263b | `leaving while a No is sent …`, `leaving while I didn't start this is sent …` |
| Argon p5 #2: "Use another account" leaves a session | fixed: temporary session signed out; KEY_INVALIDATED, lock-out, pending handled | 2d6c7e93 | `ConfirmItsYouViewModelTest` |
| Argon p5 #3: stale OIDC callback | fixed | d4b503bb | `a callback after an abandoned SSO start disturbs no running sign-in` |
| Argon p5 #4: password stays in `uiState` | fixed: cleared after sign-in / forced change | d4b503bb | `the typed password leaves the form …` |
| Argon p5 #5: 429/5xx poll ignores expiry | fixed: live check, Retry-After capped | d4b503bb | `server trouble never polls past the attempt's end` |
| Argon p5 #6: concurrent passkey and attempt | fixed in part: `begin()` refuses while busy | d4b503bb | `a double tap sends one sign-in`; **deferred to LP:** LP's passkey button has no `enabled` separate from its busy label (the start window is short and the single install point refuses a second session) |
| Argon p5 #7: blank labels; non-ChallengeRejected in `start` | fixed | dec2263b | `a blank label or place is treated as missing` |
| Argon p6 #1 (blocker): retry/new link wipes a claimed key | fixed: CLAIMED → waiting screen, unresolved claim never replaced | 8448ea43 | `try again after an unsure answer …`, `a new setup code while a claim is unresolved …` |
| Argon p6 #2: registration ticket lost | fixed | 1ecb813f | `a failed issuance … retries with the same ticket`, `a double tap registers once` |
| Argon p6 #3: re-prompt on resume after cancel | fixed | 8448ea43 | `a resume after a cancelled final prompt does not prompt again` |
| Argon p6 #4: PENDING asks the enrolment; code wait polled every 15 min | fixed: PENDING uses the probe (enrolments are deleted 30 days after they end, `server/src/db/queries.rs:489`), CLAIMED long-polls | 8448ea43 | pending tests (rewritten to the probe), `a claim waiting for its code long-polls …` |
| Argon p6 #5: no job tracking, no cleanup on leave | fixed: one step job, status re-check, `onCleared` deletes an unclaimed key; **rejected:** a `BackHandler` (leaving clears the ViewModel, `onCleared` cleans up) | 8448ea43 | `cancel after confirming the server …` (covers `abandonUnclaimed`); `onCleared` is protected (not called on the JVM) |
| Argon p6 #7: adopted server not restored | fixed | 8448ea43 | `cancel after confirming the server puts the saved one back` |
| Argon p6 #8: TOTP step unprotected, saveable, empty submit | fixed | 517145f9 | Compose only |
| Argon p7 #1: open-request route never exits; Back loops | fixed: effect keyed on pending+working, `working` cleared first, Back = "That's right" | b69d74a1 | `OpenRequestViewModelTest` (working false before pending null); prompt is Compose |
| Argon p7 #2: null own id → admin endpoint | fixed | 96db9cc0 | `without the own user id nothing goes to the admin route` |
| Argon p7 #3: empty label placeholders | fixed by removal | 96db9cc0 | the label was never shown (`DeviceStepUp` shows the signed one) |
| Argon p7 #4: setup link taken in composition; stale link; scanner under setup | fixed: consumed after composition (memory only), newer link replaces, scanner popped, cleared on sign-out | 30aa3fa9 | `SetupLinksTest`, `the same code again changes nothing, a newer one starts over` |
| Argon p7 #5: no suitability before scanning; key phone accepts setup codes | fixed | 30aa3fa9 | `the phone's suitability is known before the scanner opens`, `a key phone refuses a setup code` |
| Argon p7 #6: camera stays bound | fixed: unbind on dispose | 30aa3fa9 | `CameraBindingTest` |
| Argon p7 #9: recovery-code IME | fixed | 517145f9 | Compose only |
| Argon p7 #10: renewal check order; renewal over ceremonies | fixed in part: never over a ceremony; **rejected:** the order (`RenewalGate.shouldPrompt` consumes this open's flag by design, `data/auth/RenewalGate.kt:29`) | 8c2e218b | uses the tested `OpenRequestRules.showOver` |
| Argon p7 #11: notice card outside the Kaiku DM | fixed: Kaiku DM and system author only | 45d7f666 | `a security notice card only for the system account in the Kaiku DM` |
| Argon p8 #2: Security settings failures, probation remove, double submit | fixed | 96db9cc0 | `SecuritySettingsViewModelTest` (three new tests) |
| Argon p8 #3a: dialogs on every card | fixed: only the active notice | 45d7f666 | `only the notice whose action runs owns the dialogs` |
| Argon p8 #4: Back discards recovery codes | fixed | 517145f9 | Compose only |
| Argon p8 #5: contact names hide @username; picker not scrollable | fixed | 8c2e218b | `ContactNameTest` |
| Argon p8 #6: banner never retried | fixed: refresh on resume | 8c2e218b | `a passing network error does not hide the banner for good` |
| Argon p8 #7: log pagination lost on rotation, race | fixed | 8c2e218b | `a recreated screen keeps the pages already shown` |
| Argon p9: App Links script | fixed: host lists, split data tags, stray filters, no `kaiku://login`; **rejected:** pinning `/v1` (the filter is host-only by design; other versions are refused by `KaikuPayloads` with "Update Kaiku") | 27acc6a9 | run on the merged manifest (ok) and on a crafted one with a `kaiku://login` filter (fails) |

**Counts:** Codex 4 fixed. Fable 9 fixed. Argon 54 findings: 53 fixed (5 of them in part), 1 rejected (p2 #7), plus 3
rejected sub-points (p6 #5 BackHandler, p7 #10 order, p9 `/v1`), 2 deferred to LP (p3 m1
`installNativeSignIn`, p5 #6 passkey button).

**Gates after the fix round** (`--rerun-tasks`): `:lint-rules:test lintDebug testDebugUnitTest
assembleDebug` → BUILD SUCCESSFUL; `:app:testDebugUnitTest` 1235 tests in 140 classes, 0 failures;
`:lint-rules:test` 11, 0 failures; `lintDebug` 0 errors, 54 warnings (pre-existing kinds); APK built;
`scripts/android_check_app_links.sh` ok. `bun run i18n:check`: no errors (874 web keys, 587 Android
strings). `git diff origin/feat/v1.0-login-lp --stat -- server/` empty; the Task V greps over Android
`src/main` find nothing. Instrumented tests not run (no device attached).

## PR review round 2 (Codex r3, Fable, Gemini Flash r3; 2026-11-03)

Codex confirmed its M1–M3 fixed and found one regression; Fable APPROVE WITH FIXES; Flash (standing in
for Argon until 2026-11-08) APPROVE / APPROVE WITH FIXES.

| Finding | Verdict | Commit | Test / reason |
|---|---|---|---|
| Codex r3: after a failed write the kept old store can be moved in after a sign-in + logout | fixed: `TokenStorage` reports every new session and every removal; `LegacyTokenMigration.discardPending` then deletes the old store and the counter (committed) | fed47c59 | `a failed move is void once the person signs in and out before the next start` (failed move → sign-in → logout → restart → no session) |
| Fable R1: `TokenStorage.clear()` lost `@Synchronized` and KDoc (76adc1d9) | fixed | e8c74ff6 | `concurrent logouts and sign-ins never lose a session generation` (fails without the lock: checked) |
| Fable R2: a message heuristic forgets the key | fixed: only `KeyPermanentlyInvalidatedException` in the cause chain (no public `KeyStoreException` code exists for it, API 33–35); the rest is `Failed("KEYSTORE_…")` | 050e9726 | `only a real permanent invalidation counts …`, `a failure that only reads like an invalidation never forgets the key`, `the platform's permanent invalidation forgets the key`; this reverses round 1's message mapping (a3704860), the removed screen lock is still caught by `uvPreflight` |
| Flash A #1: Cancel leaves `startJob` running, "Try again" blocked | fixed: `cancel()` cancels it | f2bdc55d | `try again right after cancel starts a new issuance` |
| Flash A #2: `signOutAndRemovePhone` swallows cancellation | **rejected (no change needed):** the chain already rethrows `CancellationException` first (`KeyDeviceSignOut.kt:36`); pinned by a test. Grep of the L2b code: every other catch-all is in non-suspending code or deliberately inside `NonCancellable` (`AuthRepository` replaced-session logout and `endUnused`, `KeyInvalidation`) | f0f15840 | `a cancelled removal propagates the cancellation` |
| Fable (optional): one alias, two setup ViewModels | fixed: only the key this instance generated, if it is still the stored one, is deleted | af7dc965 | `leaving one setup screen never deletes another one's unclaimed key` |

Gates after round 2 (`--rerun-tasks`): BUILD SUCCESSFUL; `:app:testDebugUnitTest` 1241 tests in 140
classes, 0 failures; `:lint-rules:test` 11, 0 failures; `lintDebug` 0 errors, 54 warnings; APK built;
`i18n:check` no errors; server diff against LP empty. Instrumented tests not run (no device).
