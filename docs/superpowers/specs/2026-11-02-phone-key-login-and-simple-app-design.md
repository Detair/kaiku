# Kaiku: phone-as-key login (v1.0) and simple Android app (v1.2)

**Status:** Revision 3, for the maintainer and a confirming design review. First draft 2026-11-02.
Revision 1 (2026-11-03) added the maintainer decisions, a standards check and the approvals model
from the package cut. Revision 2 resolved the first round of three independent design reviews
(Fable, Codex, Gemini Argon). Revision 3 (2026-11-03) resolves the second round (Fable: READY WITH
FIXES; Codex and Argon: NOT READY) and removes mechanism instead of patching it: session upgrades
(Trust), the key-device step-up for PCs, every network signal, automatic cancel by "owner
evidence", the dormancy limit and the separate bootstrap ceremony are gone, and seven state machines
became four. The appendices map every finding to its resolution.
**Target:** split release (maintainer decision 2026-11-02):

- **v1.0** ships the new login: Part B (sign-in with passkeys and the phone as key) and Part C
  (security notices). The family server does not go live with the old login design. This is a
  deliberate exception to the v1.0 feature freeze.
- **v1.2** ships Part A: the simple Android app ("Kaiku Einfach"), 1:1 video calls and voice
  messages. Planned after v1.1 (admin MCP / agent bots); its Android UI work may start in parallel.

**How to read this document:** the maintainer decisions bind. Coordinator calls (CC1–CC10) are
binding for the builder unless the maintainer or a design review overturns them; calls changed in
revision 3 are marked **(rev 3)**. Items marked **(design call)** are choices made where neither the
decisions nor the coordinator calls say anything; reviewers should challenge them first. The state
machines in B.7 are normative: where prose and a state machine disagree, the state machine wins.
Citations use the short labels in "Standards baseline".

## Decisions 2026-11-03 (maintainer GO)

These override anything below that contradicts them.

1. **B1 and B2 are v1.0**: admin device setup for another user and
   passwordless accounts ship with the new login. Family members are set up
   on the full Android app at the v1.0 launch; the simple app joins in v1.2.
2. **E-mail password reset is removed** (`/auth/forgot-password`,
   `/auth/reset-password`): it is an e-mail login in disguise. Recovery is
   only another own device, backup codes / TOTP, or admin recovery.
3. **Admin recovery works in person and remotely**; the safeguard is the
   confirmation on the admin's own key device (B1.3) plus admin re-auth.
4. **Voice messages** (record, send, play) are v1.2 on all clients
   (desktop, web, full app, simple app), together with 1:1 video.
5. **State of the art is the bar, and no password at all is fine.**
   Passkeys (WebAuthn / FIDO2) are a first-class login method in v1.0, not a
   later option: Android via Credential Manager, browsers and Windows/macOS
   desktop via WebAuthn where the webview supports it. Phone-as-key with QR
   (B3) stays the universal path, notably for the Linux desktop. A password
   becomes optional for every account; users may remove theirs. Before
   implementation the login part is checked against current standards
   (NIST SP 800-63B-4, OWASP ASVS 5 and the Authentication / MFA cheat sheets,
   FIDO cross-device guidance, CISA guidance on MFA fatigue) and gets a final
   design review.

Also from the maintainer: no e-mail login of any kind; it must stay usable for grandparents whose
phones the maintainer sets up.

**How this spec applies them** (old labels → new sections: B1 → F5c, B1.3 → F5c step 5, B2 → F5d,
B3 → F2, B4 → F3, B5 → B.5, B6 → F6, C24 → F7):

- **Decision 5, "desktop via WebAuthn where the webview supports it"**, cannot work as worded. No
  Tauri webview can make assertions for the server's RP ID: the webview origin (`tauri://localhost`,
  `http(s)://tauri.localhost`) is not the server's domain (WebAuthn L3 RP ID rules, §4), WKWebView
  needs an associated-domains entitlement that a self-hosted domain cannot get, and WebKitGTK has no
  WebAuthn. The intent (passkeys first-class on the desktop) is met by CC5: the system browser on
  every OS, whose result only the desktop app that started the ceremony can redeem.
- **Decision 3:** admin setup and recovery always start on the admin's own key device and end with
  the code typed there with UV (F5c step 5), after the admin's step-up (the admin re-auth). In person
  and remote are one protocol that differs only in the recorded claimant check. Re-auth and the
  confirmation protect against a hijacked *admin account*; against a *social-engineered* admin, the
  realistic remote attack, CC7 adds the claimant check, a waiting period for every existing account
  and cancel paths that do not run through that admin. Both in-person and remote recovery stay.
- **Decision 2:** a forgotten password is replaced through an account request (a recovery code plus
  a second factor, F6) or by an admin. Passwordless accounts have nothing to reset. "Backup codes"
  are the ≥64-bit recovery codes of B.1.
- **Decision 5, "QR stays the universal path":** it does. QR sign-ins produce probation sessions for
  their whole lifetime (CC4) because they can be relayed; passkeys are offered first wherever they
  work, and the key device itself is always fully trusted.

## Coordinator calls 2026-11-03

Each item is a **coordinator call** for the maintainer to review; the detail and the sources live in
the sections named. **(rev 3)** marks calls changed in revision 3.

- **CC1: user verification on every key-device signature.** A key device needs a screen lock (PIN,
  pattern or password) or a Class 3 biometric, checked first at setup; every `K_approve` signature
  requires UV; there is no possession-only key class (NIST §3.1.3 "each use", §3.1.7; OWASP MFA
  cheat sheet: user presence alone is not a factor). B.1.
- **CC2 (rev 3): two transfers, no prompts.** The key device **scans a QR code shown on the PC**
  (sign-in, F2, F3), or the trusted side **types a 6-digit code that a new device shows**
  (enrolment, F5). The transferred value is the signed `response` and is checked before the
  signature. No approval push, no compare-and-approve, no choose-from-list (NIST §3.1.3:
  compare-and-approve is "no longer considered acceptable"; §3.1.3.2). Revision 3 removes the action
  QR and the Trust QR. B.2.
- **CC3 (rev 3): cross-device controls, each with its limit.** Collection binding stops a bystander
  or anyone who learns an attempt id, not QR phishing (the phisher holds the secret of their own
  code). Tokens are 256-bit CSPRNG values, short-lived and single use, never UUIDv7 (RFC 10027
  §6.1.2–6.1.4; ASVS 7.2.3; RFC 9562 §8). The phone shows context and a prominent "No" (§6.1.14).
  **No network signal is used in any decision:** IP, network and place are display and audit data
  only, since a shared network is an inference with limits (§6.1.1) that carrier-grade NAT, VPN
  exits or a misconfigured proxy defeat. Relayable sign-ins stay probation and can be ended from
  their notice (§6.1.9 limits the damage, it does not prevent the attack; §6.1.6). Passkeys over
  hybrid are the phishing-resistant path and are offered first (§6.2.3.5; NIST §3.2.5: manually
  transferred outputs are not phishing-resistant). Combined controls: §6.1.18; ASVS 6.6.2.
  Residual risk: B.6.1.
- **CC4 (rev 3): assurance is fixed at creation.** A session is `full` if and only if a passkey
  assertion or a device-key signature of an active key device created it. Every other sign-in, and
  every session from before the upgrade, is `probation` for its whole lifetime: chat, calls, own
  profile. Nothing upgrades a session; "Sign in with a passkey" creates a new one. Trust is cut:
  without a network gate it was as relayable as F2, and the key device does every sensitive action
  itself (RFC 10027 §6.1.9; OWASP MFA cheat sheet: fallbacks not weaker than the primary). B.4.
- **CC5 (rev 3): passkeys.** WebAuthn on the web, Credential Manager on Android (Custom Tab fallback
  deferred), the system browser on every desktop OS, for sign-in and step-up only. A desktop
  ceremony yields only a **pending result**, which takes effect when the app that started it
  redeems it with its verifier; the result reaches the app only by a form POST to its loopback,
  never in an address bar, a page or a polling response (F1; RFC 8252 §7.3, §8.1; RFC 9700 §2.1.1).
  RP ID = host of `PUBLIC_URL`, exact origins (both APK hash encodings in C29), UV required, BE fixed
  per credential (WebAuthn L3 §5.8.6, §6.1.3, §5.1.10, §5.11; OWASP Passkey Security cheat sheet;
  Android Credential Manager docs). B.1.
- **CC6 (rev 3): sessions.** Refresh rotation with reuse detection and one idempotent retry a day;
  ending a session takes effect at once (the `sid` denylist on every request and WebSocket, fail
  closed, connections closed); absolute caps with the NIST deviation stated; a concurrent-session
  limit; device-key sign-in with UV after a lapse, only while active; a plain sign-out keeps the
  key. `K_session` deferred; Keystore-wrapped storage on Android (RFC 9700 §2.2.2, §4.14.2; ASVS
  7.1.1, 7.1.2, 7.3.1, 7.3.2, 7.4.1; NIST §2.2.3, §5.1). B.4.
- **CC7 (rev 3): one account-request machine for every delayed change.** Self-service recovery (a
  recovery code plus the password, a TOTP code or a fresh OIDC sign-in; NIST §4.2.2.2: a recovery
  code plus a bound authenticator), bootstrap (password or fresh OIDC plus TOTP, once per account)
  and every admin setup or recovery of an existing account (in person or remote, one protocol with a
  recorded claimant check) wait 24 h (24–72 h), notify the user's devices, the recovery contact and
  all admins (NIST §4.2.3, §4.6), can be cancelled by someone other than the requester and the
  acting admin (on a single-admin server the account's own authenticators or older sessions, or a
  recovery contact; otherwise the host CLI), and complete in one transaction. There is no automatic
  cancel, and a cancel disables the factors the request used. Devices named lost are revoked at
  completion, not at the start; a phone in hostile hands is removed at once with "Remove device now"
  (NIST §4.3). Accounts newly created by an admin are active at once. Recovery codes have ≥64 bits
  (§4.2.1.1). Also ASVS 6.4.3, 6.4.4, 6.5.6. F5c, F6, F8, SM3.
- **CC8 (rev 3): passwords.** Optional; checked against a breached and common password blocklist;
  no password-only accounts after the upgrade, since their first strong authenticator comes from an
  admin (stricter than NIST §4.1.2.1). **Repudiation disables the factor that was used:** the
  password is flagged (refused everywhere until replaced) and TOTP disabled (until renewed). The
  flag no longer closes TOTP (NIST §3.1.1.2: blocklist SHALL, no composition rules, change on
  evidence of compromise; §2.2.1; ASVS 6.2.4, 6.2.11, 6.2.12, 6.3.3, 6.3.5; OWASP MFA cheat sheet:
  correct password + failed second factor = compromised password). B.1.
- **CC9 (rev 3): throttling.** Per account and per IP; a growing delay after 5 consecutive failures,
  a notice at 20, the password or TOTP disabled at 100 (NIST §3.2.2; ASVS 6.1.1, 6.3.1). The recovery
  entry is delayed but never disabled; passkeys, QR login, device-key sign-in and sessions are never
  affected; agent approvals (v1.1) get their own limit class (K9). B.5.
- **CC10 (rev 3): one signature format, one approvals core.** Every `K_approve` signature answers a
  single-use server challenge with a nonce, an expiry, the origin, user and device, and a context
  hash that the device recomputes from what it shows. Approvals (`login`, `enroll`) carry the
  transferred secret; proofs (`device_login`, `device_stepup`, `enroll_pop`) carry none. v1.1 adds
  its kinds, side table, desktop rows and approval push in its own migration; desktop approval keys
  never answer a v1.0 kind. B.2.

## Standards baseline

Short labels used in this document:

- **NIST** = NIST SP 800-63B-4 (final, 2025).
- **ASVS** = OWASP ASVS 5.0.0, V6 Authentication and V7 Session Management.
- **RFC 10027** = RFC 10027 / BCP 247, "Best Current Practice for Security of Cross-Device Flows"
  (IETF, August 2026; formerly draft-ietf-oauth-cross-device-security).
- **WebAuthn L3** = W3C Web Authentication Level 3.
- **RFC 9700** = OAuth 2.0 Security BCP. Also RFC 8252 (native apps), RFC 9449 (DPoP), RFC 9562
  (UUIDs), RFC 8176 (`amr` values).
- OWASP MFA and Passkey Security cheat sheets; the CISA fact sheet on number matching.

**Target:** AAL2 for every account, with at least one phishing-resistant option offered (NIST
§2.2.2: passkeys), and ASVS 5 Level 2 for V6 and V7. Syncable passkeys are acceptable at AAL2
(NIST Appendix B); AAL3 is a non-goal. Every authentication path and its controls are listed in
B.3 and B.4 and modelled in the state machines of B.7, so all paths are documented and held to the
same strength (ASVS 6.1.3). Deviations are listed in B.6.

## Intent

Kaiku's family server has two kinds of users:

- **Full users** (the maintainer, siblings): want the whole feature set:
  guilds, voice channels, roles, bots, settings.
- **Parents and grandparents**: want to chat, send photos and call. The full
  client confuses them. They should get an app that is as simple as WhatsApp,
  and they should never have to deal with passwords, codes or server URLs.
  The maintainer sets up their phones for them.

At the same time, signing in should get both simpler and safer. Passkeys are the first choice
wherever they work. The phone becomes the key everywhere else: a Kaiku app with a screen lock is
the user's authenticator, and new sign-ins on a PC are confirmed by scanning the PC's screen with
it. A password becomes optional, and having none at all is fine.

## Goals

1. **"Kaiku Einfach"** (v1.2): a second Android app with chat list, chat, and voice
   and video calls. Nothing else.
2. **Passkeys first-class** (v1.0) on web, desktop (all OSes) and Android.
3. **Phone as key**: a Kaiku Android app (either app) with a screen lock is a user-verified
   authenticator. It confirms sign-ins on other devices and new devices, and does every sensitive
   action itself.
4. **QR login on the PC**: the PC shows a QR code, the phone scans it, the
   user sees who is signing in and confirms, and the PC is signed in.
5. **Password optional**, never the only factor; password login is confirmed with the phone.
6. **No approval fatigue**: there is no approval prompt to spam. Every approval starts with a scan
   or a typed code on the approving phone, and repeated failures slow the attacker down.
7. **Device setup on behalf of someone else**: the maintainer (admin) sets up a family member's
   phone by QR; such accounts need no password.
8. **Recovery that is not weaker than sign-in**, without e-mail.
9. **Security notices in chat**, not by e-mail.
10. **Meets the standards baseline**, with documented deviations only.

