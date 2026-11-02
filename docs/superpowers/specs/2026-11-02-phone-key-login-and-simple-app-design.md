# Kaiku: phone-as-key login (v1.0) and simple Android app (v1.2)

**Status:** Draft for maintainer review (2026-11-02)
**Target:** split release (maintainer decision 2026-11-02):

- **v1.0** ships the new login: Part B (phone as key) and Part C (security
  notices). The family server does not go live with the old login design.
  This is a deliberate exception to the v1.0 feature freeze.
- **v1.2** ships Part A: the simple Android app ("Kaiku Einfach") and video
  calls. Planned after v1.1 (admin MCP / agent bots); its Android UI work may
  start in parallel.

## Intent

Kaiku's family server has two kinds of users:

- **Full users** (the maintainer, siblings): want the whole feature set:
  guilds, voice channels, roles, bots, settings.
- **Parents and grandparents**: want to chat, send photos and call. The full
  client confuses them. They should get an app that is as simple as WhatsApp,
  and they should never have to deal with passwords, codes or server URLs.
  The maintainer sets up their phones for them.

At the same time, logging in should get both simpler and safer: the phone
becomes the key. A phone with a Kaiku app is the second factor; new logins
are confirmed in the app, ideally by scanning a QR code shown on the PC.

## Goals

1. **"Kaiku Einfach"**: a second Android app with chat list, chat, and voice
   and video calls. Nothing else.
2. **Phone as key**: an active, signed-in mobile app (either app) is the
   user's second factor. Every new login on another device is confirmed on
   the phone.
3. **QR login on the PC**: the PC shows a QR code, the phone scans it, the
   user sees who is signing in and confirms, and the PC is signed in.
4. **Password login stays possible**, but needs confirmation in the app.
5. **No approval fatigue**: repeated rejected confirmations stop further
   prompts for a while and slow the attacker down.
6. **Device setup on behalf of someone else**: the maintainer (admin) sets up
   a family member's phone by QR; such accounts need no password.
7. **Security notices in chat**, not by e-mail.

## Non-goals

- **E-mail login or magic links**, also not as a fallback (decided: an e-mail
  account is a weaker key than the phone; see the 2026-10-31 web.de hijack).
- iOS.
- Android E2EE. E2EE is off for v1.0 everywhere and Android has none
  (`2026-10-20-v1-family-release-goal-design.md`, item 3). The simple app
  shows the same placeholder as the full app for any legacy encrypted message.
  Real Android E2EE remains its own project.
- Group video calls (more than two video participants). Group *voice* calls
  are in scope (see below).
- Moving the simple app to a separate repository or a separate backend.

## Anti-goals (a failure even if it works)

- Grandma's phone shows a settings screen, a permission maze or a password
  prompt she cannot get past.
- An attacker who knows a password gets in by spamming confirmations until
  someone taps "yes" (the 2022 Uber pattern).
- An attacker tricks a user into scanning *the attacker's* QR code and
  thereby signs the attacker in (QRLjacking) without the user noticing.
- An attacker locks the whole family out by triggering IP blocks: the family
  shares one home IP.
- Account recovery that is weaker than the login (anything that lets someone
  in with less than the phone).
- Two apps that drift apart because they share no code.

## Current state (2026-11-02, main)

- Android: native Kotlin/Compose (`mobile/android`), Hilt, login (password,
  OIDC, MFA), DMs, channels, voice (WebRTC, `VoiceCallService`), push via FCM
  and UnifiedPush including call wake (`push/CallWake*`).
- **QR login desktop → phone exists**: `POST /auth/qr/create` (authenticated,
  120 s Valkey token, `NX`) and `POST /auth/qr/redeem` (public, rate limited,
  `GETDEL` one-time) in `server/src/auth/mfa.rs`; Android
  `QrScannerScreen` / `QrRedeemScreen`.
- MFA: TOTP plus backup codes (`mfa.rs`), C24 re-auth (`reauth.rs`).
- Rate limiting: Valkey Lua limiter incl. `failed_auth.lua`
  (`server/src/ratelimit`).
- DM calls with ringing exist in the web/desktop client (`components/call`,
  `stores/call`). **Video calls exist nowhere yet**; Android renders remote
  video only for screen shares.
- `users.password_hash` is nullable, but a DB constraint requires a password
  for `auth_method = 'local'`.

