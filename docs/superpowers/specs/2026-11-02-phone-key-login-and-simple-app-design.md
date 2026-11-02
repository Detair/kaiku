# Kaiku: phone-as-key login (v1.0) and simple Android app (v1.2)

**Status:** Draft for three independent design reviews (Fable, Codex, Gemini). First draft
2026-11-02; updated 2026-11-03 with the maintainer decisions, a standards check and the
approvals model from the package cut.
**Target:** split release (maintainer decision 2026-11-02):

- **v1.0** ships the new login: Part B (sign-in with passkeys and the phone as key) and Part C
  (security notices). The family server does not go live with the old login design. This is a
  deliberate exception to the v1.0 feature freeze.
- **v1.2** ships Part A: the simple Android app ("Kaiku Einfach"), 1:1 video calls and voice
  messages. Planned after v1.1 (admin MCP / agent bots); its Android UI work may start in parallel.

**How to read this document:** the maintainer decisions bind. Coordinator calls (CC1–CC11) are
binding for the builder unless the maintainer or a design review overturns them. Items marked
**(design call)** are choices made in this update where neither the decisions nor the coordinator
calls say anything; reviewers should challenge them first. Citations use the short labels in
"Standards baseline".

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

**How this spec applies them** (old labels → new sections: B1 → F5, B1.3 → F5c step 5, B2 → F5d,
B3 → F2, B4 → F3, B5 → B.5, B6 → F6, C24 → F7):

- **Decision 5, "desktop via WebAuthn where the webview supports it"**, cannot work as worded. No
  Tauri webview can make assertions for the server's RP ID: the webview origin (`tauri://localhost`,
  `http(s)://tauri.localhost`) is not the server's domain (WebAuthn L3 RP ID rules, §4), WKWebView
  needs an associated-domains entitlement that a self-hosted domain cannot get, and WebKitGTK has no
  WebAuthn. The intent (passkeys first-class on the desktop) is met by CC5: the system browser on
  every OS.
- **Decision 3:** admin re-auth plus the confirmation on the admin's key device protect against a
  hijacked *admin account*. They do not protect against a *social-engineered* admin, which is the
  realistic remote attack. CC7 adds a code typed by the admin, a delay and notices. In-person and
  remote recovery both stay.
- **Decision 2:** a forgotten password is replaced through self-service recovery with two factors
  (F6) or by an admin. Passwordless accounts have nothing to reset.
- **Decision 5, "QR stays the universal path":** it does. QR sign-ins produce restricted sessions
  (CC4) because they can be relayed; passkeys are offered first wherever they work.

## Coordinator calls 2026-11-03

Each item is a **coordinator call** for the maintainer to review. Together they replace the earlier
builder calls (table at the end of this section).