## Non-goals

- **E-mail login, magic links or e-mail password reset**, also not as a fallback (decided: an
  e-mail account is a weaker key than the phone; see the 2026-10-31 web.de hijack).
- iOS.
- AAL3 (hardware-bound, non-syncable authenticators only).
- Android E2EE. E2EE is off for v1.0 everywhere and Android has none
  (`2026-10-20-v1-family-release-goal-design.md`, item 3). The simple app
  shows the same placeholder as the full app for any legacy encrypted message.
  Real Android E2EE remains its own project.
- Group video calls (more than two video participants). Group *voice* calls
  are in scope (see below).
- Moving the simple app to a separate repository or a separate backend.
- Later options, noted only: the Kaiku app as an Android 14+ Credential Provider (so its key could
  answer browser hybrid requests directly); the native Windows WebAuthn API on the desktop; the
  Linux credentials portal once it ships; Device Bound Session Credentials (DBSC) in browsers.
- **Deferred to v1.x** (they add complexity without adding security at the AAL2 target): verified
  Android key attestation (v1.0 keeps a minimum, B.1); device-bound refresh with `K_session`; the
  Android Custom Tab passkey fallback for servers that Google cannot reach; mapping an IdP's `amr` /
  `acr` to full assurance; OWASP-style device cookies that keep known clients out of the
  per-account delay (B.5); one step-up for several setup codes; a proximity-proven way for the key
  device to give a PC full access (it would need a FIDO-grade proximity check; see Q5).

## Anti-goals (a failure even if it works)

- Grandma's phone shows a settings screen, a permission maze or a password
  prompt she cannot get past.
- An attacker who knows a password gets in by spamming confirmations until
  someone taps "yes" (the 2022 Uber pattern).
- An attacker tricks a user into scanning *the attacker's* QR code and
  thereby signs the attacker in (QRLjacking) without the user noticing.
- A stolen browser or desktop session becomes a permanent takeover: it adds its own key device
  or passkey, or throws out the owner's.
- An attacker locks the whole family out by triggering IP blocks: the family
  shares one home IP.
- An agent (v1.1) pauses or starves the owner's sign-ins by spamming approvals.
- Account recovery that is weaker than the login (anything that lets someone
  in with less than the phone).
- Two apps that drift apart because they share no code.

## Current state (2026-11-03, main)

- Android: native Kotlin/Compose (`mobile/android`, minSdk 26), Hilt, login (password, OIDC, MFA),
  DMs, channels, voice (WebRTC, `VoiceCallService`), push via FCM and UnifiedPush including call
  wake (`push/CallWake*`). Tokens are stored with `androidx.security:security-crypto` (an alpha,
  deprecated upstream). `QrScannerScreen` takes the server from the scanned
  `kaiku://qr/login?server=…` payload.
- **QR login desktop → phone exists**: `POST /auth/qr/create` (authenticated, 120 s Valkey token,
  `NX`) and `POST /auth/qr/redeem` (public, rate limited, `GETDEL` one-time) in
  `server/src/auth/mfa.rs`; Android `QrScannerScreen` / `QrRedeemScreen`. The token is a UUIDv7,
  and redeem returns a session without any confirmation.
- MFA: TOTP plus backup codes (10 × 8 alphanumeric characters, about 47.6 bits each), C24 re-auth
  (`reauth.rs`: password + TOTP or OIDC).
- The OIDC desktop hand-off (one-time code + S256 verifier + loopback, 60 s, session created only on
  redemption) exists and is the pattern for desktop passkeys (`server/src/auth/AGENTS.md`). Its
  loopback redirect carries the code in the URL, and its re-auth variant elevates the session at the
  callback, before the desktop app is involved.
- Sessions: 15-min access JWT; the refresh token rotates in place (stable `sid`) with a 7-day
  sliding expiry, so a used session never ends. A superseded refresh token simply gets 401; reuse
  is not detected. `DELETE /auth/sessions` and `DELETE /auth/sessions/{id}` need only a bearer
  token. Revoked `sid`s go into a Valkey denylist that every authenticated request and the WebSocket
  check, failing closed (`auth/revocation.rs`, `ws/handlers.rs`).
- Rate limiting: Valkey Lua limiter incl. `failed_auth.lua` (`server/src/ratelimit`). Failed logins
  are counted per IP only; the login TOTP step has no per-account counter (re-auth has one).
- Passwords: only the length (8..128) is checked.
- `geoip.rs` calls a configurable external HTTP API, at login and at refresh (`login.rs`), so login
  IPs go to a third party.
- `PUBLIC_URL` is optional today (required only for OIDC).
- Registration follows the `registration_policy` server setting.
- Calls: DM calls with ringing exist in the web/desktop client (`components/call`, `stores/call`)
  and run in voice rooms. **Webcam tracks already exist in voice rooms on web and desktop**
  (`server/src/voice/webcam.rs`, `WebcamButton`, `TauriVideoFrame`); there is no 1:1 video call UI
  yet. Android has no camera capture and renders remote video only for screen shares.
- `users.password_hash` is nullable, but a DB constraint requires a password
  for `auth_method = 'local'`.
- Android `OidcHandler` still accepts `access_token` / `refresh_token` from a
  `kaiku://auth/callback` deep link, so any app or link can plant a session (L2 deletes that branch).

## Part A: Kaiku Einfach (the simple app, v1.2)

### What the user sees

Three screens (mockup agreed in the brainstorm session):

1. **Chats**: one list of DMs and groups, newest first, large text, avatar,
   last message, unread count. Search.
2. **Chat**: messages with sender names in groups, photos, voice messages,
   text input with a microphone button; call and video-call buttons in the
   header.
3. **Calls**: full-screen incoming call with two large buttons (decline /
   accept), also on the lock screen; in-call screen with mute, speaker,
   camera on/off, hang up.

The start screen of an unconfigured simple app offers only "Scan setup code"; a setup link opened
on the phone leads to the same step (admin setup, F5c).

**Guilds map to groups, one per text channel** (decided, option B): a guild
"Familie" with text channels "Allgemein" and "Urlaub" appears as two groups
"Familie · Allgemein" and "Familie · Urlaub". Voice channels, threads,
categories and all settings do not appear.

**Group calls**: the call button in a group rings every member, like
WhatsApp group calls (maintainer confirmed). *How* the server models this
(ad-hoc voice room bound to the text channel, or extending DM calls to
multi-party) is the builder's call; it must work between the simple app, the
full app and the desktop client. Video in group calls is out (see
non-goals); a 1:1 video call is in.

**Not in the simple app**: creating guilds or channels, inviting people,
roles, permissions, bots, reaction roles, server or guild settings, admin
tools, threads, account settings beyond name and photo (the security log is shown read-only). The
maintainer does these in the full app.

### How it is built

- **One codebase, two apps**: Android product flavors (or a separate `:app-simple`
  application module) on shared modules for API, WebSocket, auth, approvals, push,
  voice/video and storage. Only the UI layer differs. The split into modules
  is the builder's call; the constraint is that a fix in login, push or calls
  lands in both apps at once.
- **Separate application id** so both apps can be installed side by side
  (the maintainer may want both on one phone for testing). Separate launcher
  name ("Kaiku Einfach") and icon variant. Both application ids and the release certificate are in
  the passkey configuration (`assetlinks.json`, origin allow-list; B.1).
- Both apps are built and signed by `release.yml` and attached to the
  release; distribution stays sideloaded APK until the store decision.
- i18n rules from `mobile/android/AGENTS.md` apply; the simple app is German
  first in practice, but all strings go through resources as usual.

### 1:1 video and voice messages (v1.2, all clients)

Webcam tracks already exist in voice rooms on web and desktop, and DM calls run in voice rooms, so
1:1 video there is mostly call-UI work. Android has no camera capture yet. v1.2 adds **1:1 video in
DM calls** on desktop, web, the full Android app and the simple app: camera on/off, front/back
camera on Android, self-view, remote video full screen. Group video is out. **Voice messages**
(record, send, play) ship on the same four clients (Decision 4).

## Part B: Sign-in (v1.0)

### B.0 Terms

- **Key device**: a Kaiku Android app (full or simple) with an enrolled, user-verified approval key
  for its user. A user can have several. A desktop client is never a key device; v1.1 desktop
  approval keys answer agent approvals only (CC10).
- **Strong authenticator**: an active passkey or an active key device.
- **PC**: shorthand for the device that asks to be signed in: desktop client, browser, or a second
  phone.
- **Approval**: a key device's UV signature over a sign-in or enrolment request it reached by a
  transfer, carrying the transferred secret (B.2).
- **Transfer**: a secret moves to the approving side: the phone scans a QR code shown by the PC
  (sign-in), or the trusted side types a code that a new device shows (enrolment). (CC2)
- **Phishing-resistant**: bound to the server's origin, so a relaying page cannot use it (NIST
  §3.2.5). Passkeys (UV) and device-key sign-in on the key device itself are phishing-resistant.
  Transfers by scan or typing are not.
- **Session assurance**: `full` or `probation`, fixed when the session is created (CC4, B.4).
- **Step-up**: a single-use grant for one named action and target, valid for at most 5 minutes and
  bound to a full session (F7).
- **Account request**: a delayed change to an account's authenticators (SM3): self-service
  recovery, bootstrap, or an admin setup or recovery of an existing account.
- **Devices named lost**: authenticators that an account request says are lost or replaced; they
  are revoked when it completes.
- **Owner action rule**: who may act for the owner against an event ("That wasn't me", Cancel; B.2).
- **Recovery contact**: another user, never an admin, whom the account holder (or the admin at
  setup) names. The contact receives the high-severity notices and may cancel a pending account
  request; nothing else (Part C).
- **Bootstrap entitlement**: the one-time right of a migrated account with a password (or OIDC) and
  TOTP to bind its first strong authenticator through an account request (F8).

### B.1 Authenticators and their roles

| Authenticator | Role | Resulting session |
|---|---|---|
| **Passkey** (WebAuthn L3, discoverable, UV required) | primary phishing-resistant sign-in and step-up | full |
| **Key device** (Kaiku Android app, UV-bound key) | its own sign-in (device-key sign-in) and step-up; approver for sign-ins on PCs and for enrolments; the only sign-in of passwordless family accounts | full on the device itself; PCs it confirms get probation |
| **TOTP** | second factor after a password or OIDC (F4); one factor of an account request | probation |
| **Recovery codes** (≥64 bits) | one factor of an account request; never a sign-in factor **(rev 3)** | – |
| **Password** (optional) | first factor only (F3, F4); one factor of an account request | – |
| **OIDC** | first factor through the IdP, followed like a password; a fresh OIDC sign-in can be one factor of an account request | probation |

**Key devices:**

- **One Keystore key in v1.0**, `K_approve` (P-256): non-exportable and hardware-backed (TEE,
  StrongBox where available). It makes every device signature of B.2, with UV on every use.
- **User verification (CC1):**
  - Setup checks for a secure lock screen or a Class 3 biometric **and** a hardware-backed keystore
    first, before the admin or the user scans or types anything, so an unsuitable phone fails in the
    first minute. Without a screen lock, setup stops with a guided "Turn on a screen lock" step.
  - For a relative's phone, the relative chooses the PIN together with the admin and writes it down,
    and a fingerprint is added where the phone has a reader. The UV prompt is the phone's own screen
    lock prompt; there is no separate Kaiku PIN, and the admin guide says so.
  - API 30+: per-use authentication (`setUserAuthenticationParameters(0, AUTH_BIOMETRIC_STRONG |
    AUTH_DEVICE_CREDENTIAL)`), with a BiometricPrompt and CryptoObject per signature.
  - API 26–29: a strong biometric per use (CryptoObject). On a phone with only a PIN, pattern or
    password: a key with an authentication window of at most 10 s, opened by a credential prompt
    that Kaiku shows immediately before each signature **(design call; a deviation, B.6)**.
  - `setInvalidatedByBiometricEnrollment(false)`, so a grandchild adding a fingerprint does not
    destroy the key (tested on both API ranges). When Android invalidates the key anyway (screen
    lock removed), the device stops being a key, and the user and the admins get a notice. The key
    is never replaced silently.
- **Key attestation (deferred; v1.0 minimum):** the app sends an attestation chain with the
  enrolment nonce as its challenge; the server stores it for a later check, records the reported key
  properties as risk information, and refuses an enrolment whose reported properties fall below this
  section (that catches app bugs, not a modified app). The v1.x verifier checks the chain against
  Google's roots and revocation list, the hardware security level, `userAuthRequired` with the
  timeouts above, the challenge and the `attestationApplicationId` (operator override for self-built
  APKs). Deferring is acceptable: attestation is not required at AAL2, and B.6's relays use genuine
  phones.
- **Screen hardening:** `FLAG_SECURE`, `filterTouchesWhenObscured`; lock-screen notifications are
  `VISIBILITY_PRIVATE` and carry no action that approves anything.