## Part A: Kaiku Einfach (the simple app)

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
tools, threads, account settings beyond name and photo. The maintainer does
these in the full app.

### How it is built

- **One codebase, two apps**: Android product flavors (or a separate `:app-simple`
  application module) on shared modules for API, WebSocket, auth, push,
  voice/video and storage. Only the UI layer differs. The split into modules
  is the builder's call; the constraint is that a fix in login, push or calls
  lands in both apps at once.
- **Separate application id** so both apps can be installed side by side
  (the maintainer may want both on one phone for testing). Separate launcher
  name ("Kaiku Einfach") and icon variant.
- Both apps are built and signed by `release.yml` and attached to the
  release; distribution stays sideloaded APK until the store decision.
- i18n rules from `mobile/android/AGENTS.md` apply; the simple app is German
  first in practice, but all strings go through resources as usual.

### Video calls (new, all clients)

Video calls do not exist yet. v1.2 adds **1:1 video in DM calls** on desktop,
web, the full Android app and the simple app: camera on/off, front/back
camera on Android, self-view, remote video full screen. Group video is out.

## Part B: Phone as key

### Terms

- **Key device**: a signed-in mobile app (simple or full) registered as an
  approver for its user. A user can have several. A desktop client is never
  a key device.
- **Approval**: a request the server sends to all key devices of a user
  ("Firefox on Windows wants to sign in"), answered in the app.

### B1. Device setup by QR (exists, extended)

Today a signed-in desktop creates a QR token that a phone redeems. v1.2
extends this:

1. **Admin sets up a phone for another user**: in the full app, an admin
   creates the account (or picks an existing one) and taps "Set up device".
   A QR code appears with the server URL and a one-time token (TTL ~10 min,
   single use).
2. The simple app's first screen is only "Scan QR code". Scanning configures
   the server URL and signs in.
3. **Confirmation on the admin's side**: after the scan, the admin's screen
   shows "Phone X wants to become Oma's device" and the admin confirms. Only
   then does the phone get tokens and become a key device.
4. **Creating a setup QR for an existing account that already has a key
   device requires the admin's own re-auth with their second factor** (C24),
   because a setup QR is otherwise a back door into that account.
5. Every setup is audit-logged and triggers a security notice (Part C) to
   the user's existing devices.

The QR payload never contains a password, a refresh token or any long-lived
secret.

### B2. Passwordless accounts

Accounts set up this way may have **no password at all** (maintainer
confirmed). Their only login paths are: the key device itself, and QR login
on another device confirmed by the key device. A passwordless account cannot
sign in with username + password (there is none). The DB constraint for
`auth_method = 'local'` needs a new method or a relaxed rule; the builder
chooses.

### B3. QR login on the PC (new direction)

1. The PC (desktop client or web) shows "Sign in with your phone" with a QR
   code bound to *this* PC session (TTL ~60 s, single use, refreshed while
   the screen is open).
2. The user scans it with a key device.
3. The app shows **who is asking**: device/browser, OS, approximate location
   from IP, IP, time, and a **two-digit number** that the PC also shows.
4. The user taps the number shown on the PC (number matching) and confirms.
5. The PC session receives its tokens; no password is needed.

Number matching and the context screen are the defence against QRLjacking:
a user who scans a code from a phishing page sees an unknown device and a
number they cannot match.

**Passkeys (WebAuthn hybrid transport)** would give the same flow with an
OS-level Bluetooth proximity check and are the phishing-resistant standard.
They are not required for v1.2; the builder should note in the plan whether
Tauri on Linux/Windows/macOS supports them well enough to add later.

### B4. Password login with app confirmation

1. Username + password as today. Only **after** the password is correct does
   the server create an approval.
2. Key devices get a push: "Sign-in on Firefox / Windows, Berlin. Is this
   you?" with the same context screen and **number matching** (the PC shows
   the number, the app asks for it). A plain "yes" button is not enough.
3. The app offers **"That wasn't me"**: rejects, marks the password as
   compromised (user must change it at the next login on a trusted device),
   and sends a security notice.
4. Approvals expire after ~2 minutes.
5. If the user has **no key device** (no phone, full user only), the existing
   TOTP / backup code step applies instead. Users with a key device may still
   use TOTP / backup codes as the fallback path (phone dead or offline).
6. OIDC logins: after the IdP returns, the same app approval applies on top.

