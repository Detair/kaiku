# Kaiku: phone-as-key login (v1.0) and simple Android app (v1.2)

**Status:** Revision 2, for the maintainer and a confirming design review. First draft 2026-11-02.
Revision 1 (2026-11-03) added the maintainer decisions, a standards check and the approvals model
from the package cut. Revision 2 (2026-11-03) resolves every blocker and major issue of three
independent design reviews (Fable, Codex, Gemini Argon; all three returned NOT READY). The appendix
"Review response (round 1)" maps each finding to its resolution.
**Target:** split release (maintainer decision 2026-11-02):

- **v1.0** ships the new login: Part B (sign-in with passkeys and the phone as key) and Part C
  (security notices). The family server does not go live with the old login design. This is a
  deliberate exception to the v1.0 feature freeze.
- **v1.2** ships Part A: the simple Android app ("Kaiku Einfach"), 1:1 video calls and voice
  messages. Planned after v1.1 (admin MCP / agent bots); its Android UI work may start in parallel.

**How to read this document:** the maintainer decisions bind. Coordinator calls (CC1–CC11) are
binding for the builder unless the maintainer or a design review overturns them; calls changed in
revision 2 are marked **(rev 2)**. Items marked **(design call)** are choices made where neither the
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

**Revision 2 note.** The section above is kept word for word. Revision 2 refines how three of the
decisions are applied, without overturning any of them:

- **Decision 3:** in-person and remote admin recovery both stay, and both still end with the typed
  code on the admin's own key device plus admin re-auth (F5c step 5). They are now two different
  protocols: "in person" must be proven (the setup code is shown on the admin's phone, and the new
  phone must be on the same network), and remote needs a recorded call-back check and, for an
  existing account, a 24 h delay (CC7, F5c, SM5).
- **Decision 2:** self-service recovery takes a recovery code plus the password or a TOTP code (F6).
  A user who still has a key device or passkey simply signs in with it and sets a new password.
- **Decision 5:** QR sign-ins stay restricted (probation) until a passkey or a transfer from a full
  key device upgrades them; time never upgrades anything (CC4).
- F5b (enrolment started on the new phone) is removed; adding a phone always starts on something
  already trusted (F5a). "F5c step 5" still names Decision 3's confirmation.

## Coordinator calls 2026-11-03

Each item is a **coordinator call** for the maintainer to review. Together they replace the earlier
builder calls (table at the end of this section). Changes in revision 2 are marked **(rev 2)**; the
findings behind them are in the appendix.