- **Origin pinning:** a configured app accepts only QR payloads and links whose origin equals its
  own server and ignores any server a payload names (L2 inverts today's `QrScannerScreen`). Only an
  unconfigured app opened with a setup link accepts a server, and shows its name before sending
  anything.
- **May approve** (account flag `may_approve`): whether this account's key devices may confirm
  sign-ins on other devices (F2, F3) and add phones (F5a). Off for accounts created by admin setup,
  on for others; **only an admin changes it**, with a step-up. With the flag off, the app refuses a
  sign-in QR with a friendly explanation and offers no "Add a phone": QR phishing targets the least
  experienced users (Q4). The flag never limits "That wasn't me" or Cancel **(rev 3)**.
- **Lifecycle (SM4):** `pending` → `active` → `revoked`. A key device **outlives its refresh
  session** and signs in again with UV (B.4). A plain sign-out ends only the session; a ban is
  checked at use time; approve calls must come from the device's current session. There is no
  dormancy limit **(rev 3; cut)**: every use needs the screen lock anyway.

**Passkeys (CC5; WebAuthn L3, OWASP Passkey Security cheat sheet):**

- **RP ID** = host of `PUBLIC_URL`: never a parent domain, immutable, checked at startup.
  `PUBLIC_URL` becomes mandatory, and the server fails closed without it. Changing the domain later
  invalidates every passkey and key-device signature, and the admin guide says so.
- **Origin allow-list:** exact. It holds the web origin plus `android:apk-key-hash:<hash>` for both
  APKs, plus operator overrides for self-built APKs. In `clientDataJSON` the hash is the unpadded
  base64url SHA-256 of the signing certificate; `assetlinks.json` carries the same hash as
  upper-case colon-separated hex. C29 fixes both forms with a test vector, and the server matches
  Android origins before any generic URL parsing. Related origins (§5.11) never list
  `tauri.localhost`: every Tauri app on Windows shares that origin.
- **Registration:** `residentKey: required`, `userVerification: required`, attestation `none`,
  `excludeCredentials`. Challenges are ≥16 random bytes, single use, and bound to the ceremony,
  the session (or request or ticket) and the account. Registration runs only in a browser or the
  Android app, never inside the desktop app **(rev 3)**:
  - on the same client, in a full session with a step-up "add a passkey", inside a `ready` account
    request, or inside a registration ticket;
  - on another client, as an enrolment from a setup link issued by a trusted device or session
    (F5a, F5c; SM2).
- **Sign-in:** UV=1 is checked; UV=0 is refused, not treated as a weaker login (§5.8.6). The BE flag
  must equal the value stored at registration (§6.1.3: BE is fixed when the credential is created);
  a mismatch is refused. The signature counter is a signal, never an automatic lockout.
  `allowCredentials` is always empty (discoverable credentials), so a passkey request never reveals
  whether a username exists.
- **Stored per credential:** AAGUID, BE/BS flags (BS as risk input), transports, created, last
  used, and the state (`pending` | `active` | `revoked`, SM4).
- **Signals (§5.1.10):** `signalUnknownCredential` after a delete and at the next use of a
  credential whose enrolment failed, was cancelled or expired; `signalAllAcceptedCredentials` after
  a delete. A `pending` passkey is answered "ready at …", never signalled as unknown.
- **Clients:** web: conditional mediation in the username field plus a "Sign in with passkey"
  button. Desktop: the system browser, for sign-in and step-up (F1). Android: Credential Manager;
  the server generates `/.well-known/assetlinks.json` from config (both application ids, release
  certificate SHA-256, operator override) with `delegate_permission/common.get_login_creds` and
  `delegate_permission/common.handle_all_urls` (setup links), `Content-Type: application/json`. The
  RP domain must be reachable by Google (no `robots.txt` block; `/.well-known/` routed to the
  server); a server Google cannot reach has no passkeys inside the Android app in v1.0, and the app
  itself is the key there.
- **Policy:** a UV passkey assertion is a complete sign-in and yields a full session; no phone
  approval is added on top, and it is not a step-up (F7). A user cannot remove their own last strong
  authenticator or leave the account password-only; an admin may remove a stolen one (F6). Every add
  and remove sends a notice.
- **Library:** `webauthn-rs` (MPL-2.0, allowed). Spike first: its `openssl` dependency under
  `cargo deny` and in the server image, and acceptance of the `android:apk-key-hash:` origin.

**Passwords (CC8):**

- Optional for every account; admin-created accounts have none. When set: length 8..128, no
  composition rules, an offline blocklist (at least the top 100k breached passwords) plus context
  words (product, server and user name; NIST §3.1.1.2; ASVS 6.2.4, 6.2.11, 6.2.12), Argon2id. The
  8-character minimum stays because a password is never the only factor (NIST asks for 15 only
  when it is).
- **Compromise flag** (`password_compromised_at`): set by "I didn't start this" on an F3 attempt,
  "That wasn't me" on a password sign-in, a cancelled account request that used the password, or an
  admin. While set, every use of the password gets F3's neutral answer, and the account's open
  password attempts are cancelled; TOTP after an OIDC first factor stays usable **(rev 3)**. It
  clears when a new password is set (or the password removed) from a full session with a step-up,
  or by a completed account request; the UI then offers to renew TOTP and the recovery codes.

**TOTP and recovery codes (CC7):**

- **TOTP:** as today. **Disabled** (closed until renewed) after 100 consecutive failures, when a
  cancelled request or a repudiated sign-in used it, or by an admin; renewed from a full session
  with a step-up or by a completed account request.
- **Recovery codes** replace backup codes: 10 single-use codes, each with ≥64 bits from a CSPRNG
  (for example 13 base32 characters, shown in groups), Argon2-hashed (NIST §4.2.1.1). Their only use
  is as one factor of an account request; they are never a sign-in factor **(rev 3)**, so the delay
  and the notices always apply. Every completed request replaces the set and shows it once; they are
  also created at registration (with TOTP) and from a full session with a step-up. Legacy
  8-character backup codes stop working at the upgrade (F8).

**OIDC:** a first factor, followed by F3's confirmation QR or, without an eligible key device, by
TOTP (F4); the session is probation. No trusted-IdP exception: an `amr` such as `hwk` (RFC 8176)
names a method, it does not establish phishing-resistant MFA with UV (mapping deferred). OIDC
re-auth is not a step-up; with the `auth_time` check (≤5 min) it can be one factor of an account
request. OIDC-only accounts without TOTP migrate like password-only accounts (F8).

### B.2 Device signatures and approvals (CC10)

**Principle.** Every signature by `K_approve` answers a **challenge** of the `approvals` core in
`server/src/approvals/`. A **kind** decides who may ask (the *requester*) and who consumes the
result; who answers (the *approver*) and with which device; for whom (the *subject*); whether a
secret is transferred; the TTL and limit class; and what consuming does. Every challenge is single
use and is consumed in the same transaction as its effect (B.7). v1.0 defines only its own kinds;
v1.1 adds its agent kinds, side table, desktop rows and approval push in its own migration and never
forks the core.

**Kinds (v1.0):**

| Kind (variants) | Flow | Requester (consumer) | Answering device | `response` | TTL |
|---|---|---|---|---|---|
| `login` (`qr`, `password`, `oidc`) | F2, F3 | login attempt (the PC) | the active key device that scanned, `may_approve` on; for `password` and `oidc` one of the account's own | the PC's QR token (32 bytes) | 2 min from the claim; attempt ≤10 min |
| `enroll` (`own`, `admin`) | F5a on a key device, F5c | enrolment (SM2) | the issuing active key device | the 6 digits the new device shows, typed | code 5 min, 3 tries |
| `device_login` | B.4 | sign-in request | the key device itself | empty | 2 min |
| `device_stepup` | F7, B.2 rule table | the device's full session | the key device itself | empty | 2 min; the grant ≤5 min |
| `enroll_pop` | SM2 claim | enrolment | the new device | empty | the setup token's lifetime |

`login` and `enroll` are **approvals**: they need a transfer, and only they can be denied or
reported. The others are **proofs**. All v1.0 kinds use the `login` limit class. Every challenge
completes within 10 minutes (NIST §3.1.3.2); delays (SM3) live in `account_requests`, never in
`approvals`. Passkey ceremonies are WebAuthn, not part of this format.

**How a challenge is created:**

- **`login` (claim by transfer):** the device sends the scanned token from its own live session
  (`POST /api/approvals/claim`). The server finds the attempt by the token's keyed hash, checks that
  the device is eligible, creates the challenge and returns the bytes to sign with the canonical
  context. A token that matches nothing creates nothing.
- **`enroll`:** after the new device has claimed (SM2), the issuing key device asks for the challenge
  of the enrolment open on its own screen. The server returns the bytes and the context, which name
  the new device's model and the hash of its public key. The user types the code the new device
  shows; the digits are the `response`.
- **Proofs:** the device asks for a challenge from its own session (`device_stepup`, naming the
  action and target), from the sign-in endpoint (`device_login`, naming the device), or with the
  setup token's non-consuming open (`enroll_pop`, SM2).
- v1.0 has no list of open approvals and no approval push. `GET /api/approvals?state=open`,
  `t=approval` and the `approval_pending` / `approval_resolved` events arrive with v1.1's agent
  kinds, whose requester (an agent) cannot show the owner a QR code.

**Signed bytes** (owned by L1a; Android uses them unchanged in v1.0, the desktop in v1.1):

```
"kaiku-sig-v1" ‖ kind[1] ‖ challenge_id[16] ‖ nonce[32] ‖ expires_at i64 BE[8] ‖ lp(origin) ‖
user_id[16] ‖ device_id[16] ‖ context_hash[32] ‖ decision[1] ‖ lp(response)
```

- `nonce`: 32 bytes from the server's CSPRNG per challenge. With single use and the expiry it makes
  every signature unreplayable, including the proofs (rev 3).
- `user_id` is the approver (for `enroll_pop`, the subject); `device_id` the answering device.
- `context_hash` = `SHA-256("kaiku-sig-context-v1" ‖ kind ‖ canonical context)`, where the context
  per kind is:
  - `login`: attempt id, the hash of the collection challenge, the variant, the client description,
    `created_at` and the display fields;
  - `enroll`: enrolment id, subject, variant, the new device's public key (or passkey credential id)
    and model label, and for `admin` the claimant-check method;
  - `device_login`: the sign-in request id that receives the session and the client class;
  - `device_stepup`: the `sid`, the action and the target hash;
  - `enroll_pop`: enrolment id, the new public key and the attestation challenge.
- The server sends the canonical context with the bytes. **The device recomputes `context_hash` from
  the fields it displays** and refuses on a mismatch, so the signature covers exactly what the
  phone showed (ASVS 6.6.2). The server rebuilds every input from its stored rows.
- `decision`: approve, deny or report for approvals; approve for proofs.
- **`response`** is the transferred secret for approvals and is never empty: the 32 scanned token
  bytes, or the ASCII digits the user typed. For proofs it is empty. The server compares it with the
  stored keyed hash in constant time and only then verifies the signature and changes state; a
  signature without the right secret changes nothing. The server sends everything before
  `response`; the device appends it, and never learns a code it did not see.
- **Before signing**, the device checks: `origin` equals its pinned server, `kind` matches the screen
  it shows, `user_id` is its user, `device_id` is itself, `expires_at` lies in the future, and the
  context hash matches. Otherwise it refuses and reports an error.
- `origin` comes from `PUBLIC_URL` and fails closed when unset.
- Kind byte values, `lp()` and every canonical context encoding are fixed in contract C26, with
  golden vectors for a scanned and a typed `response` and for each proof, shared by the server, the
  Android and the v1.1 desktop tests.

**Challenge states:** `issued` → `approved` → `consumed`; `issued` → `denied` | `reported`;
`issued` | `approved` → `expired`. Transitions follow the B.7 rules.

**Data model** (L1a migration):

Every stored secret (transfer token, typed code, setup token, request secret) is kept only as an
HMAC under a server key held outside the database, never in plain or as a plain hash (a lookup
table would reverse a 6-digit code).

- **`approvals`** (the challenges): `id`, `kind`, `variant`, `approver_id`, `subject_id`,
  `device_id`, `requester_kind` (`login_attempt`|`enrollment`|`signin`|`session`), `requester_id`,
  `nonce`, `context_hash`, `display`, the keyed transfer hash, `state`, `created_at`, `expires_at`
  (CHECK ≤ `created_at` + 10 min), `decided_at`, `consumed_at`, `limit_class`; the kind CHECK lists
  the v1.0 kinds only.
- **`approval_devices`:** `id`, `user_id`, `device_id` (per install), `platform` (`android` only),
  `public_key`, attestation chain and reported properties, `label` (model), `state`
  (`pending`|`active`|`revoked`), `enrolled_via` (`own`|`admin`|`request`|`registration`|`cli`),
  `enrollment_id`, timestamps, `revoke_reason`. Created when its enrolment is confirmed, so failed
  enrolments leave nothing. The v1.1 plan's `session_id … ON DELETE SET NULL` trigger is dropped:
  expired-session cleanup would revoke every key device. **`passkeys`** (from LP) gain `state` and
  `enrollment_id`.
- **`login_attempts`** (SM1): `id`, `kind`, the collection-secret hash or S256 challenge, the keyed
  hash of the current QR token, `user_id` (null for QR until the claim), IP, user agent and
  approximate place (display and audit only), `state`, `expires_at`, `consumed_at`. Tokens are
  minted at consumption and never stored.
- **`enrollments`** (SM2): `id`, `variant`, subject, issuer (session, device, request, ticket or
  CLI), keyed hashes of setup token and code, the nonce, the claimed key or credential id and model
  label, `tries`, `claimant_check` (admin: method and note), `request_id`, `state`, timestamps.
- **`account_requests`** (SM3): `id`, `user_id`, `kind` (`self`|`admin`), `factors`,
  `lost_device_ids`, `enrollment_id` (one live child at most), the keyed request-secret hash,
  `state`, `created_at`, `ready_at`, `expires_at`, `cancelled_by`, `cancel_reason`; one open request
  per account (partial unique index).
- **`step_up_grants`** (F7): `sid`, `action`, target hash, `via`, authenticator id, `expires_at`,
  `used_at`. **Desktop flows** (F1) live in Valkey like the OIDC hand-off grants (≤10 min).
- **`sessions`** gain `assurance`, `auth_methods`, `auth_time`, `client_class`, the absolute expiry
  and B.4's refresh fields. **`users`** gain `may_approve`, `password_compromised_at`,
  `recovery_contact_id`, `bootstrap_open`; the `auth_method = 'local'` constraint is relaxed.
- **`security_events`**: append-only, readable by the user, no delete route; a `system_audit_log`
  row for the admins in the same transaction.
- **Not in v1.0** (v1.1, M2): agent kinds, `approval_agent_requests`, `approval_owner_settings`,
  desktop device rows, the approval push and its budgets.

**Remediation (one rule for every action against an event, rev 3):**

| Action | Where | Accepted from | Effects |
|---|---|---|---|
| **No** (deny) | context screen after a scan | the device that claimed the challenge (unsigned) | the challenge and its attempt end |
| **I didn't start this** (report) | context screen after a scan | the claiming device, signed with UV | as deny, plus `reported`; F3: the password is flagged (CC8); OIDC: the user is told to change the IdP password; notice; counts toward B.5 |
| **That wasn't me** | a notice about a sign-in, a new authenticator or a step-up action | the owner action rule | ends the reported session or revokes the reported authenticator, and disables the factors that produced it: the password is flagged, TOTP disabled, the passkey or key device that signed in revoked. A key device that only approved a relayed QR is not revoked (its owner was deceived, the key is sound). Notice; admin copy |
| **Cancel** | a `waiting` or `ready` account request | the owner action rule; the recovery contact from a full session with a step-up; any admin from a full session with a step-up, including the one who started it | SM3 cancel effects |

**Owner action rule.** "That wasn't me" and an owner's Cancel are accepted from:

1. a step-up (F7) by an active strong authenticator of the account that existed before the event
   and is not its subject: a `device_stepup` signature with UV or a passkey assertion, naming the
   action. A device named lost in a request still qualifies: whoever started the request chose that
   name, and an owner who still holds the phone must be able to stop a fake claim;
2. only while the account has no active strong authenticator other than devices named lost in the
   request: a live session of the account that existed before the event. Probation sessions qualify
   here, because the owner has nothing stronger at hand and acting against an event is the safe
   direction.

Never accepted: from the client that started a request (it may only withdraw it, without effects),
or from anything the event created. `may_approve` plays no part. A phone stolen together with its
screen lock is a full key in hostile hands and could cancel too; the remedy is its immediate removal
("Remove device now", or removal by another strong authenticator), after which it can do nothing.

**Delivery:** v1.0 delivers no approvals. Every approval starts on the approving device with a scan
or a typed code, so there is nothing to push and nothing to spam. The PC learns the outcome through
its own attempt (by polling with its collection secret, or a WebSocket channel bound to it).
Security notices (Part C) are pushed, but none of them carries an action that approves anything.

**What the v1.1 owner-approvals spec must change** (conflicts K1–K18): the generic core, this format
and one device table with `platform` ship in v1.0; v1.1 adds its kinds, side table and desktop rows
(agent kinds only) in its own migration (K1, K2, K10). Devices are revoked only on explicit removal,
with no SET NULL trigger (K3); UV is required, PIN-only API 26–29 phones use the short window (K4);
every user enrols, and v1.1 Task 4 is dropped (K5); `approver_id` + `subject_id`, a requester enum,
tokens minted at consumption (K6, K7); per-kind TTLs with a DB bound of 10 min (K8); separate limit
classes and an agent push budget (K9); the remediation rule above and notices in the Kaiku DM, with
`t=approval_device` dropped (K11, K12); login kinds have no TOTP page, agent kinds keep theirs (K13);
no number matching, desktop passkeys via the system browser, stale statements corrected (K14–K17);
C24 accepts a passkey or a `device_stepup` and no longer password + TOTP or OIDC (K18).

### B.3 Flows

Every flow lists the session it produces. "Notice" means a Part C notice to all of the user's
devices, and for high-severity events also to the recovery contact and the admins.

#### F1. Passkey sign-in → full session

1. **Web:** passkey autofill in the username field, or "Sign in with passkey". The login page lists
   passkeys first.
2. **Desktop (Windows, macOS, Linux) (rev 3):**
   - The app asks the server for a flow (purpose `sign_in`, or `step_up` with action and target from
     its own session), sending `S256(verifier)` and its loopback port, with `X-Kaiku-Client:
     desktop` as the OIDC desktop start does. The app opens the system browser at the returned
     ceremony URL.
   - The ceremony page names the purpose and the app ("Sign in to the Kaiku desktop app on this
     computer", or "Allow the Kaiku desktop app on this computer to remove the device 'Pixel 8'")
     and says "If someone sent you this link, close this page."
   - When the assertion verifies, the server stores a **pending result** for the flow (60 s) and
     sends the browser to `http://127.0.0.1:<port>/callback` by an automatically submitted form POST
     that carries a one-time code. The code never appears in an address bar, on the page, in a
     redirect URL or in any polling response; `no-store`, `no-referrer`.
   - The app redeems code + verifier, for a step-up with the access token of the session that
     started the flow. Only then does the server create the session or the step-up grant, after
     re-checking the account, the passkey and (step-up) that session. A pending result that is not
     redeemed expires and changes nothing. Tokens stay in Rust.
   - The existing OIDC desktop hand-off moves to the same form-POST delivery.

   Why this binds the ceremony to the desktop that started it: an attacker who starts a flow on
   their own computer and forwards the link gets nothing from the victim's genuine assertion. The
   result lands on the victim's own loopback, and nothing is applied before redemption: PKCE (RFC
   9700 §2.1.1) protects only the redemption, so no effect may precede it. A loopback-first start
   and a flow-bound cookie were considered and not adopted: whoever starts the flow can forward the
   start link before any cookie exists, so they bind nothing that this does not (appendix). Residual
   risk: B.6.2.

   On Linux, Chromium-based browsers offer hybrid (phone via Bluetooth); Firefox does not, so the
   dialog says "No phone option? Use 'Sign in with phone' (F2) or a security key."
3. **Android:** Credential Manager (B.1).
4. The server verifies origin, RP ID hash, UV, BE, the challenge and the signature, and records BS
   and last use.
5. The session is full.

#### F2. Sign in with phone: QR login (was B3) → probation

1. The PC opens "Sign in with your phone" and creates a login attempt (SM1).
   - It keeps a **collection secret** that never appears in the QR, a URL or the page:
     - desktop and second phones send only `S256(verifier)` and keep the verifier (desktop: in Rust
       memory);
     - the web client receives it as an HttpOnly, `Secure`, `SameSite=Strict` cookie scoped to the
       attempt endpoints.
   - The QR code holds the server origin and a 256-bit token. It rotates every 60 s, one live
     attempt per screen, and the PC replaces it automatically. A rotation does not reset any failure
     counter (NIST §3.1.3.2).
   - There is no typed code. A phone that cannot scan uses a passkey or the TOTP fallback.
2. The user scans with a key device. The app checks the origin, then claims the attempt from its own
   session (SM1). The server binds the attempt to that user and creates a `login` challenge. With
   `may_approve` off, the app refuses here. A stale code shows "This code has just changed – scan
   the new one".
3. The **context screen** shows what is signing in (client kind first: "Kaiku desktop on Windows",
   "Firefox on Linux"), the server name, the time and, where the operator installed a local GeoIP
   database, the approximate place (Kaiku no longer sends sign-in IPs to an external GeoIP
   service). The question is "Are you signing in on a PC in front of you right now?", with "Yes,
   sign in", a prominent "No", and a smaller "I didn't start this" (RFC 10027 §6.1.14). The place is
   a hint, never a control (§6.1.1).
4. UV and signature, with the scanned token as `response`, then approval. Backing out releases the
   claim (SM1).
5. The PC collects with its collection secret, and the server mints tokens at consumption. A
   bystander who photographs the QR code, or anyone without the PC's secret, gets nothing. A
   phishing page that shows the victim the attacker's own code is not stopped by this (CC3).
6. The session is **probation** for its whole lifetime. The notice carries "That wasn't me", which
   ends exactly that session (RFC 10027 §6.1.6). For full access the user signs in with a passkey.

Why this is not called phishing-resistant: a live phishing page can proxy the real QR code to a
victim, who scans it believing it is genuine. The scan is a real transfer (NIST §3.1.3) but not
origin-bound (§3.2.5). The combined mitigations are those of CC3, plus `may_approve` off for family
accounts and origin pinning on the phone; the rest is residual risk (B.6.1). Passkeys over hybrid
are listed first wherever they work (RFC 10027 §6.2.3.5: FIDO cross-device authentication SHOULD be
used whenever the devices are capable).

#### F3. Password (or OIDC) + phone confirmation (was B4) → probation

This flow stays, as the maintainer asked ("username + password, then confirmed in the app"); the
phone is never prompted.

1. Username + password. Per-account and per-IP throttles apply (B.5).
   - If the username is unknown or the password path is closed for the account (flagged; disabled
     after 100 failures; a password-only account waiting for an admin setup; or no password at
     all), every password request gets the same neutral answer, whether the password is right or
     wrong, with the same status and in the same time as a wrong password: "Password sign-in is not
     available for this account right now. Use a passkey or 'Sign in with phone', or ask an admin."
     The answer is never an oracle for an account or a correct password.
2. Password correct and the account has an active key device with `may_approve` on: the server
   creates a login attempt (`password`, SM1) and returns `confirmation_required`. The PC keeps its
   collection secret (as in F2) and shows a **confirmation QR**: "Open Kaiku on your phone and scan
   this code." Nothing is sent to the phone.
   - No such key device: F4 if the account has TOTP, otherwise the neutral answer.
3. The user opens Kaiku and taps Scan (one tap from the chat list in the full app), scans the
   confirmation QR, sees the context screen (F2 step 3, plus "with your password"), and approves
   with UV.
4. "I didn't start this": report (B.2). The password is flagged (CC8). Notice.
5. The confirmation screen also offers "Phone not at hand?", which continues the same attempt in F4
   when the account has TOTP, also while a phone holds the claim (SM1).
6. If the attempt ends without a confirmation (expired or abandoned), the account gets the notice
   "Someone entered your password but did not finish signing in", coalesced (B.5; OWASP MFA cheat
   sheet). Its "That wasn't me" flags the password.
7. Success: a **probation** session plus a notice with "That wasn't me".
8. **OIDC variant:** after the IdP callback, steps 2–7 apply. "That wasn't me" after an OIDC login
   tells the user to change the IdP password, since there is no local password to flag.

Why no push **(design call)**: approving needs the code on the PC screen anyway, so a push would
only save one tap, and without it there is no push bombing at all. The owner still learns about a
correct password that was not confirmed (step 6). Passwordless accounts cannot be probed for a
password at all.

#### F4. TOTP fallback → probation

1. **Users without an eligible key device:** password (or OIDC), then a TOTP code. This is their
   normal path (see Q2).
2. **Users with a key device:** "Phone not at hand?" on the confirmation screen.
3. Never TOTP alone, and never a recovery code: recovery codes only start an account request (F6).
4. Result: a **probation** session.
   - Users with key devices get a notice on them with "That wasn't me".
   - A correct password followed by repeated TOTP failures sends "Someone entered your correct
     password" (OWASP MFA cheat sheet; ASVS 6.3.5).

#### F5. Device setup

**F5a. Add a phone or a passkey from something already trusted ("Add a phone") → full.** The setup
token can only come from a device or session that is already trusted, so an attacker cannot
produce one to relay.

1. **Start** (SM2 `own`, `request`, `registration`) on one of:
   - an active key device ("Add a phone"), with a `device_stepup` (`may_approve` on);
   - a full PC session, with a passkey step-up "add a key device or passkey" (F7);
   - a `ready` account request or a registration ticket (F6, F8).

   The starting screen shows a setup QR (origin + 256-bit token, 10 min, single use) for a phone, or
   offers "Share link" for a browser on another computer, which the user sends to themselves.
2. The new device opens it. An unconfigured phone shows the server name first; opening never
   consumes the token. The phone checks the screen lock and the hardware keystore (B.1), generates
   `K_approve` and claims the token with its public key and an `enroll_pop` signature; a browser
   registers a passkey instead (SM2).
3. The server sends a 6-digit code *only to the claiming device*, which shows it. The starting screen
   names the claiming device ("Pixel 8 wants to become your key") and asks for the code, with "Type
   the code from the device in your hand. Never type a code someone tells you." On a key device the
   entry is an `enroll` approval with UV; on a PC, the step-up grant authorises it. The entry can be
   corrected before it is sent; three wrong entries end the enrolment. If someone else claimed the
   token first (a photographed screen), the right device shows "already used" and nothing matches;
   the user cancels and starts again.
4. The authenticator is active. A new phone then signs in with device-key sign-in (full, B.4); a new
   passkey signs in with itself. Notice with "That wasn't me" to all devices, copied to the admins
   and the recovery contact.

**Binding strength (NIST §4.1.2.2):** the 256-bit, single-use, 10-minute setup token is the binding
code, and the `enroll_pop` signature binds the claimed key to it. The 6-digit code is an additional
check that the device being bound is the one in front of the person at the starting screen; it is
never sufficient alone and is never accepted without the token's claim. Full is safe here because
the token exists only because a trusted device, or a full session with a fresh step-up, asked for
it.

**F5c. Admin setup for another user (was B1): one protocol, in person or remote (rev 3).**

1. In the full app on their **own active key device**, the admin creates the account (no password)
   or picks an existing one, and taps "Set up a phone". Each setup code needs a `device_stepup` (the
   admin re-auth of Decision 3). The target is never the acting admin; another admin's account may
   be a target.
2. Before any code exists, the admin records the **claimant check**: "in person" (the person is with
   the admin), or "called back" (the admin ended the incoming contact and called the person back on
   a number or account known before this request, and recognised them), plus a short note. Both go
   into the audit log.
3. The admin's phone shows the setup QR (in person), or shares the setup link over the checked
   channel (remote, or a person without a phone, whose computer opens it). It never contains a
   password, a refresh token or any long-lived secret. On a phone the link is an Android App Link;
   where Google cannot verify `assetlinks.json`, the link's page offers "Open in Kaiku". Opening the
   link never consumes it (link previews are harmless), and the person's name is fetched only after
   the phone has shown the server name.
4. The new phone shows "Set up Kaiku for <name> on <server>?", checks the screen lock (CC1; the
   person chooses a PIN with the admin if needed), generates its key and claims the token (SM2).
5. The phone shows a 6-digit code (sent only to it). The admin **types** it on the setup screen of
   their own key device, with UV (`enroll` approval, variant `admin`; this is Decision 3's
   confirmation). A transfer, not compare-and-tap. Three tries. In person, the admin reads it off
   the phone; remotely, the person reads it out on the call from step 2.
6. **New account** (created in this setup, so there is nothing to take over): active at once; notice
   to all other admins. **Existing account**, in person or remote, setup or recovery: an admin
   account request (SM3) waits 24 h, with notices and Cancel to the account's devices, the recovery
   contact and all admins; the new phone shows when it will be ready and has no session until then,
   and activates automatically at the end. Revision 2's in-person exceptions (same network, owner
   evidence in the last 7 days) are gone: neither was a reliable signal. An honest phone upgrade
   waits a day while the old phone keeps working.