### B5. Anti-fatigue limits

- **Per account**: after 3 rejected or expired approvals within 15 minutes,
  the server sends no further approval pushes for that account for
  15 minutes; password logins during that window fail with a neutral message
  ("Confirmation is paused, try again later or sign in by QR"). QR login (B3)
  and TOTP/backup codes stay available, because they need the physical
  phone or a secret the attacker lacks.
- **Per IP**: after N rejected approvals from one IP across accounts, that IP
  is blocked from the password and approval endpoints for X minutes, using
  the existing Valkey limiter. Values are configurable; defaults are the
  builder's call (suggestion: 10 per hour, 1 h block).
- The per-IP block never blocks QR login or already-signed-in sessions, so
  an attacker on the family's shared home IP cannot lock everyone out.
- All rejects, pauses and blocks are audit-logged and visible to admins.

### B6. Recovery

In order of preference:

1. Another own device that is already signed in (second phone, desktop):
   register a new key device from there.
2. TOTP / backup codes (full users).
3. **Admin recovery**: an admin revokes the lost device and issues a new
   setup QR (B1, with admin re-auth). This is the only path for passwordless
   family accounts.
4. Last resort when the only admin has lost everything: a server-side CLI
   command on the host. A second admin (Matthias) avoids needing it; the
   design does not require one.

## Part C: Security notices in chat

- A built-in **system account "Kaiku"** sends security notices into a
  read-only DM with each user: new device set up, new login, rejected
  approval, approvals paused, password marked compromised, device revoked.
- Each notice also goes out as a push to all the user's devices.
- Notices cannot be deleted from the chat by the user's sessions, and every
  event is also listed in a **security log** in the account settings (both
  apps; read-only in the simple app), so an attacker who got in cannot erase
  the trace.
- No e-mail notices in v1.2.

## Constraints

- `CLAUDE.md` rules apply: feature branches/worktrees, squash-merge PRs,
  license constraints (`cargo deny`; any QR, camera or Android library must be
  MIT/Apache/BSD-compatible), UI contrast rules, changelog, quality gates.
- `server/src/auth` is security critical: every change gets a security
  review (Faramir) and an external review before merge, and the v1.0 auth
  changes get a final Fable review before the tag.
- Approvals must not depend on push alone: an open app receives pending
  approvals over the WebSocket, and opening the app shows them, so a delayed
  FCM/UnifiedPush message does not block a login.
- Public repository: no private hostnames, IPs or exploit details in code,
  docs or PRs.
- JWT/refresh lifetimes and token storage rules from `server/src/auth/AGENTS.md`
  stay as they are.
- Accessibility for older users is a requirement, not polish: large touch
  targets, readable text sizes, the contrast rules above, no gestures as the
  only way to do something.

## Migration of existing accounts (v1.0)

The family server already has accounts (RC testing) that use password + TOTP
and have no key device. After the upgrade:

- A user who signs in on a device without a key device is guided once to
  "Set up your phone" (existing desktop → phone QR, B1) and from then on is
  under the new rules.
- Users without any phone keep password + TOTP / backup codes (B4.5); they
  are not forced to install the app.
- Existing sessions stay valid; the new rules apply to new logins only.
- Admins can see per user whether a key device exists.

## Sequencing

- **v1.0 (before the Christmas release)**: Part B (B1–B6) and Part C, on the
  existing full Android app and the desktop/web clients. Plan, implement,
  review (security review + external review per PR), then RC3 and a
  **final Fable review of the auth changes before the v1.0 tag**.
  This is the critical path for v1.0; other v1.0 work must not wait on it
  and it must not ride along in unrelated PRs.
- **In parallel, out of the v1.0 release workflow**: the simple-app UI
  (Part A) on the existing APIs.
- **v1.2**: Part A ships (second APK in `release.yml`), video calls.
- v1.1 (admin MCP / agent bots, PR #726) keeps its order and builds on the
  new login (its re-auth uses the same C24 flow).

## Left to the builder

- Module/flavor split of the Android codebase.
- Server model for group calls and for approvals (tables, Valkey keys, WS vs
  push delivery).
- Exact TTLs and limit defaults within the ranges above.
- How the system account and the read-only DM are modelled.
- Video call implementation details (codec, simulcast or not, bandwidth).
- Whether passkeys are feasible later (note only).
