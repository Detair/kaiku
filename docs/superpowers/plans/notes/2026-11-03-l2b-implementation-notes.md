# L2b (Android) implementation notes, 2026-11-03

Plan: `docs/superpowers/plans/2026-11-03-v1.0-login-clients.md`, Section B. Spec rev 5 and contracts
C26–C28 are binding; where the plan's interface sketch and the contracts differ, the contracts win.
Branch `feat/v1.0-login-l2b`, built on `main` (`7a9e7677`) before L1a, LP and L1b merged, against
fakes and mock engines only. No server file is changed.

## Status

| Task | Status |
|---|---|
| B1–B9 (L2b-1) | done; the last commit of L2b-1 carries the line "L2b-1 boundary" |
| B10–B13 | done |
| B14 | **not started**: marked "after LP rebase" in the plan; it edits LP's `LoginScreen`, `LoginViewModel`, `AuthErrorMapper` and `AuthRepository.login`. Until then the Android password/OIDC sign-in still uses the pre-v1.0 `/auth/login` with `mfa_code` and the OIDC token deep link, which the v1.0 server no longer serves. |
| B15 | partly: gates run (see the PR); `KaikuSigGoldenFileTest` needs L1a's `golden/kaiku_sig_v1.json` and is added after the rebase on L1a. The C26 vectors are reproduced byte for byte in `KaikuSigGoldenTest` from the contract text. Device checks D-B1…D-B11 are for the maintainer. |

## Deviations from the plan (all small; each also in the SDD ledger)

1. **LP seam.** `StepUpProvider` is declared in `data/auth/StepUpProvider.kt` with LP's exact signature,
   because LP is not on `main`. On the LP rebase, LP's declaration replaces it and the Hilt binding of
   `DeviceStepUpProvider` with LP's qualifier is added (the plan says "after LP rebase"). `StepUpGrant`
   lives in `domain/model/StepUp.kt` (the plan's B4 file).
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
8. **Passkeys section.** LP's `PasskeysSection` is not on `main`; Security settings shows the passkey
   count until the rebase mounts LP's section unchanged.
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

- B14 after the LP rebase; then B15's golden-file test after the L1a rebase.
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