7. Once active, the new phone signs in with device-key sign-in (full). In **recovery mode** the
   admin has named the lost devices, which are revoked at completion; a stolen one is removed at
   once with "Remove device now" (F6).
8. The setup is audit-logged and copied to every admin's security log. `may_approve` stays off
   unless the admin turns it on. The admin names the account's recovery contact (another family
   member who is not an admin).
9. **Person without a phone:** the setup link opened in their computer's browser registers a
   passkey instead; that browser shows the code, and the same rules apply.

**F5d. Passwordless accounts (was B2).** Accounts set up by F5c have no password. They sign in with
the key device itself, a passkey, or QR login on another device if `may_approve` is on. Any user may
remove their password from a full session with a step-up once they have a strong authenticator. The
`auth_method = 'local'` constraint gets a relaxed rule or a new method (builder's choice).

#### F6. Recovery (was B6) → never weaker than sign-in

Three ways, plus the host CLI, in order of preference (rev 3: the bootstrap ceremony is one kind of
account request, not a fourth way):

1. **Another key device or a passkey:** sign in with it (full), add the new phone (F5a), and remove
   the lost device with a step-up.
2. **Self-service account request (SM3):**
   - An entry "I lost my phone / forgot my password" on the sign-in page, separate from sign-in:
     username + one recovery code + one of the password, a TOTP code or a fresh OIDC sign-in (NIST
     §4.2.2.2: a recovery code plus a bound authenticator; ASVS 6.4.3). The recovery code is
     checked first and consumed; without a valid one nothing else is checked (B.5).
   - It works while the password path is closed, because the recovery code carries it; the flagged
     password itself is refused.
   - The client gets a **request secret**, not a session: no chat, no settings. It keeps it in an
     HttpOnly, `Secure`, `SameSite=Strict` cookie scoped to the request endpoints (web), the keyring
     (desktop) or Keystore-wrapped storage (Android), for the request's lifetime. With a lost secret
     the user waits for the request to expire and starts again with another code.
   - The request waits 24 h and notifies every address; the cancel paths and effects are those of
     SM3.
   - When the request is ready, the same client may, within 24 h: enrol one strong authenticator (a
     passkey in that browser, or a phone through a setup QR with the F5a mechanics), set a new
     password, renew TOTP and name a recovery contact. It always receives new recovery codes. A
     phone-less user can thus replace a forgotten password or a lost authenticator app without a
     passkey.
   - The result is full where a strong authenticator was enrolled. The recovery code, the second
     factor, the wait and the absence of a cancel authorise it; time alone never does (CC4).
   - Passwordless users without recovery codes recover only through an admin, and the UI says so.
   - The entry also says: "Phone stolen, and someone may know its screen lock? Ask an admin to
     remove it now." A thief with the screen lock holds a full key, which a 24-h request cannot beat.
3. **Admin recovery, in person or remote** (F5c in recovery mode, SM3): the admin names the lost
   devices; they are revoked when the request completes, not at the start. "Remove device now"
   (immediate, audited, notified to the account, including the removed device, and to the admins;
   NIST §4.3) is separate, and removing a device never shortens or skips a delay.
4. **Host CLI** (last resort; needs root on the host): it issues a one-time setup link for any
   account (active at once, since the operator is trusted), removes devices, resets throttles, the
   compromise flag and disabled TOTP, and ends sessions. Every use is audited and sends the same
   notices to the account, its recovery contact and all admins.

**A lost sole phone is removed without that phone** (NIST §4.3; ASVS 6.5.6): by an admin at once,
by another strong authenticator with a step-up, or when an account request completes.

#### F7. Step-up (re-auth, C24) → an action-bound grant

- **What it is:** a grant for one named action and target (for example "remove the device Pixel
  8"), bound to the session's `sid`, valid for at most 5 minutes, consumed by that action. One
  step-up covers one action.
- **Methods (rev 3):**
  - a passkey assertion naming the action: in the page on the web, through the F1 desktop flow,
    or through Credential Manager on Android;
  - on a key device, a `device_stepup` signature with UV over the action and target.

  Revision 2's `reauth` approval (the phone scanning an action QR for a PC session) is cut: every
  full PC session was created by a passkey, which can step up directly.
- **Not step-ups:** a password, TOTP, a recovery code or OIDC, alone or together (ASVS 7.5.1).
- Only full sessions step up.
- **At use**, the server re-checks that the session is live and full, that the authenticator that
  produced the grant is still active, and that action and target match; the grant is consumed in the
  action's transaction (B.7).
- **Required for** (ASVS 7.5.1, 7.5.2):
  - adding or removing key devices, passkeys or TOTP;
  - setting, changing or removing the password;
  - creating recovery codes;
  - ending other sessions;
  - setup codes;
  - admin elevation and every admin action on another account;
  - naming a recovery contact;
  - Cancel and "That wasn't me", except under part 2 of the owner action rule (B.2);
  - deleting the account.

  Ending a key device's session is shown as "Remove device". `may_approve` is changed only by an
  admin, with the admin's step-up.
- The notice after a step-up action names what was done.

#### F8. Migration and new accounts (v1.0)

The family server already has accounts (RC testing): some have password + TOTP, some a password
only. None has a key device.

- **At the upgrade:**
  - every existing session becomes **probation** (sessions minted by the old unconfirmed QR redeem
    cannot be told apart, so none is trusted);
  - sessions older than the absolute caps (B.4) end, and so do the sessions of password-only and
    OIDC-only (no TOTP) accounts;
  - legacy backup codes stop working.
- **Bootstrap entitlement (rev 3):** every account that has a password (or OIDC) and TOTP but no
  strong authenticator gets `bootstrap_open`.
  - With it, the account may start a **bootstrap request** (SM3, "Secure my account", from the
    sign-in page or a session): the password or a fresh OIDC sign-in plus a TOTP code. It waits 24 h
    like every account request, and the owner's older sessions may cancel it (B.2 owner rule,
    part 2). On completion it can enrol the first strong authenticator, set a new password or renew
    TOTP, and always yields recovery codes.
  - The entitlement is consumed, under the account lock, when any strong authenticator of the
    account becomes active by any path, and an open bootstrap request is then void. It is never
    restored: after revocation or a lost device the account recovers through F6. A flagged password
    or disabled TOTP cannot be used.
- **Password-only and OIDC-only accounts** (without TOTP): password sign-in gets the neutral answer
  (F3). Their first strong authenticator comes from an admin setup (F5c). The password may already
  be known to someone else, so it alone must not bind the authenticator that will own the account
  (stricter than NIST §4.1.2.1, which would allow it at AAL1).
- **Admins:** as part of the upgrade, the operator gives at least one admin a strong authenticator
  with the host CLI (a setup link, active at once). Further admins use a bootstrap request or an
  admin setup by that admin. Revision 2's open-ended admin exception to the bootstrap window is cut.
- **Phone-less users** keep password + TOTP (probation). "Secure my account" gives them recovery
  codes and offers a passkey first (see Q2).
- **New accounts** (registration under `registration_policy`, or the first admin on a fresh server):
  registration yields a **registration ticket** (30 min), not a session. It binds a passkey on that
  client (full), a key device with the F5a mechanics (full), or TOTP (the account then signs in
  through F4 on probation and gets `bootstrap_open`). An account that binds nothing within 30 minutes
  is deleted. The first admin must bind a passkey or a key device, because elevation needs a full
  session.
- **Admin upgrade checklist:** set `PUBLIC_URL` first (the server fails closed without it);
  password-only admins enable TOTP or register a passkey before the upgrade, otherwise only the CLI
  helps; give the first admin a phone or passkey with the CLI; name a recovery contact for every
  family account (an admin request needs one on a single-admin server, SM3); consider a second
  admin.
- **Removed:** `/auth/qr/create`, `/auth/qr/redeem` (a session without confirmation),
  `/auth/forgot-password`, `/auth/reset-password`, and the Android `OidcHandler` branch that accepts
  tokens from a deep link.
- **Admins see per user:** strong authenticators and their state, TOTP (and whether disabled),
  recovery codes left, password set, flagged or disabled, `bootstrap_open`, the recovery contact,
  open requests, and the assurance of open sessions.

### B.4 Sessions

**Assurance classes (CC4):**

| Class | Created by | May |
|---|---|---|
| `full` | a passkey sign-in (UV); a device-key sign-in of an active key device | everything, with an action-bound step-up where F7 requires it |
| `probation` | QR login; password or OIDC + phone; TOTP fallback; every session that existed before the upgrade | chat, calls, profile name and photo, notification settings, read the security log, sign itself out; act under part 2 of the owner action rule (B.2) |

Probation cannot change sign-in methods (passkeys, key devices, TOTP, recovery codes, password), end
other sessions or devices, deny or report approvals, act under part 1 of the owner rule, create
setup codes, elevate as admin, approve anything or delete the account. A session's assurance never
changes (rev 3); the probation banner offers "Sign in with a passkey", which runs F1 and replaces
the probation session with a new full one. Two server guards enforce this, "full session" and
"step-up grant for this action"; every route in F7's list uses both, except the owner rule's
part 2.

**Lifetimes (CC6):**

| | Browser | Desktop | Android |
|---|---|---|---|
| Access JWT | 15 min | 15 min | 15 min |
| Refresh, inactivity (sliding) | 7 d | 7 d | 7 d |
| Absolute, from `created_at` **(design call)** | 30 d | 90 d | 90 d, then device-key sign-in with UV |

- **Documented deviation** (ASVS 7.1.1): NIST §2.2.3 says reauthentication at AAL2 SHOULD happen at
  least every 24 h and after 1 h of inactivity. Kaiku is a messaging and calling client used daily;
  it compensates with reuse detection, notices with revocation, probation for relayable sign-ins and
  an action-bound step-up for every sensitive action. The absolute caps hold for every client (NIST
  §5.1).
- **Concurrent sessions** (ASVS 7.1.2) **(design call)**: at most 50 live sessions per account
  (configurable); a new sign-in beyond that ends the least recently used session, with a notice.
- **Android renewal and calls:** the app renews at a normal app open shortly before the cap, so it
  never asks for UV during an incoming call; after a lapse the prompt is the phone's own screen lock
  ("Confirm it's you"), never a password. Call-wake pushes reach an active key device even when its
  session has lapsed; after "Accept" it asks "Confirm it's you" and joins the call.

**Ending sessions (rev 3: at once):** every end (sign-out, "That wasn't me", reuse, revocation of
its authenticator, recovery clean-up, ban, session limit) goes through `revocation::end_sessions`,
which puts the `sid` on the existing denylist (checked on every authenticated request and by the
WebSocket, failing closed) and closes the session's WebSocket and voice connections, so the rest of
its access JWT's lifetime is useless. Signing out the current session is always allowed; on a key
device "Sign out" keeps the key (the next open asks "Confirm it's you"), "Sign out and remove this
phone" revokes it (UV). Ending other sessions needs a full session and a step-up (ASVS 7.5.2).
After an authenticator change, the UI offers "Sign out everywhere else" (ASVS 7.4.3).

**Refresh rotation with reuse detection** (RFC 9700 §2.2.2, §4.14.2):

- Refresh rotates on every use, in place (the stable `sid` stays). The server recognises every
  token it has issued for a session until the session ends, without letting a forged token end
  someone else's session: it keeps the hashes of spent tokens, or the token carries `sid` and
  generation authenticated with a server key held outside the database (builder's choice).
- **Lost response (rev 3):** the immediate predecessor, presented within 10 s of its rotation, gets
  the **same** successor again (a recomputation with the authenticated format; with spent hashes the
  successor is kept encrypted for those 10 s), at most once per session per 24 h.
- Any other earlier token, or a second retry within 24 h, is reuse: the session ends with the
  notice "A session was ended because its key was used twice". An unrecognised token gets 401 and
  changes nothing.
- Clients serialise refreshes: Web Locks across tabs, one Rust task on the desktop, a single flight
  on Android.
- Documented exception: a thief who presents a predecessor within those 10 s receives the same
  successor; the next rotation by either side turns the other copy into an earlier token, and the
  session ends (B.6.10).

**Device-key sign-in** (K3; NIST §5.1; SM4): after a lapse (inactivity, absolute cap or a plain
sign-out on that device), and for a newly enrolled phone, the app asks the sign-in endpoint for a
`device_login` challenge naming its device and signs it with UV. Under the account lock the server
checks that the device is `active` and the account not banned, consumes the challenge and mints a
full session; rate-limited and audited. A `pending` device gets "ready at …"; a revoked device or a
banned account is refused. Never silent, never with a key that needs no UV.

**Token storage:** Android keeps tokens in Keystore-wrapped storage (an AES key in the TEE, excluded
from backups), replacing `security-crypto`; the `K_session` proof is deferred, since stealing a
token from that storage already needs code inside the app, which could use a session key as well.
The browser's refresh cookie gets the `__Secure-` prefix, HttpOnly and `SameSite=Strict` (DBSC
later). The desktop uses the keyring as today.

### B.5 Limits and fatigue (was B5)

All values are configurable; the defaults below are the builder's starting point. All counters live
in Valkey, so they survive restarts. All rejects, delays and disables are audit-logged and visible to
admins. ASVS 6.1.1 asks that lockout abuse be documented: these are the rules.

- **No approval prompts** (CC2): nothing reaches a phone unless the phone scans first, so there is
  nothing to bomb.
- **Per-account throttling** (CC9; NIST §3.2.2):
  - separate counters for the password and for TOTP, across all IPs; password tries against an
    unknown username or an account without a password count like failures and get the same answer;
  - after 5 consecutive failures, an increasing delay per account (from 30 s, doubling, at most
    1 h);
  - at 20 consecutive failures, a notice to the user and the admins ("Someone is trying to guess
    …"), coalesced;
  - at 100 consecutive failures, the password or TOTP is disabled for the account (NIST §3.2.2). It
    is re-enabled by a successful sign-in of the account by another method, a completed account
    request, or an admin;
  - the message suggests a passkey or QR sign-in.
- **Recovery entry (rev 3):** the recovery code is checked first. A request without a valid recovery
  code gets the same answer and timing as one for an unknown username, adds a per-account delay with
  the same curve, and never disables anything: a 64-bit code cannot be guessed in any practical
  number of tries, and a disable would hand attackers a lockout lever. A valid code is consumed even
  when the second factor then fails, and that failure counts on the second factor's counter.
- Passkeys, QR login, device-key sign-in and existing sessions are never affected, so a hostile
  guesser cannot lock anyone out of the strong paths. **Lockout abuse:** someone who knows the
  username of a phone-less user without a passkey can slow that user's password sign-in and, after
  about four days of guessing at the growing delay, disable the password. The user and the admins
  are told at 20 failures; an account request (recovery code + TOTP) or an admin brings it back
  (see Q2).
- **Per IP:**
  - the existing `failed_auth` limiter, keyed by the client IP as the trusted-proxy rule
    (Constraints) determines it;
  - it never applies to QR login, passkeys, device-key sign-in or signed-in sessions, so an attacker
    on the family's shared home IP cannot lock everyone out;
  - the public endpoints that create login attempts, passkey challenges, desktop flows, enrolment
    claims and account requests have their own request limits.
- **Typed enrolment codes:** three wrong entries end that enrolment. There is no per-account counter
  (rev 3): the code is typed on the trusted side, so nobody outside can guess it.
- **Open limits:** at most 5 open setup codes per issuer; one live login attempt per PC screen; one
  open account request per account.
- **Notices:** repeated notices of the same type for the same account within 10 minutes are
  coalesced into one with a count, which still lists every affected session, authenticator or
  request with its own action; the audit rows stay individual.
- **v1.1:** `login` and `agent` limit classes have separate counters; agent approval pushes get their
  own budget in Valkey (6 per 10 min per device). Agent denials can never pause or slow human
  sign-ins (CC9, K9).

### B.6 Residual risks and documented deviations

1. **Real-time relay of F2 and F3** by a phishing page that proxies the QR code. It yields a
   probation session, which can read the user's chats and write as the user until it is noticed and
   ended. It never becomes full: full control needs a passkey or the key device itself (CC4).
   Limited further by CC3 and `may_approve`. Passkeys are the phishing-resistant path (NIST §2.2.2 is
   met by *offering* them).
2. **Forwarded desktop ceremony:** an attacker starts a desktop sign-in or step-up and forwards the
   link; the victim completes the genuine passkey ceremony. The result goes to the victim's own
   loopback by a form POST and is never shown, so the attacker gets something only if the victim
   copies it out of the browser's developer tools and sends it back. The page names the app and
   the action and warns against forwarded links. Software already running on the user's computer
   is out of scope, as for the existing desktop hand-off (`server/src/auth/AGENTS.md`; RFC 8252
   §8.1).
3. **Dictation in the enrolment direction:** a scammer who sees the victim's setup QR (screen
   sharing in a "support" call) scans it with their own phone and reads that phone's code to the
   victim, who types it. Limited by the claiming device's name on the typing screen, the warning
   text, `may_approve` off for family accounts (no "Add a phone"), and the notice with "That wasn't
   me". In F5c the person reads the code to the admin by design; the claimant check covers that.
4. **A social-engineered admin.** The recorded claimant check, the 24 h wait for every existing
   account, cancel paths that do not run through that admin (SM3: independent canceller), and
   notices to the account's devices (including those named lost), the recovery contact and the other
   admins. A new account set up remotely has no wait; an impostor posing as a new family member is
   stopped only by the claimant check. Someone physically present who deceives the admin is not
   covered.
5. **The admin's phone is the root of trust** for every admin setup and recovery. Admins should use
   a phone with a fingerprint reader and StrongBox, and a second admin.
6. **Stolen or phished request factors:** a recovery code plus a second factor, or (with the
   bootstrap entitlement) a password plus TOTP, starts a request that completes after 24 h unless
   someone cancels it. A cancel disables the factors used, so an owner who cancelled, or whose
   request was cancelled wrongly, needs an admin to regain password or TOTP sign-in.
7. **A stolen phone plus a known screen lock** equals the key, as with any phone authenticator: until
   it is removed it can do whatever a key device can, including cancelling a request that names it
   lost. The remedy is immediate removal by an admin ("Remove device now") or by another strong
   authenticator of the owner (F6), then a request.
8. **Full access on a PC needs a passkey.** On Linux with Firefox (no hybrid) that means a USB
   security key or a Chromium-based browser; otherwise QR login (probation) is the PC path and
   security and admin tasks are done in the Android app (Q5).
9. **Lockout abuse** of phone-less users without a passkey (B.5).
10. **Refresh retry window:** a refresh token stolen within 10 s of its rotation gives the thief the
    successor until the next rotation (B.4).
11. **All notification addresses live on this server.** The user's devices, the recovery contact
    and the admins are separate addresses (NIST §4.6), but a server that is down notifies nobody;
    it cannot complete a request either.
12. **Deviations:**
    - session lifetimes (B.4, NIST §2.2.3);
    - PIN-only API 26–29 phones use a 10 s authentication window instead of a per-use activation
      (NIST §3.1.3 "each use"; B.1);
    - Android key attestation is not verified in v1.0 (B.1);
    - Android refresh tokens are not sender-constrained in v1.0; RFC 9700 §2.2.2 is met by rotation
      with reuse detection (B.4).

### B.7 State machines (normative)

Each machine lists its states, the transitions, who may trigger each one, the timers and what a
cancel does. Four rules apply to all of them (rev 3):

1. **One transaction per transition, serialised per account.** Every transition first locks the
   account's `users` row (`SELECT … FOR UPDATE`), then re-checks every precondition inside the
   transaction: the object's state and deadline; its parent (attempt, enrolment, request) and that
   parent's state and deadline; the account (not banned, not deleted); and the authenticator,
   session or grant that authorises the transition (still active, live and full where required). It
   consumes the challenge or grant it uses, applies the effect and writes the audit row. A
   transition whose preconditions no longer hold fails without effect; of two racing transitions,
   the first to commit wins.
2. **Invalidation cascades.** Ending a session voids its step-up grants, its desktop flows and the
   enrolments it issued. Revoking an authenticator ends its sessions and voids its open challenges
   and the grants it produced. Cancelling, expiring or voiding a request voids its child enrolment
   and its pending authenticator.
3. **Timers** are applied by a sweep every minute under rule 1.
4. **Consumption re-checks authorisation** (ASVS 7.4.1): an approved attempt is collected only while
   the approving device is still active; a grant is used only while its session and authenticator
   are; an enrolment is confirmed only while its issuer is valid.

#### SM1. Login attempt (F2, F3, F4)

States: `open`, `claimed`, `approved`, `consumed`; terminal: `denied`, `reported`, `cancelled`,
`expired`.

| From | Event | Who may trigger | To | Rules and timers |
|---|---|---|---|---|
| – | create | F2: any client ("Sign in with phone"), request-limited; F3 and F4: the server after a correct first factor | `open` | the PC holds the collection secret; the attempt lives ≤10 min; the QR token rotates every 60 s and older tokens die |
| `open` | claim with the current token | an active key device, from its own live session, `may_approve` on; for F3 it must belong to the attempt's user | `claimed` | creates the `login` challenge (B.2); one claim at a time |
| `claimed` | approve: UV signature with the token as `response` | the claiming device | `approved` | within the challenge's 2 min |
| `claimed` | No / I didn't start this (UV) | the claiming device | `denied` / `reported` | report: F3 flags the password; notice |
| `claimed` | release (back, prompt dismissed) or the challenge expires | the claiming device, server | `open` | the challenge is void; a new scan may claim |
| `open`, `claimed` | TOTP code together with the collection secret | the PC; F3 and F4 attempts only | `consumed` | password not flagged, TOTP not disabled; per-account TOTP counter (B.5); a claimed challenge is void; tokens minted now; the session is probation |
| `approved` | collect with the collection secret | the PC | `consumed` | ≤60 s after approval; the approving device must still be active; tokens minted now; the session is probation |
| `open`, `claimed` | close the screen | the PC | `cancelled` | an F3 attempt that ends unconfirmed sends one coalesced notice |
| any non-terminal | timer | server | `expired` | as `cancelled` for notices |

#### SM2. Enrolment of one key device or passkey

Variants and their preconditions:

| Variant | Issued by | Precondition | The code is typed on |
|---|---|---|---|
| `own` (F5a) | an active key device, or a full PC session | key device: `device_stepup`, `may_approve` on; PC: a passkey step-up grant "add a key device or passkey" | the issuing screen; on a key device as an `enroll` approval with UV |
| `admin` (F5c) | an admin's own active key device | `device_stepup` per code; the target is not this admin; claimant check recorded; for an existing account an independent canceller (SM3) | the issuing key device, as an `enroll` approval with UV |
| `request` (F6, F8) | a `ready` account request | the request secret | the request client |
| `registration` (F8) | a registration ticket | ticket ≤30 min old | the ticket client |
| `cli` (F6) | the host CLI | root on the host | the CLI |

| From | Event | Who may trigger | To | Rules and timers |
|---|---|---|---|---|
| – | issue | per variant | `issued` | setup token 256-bit, single use, 10 min; ≤5 open per issuer; a request has at most one live child, and issuing a new one voids the old |
| `issued` | open (GET) | anyone with the token | `issued` | returns the server name and the enrolment nonce; consumes nothing |
| `issued` | claim (POST): public key, attestation chain and `enroll_pop` signature with UV, or a passkey registration on another client | the new device | `claimed` | screen lock and hardware keystore checked first; a second claim gets "already used"; the 6-digit code goes to the claimer only |
| `issued` | passkey registration on the issuing client itself (`own` in a full session, `request`, `registration`) | the issuing client | `done` | no code |
| `claimed` | confirm with the right code | the issuer (variant table) | `done` | the code is valid 5 min; a wrong code counts; the third wrong code → `failed` |
| `issued`, `claimed` | cancel | the issuer; for `admin` also any admin; for `request` the request's cancel | `cancelled` | the claiming device shows "Setup cancelled" |
| `issued`, `claimed` | timer, or the issuer is no longer valid (rule 2) | server | `expired` | |

At `done`, in the same transaction, the authenticator row is created: `active`, except for an
`admin` enrolment of an existing account, which creates it `pending` together with an admin account
request (SM3). An active key device then signs in with device-key sign-in; a new passkey signs in
with itself. Notices go to all addresses. For a `request` child, `done` is part of the request's
completion transaction.

#### SM3. Account request (self-service recovery, bootstrap, admin setup or recovery)

States: `waiting`, `ready`, `completed`; terminal: `cancelled`, `withdrawn`, `expired`, `void`.

| From | Event | Who may trigger | To | Rules and timers |
|---|---|---|---|---|
| – | start (`self`) at the recovery entry or "Secure my account" | anyone (request-limited; same answer for unknown usernames) | `waiting` | the recovery code is checked first and consumed (B.5); factors: recovery code + password, TOTP or fresh OIDC, or, while `bootstrap_open`, password or fresh OIDC + TOTP; a flagged password or disabled TOTP is refused; the requester may name devices lost; one open request per account; independent canceller required; the client keeps the request secret; `ready_at` = start + delay |
| – | start (`admin`): an `admin` enrolment for an existing account reaches `done` (SM2) | the admin | `waiting` | the pending authenticator and the devices named lost are recorded; independent canceller checked at issue and again here |
| `waiting` | timer | server | `ready` (`self`) or `completed` (`admin`, by the completion transaction) | notice |
| `ready` | complete: the requester's choices | the request client, with the request secret | `completed` | completion transaction; at most 24 h after `ready` |
| `waiting`, `ready` | Cancel | the B.2 rule: owner action rule, recovery contact, any admin | `cancelled` | cancel effects |
| `waiting`, `ready` | withdraw | the request client (`self`) | `withdrawn` | no other effect |
| `ready` | timer (24 h) | server | `expired` | the child enrolment is void; notice |
| `waiting`, `ready` | the bootstrap entitlement is consumed by another path (bootstrap only) | server | `void` | no factor is disabled |

- **Delay:** 24 h by default, configurable from 24 to 72 h (Q1). One rule for all kinds; there is no
  shorter in-person or same-network variant.
- **Completion transaction** (rule 1): the request is `ready` (or, for `admin`, `waiting` at its
  timer) and within its deadline. For `self`, its single child enrolment, if any, is confirmed in
  this transaction; for `admin`, its pending authenticator becomes active. It activates at most one
  new authenticator; sets a new password or TOTP if chosen (clearing the flag or the disable);
  revokes the devices named lost; consumes `bootstrap_open` if a strong authenticator became active;
  replaces the recovery codes (`self`: shown once to the request client); and, if it named devices
  lost or changed the password or TOTP, ends every other session of the account except those of
  active key devices that are not named lost. Then notices.
- **Cancel effects:** the request ends; its child enrolment and pending authenticator are voided;
  for `self`, the factors it used are disabled (the password flagged, TOTP disabled; the recovery
  code was already consumed); for `admin`, the admin must start again with a new claimant check.
  A report event is written, and the notices name who cancelled.
- **Independent canceller:** a request is accepted only if someone other than its requester and the
  acting admin could cancel it: another admin, the recovery contact (with an active strong
  authenticator), or an authenticator or older session of the account under the owner action rule.
  Otherwise it is refused with "Name a recovery contact first, or use the host CLI". This matters
  only on a single-admin server.
- **Notices:** at the start (high severity, ignoring quiet hours), 1 h before `ready`, and at ready,
  completion, cancel, expiry and void. They go to the account's devices (including devices named
  lost, so an owner who has not lost the phone learns of the claim), the recovery contact and all
  admins. The owner's devices show the open request full-screen at the next app open, with Cancel.