- **CC1 (coordinator call): key devices require user verification.** A key device needs a screen
  lock (PIN, pattern or password) or a Class 3 (strong) biometric. Setup checks for one and walks
  the user (or the admin setting up a relative's phone) through turning on a screen lock. Every
  signature with the approval key requires user verification (UV). There is no possession-only key
  class (resolves K4 against the package cut's proposal). Sources: NIST §3.1.3 ("each use … SHALL
  require the presentation of the activation factor" for multi-factor OOB), NIST §3.1.7, OWASP MFA
  cheat sheet (user presence alone is not a factor). Details in B.1.
- **CC2 (coordinator call, rev 2): no number matching; a real secret moves to the approving device,
  and the signature carries it.**
  - Password login (F3), QR login (F2), step-up (F7) and Trust (B.4) are confirmed by the phone
    **scanning a QR code shown on the PC**.
  - Revision 2 drops the typed fallback in this direction (a code shown on the PC and typed on the
    phone): a code can be dictated in a phone scam, and passkeys and the TOTP fallback cover a phone
    whose camera does not work. Typed codes remain only where a *new* device shows a code and the
    trusted side types it (enrolment, F5).
  - The transferred secret is the signed `response`, and the server checks it before it accepts the
    signature. There is no approval without a transfer (B.2).
  - v1.0 sends no approval push: every approval starts with a scan or a typed code on the approving
    device. There is no compare-and-approve and no choose-from-list anywhere.

  Sources: NIST §3.1.3 (compare-and-approve "no longer considered acceptable"; a list of secrets to
  compare "is not sufficient"; transfer by typing or scanning a QR code), NIST §3.1.3.2 (≥6 decimal
  digits, ≤10 min).
- **CC3 (coordinator call, rev 2): cross-device flows; what each control does.** Revision 1
  overstated some controls. Each one, and its limit:
  - **Collection binding:** the PC keeps its own collection secret, and tokens are handed out only
    against it. A bystander who photographs the QR code, or anyone who learns an attempt id, cannot
    collect the session. It does **not** stop QR phishing: an attacker who shows a victim the
    attacker's own QR code also holds that code's collection secret.
  - **Short-lived, single-use, unique tokens:** QR, attempt and setup tokens are 256-bit CSPRNG
    values, never UUIDv7 (RFC 10027 §6.1.2–6.1.4; ASVS 7.2.3; RFC 9562 §8).
  - **Context and a clear decline:** the phone shows what is signing in, the server, the time and
    the IP, the approximate location where a local database is installed, a "different network"
    warning, and a prominent "No" (RFC 10027 §6.1.14). Network and location are never shown as
    reassurance (§6.1.1: they are unreliable).
  - **Proximity only to refuse:** the two ceremonies that turn a relayable result into full control
    (Trust, B.4; in-person admin setup, F5c) are refused unless both devices share a network
    (§6.1.1). A shared network never grants anything by itself.
  - **Limited scope and remediation:** relayable sign-ins yield probation sessions (CC4; §6.1.9 notes
    that this limits the damage, it does not prevent the attack), and each one can be ended from its
    notice (§6.1.6).
  - **Residual risk:** a real-time relay of F2 or F3 still yields a probation session, which can read
    the user's chats and write as the user (B.6). Passkeys over the hybrid transport are the
    phishing-resistant path and are offered first (§6.2.3.5; NIST §3.2.5: manually transferred
    outputs are not phishing-resistant). Combined controls: §6.1.18; ASVS 6.6.2.
- **CC4 (coordinator call, rev 2): probation sessions; nothing is upgraded by time.**
  - Every sign-in that is not phishing-resistant (QR login, password or OIDC + phone, the TOTP or
    recovery-code fallback) and every session that existed before the upgrade is a restricted
    *probation* session: chat, calls and the own profile; no change of sign-in methods, no ending
    other sessions or devices, no setup codes, no elevation, no approvals.
  - A probation session becomes full only by **(1)** a passkey assertion in that session or **(2)**
    the Trust ceremony: a transfer from a full key device, started on that device, bound to that one
    session, and refused unless both devices share a network (B.4). Revision 1's automatic upgrade
    after 24 h is removed.
  - **Assurance belongs to the authenticator.** A session never has more assurance than the
    authenticator that created it, so re-signing in, refreshing or waiting cannot raise it (SM6).
    v1.0 creates no probation authenticators: an enrolment either completes as full, or stays
    pending and can do nothing (B.7).

  Sources: RFC 10027 §6.1.9 (limited scopes reduce the impact), §6.1.6 (detect and remediate);
  OWASP MFA cheat sheet (fallbacks must not be weaker than the primary).
- **CC5 (coordinator call, rev 2): passkey architecture.** Web uses WebAuthn directly. The desktop
  always uses the system browser and the existing desktop OIDC hand-off pattern (one-time code, S256
  verifier, loopback), on Windows, macOS and Linux. The one-time code reaches the desktop only
  through the loopback redirect; there is no polling endpoint, so a ceremony link forwarded to
  someone else delivers its code to that person's computer, not the attacker's. Android uses
  Credential Manager with `assetlinks.json`; the Custom Tab fallback is cut (deferred). The RP ID is
  the host of `PUBLIC_URL`; the origin allow-list is exact (both encodings of the APK key hash are
  fixed in C29); UV is required; the BE flag is fixed per credential. Sources: WebAuthn L3 §5.8.6,
  §6.1.3, §5.1.10, §5.11; OWASP Passkey Security cheat sheet; Android Credential Manager docs;
  RFC 8252 §7.1, §8.1.
- **CC6 (coordinator call, rev 2): sessions.** Refresh tokens rotate, and the server recognises every
  token it has issued for a session until the session ends, so any reuse ends the session. One lost
  response is recovered by a single re-rotation within 10 s; clients serialise refreshes. Absolute
  session lifetimes are documented, with the deviation from NIST stated. A key device signs in again
  after a lapse only with UV, only while it is active, and only with its own assurance. A plain
  sign-out on a phone keeps its key; "Remove this phone" revokes it. Device-bound refresh
  (`K_session`) is deferred to v1.x; Android keeps tokens in Keystore-wrapped storage. Sources:
  RFC 9700 §2.2.2, §4.14.2; ASVS 7.1.1, 7.3.1, 7.3.2, 7.4.1; NIST §2.2.3, §5.1.
- **CC7 (coordinator call, rev 2): recovery.**
  - **Self-service** needs a recovery code plus the password or a TOTP code (NIST §4.2.2.2: a
    recovery code plus a bound authenticator). It has its own entry, separate from sign-in, so it
    works while the password path is closed. It yields a recovery-only capability, not a session. It
    waits 24 h by default (was 1 h). Owner evidence (any use of the account's key device or passkey)
    cancels it automatically; the owner, the recovery contact and any admin can cancel it. A cancel
    blocks self-service recovery for 7 days.
  - **Admin recovery** has two protocols. In person must be proven: the setup code is shown on the
    admin's own key device, and the new phone must be on the same network. Remote needs a recorded
    claimant check (the admin calls back on a channel known before the request) and, for any
    existing account, a 24 h delay with the same cancel paths. Admin accounts always get the delay.
  - **Lost devices** named in a recovery are revoked when the new authenticator becomes active, not
    at the start. "Remove device now" is a separate, immediate, audited admin action (NIST §4.3).
  - Recovery codes have ≥64 bits (NIST §4.2.1.1). Notices go to the user's devices, the recovery
    contact and all admins (NIST §4.2.3, §4.6).

  Sources: also ASVS 6.4.3, 6.4.4, 6.5.6.
- **CC8 (coordinator call, rev 2): passwords.** A password is optional. When one is set, it is checked
  against a breached and common password blocklist. After the upgrade there are no password-only
  accounts: a password-only or OIDC-only account gets its first key device or passkey through an
  admin setup (F5c, F8). This is stricter than NIST §4.1.2.1, which would accept AAL1 binding here.
  Once a password is flagged compromised ("That wasn't me" on a password sign-in, a cancelled
  recovery that used it, or an admin), the password path and the TOTP fallback close for the whole
  account until the password is replaced from a full session or at the end of a recovery. Sources:
  NIST §3.1.1.2 (blocklist SHALL, no composition rules), §2.2.1; ASVS 6.2.4, 6.2.11, 6.2.12, 6.3.3;
  OWASP MFA cheat sheet (correct password + failed second factor = compromised password).
- **CC9 (coordinator call, rev 2): throttling.** Failed attempts are throttled per account (password,
  second factor, typed codes, recovery entry) as well as per IP. After 5 consecutive failures the
  delay grows; at 20 the user and the admins get a notice; at 100 the authenticator is disabled for
  that account, as NIST requires. Passkeys, QR login, device-key sign-in and existing sessions are
  never affected, and a successful sign-in with one of them, a completed recovery or an admin
  re-enables it. v1.0 sends no approval pushes, so it needs no push budgets. Agent approvals (v1.1)
  get their own limit class, so they can never pause human logins (K9). Sources: NIST §3.2.2 (no
  more than 100 consecutive failures, then the authenticator is disabled); ASVS 6.1.1, 6.3.1.
- **CC10 (coordinator call, rev 2): one approval mechanism, v1.0 kinds only.** v1.0 ships the generic
  core (approver / subject / requester split, per-kind TTL, signed bytes with a kind byte and the
  transferred secret) with the v1.0 kinds only. v1.1 adds its agent kinds, side table, desktop device
  rows and approval push in its own migration; nothing in v1.0 pre-declares them. Desktop approval
  keys (v1.1) never approve logins, enrolments, step-ups or Trust. Details in B.2.
- **CC11 (coordinator call): stale statements fixed.** Old "v1.2" wording in B1, B3 and Part C now
  reads v1.0. B3's "passkeys not required" is replaced by Decision 5. "Video calls exist nowhere" is
  corrected: webcam tracks exist in voice rooms on web and desktop; Android has no camera capture.

**Superseded builder calls** (they stood in the Decisions section before revision 1):

| Earlier builder call | Now |
|---|---|
| Approvals are signed by the device (P-256 key in the Android Keystore) | kept; UV on every use (CC1); attestation deferred with a stated minimum (B.1) |
| One approval mechanism; the v1.1 owner-approvals foundation moves to v1.0 | kept, with v1.0 kinds only (CC10, B.2) |
| Adding or removing a key device needs approval on an existing key device; the first key device of a migrating account needs a fresh re-auth | adding: started on a full device or session with an action-bound step-up (F5a), or an admin setup (F5c); removing: any other strong method or an admin (CC7); the first strong authenticator of a migrating account: the bootstrap ceremony or an admin setup (F8) |
| TOTP / backup codes only after an approval expired, with a notice; closed for that attempt after "That wasn't me" | TOTP or a recovery code is offered on the confirmation screen and gives probation (F4); closed for the whole account after a report (CC8); legacy backup codes end at the upgrade (F8) |
| Admin re-auth (C24) accepts a key-device approval or a passkey | kept; the key-device approval is action-bound and carries the scanned secret (CC2, F7); password + TOTP is no longer a step-up |
| A lapsed key device signs in with its device key; token lifetimes unchanged | kept with UV, only while the device is active, with the device's assurance, a dormancy limit and absolute caps (CC6, B.4) |
| QR login warns when IP or location differ; relay risk documented | kept as a hint (CC3); the same-network check is used only to refuse (Trust, in-person setup) |

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
   authenticator. It confirms sign-ins on other devices, step-ups and new devices.
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
- **Deferred to v1.x by revision 2** (cut from v1.0 because they add complexity without adding
  security at the AAL2 target): verified Android key attestation (v1.0 keeps a minimum, B.1);
  device-bound refresh with `K_session`; the Android Custom Tab passkey fallback for servers that
  Google cannot reach; mapping an IdP's `amr` / `acr` to full assurance; OWASP-style device cookies
  that keep known clients out of the per-account delay (B.5).

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
- **Approval**: a decision by a key device on a request it reached by a transfer, answered with a
  user-verified signature that carries the transferred secret (B.2).
- **Transfer**: a secret moves to the approving side. In v1.0 the phone scans a QR code shown by the
  PC (sign-in, step-up, Trust), or the trusted side types a code that a new device shows
  (enrolment). (CC2)
- **Phishing-resistant**: bound to the server's origin, so a relaying page cannot use it (NIST
  §3.2.5). Passkeys (UV) and device-key sign-in on the key device itself are phishing-resistant.
  Transfers by scan or typing are not.
- **Session assurance**: `full` or `probation` (B.4).
- **Step-up**: a single-use grant for one named action and target, valid for at most 5 minutes and
  bound to the session (F7).
- **Owner evidence**: a UV signature or foreground use of the Kaiku app on any key device of the
  account, or a passkey assertion for the account. It cancels pending recoveries and activations
  (SM7) and decides whether an in-person setup is delayed (SM5).
- **Recovery contact**: another user whom the account holder names (by default, the admin who set
  up the account). The contact receives the high-severity notices and may cancel a pending recovery
  or activation of that account; nothing else (Part C).
- **Recovery capability**: what a self-service recovery request gives the client that started it:
  no session, only the right to finish that recovery (F6, SM4).

### B.1 Authenticators and their roles

| Authenticator | Role | Resulting session |
|---|---|---|
| **Passkey** (WebAuthn L3, discoverable, UV required) | primary phishing-resistant sign-in and step-up | full |
| **Key device** (Kaiku Android app, UV-bound key) | its own sign-in (device-key sign-in); approver for sign-ins on PCs, step-ups, Trust and enrolments; the only sign-in of passwordless family accounts | full on the device itself; PCs it confirms get probation until Trust |
| **TOTP + recovery codes (≥64 bits)** | second factor for users without a phone; fallback for users with one; a recovery code also authorises self-service recovery | probation |
| **Password** (optional) | first factor only, followed by a scan on a key device or by TOTP / a recovery code; one half of the bootstrap ceremony (F8) and of self-service recovery (F6) | – |
| **OIDC** | first factor through the IdP, followed like a password | probation |

**Key devices:**

- **One Keystore key in v1.0**, `K_approve` (P-256): non-exportable and hardware-backed (TEE,
  StrongBox where available). It signs approvals, device-key sign-in and device step-up, with UV on
  every use. The second key of revision 1 (`K_session`) is deferred (B.4).
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
    that Kaiku shows immediately before each signature **(design call; a deviation, B.6)**. The
    server accepts at most one `K_approve` signature per 10 s from such a key.
  - `setInvalidatedByBiometricEnrollment(false)`, so a grandchild adding a fingerprint does not
    destroy the key (tested on both API ranges). When Android invalidates the key anyway (screen
    lock removed), the device stops being a key. The user and, for passwordless accounts, the admins
    get a notice. The key is never replaced silently.
- **Key attestation (deferred; v1.0 minimum).**
  - v1.0: the app requests an attestation with the enrolment challenge as the attestation
    challenge and sends the certificate chain. The server stores the chain, so v1.x can check
    enrolled devices later, and records the key properties the app reports (security level,
    user-auth-required, auth timeout, invalidation flags) as risk information. It refuses an
    enrolment whose reported properties fall below this section; that catches app bugs, not a
    modified app. Every enrolment carries a UV signature by `K_approve` over the enrolment challenge
    (proof of possession).
  - v1.x verifier profile: chain to Google's published roots plus the revocation status list;
    hardware security level; `userAuthRequired` with no timeout (API 30+) or at most 10 s (API
    26–29); the attestation challenge; an `attestationApplicationId` naming a Kaiku application id
    and the release certificate (operator override for self-built APKs); verified boot recorded as
    risk information, not required. A failure shows "This phone cannot be a key" and is never
    downgraded silently.
  - Why deferring is acceptable: attestation is not required at the AAL2 target, and the relay
    attacks in B.6 use genuine phones, which attestation would accept.
- **Approval screen hardening:** `FLAG_SECURE`, `filterTouchesWhenObscured` (tapjacking).
  Notifications are `VISIBILITY_PRIVATE` on the lock screen and carry no action that approves
  anything.
- **Origin pinning:** a signed-in app accepts only QR payloads whose origin equals its own server
  exactly; a scan never switches servers. An unconfigured app shows the server name before it sends
  anything (F5c). Before it signs, the app checks the bytes it is asked to sign (B.2).
- **May approve** (account flag `may_approve`): whether this account's key devices may confirm
  sign-ins on other devices (F2, F3), trust them (B.4) and add phones (F5a). Off for accounts created
  by admin setup (F5c), on for others. **Only an admin changes it**, with the admin's step-up. With
  the flag off, the app refuses a sign-in or Trust QR with a friendly explanation and offers no "Add
  a phone"; the admin adds phones (F5c). QR phishing targets the least experienced users, and
  grandparents are not expected to sign in on a PC (see Q4).
- **Lifecycle (SM6):** `pending_activation` → `active` → `revoked`. A key device **outlives the
  refresh session** and signs in again with `K_approve` and UV (B.4).
  - Revoked by: "Sign out and remove this phone" on the phone itself (UV); removal from another
    device (step-up); an admin; completion of a recovery that names it; key invalidation by Android;
    the dormancy limit; account deletion. A plain sign-out ends only the session. A ban is checked
    at use time.
  - **Dormancy limit:** a key device unused for 180 days (configurable) is revoked and needs a new
    enrolment **(design call)**. Its user's devices get a notice 14 days before, so a spare phone in
    a drawer can be opened in time.
  - Approve calls must also come from the device's current session (defence in depth).

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
  the session and the account. Registration needs a full session with an action-bound step-up, or
  runs inside an enrolment (bootstrap, registration ticket, admin setup link, recovery; B.7).
- **Sign-in:** UV=1 is checked; UV=0 is refused, not treated as a weaker login (§5.8.6). The BE flag
  must equal the value stored at registration (§6.1.3: BE is fixed when the credential is created);
  a mismatch is refused. The signature counter is a signal, never an automatic lockout.
  `allowCredentials` is always empty (discoverable credentials), so a passkey request never reveals
  whether a username exists.
- **Stored per credential:** AAGUID, BE/BS flags (§6.1.3; BS as risk input), transports, created,
  last used, and the authenticator state (`pending_activation` | `active` | `revoked`, SM6).
- **After a delete:** `signalUnknownCredential` and `signalAllAcceptedCredentials` (§5.1.10).
- **Web:** conditional mediation (autofill) in the username field, plus a "Sign in with passkey"
  button. Discoverable and username-first both work.
- **Desktop:** the system browser on Windows, macOS and Linux (F1).
- **Android:** Credential Manager.
  - The server generates `/.well-known/assetlinks.json` from config (both application ids, release
    certificate SHA-256, operator override) with `delegate_permission/common.get_login_creds`, plus
    `delegate_permission/common.handle_all_urls` for the setup-link path (F5c), and
    `Content-Type: application/json`.
  - The RP domain must be reachable by Google: no `robots.txt` block, and the reverse proxy and
    production ingress route `/.well-known/` to the server.
  - A server that Google cannot reach has no passkeys inside the Android app in v1.0; the app itself
    is the key there (F5). The Custom Tab fallback of revision 1 is deferred.
- **Policy:** a UV passkey assertion is a complete sign-in. It produces a full session; no phone
  approval is added on top. A sign-in is not a step-up: sensitive actions ask for their own (F7).
  Removing the last strong authenticator, or leaving an account password-only, is refused. Every add
  and remove sends a notice.
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
- **Compromise flag** (`password_compromised_at`): set by "I didn't start this" or "That wasn't me"
  on a password sign-in (F3, F4), by a cancelled recovery that used the password (SM4), or by an
  admin.
  - While set, the password path *and* the TOTP fallback are closed for the whole account (neutral
    answer, B.5), and the password cannot be used for recovery.
  - It clears when the user sets a new password, or removes it, from a full session with a
    step-up, or at the end of a recovery that sets a new one. The UI then offers to renew TOTP and
    recovery codes as well.

**TOTP and recovery codes (CC7):**

- TOTP: as today.
- **Recovery codes** replace backup codes: 10 single-use codes, each with ≥64 bits from a CSPRNG
  (for example 13 base32 characters, shown in groups), Argon2-hashed (NIST §4.2.1.1).
  - Uses: the second factor in F4 and in the bootstrap ceremony (F8); together with the password or
    a TOTP code, self-service recovery (F6).
  - A used code is invalidated. After a recovery, the whole set is replaced and shown once.
  - Created in the bootstrap ceremony, at registration (with TOTP), after a recovery, or from a full
    session with a step-up.
- Legacy 8-character backup codes stop working at the upgrade (F8).

**OIDC:**

- A first factor. After the IdP returns, the PC shows the confirmation QR of F3, or asks for TOTP
  or a recovery code (F4) when the account has no eligible key device. The session is on
  probation.
- No trusted-IdP exception in v1.0. An `amr` value such as `hwk` (RFC 8176) names a method that was
  used; it does not establish phishing-resistant MFA with UV. Mapping IdP claims to full assurance
  is deferred.
- OIDC re-auth is not a step-up. With the existing `auth_time` freshness check it can serve as the
  first factor of the bootstrap ceremony (F8).
- OIDC-only accounts without TOTP are single-factor and migrate like password-only accounts (F8).

### B.2 The approval mechanism (CC10)

**Principle.** One `approvals` state machine with one signed-bytes format and one device table. It
lives in `server/src/approvals/` and ships in v1.0. A **kind** decides: who may ask (the
*requester*) and who consumes the result; who answers (the *approver*) and with which device; for
whom (the *subject*); how the secret is transferred; the TTL and limit class; and what consuming
does. v1.0 defines only its own kinds; v1.1 adds its agent kinds, side table, desktop rows and
approval push in its own migration and never forks the core.

**Claim by transfer (rev 2).** An approval exists only after the approving device has presented the
transferred secret:

- **Scan kinds:** the device sends the scanned token from its own session
  (`POST /api/approvals/claim`). The server finds the request by the token's keyed hash, checks that
  the device is eligible, creates the approval and returns the bytes to sign.
- **Typed kinds:** the approver's device sends the code typed for the enrolment that is open on its
  screen. A wrong code counts against that enrolment (three tries, SM2).
- v1.0 has no list of open approvals to sign and no approval push. `GET /api/approvals?state=open`,
  `t=approval` and the `approval_pending` / `approval_resolved` events arrive with v1.1's agent
  kinds, whose requester (an agent) cannot show the owner a QR code.

**Kinds (v1.0):**

| Kind / variant | Flow | Requester (consumer) | Approver (subject) | Answering device | Transfer = signed `response` | TTL |
|---|---|---|---|---|---|---|
| `login/qr` | F2 | login attempt (PC) | user of the scanning phone | the scanning active key device, `may_approve` on | the PC's login QR token | attempt ≤10 min, token rotates every 60 s; decision ≤2 min after the claim |
| `login/password`, `login/oidc` | F3 | login attempt after the first factor | account owner | an active key device of the owner that scans, `may_approve` on | the PC's confirmation QR token | as `login/qr` |
| `reauth` | F7 | full session asking for one action | that session's user | an active key device of the user that scans | the action QR token | 2 min; the grant ≤5 min, single use |
| `session_trust` | B.4 | probation session | its user | an active key device on which "Trust a sign-in" was started, `may_approve` on, same network | the Trust QR token | 2 min |
| `device_enrollment/own` | F5a started on a key device | enrolment | the user | the key device that shows the setup QR | the code shown by the new device, typed on that key device | setup token 10 min; code 5 min, 3 tries |
| `device_enrollment/admin` (in person or remote; setup or recovery) | F5c, F6.3 | enrolment | the admin (subject: the user) | the admin's active key device | the code shown by the new device, typed by the admin | as above |

All v1.0 kinds use the `login` limit class. All answers need UV. Every login-class approval completes
within 10 minutes (NIST §3.1.3.2); delays (SM7) live in `enrollments` and `recovery_requests`, never
in `approvals`.

**Not approvals** (separate signed proofs, each with its own domain-separation tag):

- device-key sign-in (`kaiku-device-login-v1`);
- device step-up on the key device itself (`kaiku-device-stepup-v1`; it signs the action and
  target);
- enrolment proof of possession (`kaiku-enroll-pop-v1`);
- passkey ceremonies (WebAuthn).

F5a started on a PC is not an approval either: the PC session's step-up grant authorises it, and the
code typed on the PC binds the new phone (SM2).

**Data model** (L1a migration):

- **`approvals`:**
  - identity: `id`, `kind`, `variant`, `approver_id`, `subject_id`, `requester_kind`
    (`login_attempt`|`session_request`|`enrollment`), `requester_id`, `limit_class`;
  - request: `request_hash` (32 bytes), `display` (JSONB, tagged by kind), the keyed hash of the
    transfer secret (HMAC under a server key held outside the database; never the plain value, and
    never a plain hash, which a lookup table would reverse for a 6-digit code);
  - state: `state`, `created_at`, `expires_at`, `decided_at`, `decided_device_id`, `reported`,
    `consumed_at`, `expired_reason`;
  - checks: the kind CHECK lists the v1.0 kinds only. The TTL CHECK is a bound
    (`expires_at <= created_at + 10 min`).
- **`approval_devices`:**
  - identity and key: `id`, `user_id`, `device_id` (per install), `platform` (`android` only in
    v1.0), `public_key`, the attestation chain and reported key properties, `label`;
  - lifecycle: `state` (`pending_activation`|`active`|`revoked`), `activates_at`, `enrolled_via`
    (`own`|`bootstrap`|`registration`|`admin_in_person`|`admin_remote`|`recovery`|`cli`),
    `enrollment_id`, `session_id` (informational), `enrolled_at`, `last_used_at`,
    `last_owner_evidence_at`, `revoked_at`, `revoke_reason`.
  - The v1.1 plan's `session_id … ON DELETE SET NULL` trigger is dropped: expired-session cleanup
    would revoke every key device.
- **`passkeys`** (from LP) gain `state`, `activates_at` and `enrollment_id`.
- **`login_attempts`** (Postgres, so consumption is atomic and audited; SM1): `id`, `kind`
  (`qr`|`password`|`oidc`), the hash of the collection secret or the S256 challenge, the keyed hash
  of the current QR token, `user_id` (null for a QR attempt until the claim), requester IP, user
  agent and approximate location, `fallback_allowed`, `state`, `expires_at`, `consumed_at`. Tokens
  are minted only when the attempt is consumed and are never stored.
- **`session_requests`** (F7, B.4): `id`, `kind` (`reauth`|`trust`), `sid`, `action`, the target
  hash, the keyed hash of the QR token, `state`, `expires_at`. **`step_up_grants`**: `sid`,
  `action`, target hash, `via` (`passkey`|`device`|`reauth`), `expires_at`, `used_at`.
- **`enrollments`** (SM2, SM5, SM7): `id`, `variant`, `protocol` (`in_person`|`remote`|null),
  subject `user_id`, initiator (session, device or admin), keyed hashes of the setup token and of
  the code, the claimed public key or passkey credential id, the network of initiator and claimer,
  `tries`, `claimant_check` (JSONB: method and note, remote only), `lost_device_ids`, `state`,
  `created_at`, `claimed_at`, `confirmed_at`, `activates_at`, `activated_at`, `cancelled_by`,
  `cancel_reason`.
- **`recovery_requests`** (SM4): `id`, `user_id`, `factors` (`recovery_code+password` |
  `recovery_code+totp`), the hash of the recovery secret, `lost_device_ids`, `state`, `created_at`,
  `activates_at`, `ready_until`, `cancelled_by`, `cancel_reason`, `enrollment_id`. At most one open
  request per account (partial unique index).
- **`sessions`** gains `assurance`, `auth_methods`, `auth_time`, `client_class`
  (`browser`|`desktop`|`android`), the absolute expiry, the refresh generation and whatever B.4's
  refresh rule needs to recognise earlier tokens.
- **`users`** gains `may_approve`, `password_compromised_at`, `recovery_contact_id` and
  `bootstrap_until`; the `auth_method = 'local'` constraint is relaxed for passwordless accounts.
- **`security_events`**: append-only, readable by the user, no delete route. A `system_audit_log`
  row for the admins is written in the same transaction.
- **v1.1 (M2), not in v1.0:** the agent kinds, `approval_agent_requests`, `approval_owner_settings`,
  desktop device rows, the approval push and its budgets.

**Signed bytes** (owned by L1a; Android uses them unchanged in v1.0, the desktop in v1.1):

```
"kaiku-approval-v1" ‖ kind[1] ‖ approval_id[16] ‖ request_hash[32] ‖ decision[1] ‖
expires_at i64 BE[8] ‖ lp(origin) ‖ approver_id[16] ‖ device_id[16] ‖ lp(response)
```

- `request_hash` = `SHA-256("kaiku-approval-request-v1" ‖ kind ‖ lp(variant) ‖ subject_id ‖
  requester_kind ‖ requester_id ‖ SHA-256(display bytes) ‖ kind-specific bytes)`.
  - Login kinds cover the attempt id, the hash of the collection challenge, the IP, the user agent
    and `created_at`.
  - `reauth` covers the `sid`, the action and the target hash; `session_trust` covers the `sid` of
    the session being trusted, its creation time and its auth methods.
  - Enrolment kinds cover the enrolment id, the new device's public key (or passkey credential id),
    the subject and the protocol.
  - The display hash makes the signature cover exactly what the phone showed (ASVS 6.6.2). The
    server rebuilds every input from the stored rows.
- **`response` is the transferred secret and is never empty (rev 2):** the 32 scanned token bytes,
  or the ASCII digits the user typed. The server compares it with the stored keyed hash in constant
  time and only then verifies the signature and changes state. A signature without the right secret
  changes nothing. The server sends everything before `response`; the device appends it, and never
  learns a code it did not see.
- **Before signing**, the device parses the prefix and checks: `origin` equals its pinned server,
  `kind` matches the screen it shows, `approver_id` is its user, `device_id` is itself, and
  `expires_at` lies in the future. Otherwise it refuses and reports an error.
- `origin` comes from `PUBLIC_URL` and fails closed when unset.
- Kind byte values and `lp()` are fixed in contract C26 with golden vectors for a scanned and a
  typed `response`, shared by the server tests, the Android tests and the v1.1 desktop tests.

**State machine:**

- An approval is created `pending` at the claim. `pending → approved → consumed`,
  `pending → denied | reported`, `pending | approved → expired`.
- Each transition is one conditional UPDATE with its audit row in the same transaction, plus a
  1-minute sweep.
- **Consumption:**
  - login attempts: the PC's collection request, with its collection secret;
  - `reauth`: the asking session, which receives the step-up grant;
  - `session_trust`: the trusted session, whose assurance becomes full;
  - enrolments: the enrolment state machine (SM2).
- A scanned token that matches nothing creates nothing. A wrong typed code counts against the
  enrolment and the per-account counter (B.5).

**Deny, report, "That wasn't me" and Cancel (one rule, rev 2):**

| Action | Where | Accepted from | Effects |
|---|---|---|---|
| **No** (deny) | context screen after a scan | the device that claimed the approval (unsigned) | the approval and its attempt end |
| **I didn't start this** (report) | context screen after a scan | the device that claimed the approval, with UV | as deny, plus `reported`; F3: the password is flagged (CC8); OIDC: the user is told to change the IdP password; notice; counts toward B.5 |
| **That wasn't me** | a notice about a sign-in, a new authenticator or a step-up action | a UV signature on an active key device of the account, or a fresh passkey assertion; never a probation session; not offered on the device the notice is about; not offered on accounts with `may_approve` off, where the admins and the recovery contact act | ends the reported session or revokes the reported authenticator; a password sign-in (F3 or F4) also flags the password; notice; admin copy |
| **Cancel** | a pending recovery or activation | owner evidence (automatic); the owner (UV on a key device or a passkey assertion); the recovery contact; any admin | SM7 |

A probation session can neither deny nor report anyone's approval, because it can never claim one.

**Delivery (rev 2):** v1.0 delivers no approvals. Every approval starts on the approving device with
a scan or a typed code, so there is nothing to push and nothing to spam. The PC learns the outcome
through its own attempt or request (by polling with its collection secret, or a WebSocket channel
bound to it). Security notices (Part C) are pushed, but none of them carries an action that
approves anything. The v1.1 plan's `t=approval_device` wake is dropped: "New device added" is a
security notice.

**Conflicts with the v1.1 owner-approvals design, resolved:**

| # | Conflict | Resolution |
|---|---|---|
| K1 | v1.1 schema is agent-only (token FK NOT NULL, agent kinds, fixed +5 min TTL) | generic core in v1.0 with v1.0 kinds only; v1.1 adds its kinds, side table and desktop rows in its own migration (rev 2) |
| K2 | "a desktop is never a key device" vs v1.1 desktop approves elevation | one device table with `platform`; v1.0 allows only `android`; v1.1 adds desktop rows that answer agent kinds only (CC10) |
| K3 | v1.1 revokes a device when its session ends, including expiry | revoke only on explicit removal; a plain sign-out keeps the key; SET NULL trigger dropped; device-key sign-in with UV (CC6) |
| K4 | v1.1 refuses API 26–29 phones without a strong biometric; family phones may be old | UV required (CC1); PIN-only API 26–29 phones use the short authentication window; no possession-only class (see Q3) |
| K5 | v1.1: only system admins enrol; enrolment via password + TOTP or OIDC `EnrollDevice` | every user; enrolment paths in F5 and F8; v1.1 Task 4 dropped |
| K6 | the admin approves another user's device; v1.1 has only `owner_id` | `approver_id` + `subject_id` |
| K7 | consumer: an agent token vs a PC without a session | `Requester` enum; login attempts with a collection secret; tokens minted at consumption |
| K8 | TTL 2 min vs 5 min fixed in a DB CHECK | per-kind TTL in code; DB bound ≤10 min; delays live outside `approvals` |
| K9 | shared limits let a prompt-injected agent pause the admin's sign-ins | separate limit classes; v1.1's agent pushes get their own budget (CC9, B.5) |
| K10 | signed bytes lack kind and response | kind byte plus `lp(response)` carrying the transferred secret (CC2, CC10) |
| K11 | unsigned deny from any session vs the side effects of "That wasn't me" | deny only from the claiming device; reports and "That wasn't me" need UV on a key device or a passkey (rule table above) |
| K12 | notices via `t=approval_device` vs Kaiku DM | security event → log + Kaiku DM; `t=approval_device` dropped |
| K13 | v1.1 TOTP page "whenever no device is reachable" | login kinds: no TOTP page; F4 is a separate sign-in that yields probation and closes after a report. Agent kinds keep the v1.1 TOTP page, with a notice |
| K14 | "tap the number" (1 in 3 blind taps succeed) | no number matching; a scan, or a typed code only from a new device to the trusted side (CC2) |
| K15 | desktop WebAuthn "where the webview supports it" | system browser (CC5) |
| K16 | "video calls exist nowhere" | corrected (Current state, Part A) |
| K17 | stale "v1.2" wording; "passkeys not required" | fixed (CC11) |
| K18 | v1.1 spec: C24 is password + TOTP or OIDC only; devices for "system admins first" | v1.1 spec update: C24 accepts a passkey and an action-bound key-device approval, and no longer password + TOTP; devices exist for all users since v1.0; only the agent kinds are admin-only |

### B.3 Flows

Every flow lists the session it produces. "Notice" means a Part C notice to all of the user's
devices, and for high-severity events also to the recovery contact and the admins.

#### F1. Passkey sign-in → full session

1. **Web:** passkey autofill in the username field, or "Sign in with passkey". The login page lists
   passkeys first.
2. **Desktop (Windows, macOS, Linux):** "Sign in with passkey" opens the system browser at the
   server's own ceremony page, which says "Signing in to the Kaiku desktop app on this computer".
   The server then hands off as in the OIDC desktop contract:
   - a loopback redirect with a one-time code, which is the only way the code leaves the server
     (there is no polling endpoint);
   - S256 verifier, 60 s, `no-store`, `no-referrer`;
   - tokens only in Rust.

   A ceremony link forwarded to someone else therefore delivers its code to that person's loopback,
   where it is useless without the initiator's verifier. Step-up and registration bind the flow
   state to the desktop session's `sid`, as the OIDC re-auth does. On Linux, Chromium-based browsers
   offer hybrid (phone via Bluetooth); Firefox does not, so the dialog says "No phone option? Use
   'Sign in with phone'" (F2).
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
   - There is no typed code (rev 2). A phone that cannot scan uses a passkey or the TOTP fallback.
2. The user scans with a key device. The app checks the origin, then claims the attempt from its own
   session (SM1). The server binds the attempt to that user and creates a `login/qr` approval. With
   `may_approve` off, the app refuses here. A stale code shows "This code has just changed – scan
   the new one".
3. The **context screen** shows:
   - what is signing in (client kind first: "Kaiku desktop on Windows", "Firefox on Linux");
   - the server name;
   - the time, the IP and, where the operator installed a local GeoIP database, the approximate
     location (Kaiku no longer sends sign-in IPs to an external GeoIP service);
   - "This PC is on a different network than your phone" as a warning line when it applies. There
     is no positive "same network" badge.

   The question is "Are you signing in on a PC in front of you right now?", with "Yes, sign in", a
   prominent "No", and a smaller "I didn't start this" (RFC 10027 §6.1.14). IP and location are
   hints, not controls (§6.1.1); warnings that fire constantly train users to ignore them.
4. UV and signature, with the scanned token as `response`, then approval.
5. The PC collects with its collection secret, and the server mints tokens at consumption. A
   bystander who photographs the QR code, or anyone without the PC's secret, gets nothing. A
   phishing page that shows the victim the attacker's own code is not stopped by this (CC3).
6. The session is **probation**. The notice carries "That wasn't me", which ends exactly that
   session (RFC 10027 §6.1.6). The session becomes full only by a passkey assertion or Trust (B.4).

Why this is not called phishing-resistant: a live phishing page can proxy the real QR code to a
victim, who then scans it believing it is genuine. The scan is a real transfer (NIST §3.1.3), but it
is not origin-bound (NIST §3.2.5). The combined mitigations (RFC 10027 §6.1.18) are: short-lived,
single-use, unique tokens (§6.1.2–6.1.4); collection binding; origin pinning on the phone; the
context screen with its question and decline (§6.1.14); `may_approve` off for family accounts;
probation (§6.1.9); revocation from the notice (§6.1.6); and a network-gated Trust. The rest is
accepted residual risk (B.6). Passkeys over hybrid are the phishing-resistant alternative and are
listed first wherever they work (RFC 10027 §6.2.3.5: FIDO cross-device authentication SHOULD be used
whenever the devices are capable).

#### F3. Password (or OIDC) + phone confirmation (was B4) → probation

Revision 2 keeps this flow, as the maintainer asked ("username + password, then confirmed in the
app"), and simplifies it: the phone is never prompted.

1. Username + password. Per-account and per-IP throttles apply (B.5).
   - If the password path is closed for the account (flagged; disabled after 100 failures; a
     password-only account waiting for an admin setup; or no password at all), every password
     request for that account gets the same neutral answer, whether the password is right or wrong,
     in the same time as a wrong password: "Password sign-in is not available for this account right
     now. Use a passkey or 'Sign in with phone', or ask an admin." The answer is never an oracle for
     a correct password.
2. Password correct and the account has an active key device with `may_approve` on: the server
   creates a login attempt (`login/password`, SM1) and returns `confirmation_required`. The PC keeps
   its collection secret (as in F2) and shows a **confirmation QR**: "Open Kaiku on your phone and
   scan this code." Nothing is sent to the phone.
   - No such key device: F4 if the account has TOTP or recovery codes, otherwise the neutral answer.
3. The user opens Kaiku and taps Scan (one tap from the chat list in the full app), scans the
   confirmation QR, sees the context screen (F2 step 3, plus "with your password"), and approves
   with UV.
4. "I didn't start this": report (B.2). The password is flagged, so the password path and the TOTP
   fallback close for the whole account (CC8). Notice.
5. The confirmation screen also offers "Phone not at hand?", which continues the same attempt in F4
   (when the account has TOTP or recovery codes and the password is not flagged).
6. If the attempt ends without a confirmation (expired or abandoned), the account gets the notice
   "Someone entered your password but did not finish signing in", coalesced (B.5; OWASP MFA cheat
   sheet). Its "That wasn't me" flags the password.
7. Success: a **probation** session plus a notice with "That wasn't me".
8. **OIDC variant:** after the IdP callback, steps 2–7 apply. "That wasn't me" after an OIDC login
   tells the user to change the IdP password, since there is no local password to flag.

Why no push **(design call)**: approving needs the code on the PC screen anyway, so a push would
only save one tap. Without it there is no push bombing at all, and the push budget, the pause and
the per-IP rejected-approval block that revision 1 needed only for this flow disappear. The owner
still learns about a correct password that was not confirmed (step 6). Passwordless accounts cannot
be probed for a password at all.

#### F4. TOTP or recovery-code fallback → probation

1. **Users without an eligible key device:** password (or OIDC), then TOTP or a recovery code. This
   is their normal path (see Q2).
2. **Users with a key device:** "Phone not at hand?" on the confirmation screen, while the password
   is not flagged.
3. Never TOTP alone, never a recovery code alone, and never TOTP + recovery code as a sign-in (that
   pair is a recovery, F6).
4. Result: a **probation** session.
   - Users with key devices get a notice on them with "That wasn't me".
   - A correct password followed by repeated second-factor failures sends "Someone entered your
     correct password" (OWASP MFA cheat sheet; ASVS 6.3.5).
5. A recovery code used here is consumed; when few remain, the user is asked to create new ones
   (from a full session, F7).

#### F5. Device setup

**F5a. Add a key device from something already trusted ("Set up your phone", "Add a phone") →
full.** Revision 2 makes this the only way a user adds a phone themselves: the setup code can only
come from a device or session that is already trusted, so an attacker cannot produce one to relay.

1. **Start**, on one of:
   - a full PC session, after an action-bound step-up "add a key device" (passkey, or a `reauth`
     approval from an existing key device; F7);
   - an active key device ("Add a phone"), with local UV (`may_approve` on);
   - inside the bootstrap ceremony, a registration ticket or a ready recovery (F8, F6).

   The starting screen shows a setup QR: origin + 256-bit token, 10 min, single use.
2. The new phone scans it. An unconfigured phone shows the server name first. The phone checks the
   screen lock and the hardware keystore (B.1), generates `K_approve`, and claims the token with its
   public key and a proof-of-possession signature (SM2).
3. The server sends a 6-digit code *only to the claiming phone*. The phone shows it, and the user
   types it on the starting screen (on a key device, with UV: `device_enrollment/own`). The entry can
   be corrected before it is sent; three wrong entries end the enrolment. If someone else claimed
   the token first (a photographed screen), the right phone shows "already used" and nothing
   matches; the user cancels and starts again.
4. The device is an active key device (full), and its first session is full. Notice with "That
   wasn't me" to all devices, copied to the admins and the recovery contact.

Why full is safe here: the code exists only because a trusted device or session (on a PC, one that
has just passed an action-bound step-up) asked for it, and the code typed back proves that the
person at that screen sees the new phone.

**F5b.** Removed in revision 2. Revision 1's "Sign in with my other phone" started on the
untrusted new phone, and an attacker could relay its QR code to the victim. A second phone that
only needs a session uses F2.

**F5c. Admin setup for another user (was B1): two protocols, in person and remote.**

1. In the full app or the admin panel, the admin creates the account (no password) or picks an
   existing one, and taps "Set up device". This needs a full admin session with an **action-bound
   step-up for every setup code**, for any existing account, whatever authenticators it has. The
   admin needs an active key device of their own (step 5). When the target is an admin account, the
   confirming admin must be another admin, and the delay of step 7 always applies **(rev 2; replaces
   "the second admin must also approve", which defined no transfer)**.
2. The admin picks the protocol (SM5):
   - **In person** (the person and the new phone are with the admin): it can only be started on the
     admin's own key device, which shows the setup QR on its screen. The new phone must claim it
     from the same network as the admin's phone (the same public IPv4 address or IPv6 /64), or the
     claim is refused: "Put both phones on the same Wi-Fi (or share your phone's hotspot), or use the
     remote setup."
   - **Remote:** before any code exists, the admin records how they checked who is asking. They end
     the incoming contact, call the person back on a number or account they had before this request,
     and recognise them. The app asks for the method and a short note; both go into the audit log.
     The admin sends the setup link over that checked channel.
3. A setup QR or link (origin + 256-bit token, 10 min, single use) appears. It never contains a
   password, a refresh token or any long-lived secret. Opened on a phone, the link leads straight to
   step 4 (an Android App Link), so a phone without a second screen nearby can be set up remotely.
4. The new phone scans or opens it, shows "Set up Kaiku for <name> on <server>?", checks the screen
   lock (CC1; the person chooses a PIN with the admin if needed), and generates its keys.
5. The phone shows a 6-digit code (sent only to it). The admin **types** it into the approval on the
   admin's own key device, with UV (`device_enrollment/admin`; this is Decision 3's confirmation). A
   transfer, not compare-and-tap. Three tries. In person, the admin also confirms "<name> is here
   with me"; remotely, the person reads the code out on the call from step 2.
6. **In person:** the device is active and full at once, unless the target is an admin account or an
   authenticator of the account showed owner evidence in the last 7 days. In those cases step 7's
   delay applies: the owner may still hold a working phone, and then "Add a phone" on it (F5a) is
   the honest path.
7. **Remote, new account** (created in this setup, so there is nothing to take over): active and full
   at once; notice to all other admins. **Remote, existing account:** pending for the delay (CC7:
   default 24 h, SM7). The new phone shows when it will be ready and has no session until then.
   Notices with "Cancel" go to the user's devices, the recovery contact and all other admins, and
   owner evidence cancels automatically. Afterwards the device is active and full.
8. The setup is audit-logged and copied to every admin's security log. The account's `may_approve`
   stays off unless the admin turns it on. The admin who set the account up becomes its recovery
   contact unless the admin names someone else.
9. **Person without a phone:** the setup link opened on their PC registers a passkey instead. The PC
   shows the code, and the same protocol rules apply.

**F5d. Passwordless accounts (was B2).**

- Accounts set up by F5c have no password.
- Their sign-in paths are:
  - the key device itself (device-key sign-in);
  - a passkey;
  - QR login on another device, if `may_approve` is on.
- Any user may remove their password from a full session with an action-bound step-up, once they
  have a strong authenticator.
- The DB constraint for `auth_method = 'local'` gets a relaxed rule or a new method; the builder
  chooses.

#### F6. Recovery (was B6) → never weaker than sign-in

In order of preference:

1. **Another key device or a passkey:** sign in with it (full), add the new phone (F5a), and remove
   the lost device with a step-up.
2. **Self-service recovery (rev 2; SM4):**
   - An entry "I lost my phone / forgot my password" on the sign-in page, separate from sign-in:
     username + one recovery code + either the password or a TOTP code (NIST §4.2.2.2: a recovery
     code plus a bound authenticator; ASVS 6.4.3). Password + TOTP is a sign-in (F4), not a
     recovery. A flagged password cannot be used here.
   - It works while the password path is closed (flagged or disabled), because the recovery code
     carries it.
   - The recovery code is consumed at once. The client gets a recovery capability, not a session: no
     chat, no settings.
   - The request waits 24 h (configurable) **(rev 2; was 1 h)**. Notices go to every address (the
     user's devices, the recovery contact and all admins) at the start, after 1 h and 1 h before it
     is ready. These pushes ignore quiet hours.
   - **Owner evidence** during the wait cancels it automatically. An owner who still uses their
     phone or passkey defeats a fake request without reading anything; a true loss produces no such
     evidence.
   - **Cancel** (owner, recovery contact, any admin) ends the capability, flags the password if it
     was used, writes a report and blocks self-service recovery for 7 days.
   - When the request is ready, the same client enrols one strong authenticator (a passkey in that
     browser, or a phone through a setup QR with the F5a mechanics) and may set a new password,
     within 24 h. Then the devices named lost are revoked, other sessions end, the recovery codes are
     replaced (shown once), and notices go out.
   - The new authenticator is full. The recovery code, the second factor, the wait and the absence
     of objections authorise it; time alone never does (CC4).
   - Passwordless users without recovery codes recover only through an admin, and the UI says so.
3. **Admin recovery, in person or remote** (`device_enrollment/admin`, the F5c protocols, SM5): the
   admin names the lost devices. They are revoked when the new device becomes active, not at the
   start, so the owner's phone can still cancel a fake request. An admin who knows a device is
   stolen uses "Remove device now" (immediate, audited, notified, NIST §4.3); removing a device
   never shortens or skips a delay.
4. **Host CLI** (last resort, when the only admin has lost everything; needs root on the host): it
   issues a one-time setup link for any account (active at once, since the operator is trusted),
   removes devices, resets throttles and the compromise flag, and ends sessions. Every use is
   audited and sends the same notices to the account, its recovery contact and all admins. A second
   admin avoids needing it; the design does not require one.

**After every successful recovery** (ASVS 7.4.3; OWASP Passkey Security cheat sheet):

- the devices named lost are revoked;
- all other sessions end, except key devices the user or admin explicitly keeps;
- the user sees the list of authenticators for review;
- notices go to the user, the recovery contact and the admins.

**A lost sole phone is removed without that phone** (NIST §4.3; ASVS 6.5.6): by an admin at once,
by another strong authenticator with a step-up, or when a recovery completes.

#### F7. Step-up (re-auth, C24) → an action-bound grant

- **What it is (rev 2):** a grant for one named action and target (for example "remove the device
  Pixel 8"), bound to the session's `sid`, valid for at most 5 minutes, consumed by that action.
  One step-up covers one action.
- **Methods:**
  - a passkey assertion; the ceremony page names the action;
  - on a key device, a local UV signature over the action and target (`kaiku-device-stepup-v1`);
  - for a PC session, a `reauth` approval: the asking screen shows an action QR, and the phone scans
    it and shows the action ("Allow Firefox on Windows to remove the device 'Pixel 8'?") before UV.
- **Not step-ups:** a password, TOTP, a recovery code or OIDC, alone or together (ASVS 7.5.1). The
  bootstrap ceremony (F8) is not a step-up either.
- Only full sessions step up. A probation session upgrades first (B.4).
- **Required for** (ASVS 7.5.1, 7.5.2):
  - adding or removing key devices, passkeys or TOTP;
  - setting, changing or removing the password;
  - creating recovery codes;
  - ending other sessions;
  - setup codes;
  - admin elevation;
  - naming a recovery contact;
  - deleting the account.

  Ending a key device's session is shown as "Remove device". `may_approve` is changed only by an
  admin, with the admin's step-up.
- The notice after a step-up action names what was done.

#### F8. Migration and new accounts (v1.0)

The family server already has accounts (RC testing): some have password + TOTP, some a password
only. None has a key device.

- **At the upgrade:**
  - every existing session becomes **probation** (sessions minted by the old unconfirmed QR redeem
    cannot be told apart, so none is trusted) **(rev 2)**;
  - sessions older than the absolute caps (B.4) end, and so do the sessions of password-only and
    OIDC-only (no TOTP) accounts;
  - legacy backup codes stop working.
- **Bootstrap ceremony (design call):** an account without a strong authenticator may enrol its first
  one itself within its **bootstrap window**: 30 days from the upgrade or from the account's
  creation, whichever is later.
  - From a session or the sign-in page, it re-enters the password (or OIDC with a fresh `auth_time`)
    and a TOTP code (or a recovery code), all within 5 minutes, then runs F5a or registers a passkey
    in that browser.
  - The new authenticator is full (NIST §4.1.2.1: binding at the account's AAL2). The ceremony also
    creates the account's recovery codes. Notices go to the user's devices and all admins.
  - Admin accounts may also use it after the window while no admin of the server has an active
    strong authenticator, so the server can always regain a strong admin without the CLI.
- **Outside the window**, an account's first strong authenticator comes from an admin setup (F5c).
- **Password-only and OIDC-only accounts** (without TOTP): password sign-in gets the neutral answer
  (F3). Their first strong authenticator comes from an admin setup (F5c, in person or remote). The
  reason: the password may already be known to someone else, so it alone must not bind the
  authenticator that will own the account (stricter than NIST §4.1.2.1, which would allow it at
  AAL1).
- **Phone-less users** keep password + TOTP (probation) and are offered passkeys first (see Q2).
- **New accounts** (registration under `registration_policy`, or the first admin on a fresh server):
  registration yields a **registration ticket** (30 min), not a session. It binds a passkey (full), a
  key device with the F5a mechanics (full), or TOTP (the account then signs in through F4 on
  probation and has its bootstrap window). An account that binds nothing within 30 minutes is
  deleted. The first admin must bind a passkey or a key device, because elevation needs a full
  session.
- **Admin upgrade checklist:** enrol a passkey or phone within the bootstrap window; name a
  recovery contact; on a single-admin server, consider a second admin.
- **Removed:** `/auth/qr/create`, `/auth/qr/redeem` (a session without confirmation),
  `/auth/forgot-password`, `/auth/reset-password`, and the Android `OidcHandler` branch that accepts
  tokens from a deep link.
- **Admins see per user:** strong authenticators and their state, TOTP, recovery codes left,
  password set, flagged or disabled, the recovery contact, open requests, and the assurance of open
  sessions.

### B.4 Sessions

**Assurance classes (rev 2: two classes; `setup_only` is gone):**

| Class | Created by | May |
|---|---|---|
| `full` | passkey sign-in (UV); device-key sign-in of an active key device; the first session of a device enrolled by F5a, F5c or a recovery; a probation session upgraded by a passkey assertion or Trust | everything, with an action-bound step-up where F7 requires it |
| `probation` | QR login; password or OIDC + phone; TOTP or recovery-code fallback; every session that existed before the upgrade | chat, calls, profile name and photo, notification settings, read the security log, sign itself out; the bootstrap ceremony where F8 allows it |

**Probation (CC4) cannot:**

- add, remove or rename sign-in methods (passkeys, key devices, TOTP, recovery codes, password),
  except through the bootstrap ceremony (F8);
- end other sessions or devices, deny or report approvals, or use "That wasn't me";
- create setup codes;
- elevate as admin;
- approve anything;
- delete the account.

**A probation session becomes full only by (SM3):**

1. **a passkey assertion** in that session (immediate; phishing-resistant; it upgrades the session
   and is not a step-up grant);
2. **Trust (rev 2):** on an active key device, the user opens Security → "Trust a sign-in" and scans
   the **Trust QR** that the probation session shows in its banner. The phone shows the session
   (client, time, place) and "Give this device full control of your account? Only for your own
   device in front of you." The server refuses unless both devices are on the same network (the same
   public IPv4 address or IPv6 /64; RFC 10027 §6.1.1) and the account has `may_approve` on
   **(design call)**. UV, `session_trust` approval. Trust is never offered in a push or a notice;
   the sessions list only shows and ends sessions, so nothing is picked from a list; and a Trust QR
   is bound to its one session.

Nothing else upgrades a session: no timer, no refresh and no new sign-in. Revision 1's automatic
upgrade after 24 h is removed.

Server-side, two guards enforce this: "full session" and "step-up grant for this action". Every
route in F7's list uses both.

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
  - notices with revocation;
  - probation for relayable sign-ins;
  - an action-bound step-up for every sensitive action.

  The absolute caps hold for every client (NIST §5.1).
- **Renewal on Android:** the app renews proactively at a normal app open shortly before the cap,
  so it never asks for UV in the middle of an incoming call. After a lapse, the prompt is the
  phone's own screen lock, worded "Confirm it's you", never a password.
- **Incoming calls after a lapse:** call-wake pushes go to an active key device even when its session
  has lapsed (not when the device is revoked or dormant). The phone rings; after "Accept" it asks
  "Confirm it's you" (device-key sign-in) and then joins the call.

**Refresh rotation with reuse detection** (RFC 9700 §2.2.2, §4.14.2; rev 2):

- Refresh rotates on every use, in place (the stable `sid` stays).
- The server recognises every refresh token it has issued for a session until the session ends,
  without letting a forged token end someone else's session. Either it keeps the hashes of all spent
  tokens until the session's absolute expiry, or the token carries `sid` and generation and is
  authenticated with a server key held outside the database (builder's choice).
- The current token rotates.
- The immediate predecessor, presented within 10 s of its rotation and only once, means the response
  was probably lost: the server rotates again and returns a fresh token. The token from the lost
  response is then stale, and any later use of it counts as reuse.
- Any other earlier token is reuse: the session ends through `revocation::end_sessions`, with the
  notice "A session was ended because its key was used twice".
- A token the server does not recognise gets 401 and changes nothing.
- Clients serialise refreshes: the web client across tabs (Web Locks), the desktop in one Rust task,
  Android as a single flight.

**Device-key sign-in** (K3; NIST §5.1; SM6):

- **After a lapse** (inactivity, absolute cap, or a plain sign-out on that device): a `K_approve`
  signature with UV over a server challenge. It yields a session with **the device's assurance**
  (an active device is full), never more. It is rate-limited and audited.
- **Impossible** when the device is pending, revoked (removal, admin, recovery, key invalidation,
  account deletion) or dormant, or the account is banned. The device then needs a new enrolment.
- Never silent, and never with a key that needs no UV.

**Token storage (rev 2):**

- **Android:** tokens in Keystore-wrapped storage (an AES key in the TEE, excluded from backups),
  replacing `security-crypto`. The `K_session` proof on every refresh is deferred to v1.x: with
  Keystore-wrapped storage, stealing the refresh token already needs code running inside the app,
  which could use a session key just as well.
- **Browser:** the refresh cookie gets a `__Secure-` prefix, HttpOnly and `SameSite=Strict`. DBSC
  later.
- **Desktop:** the keyring as today.

**Ending sessions:**

- Signing out the current session is always allowed. On a key device, "Sign out" keeps the phone as
  a key (the next open asks "Confirm it's you"); "Sign out and remove this phone" revokes it (UV).
- Ending other sessions needs a full session and a step-up (ASVS 7.5.2).
- "That wasn't me" ends the reported session at once, under the rule in B.2 (RFC 10027 §6.1.6).
- Sessions of a revoked authenticator end with it.
- After an authenticator change, the UI offers "Sign out everywhere else" (ASVS 7.4.3).

### B.5 Limits and fatigue (was B5)

All values are configurable; the defaults below are the builder's starting point. All counters live
in Valkey, so they survive restarts. All rejects, delays, disables and blocks are audit-logged and
visible to admins. ASVS 6.1.1 asks that lockout abuse be documented: these are the rules.

- **No approval prompts** (CC2): nothing reaches a phone unless the phone scans first, so there is
  nothing to bomb.
- **Per-account throttling** (CC9; NIST §3.2.2):
  - separate counters for the password, the second factor (TOTP, recovery code), typed enrolment
    codes and the recovery entry, across all IPs. Password tries against an account without a
    password count as password failures;
  - after 5 consecutive failures, an increasing delay per account (from 30 s, doubling, at most
    1 h);
  - at 20 consecutive failures, a notice to the user and the admins ("Someone is trying to guess
    …"), coalesced;
  - at 100 consecutive failures, that authenticator is disabled for the account (NIST §3.2.2). It is
    re-enabled by a successful passkey, QR or device-key sign-in of the account, by a completed
    recovery, or by an admin;
  - the message suggests a passkey or QR sign-in.

  Passkeys, QR login, device-key sign-in and existing sessions are never affected, so a hostile
  guesser cannot lock anyone out of the strong paths. Lockout abuse: someone who knows the username
  of a phone-less user without a passkey can slow that user's password sign-in and, after about four
  days of guessing at the growing delay, disable the password. The user and the admins are told at
  20 failures; recovery (recovery code + TOTP) or an admin brings it back (see Q2).
- **Per IP:**
  - the existing `failed_auth` limiter;
  - it never applies to QR login, passkeys, device-key sign-in or signed-in sessions, so an attacker
    on the family's shared home IP cannot lock everyone out;
  - the public endpoints that create login attempts, passkey challenges, enrolment claims and
    recovery requests have their own request limits.
- **Typed enrolment codes:** three wrong entries end that enrolment; a new enrolment does not reset
  the per-account counter (NIST §3.1.3.2).
- **Open limits:** at most 5 open setup codes per admin; one live login attempt per PC screen; one
  open recovery request per account, and none for 7 days after a cancelled one.
- **Notices:** repeated notices of the same type for the same account within 10 minutes are
  coalesced into one with a count; the audit rows stay individual.
- **v1.1:** `login` and `agent` limit classes have separate counters; agent approval pushes get their
  own budget in Valkey (6 per 10 min per device). Agent denials can never pause or slow human
  sign-ins (CC9, K9).

### B.6 Residual risks and documented deviations

1. **Real-time relay of F2 and F3** by a phishing page that proxies the QR code. It yields a
   probation session, which can read the user's chats and write as the user until it is noticed and
   ended. Limited by CC3, CC4 and `may_approve`; turning it into full control needs a passkey or a
   network-gated Trust. Passkeys are the phishing-resistant path (NIST §2.2.2 is met by *offering*
   them).
2. **Relay of a step-up:** an attacker with a stolen full session shows a relayed action QR to the
   owner. The phone names the action, and the grant covers only that action.
3. **A social-engineered admin.** Remote: the recorded call-back check, the delay, the notices to
   the user's devices, the recovery contact and the other admins, and the automatic cancel on owner
   evidence. A new account set up remotely has no delay; an impostor posing as a new family member
   is stopped only by the call-back check. In person: the network check stops a caller who is not
   there; someone who is physically present and deceives the admin is not covered. A single-admin
   server has no second pair of eyes; a second admin is recommended.
4. **The admin's phone is the root of trust** for every admin setup and recovery. Admins should use a
   phone with a fingerprint reader and StrongBox, and a second admin.
5. **Bootstrap window:** a password + TOTP code phished within an account's bootstrap window lets the
   attacker bind a strong authenticator (NIST §4.1.2.1 permits binding at AAL2). Notices go to the
   user and the admins; the window closes after 30 days or as soon as the owner binds theirs.
6. **A stolen phone plus a known screen lock** equals the key, as with any phone authenticator.
   The owner removes it through an admin or another method (F6).
7. **Linux:** Firefox has no hybrid passkeys, so QR login (probation) is the fallback there.
8. **IP and location signals are unreliable** (VPNs, borders; RFC 10027 §6.1.1). They are hints, and
   the same-network check only refuses. An attacker who shares the victim's network or public IP
   (for example behind carrier-grade NAT) passes it.
9. **Lockout abuse** of phone-less users without a passkey (B.5).
10. **Deviations:**
    - session lifetimes (B.4, NIST §2.2.3);
    - PIN-only API 26–29 phones use a 10 s authentication window instead of a per-use activation
      (NIST §3.1.3 "each use"; B.1);
    - Android key attestation is not verified in v1.0 (B.1);
    - Android refresh tokens are not sender-constrained in v1.0; RFC 9700 §2.2.2 is met by rotation
      with reuse detection (B.4).

### B.7 State machines (normative)

Each machine lists its states, the transitions, who may trigger each one, the timers and what a
cancel does. Every transition is one conditional UPDATE (`WHERE state = <from>`) with its audit row
in the same transaction; whichever of two racing transitions commits first wins, and the other
fails without effect. A sweep every minute applies the timers.

#### SM1. Login attempt (F2, F3, F4)

States: `open`, `claimed`, `approved`, `consumed`; terminal: `denied`, `reported`, `cancelled`,
`expired`.

| From | Event | Who may trigger | To | Rules and timers |
|---|---|---|---|---|
| – | create | F2: any client ("Sign in with phone"), request-limited; F3 and F4: the server after a correct first factor | `open` | the PC holds the collection secret; the attempt lives ≤10 min; the QR token rotates every 60 s and older tokens die |
| `open` | claim with the current token | an active key device, from its own session, `may_approve` on; for F3 it must belong to the attempt's user | `claimed` | creates the approval (B.2); one claim per attempt |
| `open` | second factor (TOTP or recovery code) | the PC; F3 and F4 attempts only; password not flagged | `approved` | per-account second-factor counter (B.5) |
| `claimed` | approve: UV signature with the token as `response` | the claiming device | `approved` | ≤2 min after the claim |
| `claimed` | No | the claiming device | `denied` | |
| `claimed` | I didn't start this (UV) | the claiming device | `reported` | F3: the password is flagged; notice |
| `approved` | collect with the collection secret | the PC | `consumed` | ≤60 s after approval; tokens are minted now; the session is probation |
| `open`, `claimed` | close the screen | the PC | `cancelled` | an F3 attempt that ends unconfirmed sends one coalesced notice |
| any non-terminal | timer | server | `expired` | as `cancelled` for notices |

#### SM2. Enrolment of a key device or passkey (self and admin)

Variants and their preconditions:

| Variant | Issued by | Precondition | Confirmed by |
|---|---|---|---|
| `own` | a full PC session, or an active key device | PC: a step-up grant "add a key device"; key device: local UV; `may_approve` on | the issuing screen; on a key device a UV signature (`device_enrollment/own`) |
| `bootstrap` | a session or the sign-in page of an account without a strong authenticator, inside its bootstrap window (or the admin rule of F8) | password or fresh OIDC, plus TOTP or a recovery code, within 5 min | the issuing screen |
| `registration` | a registration ticket | ticket ≤30 min old | the issuing screen |
| `recovery` | a recovery request in `ready` (SM4) | the recovery secret | the issuing screen |
| `admin_in_person` | the admin's own key device | full admin session; local UV per code; SM5 | the admin's key device: typed code with UV, plus "<name> is here with me" |
| `admin_remote` | a full admin session (PC or phone) | step-up grant per code; the recorded claimant check (SM5) | the admin's key device: typed code with UV |
| `cli` | the host CLI | root on the host | the operator types the code into the CLI |

| From | Event | Who may trigger | To | Rules and timers |
|---|---|---|---|---|
| – | issue | per variant | `issued` | setup token 256-bit, 10 min, single use; ≤5 open per admin |
| `issued` | claim: token, public key (or passkey registration), proof of possession with UV | the new device | `claimed` | screen lock and hardware keystore checked first; `admin_in_person`: refused unless on the admin device's network (stays `issued`); a second claim gets "already used"; the 6-digit code goes to the claimer only |
| `claimed` | confirm with the right code | per variant | `confirmed` | the code is valid 5 min; a wrong code counts; the third wrong code → `failed` |
| `confirmed` | activate | server | `active` or `pending_activation` | SM5 decides for the admin variants; all others → `active` |
| `pending_activation` | timer, cancel | SM7 | `active` or `cancelled` | SM7 |
| `issued`, `claimed`, `confirmed` | cancel | the issuer; for admin variants also any admin | `cancelled` | the claiming device shows "Setup cancelled" |
| `issued`, `claimed` | timer | server | `expired` | |

At `active`, the authenticator becomes `active` (full); a new key device's first session is full;
notices go to all addresses.

#### SM3. Session assurance: probation → full

| From | Event | Who may trigger | To |
|---|---|---|---|
| – | passkey sign-in; device-key sign-in of an active device; first session of an `active` enrolment | the signing-in client | `full` |
| – | F2, F3 or F4 sign-in; any session that existed before the upgrade | the signing-in client, the migration | `probation` |
| `probation` | passkey assertion in this session | the session's user | `full` |
| `probation` | Trust: a `session_trust` approval for this session (scanned Trust QR, same network, UV) | an active key device of the user, `may_approve` on, on which "Trust a sign-in" was started | `full` |
| any | sign-out, "That wasn't me", refresh reuse, inactivity or absolute cap, recovery clean-up, its authenticator revoked, ban | user, owner, server, admin | ended |

There is no other transition. Time, refresh and re-sign-in never change assurance. Step-up grants
(F7) are created by a passkey assertion, a device step-up signature or a consumed `reauth` approval;
they are bound to (`sid`, action, target), live ≤5 min, are used once, and exist only for full
sessions.

#### SM4. Self-service recovery request

States: `waiting`, `ready`, `completed`; terminal: `cancelled`, `expired`.

| From | Event | Who may trigger | To | Rules and timers |
|---|---|---|---|---|
| – | start: username + recovery code + password or TOTP code at the recovery entry | anyone (request-limited, per-account counter) | `waiting` | the recovery code is consumed at once; a flagged password is refused; at most one open request per account; refused for 7 days after a cancelled one; the client keeps a recovery secret; `activates_at` = start + 24 h; notices to all addresses at the start, after 1 h and 1 h before `activates_at` |
| `waiting` | timer | server | `ready` | ready for 24 h |
| `waiting`, `ready` | owner evidence | automatic | `cancelled` | |
| `waiting`, `ready` | Cancel | the owner (UV on a key device, or a passkey assertion), the recovery contact, any admin | `cancelled` | |
| `ready` | its `recovery` enrolment (SM2) becomes `active`; optionally a new password | the recovery client | `completed` | the named lost devices are revoked; other sessions end; recovery codes are replaced; a new password clears the compromise flag; notices |
| `ready` | timer (24 h) | server | `expired` | notice |

**Cancel effects:** the recovery secret dies; if the password was one of the factors, it is flagged
(CC8); a report event is written; self-service recovery is blocked for 7 days; notices name who
cancelled.

#### SM5. Admin setup and recovery: two protocols

|  | In person | Remote |
|---|---|---|
| Started on | the admin's own key device only | the admin panel or the full app (a full admin session) |
| Step-up | local UV per setup code | an action-bound step-up per setup code |
| Claimant check | the person is present; the admin confirms "<name> is here with me" at confirmation (recorded) | before a code exists: the admin ended the incoming contact, called back on a number or account known before the request, and recognised the person; method and note recorded |
| Transfer | the QR on the admin's phone is scanned by the new phone; the admin types the new phone's code | the QR or link goes over the checked channel; the person reads the code to the admin, who types it |
| Proximity | the claim must come from the admin device's network (same public IPv4 or IPv6 /64), else refused | none |
| Activation | at once, except for an admin target or owner evidence on the account in the last 7 days → delay (SM7) | new account: at once; existing account: delay (SM7) |
| Recovery mode | the admin names the lost devices; they are revoked at activation | same |

Rules for both:

- The target must not be the confirming admin. An admin target always gets the delay, and every other
  admin can cancel. A server with one admin recovers that admin through self-service recovery or the
  host CLI.
- "Remove device now" is a separate admin action with a step-up: immediate, audited, notified. It
  never skips or shortens a delay.
- Switching protocol means starting again: an in-person setup that fails the network check does not
  turn into a remote one by itself.

#### SM6. Authenticator lifecycle and device-key re-sign-in

Authenticators (key devices and passkeys):

| From | Event | Who may trigger | To |
|---|---|---|---|
| – | enrolment reaches `active` (SM2) | server | `active` |
| – | enrolment enters `pending_activation` | server | `pending_activation` |
| `pending_activation` | delay passes / cancel (SM7) | server / SM7 parties | `active` / deleted |
| `active` | "Sign out and remove this phone" (UV), or removal from another device (step-up) | owner | `revoked` |
| `active` | "Remove device now", or the CLI | admin, operator | `revoked` |
| `active` | a recovery that names it completes | server | `revoked` |
| `active` | Android invalidates the key; 180 days without use; account deleted | server | `revoked` |

An `active` authenticator is full. `pending_activation` and `revoked` authenticators authenticate
nothing: no sign-in, no step-up, no approval. v1.0 has no other authenticator state.

Key-device sessions:

| From | Event | Who may trigger | To |
|---|---|---|---|
| `live` | 7 d inactivity, 90 d absolute cap, or a plain sign-out on the device | server, user | `lapsed` |
| `lapsed` | device-key sign-in: UV signature over a server challenge | the device's user | `live`, with the device's assurance |
| `live`, `lapsed` | the device is revoked | SM6 parties | `ended` (no device-key sign-in) |

Device-key sign-in requires an `active`, non-dormant device and an account that is not banned. It is
rate-limited and audited.

#### SM7. Pending activation and cancel

Applies to enrolments in `pending_activation` (SM5) and to recovery requests in `waiting` or `ready`
(SM4).

- **Timer:** 24 h by default, configurable from 1 to 72 h; reminders at the start, after 1 h and 1 h
  before activation.
- **While pending:** the pending authenticator has no session and cannot sign in, approve or step up.
  Its screen shows when it will be ready and whom to call.
- **Automatic cancel:** owner evidence during the delay, with the notice "Cancelled because your
  phone (or passkey) is in use. If you asked for the new phone, tell <admin>."
- **Explicit cancel:** the owner (UV on any active key device, or a passkey assertion), the recovery
  contact, any admin, including the one who started it.
- **Cancel effects:** the pending authenticator is deleted; the claiming device shows "Setup
  cancelled"; a report event is written; notices go to all addresses. An admin protocol must start
  again, with a new claimant check. A self-service request has SM4's effects.
- **Races:** a cancel that loses against activation is refused with "already active", and the
  canceller is offered "Remove this device" (the owner with a step-up, an admin with "Remove device
  now").

## Part C: Security notices in chat (v1.0)

- A built-in **system account "Kaiku"** sends security notices into a
  read-only DM with each user. The account is refused on every auth path and hidden from user
  lists, search, presence, mentions, friend paths and bot paths.
- **Events:**
  - new sign-in (method, assurance, client, approximate location); a session trusted;
  - new key device, passkey or TOTP; recovery codes created;
  - an authenticator removed; password set, changed or removed;
  - a step-up action performed (what was done);
  - approval reported; "That wasn't me" used;
  - correct password with a failed second factor; a password entered but not confirmed; repeated
    guessing (20 failures); an authenticator disabled after 100 failures; password flagged
    compromised;
  - recovery started (with "Cancel"), ready, completed, cancelled or expired;
  - admin setup or recovery for the account, including a pending activation (with "Cancel");
  - a bootstrap enrolment;
  - the recovery contact changed;
  - a session ended by refresh reuse;
  - a key invalidated by the phone; a key device about to reach the dormancy limit.
- Each notice also goes out as a push to all the user's devices, including key devices whose
  session has lapsed. Its pushes bypass Kaiku's own notification settings, and high-severity ones
  also bypass quiet hours. Repeated notices are coalesced (B.5).
- **"That wasn't me"** follows the rule in B.2: it needs UV on a key device or a passkey, never a
  probation session; it is not offered on the device the notice is about, nor on accounts with
  `may_approve` off. It ends the reported session or revokes the reported authenticator and, for a
  password sign-in, flags the password.
- **Second address** (NIST §4.6: at least two notification addresses; NIST §4.2.3: recovery notifies
  the subscriber or their designee):
  - **Recovery contact (rev 2):** each user may name one other user of the server (with a step-up).
    For accounts set up by an admin, that admin is the default. The contact receives the
    high-severity notices in their own Kaiku DM and may cancel a pending recovery or activation of
    the account; the contact gets no access and approves nothing.
  - **Admins:** high-severity events are copied to every admin's security log and Kaiku DM: new
    strong authenticator, bootstrap enrolment, recovery started, completed or cancelled, an
    authenticator removed, "That wasn't me", password flagged or disabled, admin setup. Admins see
    the event, user, device label and time, never message content. Any admin can cancel a pending
    recovery or activation.
  - On a single-admin server the admin names a recovery contact; the upgrade checklist says so.
  - After losing the only phone, an owner can stop a fake recovery through a passkey (sign in, then
    Cancel), the recovery contact, or any admin reached by phone.
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
- v1.0 approvals use no push at all (B.2, Delivery); notices never carry an approving action.
- **Lifetimes:** access 15 min and refresh 7 d sliding stay as in `server/src/auth/AGENTS.md`.
  Reuse detection and absolute caps are additions, not changes (RFC 9700 §2.2.2; ASVS 7.3.2).
- `PUBLIC_URL` is mandatory (approval origin, RP ID), and the server fails closed and warns at
  startup without it. The reverse proxy and production ingress route `/.well-known/` to the server.
- The desktop keeps tokens and collection secrets in Rust; the webview never sees them. Android
  OIDC and any browser hand-off use the one-time code + verifier contract, never tokens in a URL
  (RFC 8252 §7.1, §8.1); the `OidcHandler` token branch is deleted.
- Public repository: no private hostnames, IPs or exploit details in code, docs or PRs.
- Accessibility for older users is a requirement, not polish: large touch targets, readable text
  sizes, the contrast rules above, codes shown in groups ("123 456") with a large digit keypad that
  allows corrections before sending, and no gestures as the only way to do something.

## Sequencing

v1.0 packages, per the package cut of 2026-11-03 (model-level ownership; task lists live in the
plans):

- **Gate:** this spec (revision 2) → a confirming design review → contracts frozen:
  - C26: approvals wire, claim by transfer, golden vectors for a scanned and a typed `response`;
  - C27: login attempts, QR payloads, step-up and Trust requests, enrolments, the setup link,
    recovery requests (SM1–SM5, SM7);
  - C28: security events, the Kaiku DM, the recovery contact, coalescing;
  - C29: passkeys (RP ID, origins with both APK hash encodings, assetlinks including the setup-link
    path, the desktop hand-off without polling).
- **LP (passkeys, server + all clients), off the critical path, merges first:**
  - the WebAuthn profile (B.1), desktop via the system browser, Android Credential Manager,
    `assetlinks.json`, the passkey `state` column;
  - passkey as a C24 re-auth method;
  - the `issue_session()` extraction.

  No probation sessions exist before L1b, so LP's "fresh re-auth" gate is complete for its own
  merge; L1a adds the full-session check and the action-bound grant to LP's routes.
- **L1a (server: approvals and key devices) ★:**
  - B.2 in full: v1.0 kinds, claim by transfer, the remediation rule;
  - key devices (UV policy, the attestation minimum, lifecycle, dormancy), device-key sign-in,
    device step-up;
  - session assurance and the two guards, action-bound step-up grants, refresh recognition with
    reuse detection and the grace re-rotation, absolute caps;
  - the enrolment core (SM2) and pending activation (SM7);
  - `security_events` and the Part C plumbing (system account, read-only DM).
- **L1b (server: sign-in flows) ★, stacked on L1a:**
  - F2–F8 with SM1, SM4 and SM5, Trust, B.5 including per-account throttling;
  - the password blocklist and compromise flag, recovery codes ≥64 bits, recovery requests, the
    admin protocols with the claimant record and the network check, the bootstrap ceremony,
    registration tickets;
  - the CLI, the notices for all events, the recovery contact;
  - removal of the old QR and e-mail reset routes.

  The standards check and revision 2 add scope here. If L1b passes the review threshold, the
  smallest split is B.5 + F6 + the CLI as a follow-up PR, still before RC3.
- **L2 (clients: sign-in and key-device UX, desktop/web + Android) ★:**
  - desktop/web: confirmation QR, QR login panel, the Trust QR in the probation banner, action QRs
    for step-up, setup-code panels, the recovery entry, probation UI, security log, read-only Kaiku
    DM;
  - Android: `K_approve` and the UV rules, the scanner (one tap from the chat list), context,
    approval, action and Trust screens, the typed-code keypad, F5 flows including setup links,
    device-key sign-in, answer-then-confirm for calls after a lapse, Keystore-wrapped storage,
    deletion of the `OidcHandler` token branch.

  L2 builds against the frozen contracts and merges after L1a + L1b.
- **Order:** L1a → L1b (stacked) → L2 merge → RC3 → device tests (Fabian) → Fable final review of
  all auth changes → v1.0 tag. LP runs beside it. No RC is cut between the L1b and L2 merges.
- **v1.1** (M1 admin MCP core, M2 agent approvals on the L1a mechanism, M3 agent bots) and **v1.2**
  (E1 simple app + group calls, E2 1:1 video + voice messages): their PRs may be built early but
  **stay open until the v1.0 tag**, unless the maintainer cuts a release branch (Q5).
- The v1.1 spec gets the K18 update; it adds the agent kinds, side table, desktop rows and approval
  push in its own migration, and its desktop approval device answers agent kinds only.

## Left to the builder

- Module/flavor split of the Android codebase.
- Tables, Valkey keys and WebSocket payload details, within B.2 and B.7.
- QR and setup-link payload encoding (scheme and URL shape), as long as it carries the origin and no
  long-lived secret.
- Exact TTLs and limit defaults within the stated bounds (login-class approvals ≤10 min; delays 1–72
  h).
- How earlier refresh tokens are recognised (a spent-hash list or an authenticated token format),
  within B.4.
- How the same-network check handles dual-stack clients (for example by probing over IPv4 and IPv6),
  within the rule that it only ever refuses.
- How the API 26–29 PIN-only case is implemented, within the 10 s window rule.
- Choice of WebAuthn crate after the LP spike; choice of the optional local GeoIP database (license
  check).
- How the system account and the read-only DM are modelled.
- Wording and layout of the context, approval, action, Trust and setup screens (accessibility rules
  apply).
- Video call implementation details (codec, simulcast or not, bandwidth); the voice-message codec
  matrix.

## Open questions for the maintainer

1. **Recovery delay:** revision 2 waits 24 h for self-service recovery and for an admin setup or
   recovery of an existing account that needs a delay (was 1 h; Fable proposed 24 h, Argon 48 h).
   Keep 24 h, or choose 48 h? It stays configurable either way.
2. **Phone-less users:** as revised, password + TOTP is a chat-level (probation) login. Changing
   security settings needs a passkey, which a phone-less user can bind alone only within the 30-day
   bootstrap window (later through an admin). Is that acceptable, or must every phone-less user
   register a passkey at the upgrade?
3. **No screen lock:** what happens to a relative who will not use a PIN, pattern or biometric? As
   written, that phone cannot be a key, so a passwordless account cannot sign in on it. The
   alternative is a possession-only "chat only" device that never approves anything and needs the
   admin again after each lapse. That is below the standards bar.
4. **Family members approving PC sign-ins:** `may_approve` is off for admin-set-up accounts, so they
   cannot sign in on a PC by QR. Keep that default, or turn it on per person? The simple app has no
   scanner, so the flag matters only for relatives who use the full app.
5. **Release branch:** cut `release/1.0` at RC3 so v1.1/v1.2 can merge to main earlier, or keep
   them open until the v1.0 tag?
6. **Password + phone sign-in (F3):** Fable and Codex suggest dropping it once passkeys and phone
   login work. Keeping it gives people who think in passwords a familiar first step; dropping it
   removes a phishable first factor, the compromise flag and the "password entered" notices, while
   QR login (F2) already gives the same probation result without a password. Revision 2 keeps it,
   as you asked, without any push to the phone: should it stay after v1.0?

## Deferred minor findings

These were raised in round 1 and left for later on purpose; none of them blocks v1.0.

- **Dormancy exemption for spare phones** (Argon minor 4): v1.0 sends a warning 14 days before the
  dormancy limit; exempting a full key device while its user is active elsewhere is deferred.
- **Attestation compatibility before setup** (Codex minor): v1.0 checks the screen lock and the
  hardware keystore first; checking attestation compatibility comes with the v1.x verifier (B.1).
- **One step-up for several setup codes** (Fable minor 5): v1.0 asks for one step-up per setup
  code; a "family gathering" batch mode is deferred.
- **Known-client exemption from the per-account delay** (follows from Fable major 9): OWASP-style
  device cookies would keep a phone-less user's own browser out of an attacker-driven delay;
  deferred to v1.x (Non-goals).
- **Longer Android inactivity** (Argon minor 1, alternative fix): not adopted; answer-then-confirm
  (B.4) handles incoming calls after a lapse without lengthening sessions.

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

## Appendix: Review response (round 1)

Reviews: Fable (F), Codex (C), Gemini Argon (A); labels are the reviewers' own (B = blocker,
M = major, m = minor, cut = over-engineering; Fable's minor 3.n appears as F mn, unnumbered Codex
items in their order). Overlapping findings are merged.

**Blockers and major issues**

| # | Consolidated finding | Reviewers | Resolution |
|---|---|---|---|
| 1 | Time-based probation → full upgrade turns every relayable path into a full takeover after 24 h | F B1; C B2, C cut 2; A B2 | Fixed: upgrade by time removed; exits are a passkey assertion or Trust (CC4, B.4, SM3) |
| 2 | Assurance not bound to the authenticator; device-key sign-in always yields full | F M8; C B1 | Fixed: sessions carry at most their authenticator's assurance; v1.0 has no probation authenticators (CC4, B.4, SM6) |
| 3 | Self-service recovery: 1 h delay, undefined Cancel, owner's devices revoked, undefined "probation passkey", any probation session may start it | F B2; A B3 | Fixed: separate entry, 24 h, automatic cancel on owner evidence, Cancel with effects, revocation only at completion, passkey state (CC7, F6, SM4, SM7). Argon's 48 h → Q1 |
| 4 | "Any two" recovery factors include TOTP + recovery code; recovery mixed with sign-in | C B3; A M1 | Fixed: recovery code + password or TOTP (NIST §4.2.2.2); a recovery capability, no session; TOTP + recovery code is never a sign-in (F4, F6) |
| 5 | In person and remote use the same protocol; no claimant check; lost devices revoked at the start | F M2; C B4; A B4 | Fixed: two protocols (in person proven by the admin's phone and the network check; remote with a recorded call-back check and a delay); revocation at activation (F5c, F6.3, SM5). Argon's "always delay when an authenticator exists" adopted in a form that revoking first cannot bypass (owner evidence in 7 days); brand-new and long-idle accounts stay immediate in person so launch-day setup works |
| 6 | Bootstrap and migration deadlocks: password + TOTP users never reach full, all-admin lockout, `setup_only` vs F5a guard, first admin | F M1, F m16; C M3; A B5.1, A M3 | Fixed: bootstrap ceremony with a 30-day window, the admin rule, registration tickets, the CLI (F8, SM2) |
| 7 | Second-admin approval for admin targets undefined (compare-and-approve) and deadlocked with two admins | F M10b; A B5.2 | Fixed: another admin confirms, the delay always applies, every other admin can cancel (F5c, SM5) |
| 8 | Push budget, pause or flagged password block F4 and recovery (permanent lockout); pause contradicts the neutral answer | A B5.3; A M2 | Fixed: no approval pushes, so no budget or pause (F3); the recovery entry does not depend on the password path (F6) |
| 9 | An empty `response` on a scan skips the check of the transferred secret | A B1 | Fixed: claim by transfer; `response` is the secret and is verified before the signature (B.2) |
| 10 | Desktop passkey ceremony collectable by its initiator if the attempt can be polled | A B6 | Fixed (clarified): the code leaves the server only by the loopback redirect; no polling (CC5, F1) |
| 11 | Step-up not bound to an action; step-up relay not listed | F M3; A M4 | Fixed: single-use action-bound grants, the phone names the action (F7, SM3, B.6.2) |
| 12 | Password + TOTP accepted as a step-up and to bind a full key device on accounts with strong authenticators | F M4 | Fixed: not a step-up; only the bootstrap ceremony uses it (F7, F8) |
| 13 | "Trust this sign-in" is a pick-from-list | F M5; C m | Fixed: Trust QR bound to one session, started on the key device, network-gated (B.4) |
| 14 | Conflicting owners of "That wasn't me", deny and report | F M6, F M10c, F m11 | Fixed: one rule table (B.2, Part C) |
| 15 | No state machines for F5a, recovery requests, delayed activation; cancel/complete races | F M7; A M3; C m | Fixed: B.7 (SM1–SM7) |
| 16 | 100-failure close is a remote lockout; neutral answer incomplete for passwordless accounts | F M9 | Fixed differently: NIST §3.2.2 requires disabling, so the 24 h auto-lift is not adopted; strong paths, recovery and admins re-enable, the delay makes 100 tries take days, a notice at 20; same answer and timing for passwordless accounts (B.5, F3) |
| 17 | `may_approve`: admin-only in B.1 but a user step-up action in F7 | F M10a | Fixed: admin-only (B.1, F7) |
| 18 | Second notification address not independent; no designee; no cancel path after loss | C M5 | Fixed: recovery contact; listed cancel paths after loss (Part C, SM7) |
| 19 | Anti-relay claims overstated (collection secret vs QR phishing; §6.1.12 and §6.1.15 cited for the wrong things; probation exposes chats) | C M6 | Fixed: CC3 rewritten per control; citations corrected; probation exposure stated (CC3, F2, B.6.1) |
| 20 | OIDC `amr=hwk` does not establish phishing-resistant MFA | C M1; F cut 8 | Fixed (cut): no trusted-IdP exception in v1.0; mapping deferred (B.1) |
| 21 | Refresh reuse detection cannot work with two stored hashes | C M2 | Fixed: every issued token is recognised until the session ends (B.4) |
| 22 | Grace-window "retry answer" logs out a client whose response was lost | A M7 | Fixed: one re-rotation within 10 s; clients serialise refreshes (B.4) |
| 23 | Attestation policy incomplete (`authTimeout`, challenge, revocation, key binding); attestation cost | C M4; A M6; F cut 7; A cut 2 | Deferred with a stated v1.0 minimum (challenge-bound chain stored, reported properties, proof of possession) and a v1.x verifier profile (B.1, B.6) |
| 24 | `android:apk-key-hash` encoding differs from `assetlinks.json`; BE flag treated as risk input only | A M5 | Fixed: both encodings in C29; BE must match (B.1) |

**Minor issues**

| # | Consolidated finding | Reviewers | Resolution |
|---|---|---|---|
| 25 | A PIN chosen by the admin locks the relative out of her phone | F m1 | Fixed: she chooses it with the admin; fingerprint where possible (B.1) |
| 26 | Adding a fingerprint invalidates the key | F m2; A m3 | Fixed: `setInvalidatedByBiometricEnrollment(false)` (B.1) |
| 27 | Incoming calls on a lapsed session (7-day inactivity, 90-day cap) | F m3; A m1 | Fixed: answer, then "Confirm it's you" (B.4); longer inactivity not adopted |
| 28 | The API 26–29 window is not listed as a deviation | F m4 | Fixed (B.6), plus one signature per 10 s (B.1) |
| 29 | One typo kills an approval; admin retries; one step-up for several codes | F m5; C m | Fixed: corrections before sending, three tries; one step-up per code (batch mode deferred) |
| 30 | Plain hashes of short codes | F m6 | Fixed: keyed hash (B.2) |
| 31 | Android `OidcHandler` accepts tokens from a deep link | F m7 | Fixed: listed and deleted (Current state, F8, L2) |
| 32 | The device signs server bytes without checking them | F m8 | Fixed: the device checks origin, kind, approver and device (B.2) |
| 33 | Pre-upgrade sessions (including old unconfirmed QR sessions) stay full | F m9 | Fixed: all become probation (F8) |
| 34 | Username-first passkey requests reveal accounts | F m10 | Fixed: `allowCredentials` always empty (B.1) |
| 35 | Residual risks missing: typed-code voice scams, the admin phone as root of trust, push budgets lost on restart, 8-digit code login CSRF | F m12; A m5 | Fixed: typed PC → phone codes cut; B.6 items 3–4; counters in Valkey (B.5) |
| 36 | Host CLI powers undefined | F m13 | Fixed (F6.4) |
| 37 | `may_approve` on does nothing in the simple app | F m14 | Fixed: noted in Q4 |
| 38 | Does an F4 success reset the push budget? | F m15 | Fixed by removal: no login pushes |
| 39 | A plain sign-out destroys the phone's key | C m | Fixed: sign-out keeps the key; "Sign out and remove this phone" (B.1, B.4) |
| 40 | Unsuitable phones fail late in setup | C m | Fixed: screen lock and keystore checked first (B.1); attestation compatibility deferred |
| 41 | Notices can flood | C m | Fixed: coalescing (B.5, Part C) |
| 42 | A sole phone cannot scan a QR on its own screen in remote setup | A m2 | Fixed: setup links open on the phone (F5c step 3, Part A) |
| 43 | The dormancy limit silently expires spare phones | A m4 | Fixed: warning 14 days before; exemption deferred |

**Over-engineering named by the reviewers**

| # | Suggested cut | Reviewers | Resolution |
|---|---|---|---|
| 44 | Drop password + phone sign-in (F3) | F cut 1; C cut 4 | Rejected for v1.0: the maintainer asked for it. Simplified instead (no push, no budget or pause) and put to the maintainer as Q6 |
| 45 | Cut F5b (enrolment started on the new phone) | F cut 2; A B2 | Fixed (cut): adding a phone always starts on something trusted (F5a) |
| 46 | Cut the `setup_only` class | F cut 4; A M3 | Fixed (cut): registration tickets and admin setup (F8) |
| 47 | Drop the Android 90-day cap | F cut 5 | Rejected: it costs one UV per quarter at a normal app open, the call case is handled (#27), and NIST §5.1 asks for caps |
| 48 | `K_session` device-bound refresh and its nonce round trip | F cut 6; A cut 5 | Deferred to v1.x; Keystore-wrapped storage stays (B.4) |
| 49 | Typed codes from the PC to the phone (8-digit F2 code; 6-digit F3/F7 codes) | F cut 9; A cut 1 | Fixed (cut): typed codes remain only from a new device to the trusted side (CC2) |
| 50 | Two classes of legacy backup codes | F cut 10 | Fixed (cut): legacy codes end at the upgrade (F8) |
| 51 | Pre-declared v1.1 kinds and schema | C cut 1 | Fixed (cut): v1.1 adds them in its own migration (CC10, B.2) |
| 52 | VPN/hosting classification, the positive "same network" badge, the GeoIP database | C cut 3; A cut 4 | Fixed for VPN/hosting and the badge; the local GeoIP database stays optional (location is useful context, and Codex keeps simple context), and the external GeoIP API is no longer called (F2) |
| 53 | Android Custom Tab passkey fallback | A cut 3 | Fixed (cut; deferred, B.1) |
| 54 | Let a passkey-only admin confirm setups and recoveries | A B5.1 | Rejected: Decision 3 names the confirmation on the admin's own key device; the deadlock it targets is broken by the bootstrap rule and the CLI (F8) |

**Citation corrections made in revision 2:** NIST SP 800-63B-4 numbers AAL2 account recovery
§4.2.2.2 (was cited as §4.2.5.2), the recovery notification §4.2.3 (was §4.2.6) and saved recovery
codes §4.2.1.1. NIST §4.1.2.1 asks for binding at the lower of the account's highest AAL and the new
authenticator's AAL, so requiring an admin for password-only accounts is stricter than NIST, not
required by it. NIST §3.2.2 disables an authenticator after at most 100 consecutive failures. RFC
10027 §6.1.12 is about sender-constrained tokens and §6.1.15 about phishing-resistant authentication
on the consuming device before a flow starts; neither describes the collection secret or Trust, so
both citations were removed there. §6.1.14 (user experience: context and a clear decline) and
§6.1.1 (proximity, including a shared network) are cited instead.

**Count:** 54 consolidated findings: 49 fixed (8 of them by adopting a cut the reviewers proposed:
#20, #45, #46, #49–#53), 2 deferred with a stated minimum (#23, #48), and 3 rejected with reasons
(#44, #47, #54). Partial rejections inside fixed rows: #5 (an unconditional in-person delay), #16
(the 24 h auto-lift), #27 (longer inactivity) and #52 (removing GeoIP).