- **CC1 (coordinator call): key devices require user verification.** A key device needs a screen
  lock (PIN, pattern or password) or a Class 3 (strong) biometric. Setup checks for one and walks
  the user (or the admin setting up a relative's phone) through turning on a screen lock. Every
  signature with the approval key requires user verification (UV). There is no possession-only key
  class (resolves K4 against the package cut's proposal). Sources: NIST §3.1.3 ("each use … SHALL
  require the presentation of the activation factor" for multi-factor OOB), NIST §3.1.7, OWASP MFA
  cheat sheet (user presence alone is not a factor). Details in B.1.
- **CC2 (coordinator call): no number matching; a real secret moves from the PC to the phone.**
  Password login (F3), QR login (F2) and step-up (F7) are confirmed by the phone **scanning a QR
  code shown on the PC**. The fallback is a code of at least 6 digits shown on the PC and **typed**
  on the phone. A push only opens the app at the scanner or approval screen. There is no
  compare-and-approve and no choose-from-list anywhere. Sources: NIST §3.1.3 (compare-and-approve
  "no longer considered acceptable"; a list of secrets to compare "is not sufficient"; transfer by
  typing or scanning a QR code), NIST §3.1.3.2 (≥6 decimal digits, ≤10 min).
- **CC3 (coordinator call): anti-relay set for cross-device flows.** The PC holds its own
  collection secret, and tokens are handed out only against it. QR, attempt and setup tokens are
  256-bit CSPRNG values, never UUIDv7. The phone shows a context screen (origin, device, location,
  network relation) with a "different network" warning. TTLs are short. Every resulting sign-in can
  be revoked from its notice. The residual relay risk is documented (B.6); passkeys over the hybrid
  transport are the phishing-resistant path. Sources: RFC 10027 §6.1.1–6.1.4, §6.1.6, §6.1.12,
  §6.1.14, §6.1.16–6.1.18, §6.2.3.5; ASVS 6.6.2; ASVS 7.2.3 (≥128 bits); RFC 9562 §8 (UUIDs are not
  security capabilities); NIST §3.2.5 (manually transferred outputs are not phishing-resistant).
- **CC4 (coordinator call): probation sessions.** Every sign-in that is not phishing-resistant
  (QR login, password + phone, OIDC + phone, TOTP or recovery-code fallback, self-service and
  remote recovery) yields a restricted *probation* session: no change of sign-in methods, no
  revoking other sessions or devices, no setup codes, no elevation, no approvals. B.4 defines how a
  probation session becomes full: a passkey step-up, "Trust this sign-in" initiated on a full key
  device, or a delay with notices. Sources: RFC 10027 §6.1.9 (limited scopes), §6.1.6 (detect and
  remediate), §6.1.15 (authenticate then initiate); OWASP MFA cheat sheet (fallbacks must not be
  weaker than the primary).
- **CC5 (coordinator call): passkey architecture.** Web uses WebAuthn directly. The desktop always
  uses the system browser and the existing desktop OIDC hand-off pattern (one-time code, S256
  verifier, loopback), on Windows, macOS and Linux. Android uses Credential Manager with
  `assetlinks.json`. The RP ID is the host of `PUBLIC_URL`; the origin allow-list is exact; UV is
  required. Sources: WebAuthn L3 §5.8.6, §6.1.3, §5.1.10, §5.11; OWASP Passkey Security cheat sheet;
  Android Credential Manager docs; RFC 8252 §7.1, §8.1.
- **CC6 (coordinator call): sessions.** Refresh tokens rotate with reuse detection: a reused token
  ends its session family. Absolute session lifetimes are documented, with the deviation from NIST
  stated. A key device signs in again after a lapse only with UV, and never after revocation.
  Sources: RFC 9700 §2.2.2, §4.14.2; ASVS 7.1.1, 7.3.1, 7.3.2, 7.4.1; NIST §2.2.3, §5.1.
- **CC7 (coordinator call): recovery.** Self-service recovery needs two factors. Recovery codes have
  ≥64 bits of entropy. Remote admin recovery needs a code shown on the new phone and typed by the
  admin, a delay (default 1 h, configurable), and notices to the user's other devices and to all
  other admins. Removing a lost sole phone does not need that phone (an admin or a second-factor
  path). Sources: NIST §4.2.1 (≥64 bits), §4.2.5.2 (AAL2 recovery), §4.2.6 and §4.6 (notices, two
  addresses), §4.3 (prompt invalidation); ASVS 6.4.3, 6.4.4, 6.5.6.
- **CC8 (coordinator call): passwords.** A password is optional. When one is set, it is checked
  against a breached and common password blocklist. After the upgrade there are no password-only
  accounts: a migrating password-only account gets its first key device or passkey only with admin
  confirmation. Once a password is flagged compromised ("That wasn't me"), the password path and
  the TOTP fallback close for the whole account until the user resets methods from a full session.
  Sources: NIST §3.1.1.2 (blocklist SHALL, no composition rules), §2.2.1; ASVS 6.2.4, 6.2.11,
  6.2.12, 6.3.3; OWASP MFA cheat sheet (correct password + failed second factor = compromised
  password).
- **CC9 (coordinator call): throttling.** Failed sign-ins are throttled per account as well as per
  IP. Limit classes are separate, so agent-approval denials (v1.1) can never pause human logins
  (K9). Sources: NIST §3.2.2 (≤100 consecutive failures per authenticator and account); ASVS
  6.1.1, 6.3.1.
- **CC10 (coordinator call): one approval mechanism.** v1.0 login approvals and v1.1 agent
  approvals share one state machine (approver / subject / requester split, per-kind TTL, signed
  bytes with a kind byte). Desktop approval keys (v1.1) never approve logins, enrolments or
  re-auth. Details in B.2.
- **CC11 (coordinator call): stale statements fixed.** Old "v1.2" wording in B1, B3 and Part C now
  reads v1.0. B3's "passkeys not required" is replaced by Decision 5. "Video calls exist nowhere" is
  corrected: webcam tracks exist in voice rooms on web and desktop; Android has no camera capture.

**Superseded builder calls** (they stood in the Decisions section before this update):

| Earlier builder call | Now |
|---|---|
| Approvals are signed by the device (P-256 key in the Android Keystore) | kept; UV on every use (CC1), a second key for the session, attestation (B.1) |
| One approval mechanism; the v1.1 owner-approvals foundation moves to v1.0 | kept and generalised (CC10, B.2) |
| Adding or removing a key device needs approval on an existing key device; the first key device of a migrating account needs a fresh re-auth | adding: as before, from a full session (F5); removing: any *other* step-up method or an admin (CC7); password-only accounts: admin-confirmed (CC8) |
| TOTP / backup codes only after an approval expired, with a notice; closed for that attempt after "That wasn't me" | closed for the whole account (CC8); the session is on probation (CC4); codes ≥64 bits (CC7) |
| Admin re-auth (C24) accepts a key-device approval or a passkey | kept; the key-device approval needs a transferred code (CC2, F7) |
| A lapsed key device signs in with its device key; token lifetimes unchanged | kept with UV, never after revocation, a dormancy limit and absolute caps (CC6, B.4) |
| QR login warns when IP or location differ; relay risk documented | kept as a hint inside the anti-relay set (CC3) |

## Standards baseline

Short labels used in this document:

- **NIST** = NIST SP 800-63B-4 (final, 2025).
- **ASVS** = OWASP ASVS 5.0.0, V6 Authentication and V7 Session Management.
- **RFC 10027** = RFC 10027 / BCP 247, "Best Current Practice for Security of Cross-Device Flows"
  (IETF, August 2026; formerly draft-ietf-oauth-cross-device-security).
- **WebAuthn L3** = W3C Web Authentication Level 3.
- **RFC 9700** = OAuth 2.0 Security BCP. Also RFC 8252 (native apps), RFC 9449 (DPoP), RFC 9562
  (UUIDs).
- OWASP MFA and Passkey Security cheat sheets; the CISA fact sheet on number matching.

**Target:** AAL2 for every account, with at least one phishing-resistant option offered (NIST
§2.2.2: passkeys), and ASVS 5 Level 2 for V6 and V7. Syncable passkeys are acceptable at AAL2
(NIST Appendix B); AAL3 is a non-goal. Every authentication path and its controls are listed in
B.3 and B.4, so all paths are documented and held to the same strength (ASVS 6.1.3). Deviations
are listed in B.6.

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
   authenticator. It confirms sign-ins on other devices, step-ups and new devices.
4. **QR login on the PC**: the PC shows a QR code, the phone scans it, the
   user sees who is signing in and confirms, and the PC is signed in.
5. **Password optional**, never the only factor; password login is confirmed with the phone.
6. **No approval fatigue**: no unsolicited prompt can be approved, and repeated failures stop
   prompts and slow the attacker down.
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
  deprecated upstream).
- **QR login desktop → phone exists**: `POST /auth/qr/create` (authenticated, 120 s Valkey token,
  `NX`) and `POST /auth/qr/redeem` (public, rate limited, `GETDEL` one-time) in
  `server/src/auth/mfa.rs`; Android `QrScannerScreen` / `QrRedeemScreen`. The token is a UUIDv7,
  and redeem returns a session without any confirmation.
- MFA: TOTP plus backup codes (10 × 8 alphanumeric characters, about 47.6 bits each), C24 re-auth
  (`reauth.rs`: password + TOTP or OIDC).
- The OIDC desktop hand-off (one-time code + S256 verifier + loopback, 60 s) exists and is the
  pattern for desktop passkeys (`server/src/auth/AGENTS.md`).
- Sessions: 15-min access JWT; the refresh token rotates in place (stable `sid`) with a 7-day
  sliding expiry, so a used session never ends. A superseded refresh token simply gets 401; reuse
  is not detected. `DELETE /auth/sessions` and `DELETE /auth/sessions/{id}` need only a bearer
  token.
- Rate limiting: Valkey Lua limiter incl. `failed_auth.lua` (`server/src/ratelimit`). Failed logins
  are counted per IP only; the login TOTP step has no per-account counter (re-auth has one).
- Passwords: only the length (8..128) is checked.
- `geoip.rs` calls a configurable external HTTP API, so login IPs go to a third party.
- `PUBLIC_URL` is optional today (required only for OIDC).
- Registration follows the `registration_policy` server setting.
- Calls: DM calls with ringing exist in the web/desktop client (`components/call`, `stores/call`)
  and run in voice rooms. **Webcam tracks already exist in voice rooms on web and desktop**
  (`server/src/voice/webcam.rs`, `WebcamButton`, `TauriVideoFrame`); there is no 1:1 video call UI
  yet. Android has no camera capture and renders remote video only for screen shares.
- `users.password_hash` is nullable, but a DB constraint requires a password
  for `auth_method = 'local'`.

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

The start screen of an unconfigured simple app offers only "Scan setup code" (admin setup, F5c).

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
- **PC**: shorthand for the device that asks to be signed in: desktop client, browser, or a second
  phone.
- **Approval**: a request the server addresses to a user's key devices, answered with a
  user-verified signature (B.2).
- **Transfer**: a secret moves from the PC to the phone, by a scan of the PC's QR code or by typing
  a code shown on the PC (CC2).
- **Phishing-resistant**: bound to the server's origin, so a relaying page cannot use it (NIST
  §3.2.5). Passkeys (UV) and device-key sign-in on the key device itself are phishing-resistant.
  Transfers by scan or typing are not.
- **Session assurance**: `full`, `probation` or `setup_only` (B.4).
- **Step-up**: a fresh re-authentication within the last 5 minutes, bound to the session (F7).

### B.1 Authenticators and their roles

| Authenticator | Role | Resulting session |
|---|---|---|
| **Passkey** (WebAuthn L3, discoverable, UV required) | primary phishing-resistant sign-in and step-up | full |
| **Key device** (Kaiku Android app, UV-bound key) | its own sign-in (device-key sign-in); approver for sign-ins on PCs, step-ups and new devices; the only sign-in of passwordless family accounts | full on the device itself; PCs it confirms get probation |
| **TOTP + recovery codes (≥64 bits)** | second factor for users without a phone; fallback for users with one; half of self-service recovery | probation |
| **Password** (optional) | first factor only, always followed by a key-device confirmation or TOTP / a recovery code | – |
| **OIDC** | first factor through the IdP | probation, unless the IdP asserts phishing-resistant MFA |

**Key devices:**

- **Two Keystore keys**, both non-exportable and hardware-backed (TEE, StrongBox where available):
  - `K_approve` (P-256): signs approvals, device-key sign-in and device step-up. UV on every use.
  - `K_session` (P-256, no UV): signs a proof on every refresh, so a copied refresh token is useless
    off the phone (B.4).
- **User verification (CC1):**
  - Setup requires a secure lock screen or a Class 3 biometric. Without one, setup stops with a
    guided "Turn on a screen lock" step. For a relative's phone, the admin sets a PIN during setup.
    Without a lock, a passwordless account would be protected only by "whoever holds the unlocked
    phone".
  - API 30+: per-use authentication (`setUserAuthenticationParameters(0, AUTH_BIOMETRIC_STRONG |
    AUTH_DEVICE_CREDENTIAL)`), with a BiometricPrompt and CryptoObject per signature.
  - API 26–29: a strong biometric per use (CryptoObject). On a phone with only a PIN, pattern or
    password: a key with an authentication window of at most 10 s, opened by a credential prompt
    that Kaiku shows immediately before each signature **(design call)**.
  - When Android invalidates the key (screen lock removed, or a biometric change where the key is
    bound to biometrics), the device stops being a key. The user and, for passwordless accounts,
    the admins get a notice. The key is never replaced silently.