- **While waiting**, the pending authenticator has no session and cannot sign in, approve or step
  up; its screen shows when it will be ready and whom to call.
- **Races:** a cancel that loses against completion is refused with "already completed", and the
  canceller is offered "Remove this device" (the owner with a step-up, an admin with "Remove device
  now"); a recovery contact asks an admin.

#### SM4. Authenticator lifecycle and device-key sessions

Authenticators (key devices and passkeys):

| From | Event | Who may trigger | To |
|---|---|---|---|
| – | enrolment `done` (SM2) | server | `active`, or `pending` with an admin request |
| `pending` | its admin request completes / is cancelled | server | `active` / deleted |
| `active` | "Sign out and remove this phone" (UV), or removal from another full session (step-up) | owner | `revoked` |
| `active` | "That wasn't me" on a sign-in it made (B.2) | owner action rule | `revoked` |
| `active` | "Remove device now", or the CLI | admin, operator | `revoked` |
| `active` | an account request that names it completes | server | `revoked` |
| `active` | Android invalidates the key; account deleted | server | `revoked` |

An `active` authenticator is full. `pending` and `revoked` authenticators authenticate nothing: no
sign-in, no step-up, no approval. A user cannot remove their own last strong authenticator; an
admin can (a stolen phone), and the account then recovers through F6.

Key-device sessions:

| From | Event | Who may trigger | To |
|---|---|---|---|
| `live` | 7 d inactivity, 90 d absolute cap, or a plain sign-out on the device | server, user | `lapsed` |
| `lapsed` | device-key sign-in: UV signature over a `device_login` challenge | the device's user | `live`, full |
| `live`, `lapsed` | the device is revoked | SM4 parties | `ended` |

Session assurance has no state machine: it is set at creation (B.4) and never changes.

## Part C: Security notices in chat (v1.0)

- A built-in **system account "Kaiku"** sends security notices into a read-only DM with each user.
  The account is refused on every auth path and hidden from user lists, search, presence, mentions,
  friend paths and bot paths.
- **Events:** a new sign-in (method, assurance, client, approximate place); a new key device,
  passkey or TOTP; recovery codes created; an authenticator removed; the password set, changed,
  removed, flagged or disabled; TOTP disabled; a step-up action (what was done); an approval
  reported; "That wasn't me" used; a correct password with a failed second factor; a password
  entered but not confirmed; repeated guessing (20 failures); an account request started (with
  Cancel), 1 h before ready, ready, completed, cancelled (naming who), expired or void; an admin
  setup or recovery; the recovery contact named or changed (also to the contact); a session ended by
  refresh reuse or the session limit; a key invalidated by the phone.
- Each notice is also pushed to all the user's devices, including key devices whose session has
  lapsed, devices named lost in an open request, and a device that "Remove device now" has just
  removed (one last notice naming the admin). The pushes bypass Kaiku's notification settings, and
  high-severity ones also bypass quiet hours. Repeated notices are coalesced (B.5). "That wasn't
  me" and Cancel follow the rule in B.2.
- **Second address** (NIST §4.6: at least two notification addresses; NIST §4.2.3: recovery notifies
  the subscriber or their designee):
  - **Recovery contact:** each user may name one other user of the server who is not an admin (with
    a step-up); a phone-less user names one when an account request completes. The admin names one
    at setup and may change any account's contact with a step-up; the account and all admins get a
    notice. The contact is told when named and then receives the account's high-severity notices in
    their own Kaiku DM (event, device label, time; never IP or place). The contact may cancel a
    pending account request (full session, step-up); after an activation the contact's route is an
    admin ("Remove device now"). The contact gets no access and approves nothing.
  - **Admins:** high-severity events (new strong authenticator, account request started, completed
    or cancelled, an authenticator removed, "That wasn't me", password flagged or disabled, admin
    setup) are copied to every admin's security log and Kaiku DM, with event, user, device label and
    time, never message content. Any admin can cancel a pending request.
  - An owner stops a fake request with any own authenticator (also one the request names lost), an
    older session when no other strong authenticator is left, the recovery contact, or any admin
    reached by phone.
- The user's sessions cannot delete, mute or archive notices. Every event is also listed in a
  **security log** in the account settings (both apps; read-only in the simple app), so an attacker
  who got in cannot erase the trace. No e-mail notices in v1.0.

## Constraints

- `CLAUDE.md` rules apply: feature branches/worktrees, squash-merge PRs,
  license constraints (`cargo deny`; any QR, camera, credentials, biometric or Android library must
  be MIT/Apache/BSD/MPL-compatible; check the GeoIP database's data license), UI contrast rules,
  changelog, quality gates.
- `server/src/auth` and `server/src/approvals` are security critical. Every change gets a security
  review (Faramir) and an external review before merge, and the v1.0 auth changes get a final Fable
  review before the tag.
- v1.0 approvals use no push at all (B.2, Delivery); notices never carry an approving action.
- **Lifetimes:** access 15 min and refresh 7 d sliding stay as in `server/src/auth/AGENTS.md`.
  Reuse detection and absolute caps are additions, not changes (RFC 9700 §2.2.2; ASVS 7.3.2).
- `PUBLIC_URL` is mandatory (signature origin, RP ID), and the server fails closed and warns at
  startup without it. The reverse proxy and production ingress route `/.well-known/` to the server.
- **Client IP:** taken from the forwarding header only when the socket peer is a configured trusted
  proxy (the rightmost untrusted address), otherwise from the socket peer. It is used for per-IP
  limits, display and audit only (CC3).
- The desktop keeps tokens, collection secrets and verifiers in Rust; the webview never sees them.
  Android OIDC and every browser hand-off use the one-time code + verifier contract, never tokens in
  a URL (RFC 8252 §7.1, §7.3, §8.1); the `OidcHandler` token branch is deleted.
- Public repository: no private hostnames, IPs or exploit details in code, docs or PRs.
- Accessibility for older users is a requirement, not polish: large touch targets, readable text
  sizes, the contrast rules above, codes shown in groups ("123 456") with a large digit keypad that
  allows corrections before sending, and no gestures as the only way to do something.

## Sequencing

v1.0 packages, per the package cut of 2026-11-03 (model-level ownership; task lists live in the
plans):

- **Gate:** this spec (revision 3) → a confirming design review → contracts frozen: C26 (signature
  format, kinds, canonical contexts, golden vectors); C27 (login attempts, QR payloads, enrolments,
  setup links, account requests, desktop flows; SM1–SM4); C28 (security events, the Kaiku DM, the
  recovery contact, coalescing); C29 (RP ID, origins with both APK hash encodings, assetlinks
  including the setup-link path, the desktop flow with pending result and form-POST delivery).
- **LP (passkeys, server + all clients), off the critical path, merges first:** the WebAuthn
  profile (B.1); the desktop flow (F1) with its loopback POST route and a spike that the supported
  browsers submit that POST without a warning; Android Credential Manager and `assetlinks.json`; the
  passkey `state` column; passkey as a C24 re-auth method; the `issue_session()` extraction. No
  probation sessions exist before L1b, so LP's "fresh re-auth" gate is complete for its own merge;
  L1a adds the full-session check and the action-bound grant to LP's routes.
- **L1a (server: device signatures and key devices) ★:** B.2 in full (challenge core, kinds,
  format, claim by transfer, remediation rule); key devices (UV policy, attestation minimum,
  lifecycle), device-key sign-in, device step-up; session assurance, the two guards, step-up grants
  (replacing C24's password + TOTP and OIDC re-auth), refresh reuse detection with the retry rule,
  absolute caps, the session limit, immediate session end; the B.7 rules, the enrolment core (SM2)
  and the account-request core (SM3); `security_events` and the Part C plumbing; removal of the
  external GeoIP calls at login and refresh.
- **L1b (server: sign-in flows) ★, stacked on L1a:** F2–F8 with SM1 and SM3; B.5 with per-account
  throttling and the recovery-entry order; the blocklist, the compromise flag and TOTP disable,
  recovery codes ≥64 bits; the admin protocol with the claimant record; the bootstrap entitlement;
  registration tickets; the CLI; all notices, the recovery contact and the independent-canceller
  check; removal of the old QR and e-mail reset routes. If L1b passes the review threshold, the
  smallest split is B.5 + F6 + the CLI as a follow-up PR, still before RC3. Revision 3 removes scope
  here: Trust, the action QR, network checks, owner evidence, dormancy, the separate bootstrap
  ceremony.
- **L2 (clients) ★:** desktop/web: confirmation QR, QR login panel, the probation banner with "Sign
  in with a passkey", setup-code panels with the claiming device's name, the recovery entry and
  "Secure my account", security log, read-only Kaiku DM. Android: `K_approve` and the UV rules, the
  scanner (one tap from the chat list) with origin pinning, context and approval screens with the
  context-hash check, the typed-code keypad, F5 flows with setup links and "Open in Kaiku",
  device-key sign-in, the full-screen open-request prompt, answer-then-confirm for calls,
  Keystore-wrapped storage, deletion of the `OidcHandler` token branch. L2 builds against the frozen
  contracts and merges after L1a + L1b.
- **Order:** L1a → L1b (stacked) → L2 merge → RC3 → device tests (Fabian) → Fable final review of
  all auth changes → v1.0 tag. LP runs beside it. No RC is cut between the L1b and L2 merges.
- **v1.1** (M1 admin MCP core, M2 agent approvals on the L1a core, M3 agent bots) and **v1.2**
  (E1 simple app + group calls, E2 1:1 video + voice messages): their PRs may be built early but
  **stay open until the v1.0 tag**, unless the maintainer cuts a release branch (Q6). The v1.1 spec
  gets the changes listed at the end of B.2.

## Left to the builder

- Module/flavor split of the Android codebase.
- Tables, Valkey keys and WebSocket payload details, within B.2 and B.7.
- QR and setup-link payload encoding (scheme and URL shape), as long as it carries the origin and no
  long-lived secret.
- Exact TTLs and limit defaults within the stated bounds (challenges ≤10 min; delays 24–72 h).
- How earlier refresh tokens are recognised (a spent-hash list or an authenticated token format),
  within B.4.
- How the API 26–29 PIN-only case is implemented, within the 10 s window rule.
- How the form POST to the loopback is built, within F1 (after the LP spike).
- Choice of WebAuthn crate after the LP spike; choice of the optional local GeoIP database (license
  check).
- How the system account and the read-only DM are modelled.
- Wording and layout of the context, approval, setup and request screens (accessibility rules
  apply).
- Video call implementation details (codec, simulcast or not, bandwidth); the voice-message codec
  matrix.

## Open questions for the maintainer

1. **Waiting period:** 24 h (configurable 24–72 h) for every account request, which now includes
   every admin setup of an existing account, also in person: a relative's new phone works the next
   day, and a relative whose only phone broke is without Kaiku for a day. Fable proposed 24 h, Argon
   48 h. Keep 24 h, or choose 48 h?
2. **Phone-less users:** as revised, password + TOTP is a chat-level (probation) login for the whole
   session. Security changes (a new password, a new TOTP, recovery codes, a first passkey) go
   through a 24-h account request, and an attacker who knows the username can disable the password
   after about four days of guessing (the price of NIST §3.2.2's hard limit; recovery code + TOTP or
   an admin brings it back). Is that acceptable, or must every phone-less user register a passkey at
   the upgrade?
3. **No screen lock:** what happens to a relative who will not use a PIN, pattern or biometric? As
   written, that phone cannot be a key, so a passwordless account cannot sign in on it. The
   alternative is a possession-only "chat only" device that never approves anything and needs the
   admin again after each lapse. That is below the standards bar.
4. **Family members approving PC sign-ins:** `may_approve` is off for admin-set-up accounts, so they
   cannot sign in on a PC by QR or add a phone themselves. Keep that default, or turn it on per
   person? The simple app has no scanner, so the flag matters only for relatives who use the full
   app.
5. **Full access on a PC only by passkey:** revision 3 cuts Trust, so a PC signed in by QR stays
   chat-only, and security settings and admin work on a PC need a passkey. On Linux with Firefox that
   means a USB security key or a Chromium-based browser; otherwise these tasks are done in the
   Android app, which is always fully trusted. Acceptable? A key-device path for PCs would need a
   proximity check that Kaiku does not have.
6. **Release branch:** cut `release/1.0` at RC3 so v1.1/v1.2 can merge to main earlier, or keep
   them open until the v1.0 tag?

## References

- NIST SP 800-63B-4: https://pages.nist.gov/800-63-4/sp800-63b.html
- OWASP ASVS 5.0.0 (V6, V7): https://github.com/OWASP/ASVS/tree/master/5.0/en
- RFC 10027 / BCP 247, Best Current Practice for Security of Cross-Device Flows:
  https://www.rfc-editor.org/rfc/rfc10027
- W3C WebAuthn Level 3: https://www.w3.org/TR/webauthn-3/
- RFC 9700 (OAuth 2.0 Security BCP), RFC 8252 (OAuth for native apps), RFC 9449 (DPoP), RFC 9562
  (UUIDs), RFC 8176 (Authentication Method Reference values)
- OWASP Multifactor Authentication and Passkey Security cheat sheets:
  https://cheatsheetseries.owasp.org/
- CISA, Implementing Number Matching in MFA Applications (2022)
- Android Credential Manager and Digital Asset Links:
  https://developer.android.com/identity/sign-in/credential-manager
- Android Key Attestation: https://developer.android.com/privacy-and-security/security-key-attestation

## Appendix: Review response (round 2)

Reviews of revision 2 (fbd02b4f): Fable (F, READY WITH FIXES), Codex (C, NOT READY), Gemini Argon
(A, NOT READY). Labels are the reviewers' own (B = blocker, M = major, m = minor; Argon's section-1
items appear as A 1.n, Codex's minors in their order as C m1–m7). Overlapping findings are merged.
"Cut" means the finding is resolved by deleting the mechanism it was about.

**Blockers and major issues**

| # | Consolidated finding | Reviewers | Resolution |
|---|---|---|---|
| 1 | Desktop system-browser hand-off not bound to the initiating desktop: step-up, upgrade or registration applied at the assertion; the sign-in code can be read back from the victim's address bar | F B2; C B4; A B1, A 1.5 | Fixed: pending result, redeemed only with the verifier (step-up: from the starting session) before any effect; form-POST delivery, never displayed; no registration and no in-session upgrade in the desktop app (CC5, F1, F7, B.6.2). Loopback-first start and flow cookie: rejected, see note 1 |
| 2 | Shared-network inference gates Trust (full control) and immediate in-person recovery; broken behind a reverse proxy, refuses on a home LAN | C B3; A B2 | Cut: no network signal in any decision (CC3); Trust cut (CC4); in-person and remote share one activation rule (F5c); trusted-proxy rule for the client IP (Constraints) |
| 3 | Owner evidence both triggers the in-person delay and cancels it; the "Add a phone" pointer does not exist for `may_approve`-off accounts; probation-only accounts have no evidence | F B1a; A 1.1 | Cut: owner evidence removed; existing accounts always wait, new accounts are immediate; the old phone keeps working (F5c, SM3) |
| 4 | A device named lost (or a stolen session, by mere foreground use) cancels the owner's recovery and flags the password | F B1b; C M5; A 1.1, A B5.4 | Cut: no automatic cancel; an explicit Cancel needs UV or a passkey, and a stolen phone with a known lock is removed at once (B.2, F6, B.6.7). Not adopted: barring devices named lost from Cancel (note 2) |
| 5 | Recovery cancel or expiry leaves a child enrolment able to activate; several children; cancel vs complete races | C B1; F m4 | Fixed: one request, at most one live child; completion and cancel are single transactions under the account lock with cascades (B.7 rules 1–2, SM3) |
| 6 | Bootstrap re-entry after revocation; concurrent first bindings; open-ended admin exception | C B2 | Fixed: one-time `bootstrap_open`, consumed atomically by any first strong authenticator, never restored; the admin exception is replaced by the CLI at the upgrade (F8, SM3) |
| 7 | Bootstrap ignores the compromise flag | F M6 | Fixed: a flagged password or disabled TOTP is refused (F8, SM3) |
| 8 | A phished password + TOTP binds a full authenticator at once during the bootstrap window, and the owner cannot object | F M1 | Fixed: bootstrap is an account request (24 h); older sessions may cancel while the account has no strong authenticator (B.2 owner rule part 2, F8) |
| 9 | Cancel has no sender rule; a probation session of the contact or an admin can cancel | F M2; A 1.6, A B5.6 | Fixed: owner action rule; contact and admins need a full session with a step-up (B.2, F7) |
| 10 | A cancelled recovery flags the password but leaves the TOTP it used valid | F M3 | Fixed: a cancelled request disables every factor it used (CC8, SM3) |
| 11 | Single-admin server: no canceller independent of the deceived admin; "Remove device now" removes the owner's path | F M4 | Fixed: independent-canceller requirement or the CLI (SM3); the contact is never an admin and is named at setup (F5c, Part C); devices named lost and a just-removed device still get notices (Part C) |
| 12 | Enrolment-direction dictation is not listed | F M5 | Fixed: B.6.3; warning text and the claiming device's name on the typing screen (F5a, F5c) |
| 13 | Phone-less users cannot rotate a leaked password or TOTP, and cannot complete a recovery without enrolling a passkey or phone | F M7; A 1.3, A B5.5 | Fixed: an account request may complete with a new password or TOTP alone (F6, SM3); Q2 states the cost |
| 14 | Migrated password + TOTP users have no recovery codes after the upgrade | A 1.3, A B5.5 | Fixed: "Secure my account" (a bootstrap request) yields them (F8) |
| 15 | Unauthenticated DoS of the recovery entry through its per-account counter | A 1.3, A B5.3; F m7 | Fixed: the recovery code is checked first; delay only, never a disable; same answer for unknown usernames (B.5, SM3) |
| 16 | Permanent self-service lockout via a flag set by a hostile cancel | A 1.3, A B5.4 | Fixed by #4 and #9 (no automatic cancel; contacts and admins need a step-up); a cancel, hostile or not, still sends the owner to an admin, by design (B.6.6) |
| 17 | `admin_remote` started on a PC cannot reach the admin's phone; prefix vs claim ordering for typed kinds | A 1.4, A B3.2 | Fixed: every admin setup starts and is confirmed on the admin's key device; the challenge is requested after the claim (F5c, B.2) |
| 18 | A new key device has no way to receive its first session | A B3.1 | Fixed: device-key sign-in once active; enrolments mint no tokens (F5a, SM2, B.4) |
| 19 | Same-browser passkey registration forced through a code | A B3.3 | Fixed: same-client registration completes without a code (SM2) |
| 20 | Lost authenticators stay active for 180 days and block the admin bootstrap rule; passkey-only admins cannot run F5c | A 1.2, A B5.1–2 | Fixed: the admin rule is gone (CLI at upgrade); requests never need the lost device revoked first; a passkey-only admin adds a phone with F5a (F8, F6); dormancy cut |
| 21 | Device step-up signatures replayable; device-key sign-in proof contract incomplete | A B4; C M3 | Fixed: one format for every device signature with a single-use server nonce, expiry, origin, user, device and context (`sid`, action, target; the receiving sign-in request) (CC10, B.2) |
| 22 | Authorisation not re-validated when results are consumed | C M1 | Fixed: B.7 rules 1, 2 and 4; F7 "at use" |
| 23 | Immediate revocation promised with 15-min JWTs and open WebSockets | C M2 | Fixed: the existing `sid` denylist and WebSocket check plus connection close (CC6, B.4); no assurance change exists to propagate |
| 24 | Binding-code strength not mapped to NIST §4.1.2.2 | C M4 | Fixed: the 256-bit setup token is the binding code; the 6-digit code is an extra check (F5a) |
| 25 | Delays down to 1 h configurable, contradicting the round-1 fix | C M6 | Fixed: 24–72 h (SM3) |
| 26 | `may_approve`-off accounts cannot use "That wasn't me"; contacts cannot act after activation | C M6; A 1.6, A M2 | Fixed: repudiation is independent of `may_approve` (B.1, B.2); the contact escalates to an admin (Part C) |
| 27 | Second notification destination not specified; availability limit unstated | C M7 | Fixed: the contact is named at setup and notified when named; the shared-server limit is stated (Part C, B.6.11). A contact acceptance step: rejected (adds state to a passive role) |
| 28 | Setup links consumed by link-preview bots; App Links unverifiable on unreachable servers | A M1 | Fixed: opening never consumes; "Open in Kaiku"; the name is fetched after the server is shown (F5c, SM2) |
| 29 | Pending or failed passkey enrolments orphan discoverable credentials | A M3 | Fixed: "ready at …" for pending; `signalUnknownCredential` after failure (B.1) |
| 30 | The password flag also blocks OIDC + TOTP; in-flight password attempts survive the flag | A M4 | Fixed: the flag closes only the password and cancels open password attempts (CC8, B.1) |
| 31 | The device does not verify the request hash against what it displays | A M5; C m6 | Fixed: the device recomputes the context hash (B.2) |
| 32 | Where the recovery secret is kept during the wait | A M6 | Fixed (F6) |
| 33 | Refresh grace: dual-session survival, chained generations, unstated exception | A 1.7; C m3 | Fixed: the retry returns the same successor, once per session per 24 h; exception stated (B.4, B.6.10) |

**Minor issues**

| # | Finding | Reviewers | Resolution |
|---|---|---|---|
| 34 | `PUBLIC_URL` and password-only admins in the upgrade checklist | F m1 | Fixed (F8) |
| 35 | Bootstrap from the sign-in page has no session | F m2 | Fixed: an account request has its own request secret (SM3) |
| 36 | Whether a browser session becomes full after a passkey registration | F m3 | Fixed: the user signs in with the new passkey (SM2) |
| 37 | `approval_devices` state between claim and confirmation | F m5 | Fixed: the row is created at confirmation (B.2) |
| 38 | Which IP is compared for proximity | F m6 | Cut (no comparison) |
| 39 | Neutral answer for unknown usernames (F3 and the recovery entry) | F m7; C m1 | Fixed (F3, B.5) |
| 40 | "That wasn't me" on a passkey sign-in leaves the passkey | F m8 | Fixed: repudiation revokes the factor used (B.2) |
| 41 | Define "foreground use" | F m9 | Cut with owner evidence |
| 42 | Flag the password automatically after N second-factor failures that followed a correct password | F m10 | Rejected: anyone who knows the password could then lock a phone-less user out at will; the notice (F4) and the 100-failure disable remain |
| 43 | A refused in-person claim from another network should notify | F m11 | Cut (no network check) |
| 44 | Android takes the server from the scanned payload; GeoIP is also called at refresh | F m12 | Fixed (Current state, B.1, L1a, L2) |
| 45 | A PC-only person set up in person cannot scan the admin's QR | F m13 | Fixed: the link is shared to the PC (F5c step 3) |
| 46 | Content of contact notices | F m14 | Fixed (Part C) |
| 47 | Lockout trade-off of round-1 #16 belongs in Q2 | F m15 | Fixed (Q2) |
| 48 | F4 vs SM1 `claimed`; the single-claim lock; two round trips for F4 | C m2; A m1, A m2 | Fixed: release back to `open`; F4 from `open` or `claimed` in one request (SM1) |
| 49 | Dormancy and passkeys | C m4 | Cut (no dormancy) |
| 50 | Coalesced notices lose the actionable reference | C m5 | Fixed (B.5) |
| 51 | Concurrent session limit undocumented | C m7 | Fixed (B.4) |
| 52 | One signature per 10 s on API 26–29 breaks sign-in followed by a scan | A m3 | Cut: the server rule is dropped; the app prompts before every signature (B.1) |
| 53 | OIDC accounts without password or TOTP cannot use recovery codes | A m4 | Fixed: a fresh OIDC sign-in is a request factor (F6) |
| 54 | Whose counter a wrong typed enrolment code increments | A m5 | Cut: no account counter; three tries per enrolment (B.5) |

**Simplifications decided in revision 3** (beyond the cuts above):

| Candidate | Decision |
|---|---|
| Trust-upgrade paths that are neither a full key device acting itself nor a passkey | Trust cut; in-session passkey upgrade cut (sign in anew); the `reauth` approval (action QR for a PC session) cut, since every full PC session has a passkey. Kept: passkey step-up, `device_stepup` on the key device |
| Overlapping recovery variants | Collapsed to: own other device or passkey; one account-request machine (recovery codes + second factor, bootstrap as a kind of it) with a delay; admin recovery, one protocol in person or remote; the host CLI as break-glass. Meets NIST §4.2.2.2 |
| Notification niceties that add state | Cut: the "after 1 h" reminder, the dormancy warning, the "different network" line, a "lost phone in use" alarm. Kept: start, 1 h before ready, and the outcome |
| State machines differing only in wording | SM3 (assurance) is a rule now; SM5 (admin protocols) is an SM2 variant; SM7 (pending activation) and SM4 (recovery) merged into SM3; approvals and proofs share one challenge machine |
| Other mechanism | Cut: dormancy limit; recovery codes as a sign-in factor; the 7-day block after a cancel (the disabled factors already stop a retry); the typed-code account counter; the API 26–29 server rate; the superseded builder-call table and CC11 (history is in git) |

**Notes**

1. *Loopback-first start and flow-bound cookie (F B2 fix items 1–2): rejected as ineffective.* The
   attack being closed is an attacker who starts the flow on their own computer. Whatever the
   loopback listener does to start the ceremony runs on the attacker's machine, so the attacker can
   capture the server URL and forward it before any cookie exists; the victim's browser then
   receives the cookie and completes. What binds the ceremony to the initiating desktop is the end:
   no effect before redemption with the verifier (C B4, A B1) and a result that only reaches the
   loopback of the machine whose browser completed it, in a form that is not shown. The remaining
   relay (developer tools) is B.6.2.
2. *Barring devices named lost from Cancel (F B1 fix item 1): not adopted.* In a self-service
   request the requester, possibly the attacker, chooses which devices are "lost"; barring them
   would let a fake request silence the owner's real phone, which is the attack the waiting period
   exists for. Cancel needs UV, so only someone who knows the screen lock can cancel from a lost
   phone, and such a person already holds a full key; the remedy for that is immediate removal
   (B.2, F6, B.6.7). Automatic cancel, the other half of F B1b, is gone.
3. *Fable's `activation_trust` (F B1 fix item 3): not adopted.* It needs the removed network gate
   and a new kind; an honest phone upgrade now waits a day while the old phone keeps working, which
   Fable named acceptable.
4. *F3 stays.* Codex round 2 accepts it ("F3 can remain available"); round-1 Q6 (drop it after v1.0)
   is withdrawn from the question list, since the maintainer asked for the flow.
5. *Round-1 appendix rows the reviewers called not fixed* (F: #3, #4, #5, #6, #10, #11, #12, #14,
   #15, #18, #35, #49; C: #3, #5, #6, #10, #13, #15, #18, #19; A: #3–#6, #8–#10, #14, #16, #18,
   #21, #22, #50, #54) are covered by rows 1–33 above.

**Count:** 54 consolidated findings: 44 fixed, 9 cut (the mechanism was removed: #2, #3, #4, #38,
#41, #43, #49, #52, #54), 0 deferred, 1 rejected (#42). Partial decisions inside those rows: #1
loopback-first start and flow cookie rejected (note 1); #2 a FIDO-grade proximity check for PCs
deferred (Non-goals, Q5); #3 `activation_trust` rejected (note 3); #4 barring devices named lost
from Cancel rejected (note 2); #20 a passkey-only admin confirming setups rejected (Decision 3 names
the admin's key device); #27 a contact acceptance step rejected (it adds state to a passive role).

## Appendix: Review response (round 1, compressed)

Round 1 reviewed the first revision; revision 2 resolved it. Row numbers are those cited by the
round-2 reviews. "→ r3" marks what revision 3 changed again.

- **#1** time-based probation upgrade: removed (→ r3: no upgrade of any kind, CC4). **#2** assurance
  bound to the authenticator: fixed (→ r3: fixed at creation). **#3** self-service recovery delay,
  cancel and revocation: 24 h, cancel with effects, revocation at completion (→ r3: SM3, no
  automatic cancel, owner action rule). **#4** recovery factors: recovery code + password or TOTP
  (→ r3: or fresh OIDC). **#5** in person vs remote: two protocols with a network check (→ r3: one
  protocol, no network check, every existing account waits). **#6** bootstrap and migration
  deadlocks: bootstrap window, admin rule, tickets, CLI (→ r3: one-time entitlement inside SM3, CLI
  for the first admin). **#7** second-admin approval: another admin confirms, delay, cancel. **#8**
  push budget lockouts: no approval pushes. **#9** empty `response`: claim by transfer. **#10**
  desktop ceremony polling: removed (→ r3: pending result + redemption + form POST). **#11** step-up
  not action-bound: single-use action grants (→ r3: action QR cut). **#12** password + TOTP as a
  step-up: removed. **#13** Trust as pick-from-list: Trust QR (→ r3: Trust cut). **#14**
  remediation owners: one rule table (→ r3: plus the owner action rule). **#15** missing state
  machines: SM1–SM7 (→ r3: SM1–SM4 with shared transaction rules). **#16** 100-failure lockout:
  NIST disable kept with delay and notices. **#17** `may_approve` owner: admin only. **#18** second
  notification address: recovery contact (→ r3: never an admin, named at setup). **#19** overstated
  anti-relay claims and citations: CC3 rewritten. **#20** OIDC `amr`: no trusted-IdP exception.
  **#21** refresh reuse detection: every issued token recognised. **#22** grace logout: one
  re-rotation (→ r3: the same successor, once a day). **#23** attestation: deferred with a v1.0
  minimum. **#24** APK hash encodings and BE flag: fixed in C29.
- **Minors #25–#43:** PIN chosen with the relative; `setInvalidatedByBiometricEnrollment(false)`;
  answer-then-confirm for calls after a lapse; the API 26–29 window listed as a deviation (→ r3: the
  10 s server rate cut); corrections and three tries for typed codes; keyed hashes; the
  `OidcHandler` branch deleted; the device checks what it signs (→ r3: plus the context hash); all
  pre-upgrade sessions probation; empty `allowCredentials`; typed PC → phone codes cut; host CLI
  powers; `may_approve` in the simple app (Q4); sign-out keeps the key; early suitability checks;
  coalescing; setup links on the phone; dormancy warning (→ r3: dormancy cut).
- **Over-engineering #44–#54:** F3 kept as the maintainer asked and simplified (no push); F5b cut;
  `setup_only` cut; the Android 90-day cap kept (NIST §5.1); `K_session` deferred; typed PC → phone
  codes cut; legacy backup codes end at the upgrade; pre-declared v1.1 schema cut; VPN/hosting
  classification and the "same network" badge cut (→ r3: every network signal cut); the Custom Tab
  fallback deferred; a passkey-only admin confirming setups rejected (Decision 3 names the admin's
  key device).
- **Citations corrected in revision 2:** AAL2 account recovery is NIST §4.2.2.2, the recovery
  notification §4.2.3, saved recovery codes §4.2.1.1; NIST §4.1.2.1 binds at the lower of the
  account's and the new authenticator's AAL, so requiring an admin for password-only accounts is
  stricter than NIST; NIST §3.2.2 disables after at most 100 failures; RFC 10027 §6.1.12
  (sender-constrained tokens) and §6.1.15 were removed where they had been cited for the collection
  secret and Trust, and §6.1.14 and §6.1.1 are cited instead.
- **Count (round 1):** 54 findings: 49 fixed, 2 deferred (#23, #48), 3 rejected (#44, #47, #54).
