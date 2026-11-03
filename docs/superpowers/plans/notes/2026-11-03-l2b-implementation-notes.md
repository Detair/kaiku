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
- Some new strings with counts ("%1$d tries left", "%1$d recovery codes left", "%1$d more minutes")
  could become `<plurals>` (lint `PluralsCandidate` warnings).