- **Key attestation** (should; NIST §3.1.7, Android Key Attestation): at enrolment the server
  verifies the attestation chain against Google's published roots (kept updatable in config). It
  requires a hardware-backed key, `userAuthRequired`, and an `attestationApplicationId` naming one
  of the two Kaiku application ids with the release certificate (operator override for self-built
  APKs). Verified-boot state is recorded as risk information, not required, so phones with custom
  ROMs still work **(design call)**. A failure shows "This phone cannot be a key" and is never
  downgraded silently.
- **Approval screen hardening:** `FLAG_SECURE`, `filterTouchesWhenObscured` (tapjacking).
  Notifications are `VISIBILITY_PRIVATE` on the lock screen and carry no Approve action.
- **Origin pinning:** a signed-in app accepts only QR payloads whose origin equals its own server
  exactly; a scan never switches servers. An unconfigured app shows the server name before it sends
  anything (F5c).
- **May approve** (should; account flag `may_approve`): whether this account's key devices may
  confirm sign-ins on other devices and new devices of the account. Off for accounts created by
  admin setup (F5c), on for others; only an admin changes it. With the flag off, the app refuses a
  sign-in QR with a friendly explanation. QR phishing targets the least experienced users, and
  grandparents are not expected to sign in on a PC (see Q4).
- **Lifecycle:** `probation` (new, relayable enrolment; B.4) → `full` → `revoked`. A key device
  **outlives the refresh session** and signs in again with `K_approve` and UV (B.4).
  - Revoked by: an explicit logout of that device; removal by the owner (step-up); "sign out
    everywhere"; an admin; recovery; account deletion. A ban is checked at use time.
  - **Dormancy limit:** a key device unused for 180 days (configurable) can no longer sign in and
    needs a new enrolment **(design call)**.
  - Approve calls must also come from the device's current session (defence in depth).

**Passkeys (CC5; WebAuthn L3, OWASP Passkey Security cheat sheet):**

- **RP ID** = host of `PUBLIC_URL`: never a parent domain, immutable, checked at startup.
  `PUBLIC_URL` becomes mandatory, and the server fails closed without it. Changing the domain later
  invalidates every passkey and key-device signature, and the admin guide says so.
- **Origin allow-list:** exact. It holds the web origin plus `android:apk-key-hash:<sha256>` for both
  APKs, plus operator overrides for self-built APKs. Related origins (§5.11) never list
  `tauri.localhost`: every Tauri app on Windows shares that origin.
- **Registration:** `residentKey: required`, `userVerification: required`, attestation `none`,
  `excludeCredentials`. Challenges are ≥16 random bytes, single use, and bound to the ceremony,
  the session and the account.
- **Sign-in:** UV=1 is checked; UV=0 is refused, not treated as a weaker login (§5.8.6). The
  signature counter is a signal, never an automatic lockout.
- **Stored per credential:** AAGUID, BE/BS flags (§6.1.3) as risk input, transports, created,
  last used.
- **After a delete:** `signalUnknownCredential` and `signalAllAcceptedCredentials` (§5.1.10).
- **Web:** conditional mediation (autofill) in the username field, plus a "Sign in with passkey"
  button. Discoverable and username-first both work.
- **Desktop:** the system browser on Windows, macOS and Linux (F1).
- **Android:** Credential Manager.
  - The server generates `/.well-known/assetlinks.json` from config (both application ids, release
    certificate SHA-256, operator override) with `delegate_permission/common.get_login_creds` and
    `Content-Type: application/json`.
  - The RP domain must be reachable by Google: no `robots.txt` block, and the reverse proxy and
    production ingress route `/.well-known/` to the server.
  - Fallback for servers Google cannot reach: a Custom Tab running the browser ceremony, returning
    a verifier-bound one-time code via `kaiku://` (safe under RFC 8252 §8.1).
- **Policy:** a UV passkey assertion is a complete sign-in. It produces a full session and counts as
  a fresh step-up; no phone approval is added on top. Registration and removal need a full session
  with a fresh step-up. Removing the last strong method (passkey or key device), or leaving an
  account password-only, is refused. Every add and remove sends a notice.
- **Library:** `webauthn-rs` (MPL-2.0, allowed). Spike first: its `openssl` dependency under
  `cargo deny` and in the server image, and acceptance of the `android:apk-key-hash:` origin.

**Passwords (CC8):**

- Optional for every account. Admin-created accounts have none.
- When set or changed:
  - length 8..128, no composition rules;
  - checked against an offline blocklist (at least the top 100k breached passwords) plus context
    words: product name, server name, username (NIST §3.1.1.2; ASVS 6.2.4, 6.2.11, 6.2.12);
  - Argon2id as today.
- The 8-character minimum stays because a password is never the only factor (NIST §3.1.1.2 asks for
  15 only when it is).
- **Compromise flag** (`password_compromised_at`): set by "That wasn't me" on a password approval
  (F3) or by an admin.
  - While set, the password path *and* the TOTP fallback are closed for the whole account.
  - It clears when the user sets a new password, or removes it, from a full session with a fresh
    step-up (passkey or key device). The UI then offers to renew TOTP and recovery codes as well.

**TOTP and recovery codes (CC7):**

- TOTP: as today.
- **Recovery codes** replace backup codes: 10 single-use codes, each with ≥64 bits from a CSPRNG
  (for example 13 base32 characters, shown in groups), Argon2-hashed (NIST §4.2.1). A code used for
  recovery is invalidated, and a new set is offered. Regenerating needs a full session with a
  fresh step-up.
- Legacy 8-character backup codes stay valid as a login second factor only (never for recovery)
  until the user regenerates them; the UI asks the user to do so.

**OIDC:**

- A first factor. After the IdP returns, the phone confirmation of F3 applies, or TOTP for
  accounts without a key device. The session is on probation.
- Exception: if the operator marks the IdP as trusted and its ID token's `amr` or `acr` asserts
  phishing-resistant MFA (`hwk`, or an agreed `acr`), the session is full without a phone
  confirmation (ASVS 6.8.4).
- OIDC re-auth counts as a step-up only when the IdP asserts MFA; otherwise it needs a key-device
  approval as well.

### B.2 The approval mechanism (CC10)

**Principle.** One `approvals` state machine with one signed-bytes format, one device table, one
push kind (`t=approval`) and one pair of WebSocket events (`approval_pending` /
`approval_resolved`). It lives in `server/src/approvals/` and ships in v1.0. A **kind** decides:
who may ask (the *requester*) and who consumes the result; who answers (the *approver*) and with
which devices; for whom (the *subject*); how the secret is transferred; the TTL and limit class;
and what consuming does. v1.1 adds two kinds and a side table; it never forks the core.

**Kinds:**

| Kind / variant | Rel. | Requester (consumer) | Approver (subject) | Answering devices | Transfer | TTL | Limit class |
|---|---|---|---|---|---|---|---|
| `login/password`, `login/oidc` (F3) | 1.0 | login attempt held by the PC | account owner | the owner's full key devices, `may_approve` on | phone scans the PC's confirmation QR; fallback: typed 6-digit code | 2 min | login |
| `login/qr` (F2) | 1.0 | login attempt (PC) | user of the scanning phone | only that scanning key device | the scan; fallback: typed 8-digit code | QR 60 s (refreshed); approval 2 min after the scan | login; exempt from open cap and pause |
| `reauth` (F7) | 1.0 | the session asking to step up | that session's user | the user's full key devices | phone scans the QR on the asking screen; fallback: typed 6-digit code | 2 min | login |
| `device_enrollment/own` (F5b) | 1.0 | pending enrolment (new phone) | account owner | one existing full key device, `may_approve` on | the existing device scans the new phone's QR | 5 min | login |
| `device_enrollment/admin_setup`, `/recovery`, `/admin_confirm` (F5c, F6, F8) | 1.0 | pending enrolment (new phone or PC) | the admin (subject: the user) | the admin's full key devices | the admin types the 6-digit code shown on the new device | setup code 10 min, single use; approval 5 min after the claim | login (the admin's) |
| `elevation` (agent window) | 1.1 | agent token | token owner | Android; desktop allowed | per v1.1 spec (an action, not a sign-in) | 5 min | agent |
| `action` (destructive agent action) | 1.1 | agent token | token owner | Android; desktop only with phone-only off | per v1.1 spec | 5 min | agent |

All Android answers need UV. Every login-class approval completes within 10 minutes (NIST
§3.1.3.2).

**Not approvals** (separate signed proofs, each with its own domain-separation tag):

- device-key sign-in (`kaiku-device-login-v1`);
- device step-up on the key device itself (`kaiku-device-stepup-v1`);
- refresh proofs with `K_session` (`kaiku-session-proof-v1`);
- passkey ceremonies (WebAuthn).

**Data model** (L1a migration; v1.1 adds only a side table):

- **`approvals`:**
  - identity: `id`, `kind`, `variant`, `approver_id`, `subject_id`, `requester_kind`
    (`login_attempt`|`session`|`enrollment`|`agent_token`), `requester_id`, `limit_class`;
  - request: `request_hash` (32 bytes), `display` (JSONB, tagged by kind), hashes of the QR
    confirmation token and of the typed code (never the plain values);
  - state: `state`, `created_at`, `expires_at`, `decided_at`, `decided_via`
    (`device`|`totp`|`session`), `decided_device_id`, `reported`, `consumed_at`, `expired_reason`;
  - checks: the kind CHECK pre-declares the two v1.1 kinds, so v1.1 needs no constraint swap. The
    TTL CHECK is a bound (`expires_at <= created_at + 10 min`).
- **`approval_devices`:**
  - identity and key: `id`, `user_id`, `device_id` (per install), `platform` (`android`|`desktop`;
    desktop rows from v1.1 only), `public_key` (approval key), `session_public_key`, `uv_bound`
    (always true for Android; desktop rows are false and therefore never eligible for login-class
    kinds), attestation result, `label`;
  - lifecycle: `session_id` (informational), `enrolled_via`
    (`reauth`|`approval`|`admin_setup`|`recovery`|`admin_confirm`), `assurance`, `probation_until`,
    `enrolled_at`, `last_used_at`, `revoked_at`, `revoke_reason`.
  - The v1.1 plan's `session_id … ON DELETE SET NULL` trigger is dropped: expired-session cleanup
    would revoke every key device.
- **`login_attempts`** (Postgres, so consumption is atomic and audited): `id`, the hash of the
  collection secret or the S256 challenge, the hash of the QR token, the hash of the code, `user_id`
  (null for a QR attempt until the scan), `via`, requester IP, user agent and geo,
  `fallback_allowed`, `expires_at`, `state`. Tokens are minted only when the attempt is consumed and
  are never stored.
- **`sessions`** gains `assurance`, `probation_until`, `auth_methods`, `auth_time`, `client_class`
  (`browser`|`desktop`|`android`), the absolute expiry, the refresh generation and the previous
  token hash, and the `K_session` public key for Android.
- **`users`** gains `may_approve` and `password_compromised_at`; the `auth_method = 'local'`
  constraint is relaxed for passwordless accounts.
- **`security_events`**: append-only, readable by the user, no delete route. A `system_audit_log`
  row for the admins is written in the same transaction.
- **v1.1 (M2):** `approval_agent_requests` (agent request columns, token FK) and
  `approval_owner_settings`.

**Signed bytes** (owned by L1a; Android uses them unchanged in v1.0, the desktop in v1.1):

```
"kaiku-approval-v1" ‖ kind[1] ‖ approval_id[16] ‖ request_hash[32] ‖ decision[1] ‖
expires_at i64 BE[8] ‖ lp(origin) ‖ approver_id[16] ‖ device_id[16] ‖ lp(response)
```

- `request_hash` = `SHA-256("kaiku-approval-request-v1" ‖ kind ‖ lp(variant) ‖ subject_id ‖
  requester_kind ‖ requester_id ‖ SHA-256(display bytes) ‖ kind-specific bytes)`.
  - Login kinds cover the attempt id, the hash of the collection challenge, the hash of the scanned
    token, the IP, the user agent and `created_at`.
  - Agent kinds cover token, method, path, content type and body, as in the v1.1 plan.
  - The display hash makes the signature cover exactly what the phone showed (ASVS 6.6.2). The
    server rebuilds every input from the stored row.
- `response` holds the digits the user typed, or is empty when the transfer was a scan. The server
  sends everything before `response`; the device appends it. The device never builds JSON and
  never learns the correct code (a hash of a 6-digit code would give it away).
- `origin` comes from `PUBLIC_URL` and fails closed when unset.
- Kind byte values and `lp()` are fixed in contract C26 with one golden vector, shared by the
  server tests, the Android tests and the v1.1 desktop tests.

**State machine:**

- `pending → approved → consumed`, `pending → denied`, `pending | approved → expired`.
- Each transition is one conditional UPDATE with its audit row in the same transaction, plus a
  1-minute sweep.
- **Consumption:**
  - login attempts: the PC's collection request, with its collection secret;
  - `reauth`: the asking session;
  - enrolments: the new device;
  - agent kinds: the creating agent token via `/execute` (v1.1).
- **Deny** stays unsigned (no UV is needed to refuse).
- **A wrong typed code** kills that approval and counts as a rejection (B.5).
- **"That wasn't me"** (report) = deny + `reported`, accepted only from a full key device's session,
  and audited. Effects:
  - the attempt dies;
  - `login/password` flags the password (CC8);
  - `login/oidc` tells the user to change the IdP password;
  - every report sends a notice and counts toward B.5.

**Delivery** (approvals must not depend on push alone):

- **Push `t=approval`:** carries `approval_id` only, with high priority, TTL = approval TTL,
  collapse key `approval-<id>` and its own budget. It bypasses the coalescer and read gate and goes
  only to devices eligible for the kind.
  - The notification text is generic ("Sign-in waiting – open Kaiku").
  - Tapping opens the scanner (F3, F7) or the approval screen (enrolment kinds). There are no action
    buttons (CC2).
- **WebSocket:** `approval_pending` goes to the live sessions of eligible devices (in v1.0 that
  includes the Android app); `approval_resolved` goes to all of the approver's sockets.
- **Opening the app** calls `GET /api/approvals?state=open`, so a late push never blocks a sign-in.
- **Dropped:** the v1.1 plan's `t=approval_device` wake. "New device added" is a security notice
  (Part C).

**Conflicts with the v1.1 owner-approvals design, resolved:**

| # | Conflict | Resolution |
|---|---|---|
| K1 | v1.1 schema is agent-only (token FK NOT NULL, agent kinds, fixed +5 min TTL) | generic core plus a v1.1 side table; TTL per kind; kind CHECK pre-declares the v1.1 kinds |
| K2 | "a desktop is never a key device" vs v1.1 desktop approves elevation | one device table with `platform`; desktop rows from v1.1 only, answering agent kinds only; never login, enrolment or re-auth (CC10) |
| K3 | v1.1 revokes a device when its session ends, including expiry | revoke on explicit end only; SET NULL trigger dropped; device-key sign-in with UV (CC6) |
| K4 | v1.1 refuses API 26–29 phones without a strong biometric; family phones may be old | UV required (CC1); PIN-only API 26–29 phones use the short authentication window; no possession-only class (see Q3) |
| K5 | v1.1: only system admins enrol; enrolment via password + TOTP or OIDC `EnrollDevice` | every user; enrolment paths in F5; v1.1 Task 4 dropped |
| K6 | the admin approves another user's device; v1.1 has only `owner_id` | `approver_id` + `subject_id` |
| K7 | consumer: an agent token vs a PC without a session | `Requester` enum; login attempts with a collection secret; tokens minted at consumption |
| K8 | TTL 2 min vs 5 min fixed in a DB CHECK | per-kind TTL in code; DB bound ≤10 min |
| K9 | shared limits let a prompt-injected agent pause the admin's sign-ins | separate limit classes and push budgets (CC9, B.5) |
| K10 | signed bytes lack kind and response | kind byte plus appended `lp(response)` (CC2, CC10) |
| K11 | unsigned deny from any session vs the side effects of "That wasn't me" | deny unsigned; report only from a full key device's session; effects account-wide (CC8) |
| K12 | notices via `t=approval_device` vs Kaiku DM | security event → log + Kaiku DM; `t=approval_device` dropped |
| K13 | v1.1 TOTP page "whenever no device is reachable" | login kinds: no TOTP page; the fallback (F4) only after expiry, on probation, closed after a report. Agent kinds keep the v1.1 TOTP page, with a notice |
| K14 | "tap the number" (1 in 3 blind taps succeed) | no number matching; scan or a typed code of ≥6 digits (CC2) |
| K15 | desktop WebAuthn "where the webview supports it" | system browser (CC5) |
| K16 | "video calls exist nowhere" | corrected (Current state, Part A) |
| K17 | stale "v1.2" wording; "passkeys not required" | fixed (CC11) |
| K18 | v1.1 spec: C24 is password + TOTP or OIDC only; devices for "system admins first" | v1.1 spec update: C24 also accepts a passkey and a key-device approval; devices exist for all users since v1.0; only the agent kinds are admin-only |

### B.3 Flows

Every flow lists the session it produces. "Notice" means a Part C notice to all of the user's
devices.

#### F1. Passkey sign-in → full session

1. **Web:** passkey autofill in the username field, or "Sign in with passkey". The login page lists
   passkeys first.
2. **Desktop (Windows, macOS, Linux):** "Sign in with passkey" opens the system browser at the
   server's own ceremony page. The server then hands off as in the OIDC desktop contract:
   - loopback return with a one-time code;
   - S256 verifier, 60 s, `no-store`, `no-referrer`;
   - tokens only in Rust.

   Step-up and registration bind the flow state to the desktop session's `sid`, as the OIDC re-auth
   does. On Linux, Chromium-based browsers offer hybrid (phone via Bluetooth); Firefox does not, so
   the dialog says "No phone option? Use 'Sign in with phone'" (F2).
3. **Android:** Credential Manager, or the Custom Tab fallback (B.1).
4. The server verifies origin, RP ID hash, UV, the challenge and the signature, and records BE/BS
   and last use.
5. The session is full and counts as a fresh step-up.

#### F2. Sign in with phone: QR login (was B3) → probation

1. The PC opens "Sign in with your phone".
   - It creates a login attempt and keeps a **collection secret** that never appears in the QR, a
     URL or the page:
     - desktop and second phones send only `S256(verifier)` and keep the verifier (desktop: in Rust
       memory);
     - the web client receives it as an HttpOnly, `Secure`, `SameSite=Strict` cookie scoped to the
       attempt endpoints.
   - The QR code holds the server origin and a 256-bit token. Below it, the PC shows an 8-digit code
     for phones that cannot scan.
   - QR and code refresh every 60 s, one live attempt per screen. A refresh does not reset any
     failure counter (NIST §3.1.3.2).
2. The user scans with a key device (or types the code under "Sign in a PC"). The app checks the
   origin, then claims the attempt from its own session. The server binds the attempt to that user
   and creates a `login/qr` approval.
   - The 8-digit code also locates the attempt, so it is longer than the 6-digit codes elsewhere and
     limited per device (B.5) **(design call)**.
   - With `may_approve` off, the app refuses here.
3. The **context screen** shows:
   - what is signing in (client kind first: "Kaiku desktop on Windows", "Firefox on Linux");
   - the server name;
   - the approximate location from a local GeoIP database, the IP and the time;
   - the network relation: "same network as this phone" as a positive signal, or "different
     network" as a warning line;
   - a stronger interstitial warning when the countries differ or the IP belongs to a known hosting
     or VPN range.

   The buttons are "Yes, the PC in front of me" / "No" / "That wasn't me". IP and location signals
   are hints, not controls (RFC 10027 §6.1.1); warnings that fire constantly train users to ignore
   them.
4. UV and signature, then approval.
5. The PC collects with its collection secret, and the server mints tokens at consumption.
   Photographing or relaying the QR code alone yields nothing (RFC 10027 §6.1.12; ASVS 6.6.2).
6. The session is **probation**. The notice carries "That wasn't me", which ends exactly that
   session (RFC 10027 §6.1.6).

Why this is not called phishing-resistant: a live phishing page can proxy the real QR code to a
victim, who then scans it believing it is genuine. The scan is a real transfer (NIST §3.1.3), but it
is not origin-bound (NIST §3.2.5). The combined mitigations (RFC 10027 §6.1.18) are: short-lived,
single-use, unique tokens; the collection secret; origin pinning on the phone; the context screen
and its question; `may_approve` off for family accounts; probation; and revocation from the notice.
The rest is accepted residual risk (B.6). Passkeys over hybrid are the phishing-resistant
alternative and are listed first wherever they work (RFC 10027 §6.2.3.5: FIDO cross-device
authentication SHOULD be used whenever the devices are capable).

#### F3. Password (or OIDC) + phone confirmation (was B4) → probation

1. Username + password. Per-account and per-IP throttles apply (B.5).
   - If the account's password is flagged, the push budget is used up, or approvals are paused,
     every password request for that account gets the same neutral answer, whether the password is
     right or wrong ("Password sign-in is not available right now. Use a passkey or 'Sign in with
     phone'."). The answer is never an oracle for a correct password.
2. Password correct and the account has eligible key devices: the server creates a login attempt
   and a `login/password` approval and returns `approval_required`. The PC keeps its collection
   secret (as in F2) and shows a **confirmation QR** with a 6-digit code below it.
   - No eligible key device: F4.
3. A push goes to the eligible key devices: "Sign-in waiting – open Kaiku". It opens the scanner.
   The user scans the PC's confirmation QR, sees the context screen (F2 step 3), and approves with
   UV. If the camera is not usable, the user types the 6-digit code on the phone's approval screen.
   One wrong entry kills the approval.
4. "That wasn't me" on the phone: report (B.2). The password is flagged, so the password path and
   the TOTP fallback close for the whole account (CC8). Notice.
5. The approval expires after 2 minutes. Only then does the PC offer "Phone not at hand?" (F4), so
   the owner always gets the full window to reject.
6. Success: a **probation** session plus a notice with "That wasn't me".
7. **OIDC variant:** after the IdP callback, steps 2–6 apply unless the IdP asserted
   phishing-resistant MFA (B.1). "That wasn't me" after an OIDC login tells the user to change the
   IdP password, since there is no local password to flag.

The strongest anti-fatigue control is structural: an unsolicited push cannot be approved, because
approving needs the secret on the attacker's screen. Passwordless accounts cannot be push-bombed at
all.

#### F4. TOTP or recovery-code fallback → probation

1. **Users without a key device:** password, then TOTP or a recovery code. This is their normal
   path (see Q2).
2. **Users with a key device:** offered only after a phone approval for this attempt expired, and
   only while the account's password is not flagged.
3. Never TOTP alone and never a recovery code alone.
4. Result: a **probation** session.
   - Users with key devices get a notice on them with "That wasn't me".
   - A correct password followed by repeated second-factor failures sends "Someone entered your
     correct password" (OWASP MFA cheat sheet; ASVS 6.3.5).
5. A recovery code used here is consumed; when few remain, the user is asked to regenerate them.

#### F5. Device setup

**F5a. Own first key device ("Set up your phone") → key device as full as the session it came
from.**

1. From a **full** session with a fresh step-up (passkey, or password + TOTP / a recovery code),
   the PC shows a setup QR: origin + 256-bit token, 10 min, single use.
2. The phone scans it. An unconfigured phone takes the server from the QR after showing the server
   name. The phone generates `K_approve` and `K_session` (with attestation) and claims the token.
3. The server sends a 6-digit enrolment code *only to the claiming phone*. The phone shows it, and
   the user types it on the PC. If someone else claimed the token first (a photographed screen),
   the right phone shows "already used" and nothing matches.
4. The device is enrolled. Its assurance is that of the PC session (full). Notice.

**F5b. Further key device → probation for the probation period.**

1. The new phone chooses "Sign in with my other phone". It shows a QR code with an enrolment
   request (origin + 256-bit token).
2. An existing full key device scans it and sees "Add a NEW phone as a key to your account?", with
   the model and the network relation. The user approves with UV (`device_enrollment/own`).
3. The new phone gets a session and becomes a key device in **probation**: it cannot approve
   anything for the probation period, and its own session is on probation.

   The reason: this pattern can be relayed exactly like F2 (an attacker's phone QR shown to the
   victim) **(design call)**.
4. Notice to all devices with "That wasn't me".

**F5c. Admin setup for another user (was B1) → full in person; remote: delay, then probation.**

1. In the full app or the admin panel, the admin creates the account (no password) or picks an
   existing one, and taps "Set up device". This needs a full admin session with a **fresh step-up,
   for every setup code, for any existing account**, whatever authenticators it has. The admin
   needs a key device of their own for step 5. When the target is an admin account and another
   admin exists, the second admin must also approve.
2. The admin picks **"in person"** or **"remote"**. For remote, the admin guide says: read the code
   on a call the admin starts, to a number already known.
3. A setup QR (origin + 256-bit token, 10 min, single use) appears. It never contains a password, a
   refresh token or any long-lived secret.
4. The new phone scans it, shows "Set up Kaiku for <name> on <server>?", checks the screen lock
   (CC1; the admin sets a PIN if needed), and generates its keys.
5. The phone shows a 6-digit code (sent only to it). The admin **types** it into the approval on the
   admin's own key device, with UV (`device_enrollment/admin_setup`; this is Decision 3's
   confirmation). A transfer, not compare-and-tap.
6. **In person:** effective immediately. The device and its session are full.
7. **Remote, existing account:** a delay (CC7: default 1 h, configurable). During the delay, the
   user's other devices and **all other admins** get notices with "Cancel". Afterwards the device
   becomes a key device and its session starts on **probation**.

   **Remote, new account:** no delay (there is nothing to protect yet), but the session starts on
   probation; notice to all other admins.
8. The setup is audit-logged and copied to every admin's security log. The account's `may_approve`
   stays off unless the admin turns it on.
9. **Person without a phone:** the setup code can be opened as a link on their PC. It then
   registers a passkey instead of a key device, with the same code confirmation.

**F5d. Passwordless accounts (was B2).**

- Accounts set up by F5c have no password.
- Their sign-in paths are:
  - the key device itself (device-key sign-in);
  - a passkey;
  - QR login on another device, if `may_approve` is on.
- Any user may remove their password from a full session with a fresh step-up, once they have a
  passkey or a key device.
- The DB constraint for `auth_method = 'local'` gets a relaxed rule or a new method; the builder
  chooses.

#### F6. Recovery (was B6) → never weaker than sign-in

In order of preference:

1. **Another key device or a passkey:** sign in with it, add the new phone (F5a or F5b), and remove
   the lost device with a step-up from a method other than that device.
2. **Self-service with two factors** (NIST §4.2.5.2; ASVS 6.4.3, 6.4.4): sign in with two of
   password, TOTP and a recovery code (probation session), then choose "I lost my phone / passkey".
   - A **recovery request** starts with the same delay as remote admin recovery (default 1 h)
     **(design call)**. Notices with "Cancel" go to the account's remaining devices and to all
     admins (NIST §4.2.6).
   - After the delay, that session may enrol one new key device or passkey (which starts on
     probation) and set a new password. This is how a full user replaces a forgotten password
     without e-mail (Decision 2).
   - The recovery code used is invalidated, and a new set is issued (NIST §4.2.1).
   - Passwordless users without TOTP recover only through an admin, and the UI says so.
3. **Admin recovery, in person or remote** (`device_enrollment/recovery`, as F5c): the admin
   removes the lost device and runs the setup with the typed code. Remote recovery gets the delay,
   notices to the user's other devices and all other admins, and probation.
4. **Host CLI** (last resort, when the only admin has lost everything): audited, and it sends the
   same notices. A second admin avoids needing it; the design does not require one.

**After every successful recovery** (ASVS 7.4.3; OWASP Passkey Security cheat sheet):

- the devices reported lost are revoked;
- all other sessions end, except key devices the user or admin explicitly keeps;
- the user sees the list of authenticators for review;
- notices go to the user and the admins.

**A lost sole phone is removed without that phone** (NIST §4.3; ASVS 6.5.6): by an admin, or by any
step-up method other than that device, or at the end of a self-service recovery.

#### F7. Step-up (re-auth, C24)

- **Methods:**
  - a passkey (UV);
  - on a key device, a local UV signature (`kaiku-device-stepup-v1`);
  - for a PC session, a key-device approval (`reauth`) with a transfer (the asking screen shows a QR
    and a 6-digit code);
  - password + TOTP or a recovery code;
  - OIDC re-auth only with an MFA `amr` (B.1).

  Never TOTP alone and never a password alone (ASVS 7.5.1).
- **Freshness:** at most 5 min, bound to the `sid`.
- **Probation sessions** can step up only with a passkey, which also upgrades them (B.4).
- **Required for** (ASVS 7.5.1, 7.5.2):
  - adding or removing key devices, passkeys or TOTP;
  - setting, changing or removing the password;
  - regenerating recovery codes;
  - ending other sessions;
  - setup codes;
  - admin elevation;
  - changing `may_approve`;
  - deleting the account.

  Ending a key device's session is shown as "Remove device".

#### F8. Migration and new accounts (v1.0)

The family server already has accounts (RC testing): some have password + TOTP, some a password
only. None has a key device.

- **Existing sessions** stay valid as full sessions. The absolute caps (B.4) apply from their
  original `created_at`, so they end within 30 / 90 days. Exception: sessions of password-only
  accounts become `setup_only`.
- **Password + TOTP accounts:** new sign-ins are on probation (F4). A one-time "Set up your phone"
  prompt (F5a) appears in any full session, including the pre-upgrade ones; "Add a passkey" is
  offered next to it.
- **Password-only accounts** (and OIDC-only accounts whose IdP asserts no MFA) get a `setup_only`
  session. It can only start an enrolment of a key device or passkey (CC8), or of TOTP (TOTP is
  included here as a design call). An admin completes that enrolment by typing the code shown on
  the user's device (`device_enrollment/admin_confirm`; in-person / remote rules as F5c).

  The reason: the password may already be known to someone else, so it alone must not bind the
  authenticator that will own the account (NIST §4.1.2.1).
- **Phone-less users** keep password + TOTP and are offered passkeys first (see Q2).
- **Legacy backup codes** keep working for login only (B.1).
- **New accounts** (registration under `registration_policy`, or the first admin on a fresh
  server) start `setup_only` until they bind a passkey (full) or a key device (F5a) or TOTP in the
  same session. No admin is needed here: nobody else can know a credential that was created a
  moment ago.
- **Removed:** `/auth/qr/create`, `/auth/qr/redeem` (a session without confirmation),
  `/auth/forgot-password`, `/auth/reset-password`.
- **Admins see per user:** key devices, passkeys, TOTP, password set or flagged, and the assurance
  of open sessions.

### B.4 Sessions

**Assurance classes:**

| Class | Created by | May |
|---|---|---|
| `full` | passkey (UV); device-key sign-in; in-person admin setup; enrolment from a full session (F5a); a new account binding its first authenticator; an upgraded probation session | everything, with step-up where F7 requires it |
| `probation` | QR login; password or OIDC + phone; TOTP or recovery-code fallback; self-service recovery; remote admin setup or recovery; F5b devices | chat, calls, profile name and photo, notification settings, read the security log, sign itself out, start a recovery request (F6) |
| `setup_only` | password-only legacy accounts; new accounts without a second factor | start an enrolment (F8), sign out |

**Probation (CC4) cannot:**

- add, remove or rename sign-in methods (passkeys, key devices, TOTP, recovery codes, password);
- end other sessions or devices, including through "That wasn't me" on someone else's notice;
- create setup codes;
- elevate as admin;
- approve anything (a probation key device is not eligible);
- change `may_approve`;
- delete the account.

The one exception is a **completed recovery request** (F6). Its delay with notices is the
confirmation for exactly one enrolment, plus the clean-up that recovery performs. The enrolled
device starts on probation.

**A probation session becomes full by:**

1. **a passkey step-up** in that session (immediate; phishing-resistant);
2. **"Trust this sign-in"**, chosen on a full key device under Security → Sessions, with UV. It is
   initiated there and never offered in a push or in the notice, so it cannot be prompted or spammed
   (RFC 10027 §6.1.15);
3. **automatically after the probation period** (default 24 h, configurable) **(design call)**, if
   no one used "That wasn't me". The sign-in notice with its revoke action went out at the start.
   The user's full devices can end the session at any time during the period.

Server-side, two guards enforce this: "full session" and "fresh step-up". Every route in F7's list
uses both.

**Lifetimes (CC6):**

| | Browser | Desktop | Android |
|---|---|---|---|
| Access JWT | 15 min | 15 min | 15 min |
| Refresh, inactivity (sliding) | 7 d | 7 d | 7 d |
| Absolute, from `created_at` **(design call)** | 30 d | 90 d | 90 d, then device-key sign-in with UV |

- **Documented deviation** (ASVS 7.1.1): NIST §2.2.3 says reauthentication at AAL2 SHOULD happen at
  least every 24 h and after 1 h of inactivity. Kaiku is a messaging and calling client used daily.
  It compensates with:
  - reuse detection;
  - device-bound refresh on Android;
  - notices with revocation;
  - probation for relayable sign-ins;
  - step-up for every sensitive action.

  The absolute caps hold even with proof of possession (NIST §5.1).
- **Renewal on Android:** the app renews proactively at a normal app open shortly before the cap,
  so it never asks for UV in the middle of an incoming call. After a lapse, the prompt is the
  phone's own screen lock, worded "Confirm it's you", never a password.

**Refresh rotation with reuse detection** (RFC 9700 §2.2.2, §4.14.2):

- Refresh rotates on every use, in place (the stable `sid` stays).
- The server keeps the hash of the current token and of its predecessor.
- The immediate predecessor presented within a short grace window (≤10 s, for concurrent
  refreshes) gets a non-revoking retry answer.
- Any other superseded token ends the session through `revocation::end_sessions`, with the notice
  "A session was ended because its key was used twice".

**Device-key sign-in** (K3; NIST §5.1):

- **After a lapse** (inactivity or absolute cap): a `K_approve` signature with UV over a server
  challenge. This is a fresh multi-factor cryptographic authentication, not a session extension,
  and it gives a full session. It is rate-limited and audited.
- **After an end** (logout of that device, removal, revocation, ban, account deletion, recovery,
  dormancy limit): impossible. The device needs a new enrolment.
- Never silent, and never with a key that needs no UV.

**Device-bound refresh** (should; RFC 9449 pattern; NIST §5.1):

- **Android:** every `/auth/refresh` carries a `K_session` signature over a server nonce, and the
  refresh token alone is refused.
- **Browser:** the refresh cookie gets a `__Secure-` prefix, HttpOnly and `SameSite=Strict`. DBSC
  later.
- **Desktop:** the keyring as today; a key binding is optional later.
- Android token storage moves off `security-crypto` to Keystore-wrapped storage.

**Ending sessions:**

- Signing out the current session is always allowed.
- Ending other sessions needs a full session and a step-up (ASVS 7.5.2).
- "That wasn't me" from a full session ends the reported probation session at once, without a
  step-up (remediation; RFC 10027 §6.1.6).
- After an authenticator change, the UI offers "Sign out everywhere else" (ASVS 7.4.3).

### B.5 Limits and fatigue (was B5)

All values are configurable; the defaults below are the builder's starting point. All rejects,
pauses, blocks and budget exhaustions are audit-logged and visible to admins. ASVS 6.1.1 asks that
lockout abuse be documented: these are the rules.

- **No approvable unsolicited prompts** (CC2): the base of the design. A push only opens the
  scanner.
- **Push budget since the last success** (NIST §3.1.3.2):
  - at most 5 unanswered or rejected login prompts per account since the last successful sign-in;
  - after that, password and OIDC sign-ins create no more pushes until the next passkey, QR or
    device-key sign-in. The answer is the neutral F3 message.
- **Short pause:** 3 rejected or expired login approvals within 15 min pause login pushes for that
  account for 15 min. QR login, passkeys, device-key sign-in and (where allowed) TOTP stay
  available, because they need the physical phone or a secret the attacker lacks.
- **Per-account throttling** (CC9; NIST §3.2.2):
  - separate counters for the password and for the second factor (TOTP, recovery code, typed
    approval code), across all IPs;
  - after 5 consecutive failures, an increasing delay per account;
  - at 100 consecutive failures, that path closes for the account until a successful passkey, QR
    or device-key sign-in, or an admin reset;
  - the message suggests a passkey or QR sign-in.

  Passkeys, QR login, device-key sign-in and existing sessions are never affected, so a hostile
  guesser cannot lock anyone out of the strong paths. A phone-less password + TOTP user has no
  strong path, so for them the increasing delay is the main brake and an admin reset is the way
  back (see Q2).
- **Per IP:**
  - the existing `failed_auth` limiter;
  - N rejected approvals per hour across accounts (default 10/h) block that IP from the password
    and approval-creating endpoints for 1 h;
  - the per-IP block never applies to QR login, passkeys, device-key sign-in or signed-in sessions,
    so an attacker on the family's shared home IP cannot lock everyone out;
  - the public endpoints that create QR attempts and passkey challenges have their own request
    limits.
- **Typed QR codes:** 5 wrong 8-digit codes per device in 10 min turn typed entry off for 10 min.
- **Open cap:** at most 3 open approvals per (approver, limit class). QR login and admin setup are
  exempt, because each needs a physical scan.
- **Separate classes** (CC9, K9): `login` and `agent` (v1.1) have separate counters, pauses and
  push budgets (6 pushes per 10 min per device *and* per class). Agent denials can never pause or
  starve human sign-ins.
- **Setup codes:** at most 5 open per admin.
- **Single replica:** push budgets per device and class live in memory, as `ringing` does. The
  module docs say so.

### B.6 Residual risks and documented deviations

1. **Real-time relay** of F2, F3 and F5b by a phishing page that proxies the QR code. This is
   accepted, and limited by CC3 + CC4 and `may_approve`. Passkeys are the phishing-resistant path
   (NIST §2.2.2 is met by *offering* them).
2. **A social-engineered admin** (remote setup or recovery). It is limited by the typed code, the
   delay, and the notices to other admins and devices. A single-admin server has no second pair of
   eyes; a second admin is recommended.
3. **IP and location signals are unreliable** (VPNs, borders; RFC 10027 §6.1.1). They are hints
   only.
4. **A stolen phone plus a known screen lock** equals the key, as with any phone authenticator.
   The owner removes it through an admin or another method (F6).
5. **Linux:** Firefox has no hybrid passkeys, so QR login (probation) is the fallback there.
6. **Deviations:**
   - session lifetimes (B.4, NIST §2.2.3);
   - the 8-digit typed QR code relies on rate limiting (B.5);
   - attestation does not require verified boot (B.1).

## Part C: Security notices in chat (v1.0)

- A built-in **system account "Kaiku"** sends security notices into a
  read-only DM with each user. The account is refused on every auth path and hidden from user
  lists, search, presence, mentions, friend paths and bot paths.
- **Events:**
  - new sign-in (method, assurance, client, approximate location);
  - new key device, passkey or TOTP; recovery codes regenerated;
  - an authenticator removed; password set, changed or removed;
  - approval rejected; "That wasn't me" used; approvals paused; push budget used up;
  - correct password with a failed second factor; password flagged compromised;
  - recovery started (with "Cancel"), completed or cancelled;
  - admin setup or recovery for the account;
  - a session ended by refresh reuse;
  - a key invalidated by the phone.
- Each notice also goes out as a push to all the user's devices. Its pushes bypass Kaiku's own
  notification settings.
- **Notices about a probation session carry "That wasn't me"**, which ends that session (and, for a
  password sign-in, flags the password). The action is available from full sessions only.
- **Second address** (NIST §4.6: at least two notification addresses; §4.2.6: the subscriber "or
  their designee"):
  - high-severity events are copied to every admin's security log and Kaiku DM: new key device or
    passkey, recovery, an authenticator removed, "That wasn't me", password flagged, admin setup;
  - admins see the event, user, device label and time, never message content;
  - any admin can cancel a pending remote recovery.
- The user's sessions cannot delete, mute or archive notices. Every event is also listed in a
  **security log** in the account settings (both apps; read-only in the simple app), so an attacker
  who got in cannot erase the trace.
- No e-mail notices in v1.0.

## Constraints

- `CLAUDE.md` rules apply: feature branches/worktrees, squash-merge PRs,
  license constraints (`cargo deny`; any QR, camera, credentials, biometric or Android library must
  be MIT/Apache/BSD/MPL-compatible; check the GeoIP database's data license), UI contrast rules,
  changelog, quality gates.
- `server/src/auth` and `server/src/approvals` are security critical. Every change gets a security
  review (Faramir) and an external review before merge, and the v1.0 auth changes get a final Fable
  review before the tag.
- Approvals must not depend on push alone (B.2, Delivery).
- **Lifetimes:** access 15 min and refresh 7 d sliding stay as in `server/src/auth/AGENTS.md`.
  Reuse detection, absolute caps and device-bound refresh are additions, not changes (RFC 9700
  §2.2.2; ASVS 7.3.2).
- `PUBLIC_URL` is mandatory (approval origin, RP ID), and the server fails closed and warns at
  startup without it. The reverse proxy and production ingress route `/.well-known/` to the server.
- The desktop keeps tokens and collection secrets in Rust; the webview never sees them. Android
  OIDC and any browser hand-off use the one-time code + verifier contract, never tokens in a URL
  (RFC 8252 §7.1, §8.1).
- Public repository: no private hostnames, IPs or exploit details in code, docs or PRs.
- Accessibility for older users is a requirement, not polish: large touch targets, readable text
  sizes, the contrast rules above, codes shown in groups ("123 456") with a large digit keypad, and
  no gestures as the only way to do something.

## Sequencing

v1.0 packages, per the package cut of 2026-11-03 (model-level ownership; task lists live in the
plans):

- **Gate:** this spec → three design reviews → contracts frozen:
  - C26: approvals wire and golden vector;
  - C27: login attempts, QR login, setup-code payload;
  - C28: security events and the Kaiku DM;
  - C29: passkeys (RP ID, origins, assetlinks, desktop hand-off).
- **LP (passkeys, server + all clients), off the critical path, merges first:**
  - the WebAuthn profile (B.1), desktop via the system browser, Android Credential Manager,
    `assetlinks.json`;
  - passkey as a C24 re-auth method;
  - the `issue_session()` extraction.

  No probation sessions exist before L1b, so LP's "fresh re-auth" gate is complete for its own
  merge; L1a adds the full-session check to LP's routes.
- **L1a (server: approvals and key devices) ★:**
  - B.2 in full;
  - key devices (UV class, attestation, two keys, lifecycle, dormancy), device-key sign-in, device
    step-up;
  - session assurance and the two guards, refresh reuse detection, absolute caps, `K_session`
    proofs;
  - `security_events` and the Part C plumbing (system account, read-only DM).
- **L1b (server: sign-in flows) ★, stacked on L1a:**
  - F2–F8, B.5 including per-account throttling;
  - the password blocklist and compromise flag, recovery codes ≥64 bits, recovery requests with
    delay, admin setup with the typed code;
  - the CLI, the notices for all events, OIDC `amr` handling;
  - removal of the old QR and e-mail reset routes.

  The standards check adds scope here. If L1b passes the review threshold, the smallest split is
  B.5 + F6 + the CLI as a follow-up PR, still before RC3.
- **L2 (clients: sign-in and key-device UX, desktop/web + Android) ★:**
  - desktop/web: confirmation QR, QR login panel, setup-code panels, probation UI, step-up via
    phone, security log, read-only Kaiku DM;
  - Android: two Keystore keys and the UV rules, scanner and approval screens, context screen,
    typed-code keypad, F5 flows, device-key sign-in, `K_session` refresh proofs, storage migration.

  L2 builds against the frozen contracts and merges after L1a + L1b.
- **Order:** L1a → L1b (stacked) → L2 merge → RC3 → device tests (Fabian) → Fable final review of
  all auth changes → v1.0 tag. LP runs beside it. No RC is cut between the L1b and L2 merges.
- **v1.1** (M1 admin MCP core, M2 agent approvals on the L1a mechanism, M3 agent bots) and **v1.2**
  (E1 simple app + group calls, E2 1:1 video + voice messages): their PRs may be built early but
  **stay open until the v1.0 tag**, unless the maintainer cuts a release branch (Q5).
- The v1.1 spec gets the K18 update; its desktop approval device answers agent kinds only.

## Left to the builder

- Module/flavor split of the Android codebase.
- Tables, Valkey keys and WS vs push payload details, within B.2.
- QR payload encoding (scheme and URL shape), as long as it carries the origin and no long-lived
  secret.
- Exact TTLs and limit defaults within the stated bounds (login-class approvals ≤10 min).
- The grace-window mechanism for concurrent refreshes.
- How the API 26–29 PIN-only case is implemented, within the 10 s window rule.
- Choice of WebAuthn crate after the LP spike; choice of local GeoIP database (license check).
- How the system account and the read-only DM are modelled.
- Wording and layout of the context, approval and setup screens (accessibility rules apply).
- Video call implementation details (codec, simulcast or not, bandwidth); the voice-message codec
  matrix.

## Open questions for the maintainer

1. **Recovery delay:** remote admin recovery waits 1 h by default (CC7), and this spec uses the same
   delay for self-service recovery. Keep 1 h, or choose longer (the standards check suggested up to
   24 h)?
2. **Phone-less desktop users:** must they register a passkey, or is password + TOTP (every new
   sign-in on probation for 24 h) acceptable as their everyday login?
3. **No screen lock:** what happens to a relative who will not use a PIN, pattern or biometric? As
   written, that phone cannot be a key, so a passwordless account cannot sign in on it. The
   alternative is a possession-only "chat only" device that never approves anything and needs the
   admin again after each lapse. That is below the standards bar.
4. **Family members approving PC sign-ins:** `may_approve` is off for admin-set-up accounts, so they
   cannot sign in on a PC by QR. Keep that default, or turn it on per person?
5. **Release branch:** cut `release/1.0` at RC3 so v1.1/v1.2 can merge to main earlier, or keep
   them open until the v1.0 tag?

## References

- NIST SP 800-63B-4: https://pages.nist.gov/800-63-4/sp800-63b.html
- OWASP ASVS 5.0.0 (V6, V7): https://github.com/OWASP/ASVS/tree/master/5.0/en
- RFC 10027 / BCP 247, Best Current Practice for Security of Cross-Device Flows:
  https://www.rfc-editor.org/rfc/rfc10027
- W3C WebAuthn Level 3: https://www.w3.org/TR/webauthn-3/
- RFC 9700 (OAuth 2.0 Security BCP), RFC 8252 (OAuth for native apps), RFC 9449 (DPoP), RFC 9562
  (UUIDs)
- OWASP Multifactor Authentication and Passkey Security cheat sheets:
  https://cheatsheetseries.owasp.org/
- CISA, Implementing Number Matching in MFA Applications (2022)
- Android Credential Manager and Digital Asset Links:
  https://developer.android.com/identity/sign-in/credential-manager
