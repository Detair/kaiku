# v1.0 Family Release — Cross-Stream Contracts (Phase 1)

Shared interfaces between the four Phase-1 plans (server, desktop, Android,
release). Each plan **must** use these names; if code reading shows a name
clashes with existing code, the server plan adapts the name here first and
the other plans follow.

Spec: `docs/superpowers/specs/2026-10-20-v1-family-release-goal-design.md`
Review: `docs/reviews/2026-10-22-v1-family-release-readiness.md`

## C1 — Message catch-up (server → Android, desktop)

`GET /api/messages/channel/{channel_id}?after={message_id}&limit={n}`
- Returns the existing `CursorPaginatedResponse<MessageResponse>`
  (`{items, has_more, next_cursor}`), **items ascending** (oldest first)
  when `after` is given; `has_more` true if more newer messages exist.
- `after` and `before`/`around` are mutually exclusive → 400.
- `limit` default 50, max 100 (same as existing).

## C2 — Server time on connect (server → clients)

`ServerEvent::Ready { user_id, server_time }` — `server_time` is an RFC 3339
UTC timestamp. Clients store it as their resync watermark.

## C3 — DM creation is pushed live (server → clients)

`ServerEvent::DmCreated { channel }` (wire `type: "dm_created"`), where
`channel` is the same DM channel object returned by the DM list endpoint.
Sent via `broadcast_to_user` to **every participant** on create /
get-or-create (only when newly created) / group DM create. The server also
auto-subscribes each participant's open sockets to the new channel.

## C4 — Admin-created accounts (server → desktop/web UI, ops)

`POST /api/admin/users` (system admin, elevated session)
- Body: `{ "username": string, "display_name": string|null, "temporary_password": string }`
- Response `201`: the admin user view of the created user.
- Created user has `must_change_password = true`.
- Login response gains `password_change_required: bool`; while true, only
  `POST /auth/me/password` (existing change-password endpoint),
  `POST /auth/refresh` and logout are allowed (others → 403 `PASSWORD_CHANGE_REQUIRED`).
- Registration policy stays `closed`/`invite_only` for the family server.

## C5 — Unknown enum tolerance (server ↔ Android)

Android must deserialize unknown `channel_type` (and other wire enums) to an
`UNKNOWN` value and hide such channels; server adds no new types in Phase 1.

## C6 — Tauri detection (desktop internal, but referenced by release)

Single helper `isTauri()` exported from `client/src/lib/tauri/common.ts`,
implemented via `@tauri-apps/api/core` `isTauri()`. The literal
`"__TAURI__" in window` is banned by lint. Desktop REST goes through
`httpRequest` with base URL + bearer token provided by Rust
(`invoke("get_auth_info")`), tokens owned by Rust/keyring.

## C7 — Versioning (release → all)

Single source of truth: git tag `vX.Y.Z[-rc.N]`. `scripts/bump-version.sh
X.Y.Z[-rc.N]` updates `client/src-tauri/tauri.conf.json`, root
`Cargo.toml` workspace version, `client/package.json`, and Android
`versionName`/`versionCode` in `mobile/android/version.properties` (`MAJOR*1_000_000 + MINOR*10_000 + PATCH*100 +
rc`, rc 1–98, 99 for final). CI fails a tag build whose files disagree.

## C8 — Server container image (release → production deployment)

`ghcr.io/detair/kaiku-server:<version>` (`<version>` = tag without the
leading `v`; multi-arch not required, amd64),
built from `infra/docker/Dockerfile`, published by `release.yml` on tags.
Configuration only via environment variables (documented in
`docs/admin-guide/configuration/`); no config files baked in.

## C9 — Documented configuration (release CI → server)

Every `env::var` read in `server/src/config.rs` must appear in
`docs/admin-guide/configuration/environment.md`; a CI check enforces it.
Server PRs that add env vars (e.g. trusted proxy CIDRs) document them there.

## C10 — Android golden fixtures (server tests → Android tests)

Test-only file `server/tests/integration/android_contract_fixtures.rs`
(+ one `mod` line) serialises real server response types to JSON fixtures
consumed by Android tests. Owned by the Android stream, lands in the server
tree — the server stream must not create a conflicting file of that name.

## Reconciliation 2026-10-22 (after all four plans were written) — binding

- **C1:** an `after` anchor that is unknown or belongs to another channel → 400.
  `next_cursor` (with `after`) = id of the newest returned item.
- **C2:** `Ready` is sent only after the socket's subscriptions are live;
  clients run catch-up after receiving `Ready`.
- **C3:** `channel` = one element of the `GET /api/dm` array (`DMListResponse`).
- **C4 (final):** the 201 body is `UserSummary` (same shape as items of
  `GET /api/admin/users`). `password_change_required: bool` is carried by the
  login response, the `/auth/refresh` response and `GET /auth/me`. While it is
  true the server answers other REST calls with 403 code
  `PASSWORD_CHANGE_REQUIRED` (clients treat that as a fallback trigger) and
  refuses the WebSocket with close code 1008. Allowed: change password,
  refresh, logout, `GET /auth/me`.
- **C6 (desktop-internal):** `get_auth_info` returns
  `{server_url, access_token, expires_at_ms}`; new Tauri commands
  `refresh_session`, `session_request`.
- **C11 — WebSocket close codes:** 1011 and 1013 from the server mean
  "reconnect with backoff, then catch up"; 1008 means "auth problem — refresh,
  or show login / password change".
- **C12 — Desktop origins (server config + production deployment):**
  `CORS_ALLOWED_ORIGINS` for the family server includes
  `https://kaiku.pmind.de`, `tauri://localhost`, `http://tauri.localhost`
  (and the compose file forwards the variable). Refresh-token delivery to
  tauri origins per server plan Task 4; `x-refresh-token` header no longer
  required by the desktop.
- **Shared files:** `ci.yml` is edited by release Tasks 1/2/4/5 and desktop
  Tasks 9/10 — the later-merging stream rebases; never reformat unrelated jobs.

## C13 — Fixed SFU UDP port range (server → production deployment) — added 2026-10-22

The SFU (webrtc-rs) must bind ICE/media only within a configurable UDP port
range so the host firewall can open exactly that range:
`VOICE_UDP_PORT_MIN` / `VOICE_UDP_PORT_MAX` (both required together; unset =
current ephemeral behaviour for dev), applied via the webrtc-rs
`SettingEngine` ephemeral UDP port range; optional `VOICE_PUBLIC_IP` for
NAT 1:1 announcement if not already present. Documented per C9. Server
stream: add as **Task 13** (after Task 6), with a test that a peer
connection's local candidates use ports inside the range.

## C14 — Multiple TURN URLs (server → the production server, OPTIONAL, post-1.0 unless needed)

To enable the prepared TURNS-on-443 component, the server must hand out
several ICE/TURN URLs (`turn:…:3478?transport=udp`, `…?transport=tcp`,
`turns:turn.kaiku.pmind.de:443?transport=tcp`) from one config value
(e.g. comma-separated `TURN_URLS`). Not in Phase 1; pick up if a family
member cannot join voice from a restrictive network.

## Server-stream clarifications (2026-10-23, from the PR #663 whole-branch review) — binding

- **C2 gap-free reconnect order (per channel):** send `subscribe` → wait for
  `subscribed` → `GET /api/messages/channel/{id}?after=<last id>` → dedupe by
  id. `Ready` only guarantees the Valkey subscriptions are live; channel events
  are forwarded only for channels the socket subscribed to. Catch-up before
  subscribing can lose messages.
- **C3:** `DmCreated` auto-subscribe only covers sockets open at creation time
  and sends no `subscribed` ack. After any reconnect clients load `GET /api/dm`
  and subscribe to their DMs themselves.
- **C11 extension:** pending-password users are refused differently by auth
  mode — frame auth: `error` event `PASSWORD_CHANGE_REQUIRED` then close 1008;
  header auth (`Sec-WebSocket-Protocol`, desktop/Tauri): HTTP 403 at upgrade, no
  close frame. Clients treat HTTP 401/403 at the WS handshake like 1008
  (refresh, or show login / password change) — never a blind retry loop.

## C13b — SFU ICE interface filter (server → the production server) — added 2026-10-23

With `hostNetwork`, webrtc-ice binds one UDP port per local address per peer
connection (2 PCs per participant), exhausting the range and advertising
internal addresses (overlay/pod networks) as host candidates. Optional:
`VOICE_ICE_INTERFACES` (comma list of allowed interface names) and
`VOICE_ICE_IP_FAMILY` (`v4`|`v6`|`both`, default `both`). Unset = current
behaviour. the production server sets `VOICE_ICE_INTERFACES=eth0`, `VOICE_ICE_IP_FAMILY=v4`.
Note: the advertised IP is `PUBLIC_IP` (existing var); ports are advertised
unchanged, so the firewall forwards the same UDP port numbers 1:1.

## Phase 2 contracts (adopted 2026-10-23 by the coordinator)

- **C15 — i18n key naming:** web and Android share key names (`area.screen.element`, Android `area_screen_element`); one web catalog file per area. Details: `2026-10-23-v1-i18n-phase2.md`.
- **C16 — error text:** HTTP/WS errors carry the server error `code`; user-facing text is produced only by the client `errorText(code)` mapping (web) / string resource lookup (Android), never from server message strings.
- **C17 — locale completeness:** a locale folder is either empty (pure fallback) or complete; CI enforces identical key sets.
- Rulings: build-time-only Android lint-rule dependency (Apache-2.0) accepted; Android lint becomes a blocking CI step (i18n plan Task 10).
- **C18–C21 — Push (adopted 2026-10-23):** C18 push config discovery, C19 per-device registration/delete, C20 wake-signal wire format (incl. `read` kind; FCM data values as strings; priorities; batching), C21 mention syntax and push targets. Definitions in `2026-10-23-v1-push-phase2.md` (renumbered from the planner's C15–C18 to avoid clashing with the i18n contracts).
- **C22–C25 — Before-rollout (adopted 2026-10-23):** C22 optional features flag set, C23 `guild_ids` on admin-created accounts, C24 re-auth for elevation, C25 immediate session end on ban/revoke. Definitions in `2026-10-23-v1-before-rollout-phase2.md`.
- Ruling (coordinator, Kaiku decision delegated by the maintainer): forum, announcement channels, scheduled events and membership screening default OFF on the family server via the C22 flag — reversible per server setting; the maintainer may flip any of them on.

- **C19 amendment (2026-10-24, server security review):** registering a `device_id` does NOT remove other users' rows with the same `device_id` (client-asserted; would let anyone disable another user's push). Cross-user cleanup only on an exact `(provider, endpoint)` match. Android uses a random per-install UUID as push `device_id`, never the E2EE device id; logout deletes the device row.
- **C19 amendment 2 (2026-10-24):** push token/endpoint max 2048 bytes (PostgreSQL B-tree index limit on the unique (user_id, endpoint) index); longer → 400 `VALIDATION_ERROR`.
- **C20 note 1 (2026-10-24):** FCM keeps at most 4 collapse keys per offline device (Firebase docs, "Non-collapsible and collapsible messages") (collapse_key = channel id) with no guarantee which survive → clients must never rely on push for completeness; app start/reconnect always runs C1 catch-up.
- **C20 note 2 (2026-10-24):** wake/read ordering is not guaranteed; on a wake the client fetches the channel and notifies only for messages newer than the server read cursor — if there are none, it shows no notification; on `read` it clears the channel notification. A trailing wake is only a fetch hint.
- **C20 note 3 (2026-10-24):** `read` and wakes share collapse_key = channel id, so FCM may drop a `read` offline: on a wake with nothing unread after the read cursor, the client actively clears any existing notification for that channel.
- **C24 amendment (2026-10-25, maintainer: "OIDC und Passwort"):** re-auth for elevation accepts password (+TOTP if enabled) OR a fresh OIDC login (+TOTP if enabled; a provider `amr` claim does not replace it). `GET /auth/reauth/methods` → `{password, mfa_required, oidc:[{provider_id, display_name}]}` (oidc = usable providers linked before the session first authenticated or linked while elevated). Start `POST /auth/reauth/oidc/authorize/{provider_id}` `{redirect_uri|null, reason|null, mfa_code|null}` → `{url}`; IdP gets prompt=login, max_age=0, redirect always `{PUBLIC_URL}/auth/oidc/callback`; auth_time must be ≤300 s old (30 s skew), missing → REAUTH_AUTH_TIME_MISSING. Hand-back: desktop `?reauth=elevated&expires_at=` / `?reauth_error=CODE`, browser postMessage `oidc-reauth-callback`. Password path adds 400 REAUTH_OIDC_REQUIRED; REAUTH_UNAVAILABLE only with neither method. Error codes MFA_CODE_REQUIRED / INVALID_CREDENTIALS shared by both paths.
- **C20 addendum "call wake" (2026-10-26, final; server + Android agreed):** data-only `t=call`, `channel_id` (DM), `call_id` (uuid, all states), `caller_id` (all states), `call_state=ringing|ended`, `reason` (ended only: no_answer|cancelled|all_declined|last_left); no names/content/count. HIGH priority; collapse_key `call-<channel_id>` (separate from message wakes); TTL ringing 45 s, ended 1 h. Recipients: all participants except the caller; ended only to those rung; DM block rules; bypasses coalescing window; own per-callee budget (~10 ringing/min) separate from message wakes; `ended` for a sent ringing is never rate-limited; ringing dropped by budget → no ended. Client: ringing → heads-up "X is calling you – open Kaiku on the PC"; ended+no_answer/cancelled → "Missed call from X"; all_declined/last_left → remove; ignore ended with a different call_id; must not rely on ringing arriving. No `answered` state (accepted elsewhere → notification stays until ended or until Android's ~60 s ringing timeout).

## Login contracts C26–C29 (v1.0, frozen 2026-11-03)

Spec: `docs/superpowers/specs/2026-11-02-phone-key-login-and-simple-app-design.md`, **revision 5**
(frozen, `d6ca7109`). Section labels (B.x, Fx, SMx, CCx, Kx) point there and carry the rationale;
where this file and the spec disagree on behaviour, the spec wins and this file is corrected. The package cut
(coordinator working notes, 2026-11-03, §1) is used only where the spec is silent; the spec drops its
approval push, number matching and possession-only keys. The "C29 proposal" in
`2026-11-03-v1.0-lp-passkeys.md` is superseded by C29 below (deltas listed there).

**Common rules (C26–C29)**

- **Wire:** JSON, `snake_case`; ids are lowercase hyphenated UUID strings; times RFC 3339 UTC, except
  the integer Unix seconds inside signed contexts (C26); binary values are base64url without padding
  (**b64u**). Unless stated: `id` / `*_id` are UUID strings, `*_at` RFC 3339 strings, counts
  integers, names and labels strings, flags booleans. Clients ignore unknown fields and map unknown
  enum values to UNKNOWN (C5).
- **Errors:** the existing body `{"error": CODE, "message": string}`; codes listed with fields (e.g.
  `{tries_left}`) add those fields next to `error`. Text only via `errorText(code)` / string resources
  (C16). 429 `TOO_MANY_ATTEMPTS` carries `Retry-After`; Valkey or database outage → 503
  `SERVICE_UNAVAILABLE`.
- **Client class:** `X-Kaiku-Client: desktop` → `desktop`, `X-Kaiku-Client: android` → `android` (L2
  sends it on every Android request), otherwise `web`. Native clients get every token and secret in
  response bodies and keep them in Rust / Keystore-wrapped storage; browsers get them only as the
  cookies below (`HttpOnly; Secure; SameSite=Strict`, path-scoped).
- **Pre-session secrets** (256-bit CSPRNG, stored only as keyed hashes, B.2 "Data model"):

| Secret | Created at | Web | Native |
|---|---|---|---|
| collection secret (SM1) | attempt create, `/auth/login`, OIDC callback | cookie `__Secure-kaiku_attempt`, `Path=/auth/login-attempts` | `client_challenge` = S256(verifier) at create (43 chars, as the OIDC desktop hand-off); `verifier` in the body of every later attempt call |
| request secret (SM3) | request start | cookie `__Secure-kaiku_request`, `Path=/auth/account-requests` | body `request_secret`; header `X-Kaiku-Request-Secret` |
| registration ticket (F8) | `/auth/register` | cookie `__Secure-kaiku_ticket`, `Path=/auth/registration` | body `registration_ticket`; header `X-Kaiku-Ticket` |
| claim secret (SM2) | enrolment claim | cookie `__Secure-kaiku_enroll`, `Path=/auth/enrollments` | body `claim_secret` |

- **Two guards (B.4):** *full session* → 403 `FULL_SESSION_REQUIRED`; *step-up grant* for (`sid`,
  action, target), consumed in the action's transaction → 403 `STEP_UP_REQUIRED`. Both decide from
  committed rows (`sessions.assurance`, read on every request per B.4 rev 5, and `step_up_grants`),
  never from a token alone.
- **Step-up actions (F7)**, shared by C26 `device_stepup` and C29 passkey step-up. `target` is a
  canonical string: lowercase UUID, the literal `others`, or `""` (none); `target_hash =
  SHA-256(lp(target))` (`lp` in C26). `StepUpGrant = {action, target, via: "passkey"|"device",
  expires_at}` (≤ 300 s, single use, bound to the `sid`; F7 "At use" re-checks).

| Action | Target | Consumed by |
|---|---|---|
| `passkey.add` | – | `POST /auth/passkeys/register/*` (C29) |
| `passkey.remove` | passkey id | `DELETE /auth/passkeys/{id}` |
| `key_device.remove` | key device id | `DELETE /api/approval-devices/{id}` (C26) |
| `enrollment.issue` | – | `POST /auth/enrollments` (`own`; consumed at issue, recorded on the enrolment, F5a) |
| `password.set` / `password.remove` | – | `POST` / `DELETE /auth/me/password` |
| `totp.set` / `totp.remove` | – | `/auth/mfa/setup` + `/auth/mfa/verify` / `/auth/mfa/disable` |
| `recovery_codes.create` | – | `POST /auth/recovery-codes` |
| `identity.link` / `identity.unlink` | – / identity id | `/auth/me/identities/authorize/{provider}` / `DELETE /auth/me/identities/{id}` (contract call: an OIDC identity is a sign-in method, B.1) |
| `sessions.end` | session id or `others` | `DELETE /auth/sessions/{id}`, `DELETE /auth/sessions` |
| `recovery_contact.set` | contact user id, `""` to remove | `PUT` / `DELETE /api/me/recovery-contact` (C28) |
| `request.cancel` | request id | `POST /api/account-requests/{id}/cancel` (C27) |
| `event.repudiate` | security event id | `POST /api/me/security-events/{id}/repudiate` (C28) |
| `account.delete` | – | the existing account deletion route |
| `admin.elevate` | – | `POST /api/admin/elevate` (opens the existing 15-min window, F7) |
| `admin.enrollment.issue` | subject user id | `POST /api/admin/users/{user_id}/enrollments` (C27; one grant per code) |
| `admin.authenticator.remove` | key device or passkey id | "Remove device now" (C26) |
| `admin.may_approve` / `admin.factor.set_state` / `admin.recovery_contact.set` | account user id | C27 / C27 / C28 admin routes |

- **Versioning:** JSON changes are additive only; removing or re-meaning a field needs a new route,
  and any change to signed bytes needs a new magic string (C26). Every value list here (kinds,
  actions, states, event types, prompts, reasons) is append-only.
- **Ownership:**

| Contract | Server side | Consumers |
|---|---|---|
| C26 | L1a (challenge core, signing, key devices, device-key sign-in, device step-up, enrolment core, golden file); L1b: admin issue route, "Remove device now" | L2 Android (signer, scanner, approval screens), L2 desktop/web (enrolment issuer panels, key-device list); v1.1 M2 (desktop signer) |
| C27 | L1b (attempts, password/OIDC login, QR and setup payloads, admin setup, account requests, tickets, routes moved behind guards); L1a (assurance, claims, guards, grants, refresh reuse, caps, session limit) | L2 desktop/web, L2 Android |
| C28 | L1a (system account, Kaiku DM, `security_events`, log API, `t=security`, coalescing, WS); L1b (every login, request and contact notice, recovery-contact routes) | L2 desktop/web, L2 Android; v1.2 E1 (read-only log and notice actions) |
| C29 | LP (WebAuthn routes, config, `assetlinks.json`, desktop flow and loopback page, Android Credential Manager); L1a (grant store, full-session guard); L1b (request, ticket and enrolment passkey variants, OIDC desktop delivery) | LP clients (web, desktop, Android); L2 |

## C26 — Device signatures, approvals and key devices (L1a → L2 Android, L2 desktop/web; v1.1 M2)

**Kinds** (B.2; JSON `kind` / byte):

| Kind | Byte | Variants (byte) | Answered at | `response` | TTL |
|---|---|---|---|---|---|
| `login` | 0x01 | `qr` 1, `password` 2, `oidc` 3 | `POST /api/approvals/{id}/answer` | the 32 scanned token bytes | 2 min from the claim |
| `enroll` | 0x02 | `own` 1, `admin` 2 | `POST /api/approvals/{id}/answer` | the typed digits as ASCII (6 bytes, no spaces) | until the enrolment code expires (≤ 5 min) |
| `device_login` | 0x03 | – | `POST /auth/device-login` | empty | 2 min |
| `device_stepup` | 0x04 | – | `POST /api/approvals/{id}/answer` | empty | 2 min; grant ≤ 5 min |
| `enroll_pop` | 0x05 | – | `POST /auth/enrollments/claim` | empty | the setup token's lifetime |

`login` and `enroll` are approvals: `login` is answered approve or report, `enroll` approve only
(its "No" is the unsigned deny, which cancels the enrolment its own issuer started); the others are
proofs (approve only). 0x10–0x1F are reserved for the v1.1 agent kinds; a v1.0 device refuses every kind
byte it does not know. All challenges are single use and ≤ 10 min (DB CHECK).

**Encoding.** `lp(x)` = u32 big-endian byte length ‖ x; UUID = 16 bytes in RFC 9562 order (the hex
digits of the string; Java `putLong(msb)` then `putLong(lsb)`); `i64` = big-endian Unix seconds;
strings = their UTF-8 bytes exactly as received in JSON (no normalisation, no trimming); enum = 1
byte; hash = 32 raw bytes.

```
M            = "kaiku-sig-v1" ‖ kind ‖ challenge_id ‖ nonce[32] ‖ i64(expires_at) ‖ lp(origin)
               ‖ user_id ‖ device_id ‖ context_hash ‖ decision ‖ lp(response)
context_hash = SHA-256("kaiku-sig-context-v1" ‖ kind ‖ C)
```

- Magic strings are ASCII without terminator (12 and 20 bytes). `origin` = the ASCII serialisation
  of `PUBLIC_URL`'s origin (`https://chat.example.com`: lowercase, no trailing slash). `user_id` =
  the approver (for `enroll_pop` the subject); `device_id` = the answering device's per-install id
  (below). `decision`: approve 0x01, deny 0x02 (reserved; never signed in v1.0), report 0x03.
- **Canonical contexts C** (field order binding; the JSON `context` uses exactly these names):
  - `login`: `attempt_id ‖ collection_hash ‖ variant ‖ client_class ‖ lp(client_label) ‖
    i64(created_at) ‖ lp(server_name) ‖ lp(place) ‖ lp(username)`
  - `enroll`: `enrollment_id ‖ subject_id ‖ lp(subject_username) ‖ variant ‖ authenticator_type ‖
    new_key_hash ‖ lp(label) ‖ claimant_check`
  - `device_login`: `client_class`
  - `device_stepup`: `sid ‖ lp(action) ‖ target_hash ‖ lp(target_label)` (JSON carries `target`;
    the device computes `target_hash`)
  - `enroll_pop`: `enrollment_id ‖ new_key_hash ‖ attestation_challenge ‖ lp(subject_username)`
  - Bytes: `client_class` web 1, desktop 2, android 3; `authenticator_type` key_device 1, passkey 2;
    `claimant_check` none 0 (`own`), in_person 1, called_back 2.
  - Values: `collection_hash` = the attempt's S256 challenge (decoded) or SHA-256 of its cookie
    secret; `new_key_hash` = SHA-256 of the new key's SPKI DER, or of the raw passkey credential id;
    `attestation_challenge` = the enrolment nonce from the SM2 open (32 bytes); `created_at` = the
    attempt's creation; `server_name` = the server's display name; `place` = `""` without a local
    GeoIP database; `username` / `subject_username` = the account signed in / set up; `label` = the
    new device's self-reported model (or the browser label for a passkey); `target_label` = the
    server's display label for the target.
  - The display strings `username`, `subject_username` and `target_label` are added to the spec's
    binding fields under B.2's rule that the device signs exactly what it shows.
- **Keys and signatures:** P-256. `public_key` = b64u SubjectPublicKeyInfo DER, uncompressed point
  (Android `getEncoded()`). `signature` = b64u ASN.1 DER ECDSA over M with SHA-256 (Android
  `SHA256withECDSA` over the whole M; the app never prehashes). Verifiers accept high and low `s`
  (Keystore produces both; V01, V02 and V04 have high `s`).

**`ApprovalChallenge`** (server → device): `{challenge_id, kind, variant|null, expires_at,
signing_prefix, context}`. `signing_prefix` = b64u of M up to and including `context_hash`; the
device appends `decision ‖ lp(response)` (resolution: the spec's "everything before `response`"
cannot include a decision the user has not made yet). Prefix layout: magic [0,12), kind [12],
challenge_id [13,29), nonce [29,61), expires_at [61,69), origin length [69,73), origin, then
user_id, device_id, context_hash (64 bytes). Before asking for UV the device checks (B.2): magic;
kind = `kind` = the screen it shows; challenge_id; origin = its pinned origin; user_id = its user
(for `enroll_pop` the subject returned with the challenge); device_id = itself; expires_at in the
future (60 s skew); `context_hash` recomputed from `context` equals the prefix's. It displays only
values of that parsed `context`. Any mismatch → it refuses and shows an error.

**Routes** (L1a unless marked). "Key device" = a session whose `authenticator_id` is that active key
device (B.2), else 403 `KEY_DEVICE_REQUIRED`.

| Route | Auth | Body → answer |
|---|---|---|
| `POST /api/approvals/claim` | key device, `may_approve` on | `{token}` (b64u of the 32 QR bytes) → 201 `ApprovalChallenge` (`login`); `password`/`oidc` attempts only by the attempt's own user |
| `POST /api/approvals/step-up` | key device | `{action, target}` → 201 `ApprovalChallenge` (`device_stepup`) |
| `POST /auth/enrollments/{id}/challenge` | the issuing key device | – → 201 `ApprovalChallenge` (`enroll`); a new one voids the open one |
| `POST /api/approvals/{id}/answer` | the claiming / answering device | `{decision: "approve"\|"report", response, signature}` → 200 `{state, grant?: StepUpGrant, enrollment?: Enrollment}` |
| `POST /api/approvals/{id}/deny` | the claiming device | – → 204 (unsigned "No": `login` challenge and attempt `denied`; `enroll` enrolment `cancelled`) |
| `POST /api/approvals/{id}/release` | the claiming device | – → 204 (back / dismissed: the attempt returns to `open`) |
| `POST /auth/device-login/challenge` | public | `{user_id, device_id}` → 201 `ApprovalChallenge` (`device_login`) |
| `POST /auth/device-login` | public | `{challenge_id, signature}` → 200 `AuthResponse` (C27 fields, `refresh_token` in the body, `assurance: "full"`, `auth_methods: ["device_key"]`) |
| `GET /api/approval-devices` | any session | → `{devices: [KeyDevice]}` |
| `DELETE /api/approval-devices/{id}` | full + `key_device.remove`/id | → 200 `KeyDevice`; the own id is "Sign out and remove this phone" (its session ends after this response) |
| `POST /api/approval-devices/{id}/invalidated` | that device's live session | – → 200 `KeyDevice` (revoked, `key_invalidated`, notice); unsigned because it only removes; without a live session the app just shows setup again |
| `POST /api/admin/users/{user_id}/authenticators/{id}/remove` (L1b) | admin, full + `admin.authenticator.remove`/id | → 204 ("Remove device now"; another admin's account → 403 `ADMIN_TARGET_IS_ADMIN`, F6 rev 4) |

Answer results: `login` approve → `{state: "approved"}`, report → `{state: "reported"}` (F3: password
compromised); `enroll` right code → `{state: "consumed", enrollment}` (`done`); `device_stepup` → `{state: "consumed", grant}`. Order (B.2): for
`login` the server compares `response` first (mismatch → 400 `APPROVAL_RESPONSE_MISMATCH`, nothing
changes); for `enroll` it verifies the signature first, then compares: wrong → 400
`ENROLL_CODE_WRONG {tries_left}` and the challenge is spent; the third → 410 `ENROLLMENT_FAILED`.

**Enrolment core (SM2; L1a).** Issue contexts and the setup payload: C27.

- Issuer routes, with `{issuer}` = `/auth/enrollments` (bearer: `own`),
  `/auth/account-requests/current/enrollments` (request secret, request `ready`: `request`) or
  `/auth/registration/enrollments` (ticket: `registration`): `POST {issuer}` `{}` → 201
  `EnrollmentIssued {enrollment_id, setup_token, setup_url, expires_at}` (`own` needs the grant
  `enrollment.issue`: by `device_stepup` on a key device with `may_approve` on, or by passkey in a
  full PC session); `GET {issuer}/{id}` → `Enrollment`; `POST {issuer}/{id}/confirm {code}` (PC,
  request and ticket issuers) → 200 `Enrollment` or `ENROLL_CODE_WRONG` / `ENROLLMENT_FAILED`;
  `DELETE {issuer}/{id}` (issuer; for `admin` any admin) → 204. `admin` enrolments (issued in C27)
  use the bearer issuer routes.
- New-device routes (public):
  - `POST /auth/enrollments/open {setup_token}` → 200 `{server_name, enrollment_nonce,
    expires_at}`: consumes nothing, takes no lock (SM2's "open (GET)" as a POST, so the token stays
    out of URLs and logs).
  - `POST /auth/enrollments/pop-challenge {setup_token, device_id, public_key}` → 201 `{challenge:
    ApprovalChallenge, subject: {user_id, username, display_name}}`; one live challenge per enrolment.
  - `POST /auth/enrollments/claim {setup_token, challenge_id, signature, public_key,
    attestation_chain: [b64u DER, leaf first], label, uv_class: "per_use"|"window_10s", api_level}` →
    201 `{enrollment_id, state: "claimed", code, code_expires_at, claim_secret}`; the code goes only
    to this response. A claim that carries the issuing client's own request secret or ticket (the
    Android app enrolling itself, SM2) → 201 `{enrollment_id, state: "done", authenticator_state:
    "active"|"pending", ready_at|null}` without a code.
  - `POST /auth/enrollments/status {enrollment_id, claim_secret, wait?: 0..25}` (browser: the claim
    cookie) → 200 `{state, authenticator_state|null, ready_at|null}` (long-poll). Then device-key
    sign-in.
  - Passkey claims from a browser: C29.
- Limits: setup token 256-bit, single use, 10 min; code 6 digits (shown "123 456"), valid 5 min from
  the claim (unconfirmed → `expired`), 3 tries; ≤ 5 open per issuer (409 `ENROLLMENT_LIMIT`).
- `Enrollment = {id, variant: "own"|"admin"|"request"|"registration"|"cli", state:
  "issued"|"claimed"|"done"|"cancelled"|"expired"|"failed", subject: {user_id, username,
  display_name}, claimed: {authenticator_type, label, key_hash}|null, tries_left,
  code_expires_at|null, expires_at, request_id|null}`; issuer screens show `claimed.label` as "a
  device calling itself …" (F5a).
- Suitability (B.1, CC1): the app checks screen lock and hardware keystore before any scan or
  typing; the server stores the chain unverified and refuses a claim whose attestation leaf reports
  less than B.1 (security level below TEE, no user authentication, a timeout other than per-use or
  ≤ 10 s) → 400 `DEVICE_UNSUITABLE {reason: "attestation_missing"|"attestation_properties"}`.

**Key devices.** `device_id` is the app's random per-install UUID, the same value as its C19 push
`device_id`, so C28 pushes reach the device. `KeyDevice = {id, device_id, label, platform:
"android", state: "pending"|"active"|"revoked", enrolled_via: "own"|"admin"|"request"|
"registration"|"cli", uv_class, created_at, activated_at|null, ready_at|null, last_used_at|null,
revoked_at|null, revoke_reason: "owner"|"admin"|"request"|"repudiated"|"key_invalidated"|
"account_deleted"|"cli"|null, is_current}`. Device-key sign-in errors: 404 `KEY_DEVICE_UNKNOWN`, 409
`KEY_DEVICE_PENDING {ready_at}`, 403 `KEY_DEVICE_REVOKED` (the app deletes its key and shows setup),
403 `ACCOUNT_BANNED`. **C19 amendment 3:** on a key device a plain sign-out keeps the push
subscription (lapsed key devices still get call wakes and C28 notices, B.4); "Sign out and remove
this phone" and every revocation delete it, after the last notice.

**Delivery (v1.0).** No approval push and no approval list (B.2 "Delivery"). Reserved for v1.1 M2,
never sent in v1.0, ignored by clients: push `{"t":"approval","approval_id":"<uuid>"}` (HIGH,
collapse key `approval-<id>`, TTL = the challenge TTL), WS `approval_pending {approval_id, kind}` and
`approval_resolved {approval_id, state}`, `GET /api/approvals?state=open`. v1.0 WS hints, sent to
every socket of the named user (clients refetch and never act on a hint alone):
`enrollment_updated {enrollment_id, state}` (the issuer) and `authenticators_changed {user_id}` (any
key device, passkey, TOTP or password change on the account).

**Errors:** `APPROVAL_NOT_FOUND` 404 (also another device's challenge), `APPROVAL_STATE` 409,
`APPROVAL_EXPIRED` 410, `SIGNATURE_INVALID` 400, `APPROVAL_RESPONSE_MISMATCH` 400,
`LOGIN_CODE_STALE` 404 (a claim token that matches nothing; the app says "This code has just changed
– scan the new one"), `LOGIN_ATTEMPT_CLAIMED` 409, `LOGIN_ATTEMPT_OTHER_ACCOUNT` 409,
`APPROVAL_NOT_ALLOWED` 403 (`may_approve` off), `KEY_DEVICE_REQUIRED` 403, `KEY_DEVICE_UNKNOWN` 404,
`KEY_DEVICE_PENDING` 409, `KEY_DEVICE_REVOKED` 403, `ENROLLMENT_TOKEN_INVALID` 404,
`ENROLLMENT_ALREADY_CLAIMED` 409, `ENROLL_CODE_WRONG` 400, `ENROLLMENT_FAILED` 410,
`ENROLLMENT_STATE` 409, `ENROLLMENT_LIMIT` 409, `DEVICE_UNSUITABLE` 400,
`LAST_STRONG_AUTHENTICATOR` 409, `APPROVAL_UNAVAILABLE` 503 (kept for v1.1).

**Golden vectors (normative).** L1a writes them to
`mobile/android/app/src/test/resources/golden/kaiku_sig_v1.json` (L1a owns the file; L2 and M2 read
it; the server test reads the same file): per vector the inputs, `context` as sent, `context_hash`,
`signing_prefix`, `message` (hex) and `signature`. Both test suites build M from the inputs, recompute
`context_hash` from `context`, and verify `signature` with `public_key` (server `p256`; Android JVM
`java.security` `SHA256withECDSA`). The signatures are RFC 6979 deterministic (checked with OpenSSL
`nonce-type:1`); a test may reproduce them, but only verification is normative. The key is a public
test key, never used outside tests.

```
key          d = SHA-256("kaiku-sig-v1 golden key") =
             5889e045f69d4e894c45044f0147928386106b385dc59153224f1595b8be9369
public_key   MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEaGyjIg4bpThSbH_2lxjxOCbet8d6OXzr6dvBwmYvZkX9zHpvoOqY5gJuY5LHJQSE3ny7KokI1Ds-0iPwEDhAZQ
common       origin "https://chat.example.com"; nonce = bytes 00 01 … 1f; expires_at 1793707320
             (2026-11-03T12:02:00Z); user_id 11111111-1111-4111-8111-111111111111;
             device_id 22222222-2222-4222-8222-222222222222; decision approve (01);
             challenge_id 33333333-3333-4333-8333-3333333333NN, NN = vector number

V01 login (qr), response = bytes a0 a1 … bf (the scanned token);
    collection_hash = S256 of the RFC 7636 Appendix B verifier
  context      {"attempt_id":"44444444-4444-4444-8444-444444444444","collection_hash":"E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM","variant":"qr","client_class":"desktop","client_label":"Kaiku desktop on Windows","created_at":1793707200,"server_name":"Familie Müller","place":"Hamburg, DE","username":"alice"}
  C            4444444444444444844444444444444413d31e961a1ad8ec2f16b10c4c982e0876a878ad6df144566ee1894acb70f9c3
               0102000000184b61696b75206465736b746f70206f6e2057696e646f7773000000006ae9ccc00000000f46616d696c69
               65204dc3bc6c6c65720000000b48616d627572672c20444500000005616c696365
  context_hash 2e0d6af64a671a1cc0a3cf4435231ffe08b56f6c3dbe9b74791da593e017fadf
  M (198 B)    6b61696b752d7369672d76310133333333333343338333333333333301000102030405060708090a0b0c0d0e0f101112
               131415161718191a1b1c1d1e1f000000006ae9cd380000001868747470733a2f2f636861742e6578616d706c652e636f
               6d11111111111141118111111111111111222222222222422282222222222222222e0d6af64a671a1cc0a3cf4435231f
               fe08b56f6c3dbe9b74791da593e017fadf0100000020a0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9
               babbbcbdbebf
  signature    MEUCIDrPmE2yrciCLF2DnvgaeI1DzcPWXyyyPlD46qZ8WGQ8AiEAlpz219hjp91A-xULRpmddZWXEwsijLT5tAREO3WzLe0   (high s)

V02 enroll (admin), response = "123456" (typed); new_key_hash = SHA-256("kaiku-golden-new-key")
  context      {"enrollment_id":"55555555-5555-4555-8555-555555555555","subject_id":"66666666-6666-4666-8666-666666666666","subject_username":"oma","variant":"admin","authenticator_type":"key_device","new_key_hash":"Bs5H8qw80BPj7K2nOZOOcMYlOZs6KhOHt1xIDKHM_9Q","label":"Pixel 8","claimant_check":"called_back"}
  C            5555555555554555855555555555555566666666666646668666666666666666000000036f6d61020106ce47f2ac3cd0
               13e3ecada739938e70c625399b3a2a1387b75c480ca1ccffd400000007506978656c203802
  context_hash 95cc384c6457731aa857a59bceeb1709add698e15c1cdce489bc8c2eeaed3c1b
  SHA-256(M)   754b25e37dfac6fb063a3cc1c5b6238065ae171f0aae1da41aee6c3de74bd55c   (172 bytes)
  signature    MEUCIE4YiBVDbJKd0nzIeslFWb5lP4OWx3fF5__aB5oaCk0HAiEAy8QaPa9Zeh43i9i9wVVDEKfPMsdM-ELJ2Y22NEF96pU   (high s)

V03 device_login, response empty
  context      {"client_class":"android"}
  C            03
  context_hash 7bc190cbec9880136fdb564f293739d948deb11aeeb8dcaf95aa92c0424e29ae
  SHA-256(M)   a5cbee81abacb5b317edacf06d75159bf80333278d65636c2f784ed8f0592703   (166 bytes)
  signature    MEUCIQCfAESPWEcAVeTeo_Q3BDjOPw2yx_3eadQyYLl-b4MSZQIgIU0y3dtcSXviAGAepmzXTihWxL7LZHTuUZNH2lqMHp8

V04 device_stepup, response empty
  context      {"sid":"77777777-7777-4777-8777-777777777777","action":"key_device.remove","target":"99999999-9999-4999-8999-999999999999","target_label":"Pixel 8"}
  C            77777777777747778777777777777777000000116b65795f6465766963652e72656d6f7665249895243dc67001ed138a
               d73a3cf6f8f60798146055289b12bbf12c0db7ceaf00000007506978656c2038
  context_hash daa72edfe0362bf796d8c7ee30267354b9491e0ec97e36087db40e8d362f61c9
  SHA-256(M)   f76b957ca40e9625cc10ee1cb54d6e38d50380f1af778b9e24efa6a94ae0ebf7   (166 bytes)
  signature    MEYCIQDiXOgsy-XH9ewnDUbhcwC9s_NVOqI2QMCXDlGVgwWxzAIhAN5ZtWtqQTFZ9KTwykkn6uoRkWJwFyt7Ww8bCMWZXT0P   (high s)

V05 enroll_pop, response empty; user_id 66666666-6666-4666-8666-666666666666,
    device_id 88888888-8888-4888-8888-888888888888; new_key_hash = SHA-256(public_key DER);
    attestation_challenge = bytes 40 41 … 5f
  context      {"enrollment_id":"55555555-5555-4555-8555-555555555555","new_key_hash":"SNXZ3WXZTk1gg0TCFd1P6gmdzP1Rwao9hsqXg-SsX10","attestation_challenge":"QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1xdXl8","subject_username":"oma"}
  C            5555555555554555855555555555555548d5d9dd65d94e4d608344c215dd4fea099dccfd51c1aa3d86ca9783e4ac5f5d
               404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f000000036f6d61
  context_hash 0849d9cc3c467e1e5652673387a8847ba2a2803531cd9734256c5f58758122fc
  SHA-256(M)   7afd5d3b92e1a6f0f498df0cb963837c2f3f4231b55d590baf640921e85f8ad4   (166 bytes)
  signature    MEUCIQDUWAn2yLiDOjJD_wRjiR_Y-SJQESYfWARIuGnkEVzPZAIgJ4dKWYPvbvjKfpgkCU0H_byn4nO4hz3_ZTQYmJALFzY
```

## C27 — Sign-in flows, sessions and account requests (L1b, session core L1a → L2 desktop/web, L2 Android)

**Login attempts (SM1; F2–F4).** Every PC-side call after the create authenticates with the
collection secret (cookie or `verifier`); unknown id or wrong secret → 404 `LOGIN_ATTEMPT_UNKNOWN`.

| Route | Body → answer |
|---|---|
| `POST /auth/login-attempts` | `{client_challenge?}` → 201 `LoginAttempt` (`kind: "qr"`, F2 "Sign in with phone") |
| `POST /auth/login` | `{username, password, client_challenge?}` → 202 `LoginStep`, or 401 `INVALID_CREDENTIALS` (F3's neutral answer: same message, status and timing for a wrong password, an unknown user and a closed password path). Never a session; `mfa_code` is gone (sent → 400 `CLIENT_OUTDATED`) |
| OIDC (F3 variant) | the callback yields a `LoginStep` instead of a session: web postMessage `oidc-callback {status, attempt}` with the attempt cookie set; desktop and Android redeem a one-time code (C29 delivery; Android keeps its Custom Tab and gets only `kaiku://auth/callback?code=…`) at `POST /auth/oidc/desktop/exchange {code, verifier}` → 202 `LoginStep` or the neutral 401; the OIDC start's `client_challenge` is the attempt's collection challenge |
| `POST /auth/login-attempts/{id}/poll` | `{verifier?, wait?: 0..25}` → 200 `LoginAttempt` (long-poll; returns early on a state change or a new QR token) |
| `POST /auth/login-attempts/{id}/collect` | `{verifier?}` → 200 `AuthResponse` (probation; browser: refresh cookie as `/auth/login` today) |
| `POST /auth/login-attempts/{id}/totp` | `{verifier?, code}` → 200 `AuthResponse` (probation); `password`/`oidc` attempts in `open` or `claimed` |
| `POST /auth/login-attempts/{id}/cancel` | `{verifier?}` → 204 |

- `LoginStep = {status: "confirmation_required"|"totp_required", attempt: LoginAttempt}`:
  `confirmation_required` when the account has an active key device with `may_approve` on (the PC
  shows the confirmation QR; `attempt.totp_available` offers "Phone not at hand?"); `totp_required`
  without one but with an active TOTP; otherwise the neutral answer.
- `LoginAttempt = {attempt_id, kind: "qr"|"password"|"oidc", state:
  "open"|"claimed"|"approved"|"consumed"|"denied"|"reported"|"cancelled"|"expired", expires_at, qr:
  {payload, expires_at}|null, totp_available}`. `qr` is present only in a response that minted a new
  token (tokens are never stored in plain): at create, and on a poll in `open` once the current token
  is ≥ 60 s old or was used by a claim. The PC shows the newest; a rotation resets no counter.
- Native clients must send `client_challenge` and browsers must not (they get the cookie); otherwise
  400 `CLIENT_OUTDATED` / `VALIDATION_ERROR`. The TOTP step follows SM1 (first factor unchanged and
  not disabled, TOTP `active`, per-account counter).
- TTLs: attempt ≤ 10 min; QR token 60 s; claim challenge 2 min; collect ≤ 60 s after `approved`,
  while the approving device is active and the first-factor generation unchanged (B.7 rule 4).
- The PC learns outcomes only by polling (resolution: no attempt-bound WebSocket in v1.0). There is no
  typed sign-in code (F2); the only typed codes are enrolment codes (C26).
- Errors: `LOGIN_ATTEMPT_UNKNOWN` 404, `LOGIN_ATTEMPT_NOT_READY` 409, `LOGIN_ATTEMPT_ENDED` 410
  `{state}`, `INVALID_MFA` 401 (counts, B.5), `TOTP_UNAVAILABLE` 403 (none, throttled or
  compromised), `TOO_MANY_ATTEMPTS` 429 (per-account delay, also for unknown usernames),
  `CLIENT_OUTDATED` 400.

**QR and setup payloads** (versioned; an unknown version → "Update Kaiku"; `o` must equal the
configured app's pinned origin, B.1 "Origin pinning"; the origin is scheme, host and port):

- **Login** (F2, and F3's confirmation QR): `kaiku://login/v1?o=<percent-encoded origin>&t=<b64u
  token>`. Read only by the in-app scanner; the app registers no intent filter for it (a sign-in is a
  scan, never a link).
- **Setup** (one format for `own`, `admin`, `request`, `registration` and the CLI), link and QR:
  `{PUBLIC_URL}/setup#v=1&t=<b64u setup token>`. The token is in the fragment, so neither the server
  nor a link preview receives it. `/setup` is a static web-client page: on a phone it offers "Open in
  Kaiku" (`intent://setup/v1?o=<percent-encoded origin>&t=<token>#Intent;scheme=kaiku;end`, for the app's
  `kaiku://setup/v1` intent filter); on a computer it runs the C29 passkey claim. App Links: C29. Only
  an unconfigured app accepts a new origin, and it shows `server_name` from the open first.

**Admin setup (F5c; L1b).** `POST /api/admin/users/{user_id}/enrollments` from the admin's own key
device, full + `admin.enrollment.issue`/user_id per code: `{mode: "setup"|"recovery", claimant_check:
{method: "in_person"|"called_back", note: string (1..500)}, lost_device_ids: [uuid]}` → 201
`EnrollmentIssued`. Refusals: 403 `KEY_DEVICE_REQUIRED`; 400 `ENROLLMENT_TARGET_SELF`; 409
`ACCOUNT_REQUEST_OPEN`; 409 `NO_INDEPENDENT_CANCELLER` (existing account, SM3). Another admin's
account may be a target and waits like any existing account. At `done` a new account (B.0) is
active at once; an existing account gets an `admin` request (SM3), and the new phone's status shows
`authenticator_state: "pending"` with `ready_at`. Also L1b:
- `POST /api/admin/users` (C4/C23) drops `temporary_password` (sent → 400 `VALIDATION_ERROR`), gains
  `recovery_contact_id|null`, and creates a passwordless account (`password_change_required` false);
- `PUT /api/admin/users/{user_id}/may-approve {may_approve}` (+ `admin.may_approve`);
- `PUT /api/admin/users/{user_id}/factors {password?: "active"|"compromised", totp?:
  "active"|"compromised"}` (+ `admin.factor.set_state`; B.1 credential-state table);
- immediate actions on another admin's account → 403 `ADMIN_TARGET_IS_ADMIN` (F6 rev 4);
- admin user views gain `may_approve`, `key_devices`, `passkeys`, `password_state`, `totp_state`,
  `recovery_codes_left`, `bootstrap_open`, `recovery_contact`, `open_request` and `open_sessions:
  {full, probation}` (F8 "Admins see per user").

**Sessions and assurance (B.4, CC4, CC6; L1a).**

- `sessions.assurance` (`full` | `probation`) is fixed at creation and authoritative. The access JWT
  gains the claim `assurance` for clients (a token without it is probation); the server always
  decides from the committed row (B.4 rev 5).
- `AuthResponse` (every sign-in and `/auth/refresh`) gains `session_id`, `assurance`,
  `absolute_expires_at`. `GET /auth/me` gains `may_approve`, `is_system` (false) and `session: {id,
  assurance, auth_methods, auth_time, client_class, absolute_expires_at, key_device_id|null}`.
  `GET /auth/sessions` items gain `assurance`, `client_class`, `auth_methods`, `authenticator: {id,
  type, label}|null`, `last_used_at`, `absolute_expires_at` (a key device's session is shown as
  "Remove device").
- `auth_methods` by creation path: `["passkey"]`, `["device_key"]` → full; `["qr"]`,
  `["password","phone_confirm"]`, `["oidc","phone_confirm"]`, `["password","totp"]`,
  `["oidc","totp"]` → probation; `["legacy"]` = every session from before the upgrade → probation.
  Nothing changes a session's assurance; the probation banner's "Sign in with a passkey" (C29) makes a
  new full session.
- What each may do:

| Class | Routes |
|---|---|
| any session, probation included | chat, DMs, voice and calls, WebSocket; `GET /auth/me`, `POST /auth/me` (name), `/auth/me/avatar`; notification and preference settings; the own-account reads of C26–C29 (`/api/me/security`, `/api/me/security-events`, `/api/approval-devices`, `/auth/passkeys`, `/auth/sessions`, `/api/me/account-requests`); `POST /auth/logout`; owner rule part 2 (cancel, repudiate with a fresh factor) |
| full + step-up grant | every action of the step-up table (common rules) |
| full | the contact's confirmation (C28); `/api/admin/*` (with the elevation window where it applies today); creating bot, webhook or other long-lived tokens (contract call: they would outlive a relayed probation session) |
| unchanged | every other route keeps its existing permission checks and is open to probation (B.4's "cannot" list read as exhaustive) |

- Existing routes behind the guards: `POST /auth/me/password` `{new_password}` (`password.set`;
  blocklist → 400 `PASSWORD_REJECTED {reason: "length"|"breached"|"context"}`), `DELETE
  /auth/me/password`, `/auth/mfa/*`, `POST /auth/recovery-codes` → `{recovery_codes: [string]}`
  (replaces `/auth/mfa/backup-codes`; `GET /auth/recovery-codes/count` → `{left}`), `DELETE
  /auth/sessions[/{id}]`, identity link/unlink, account deletion, `POST /api/admin/elevate
  {reason|null}`. `GET /auth/reauth/methods` → `{passkey: bool, device: bool}`; C24 and its
  2026-10-25 amendment are superseded (K18). Removed (404): `/auth/qr/create`, `/auth/qr/redeem`,
  `/auth/forgot-password`, `/auth/reset-password`, `/auth/reauth/oidc/authorize/{provider_id}`.
- Refresh (B.4): rotates in place; the immediate predecessor presented within 10 s gets the same
  successor (once per session per 24 h); any other reuse ends the session → 401 `SESSION_REUSED`
  (notice); inactivity (7 d) or the absolute cap (browser 30 d, desktop and Android 90 d) → 401
  `SESSION_EXPIRED` (a key device then uses device-key sign-in); unknown → 401 `INVALID_TOKEN`; ended
  → 401 (C25). Clients serialise refreshes (Web Locks, one Rust task, one Android flight). At most 50
  live sessions per account (`MAX_SESSIONS_PER_USER`); the least recently used is ended with a
  notice. Ending is immediate in the B.4 "Revocation order"; sockets get C25's `error`
  `SESSION_REVOKED` and close 1008.

**Account requests (SM3, F6, F8; L1b).**

- **Start** `POST /auth/account-requests` (public; a session may be present for "Secure my account"
  but grants nothing):
  - recovery: `{username, purpose: "recovery", recovery_code, second: {type: "password", password} |
    {type: "totp", code} | {type: "oidc", oidc_code, verifier?}, lost: "none"|"all_key_devices"}`
  - bootstrap: `{username, purpose: "bootstrap", first: {type: "password", password} | {type: "oidc",
    oidc_code, verifier?}, totp_code}`
  - → 201 `{request: AccountRequest, request_secret?}` (native; web gets the cookie). Unknown
    username, invalid recovery code, a failed bootstrap factor or an exhausted start budget → 401
    `INVALID_CREDENTIALS` (same answer and timing; nothing counted per account before a valid code);
    a valid code with a failed second factor → 401 `REQUEST_FACTOR_INVALID {recovery_codes_left}` (the
    code is consumed, B.5); 409 `ACCOUNT_REQUEST_OPEN`; 409 `NO_INDEPENDENT_CANCELLER`.
  - `lost: "all_key_devices"` names every active key device lost (resolution: the requester has no
    session that could list devices; an admin names single devices, admin setup above).
  - `oidc_code`: `POST /auth/account-requests/oidc/authorize/{provider_id}` → `{url}` (IdP
    `prompt=login`, `max_age=0`; `auth_time` ≤ 300 s); a one-time code (60 s) comes back by web
    postMessage `oidc-request-callback {oidc_code}`, or by the C29 delivery (desktop) / the Android
    OIDC hand-back, then with `verifier`.
- **Request client** (request secret): `GET /auth/account-requests/current` → `AccountRequest`;
  `DELETE /auth/account-requests/current` → 204 (withdraw). While `ready`: one strong authenticator
  (a phone through `POST /auth/account-requests/current/enrollments`, the Android app's own key by a
  claim with the request secret, or the C29 request passkey routes), `PUT
  /auth/account-requests/current/password {password}`, `POST /auth/account-requests/current/totp` →
  `{secret, otpauth_uri}` then `POST …/totp/verify {code}`, `PUT
  /auth/account-requests/current/recovery-contact {username}`. `POST
  /auth/account-requests/current/finish` → 200 `{request, recovery_codes: [string]}` (shown once). No
  session is minted: the client then signs in with the new authenticator (full) or by F4.
- **Owner, contact, admins** (bearer): `GET /api/me/account-requests` → `{requests:
  [AccountRequestSummary]}` (open requests on the caller's account and those the caller may cancel as
  contact); `GET /api/admin/account-requests?state=open` (elevated); `POST
  /api/account-requests/{id}/cancel {reason|null, part2: {factor: {type: "totp"|"recovery_code",
  code}}|null}` → 200 `AccountRequestSummary`. Part 1, the contact and admins use the grant
  `request.cancel`/id; `part2` only while the account has no active strong authenticator (B.2). A
  cancel that loses against completion → 409 `ACCOUNT_REQUEST_COMPLETED` (the UI offers "Remove this
  device"); not a canceller → 403 `CANCEL_NOT_ALLOWED`.
- `AccountRequest = {id, kind: "self"|"admin", purpose:
  "recovery"|"bootstrap"|"admin_setup"|"admin_recovery", state:
  "waiting"|"ready"|"completed"|"cancelled"|"withdrawn"|"expired"|"void", created_at, ready_at,
  expires_at|null (Finish deadline), factors: ["recovery_code"|"password"|"totp"|"oidc"],
  lost_key_devices: int, staged: {authenticator: {type, label}|null, password: bool, totp: bool,
  recovery_contact: string|null}}`.
- `AccountRequestSummary = {id, subject: {user_id, username, display_name}, kind, purpose, state,
  created_at, ready_at, started_by: {user_id, username}|null (the admin), lost_devices: [{id,
  label}], new_authenticator: {type, label}|null, cancel: {via: "step_up"|"part2"}|null}`.
- WS hint `account_request_updated {request_id, subject_id, state}` to the subject, the recovery
  contact and all admins. Delay `ACCOUNT_REQUEST_DELAY_HOURS` (default 24, 24–72, Q1); 3 `self`
  starts per account per delay period (B.5).

**Registration tickets (F8; L1b).** `POST /auth/register` → 201 `{user_id, username,
ticket_expires_at, registration_ticket?}` (native; web: cookie), never a session. With the ticket:
`GET /auth/registration` → `{expires_at, bound: bool}`; the C26 issue route (a phone setup link, or
the Android app's own key by a claim with `X-Kaiku-Ticket`); C29 `/auth/registration/passkey/*`;
`POST /auth/registration/totp` → `{secret, otpauth_uri}` and `POST …/totp/verify {code}` →
`{recovery_codes}` (the account then signs in by F4 and has `bootstrap_open`). Nothing bound after
30 min → the account is deleted.

**Security overview and migration prompts (F8).** `GET /api/me/security` (any session) →
`{session: {assurance, auth_methods}, password: {state: "active"|"throttled"|"compromised"}|null,
totp: {state}|null, recovery_codes_left, key_devices: int, passkeys: int, may_approve,
bootstrap_open, recovery_contact: RecoveryContact|null (C28), open_request:
AccountRequestSummary|null, prompts: [string]}`. Prompts (clients ignore unknown ones):
`sign_in_with_passkey` (probation session, the account has a passkey), `add_passkey` (no passkey),
`set_up_phone` (no key device), `secure_my_account` (`bootstrap_open`), `name_recovery_contact` (none
or unconfirmed), `renew_totp` and `renew_recovery_codes` (after a replacement, or none left),
`open_request` (a `waiting` or `ready` request: full-screen at app open with Cancel). The Kaiku DM
repeats `secure_my_account` every 30 days (C28 `bootstrap_reminder`).

## C28 — Security events, the Kaiku DM and the recovery contact (L1a plumbing, L1b producers → L2 desktop/web, L2 Android)

**Event types** (`event_type`; `high` = Part C's high-severity list, also delivered to the confirmed
recovery contact and to all admins):

| Event types | Severity | `details` |
|---|---|---|
| `sign_in` | normal | `{auth_methods, assurance}` |
| `sign_in_not_finished`, `second_factor_failed`, `guessing` | normal | `{factor: "password"\|"oidc"\|"totp", count}` |
| `approval_reported` | normal | `{attempt_kind}` |
| `authenticator_added` | high for `key_device`/`passkey`, normal for `totp` | `{type}` |
| `authenticator_removed` | high | `{type, by: "self"\|"admin"\|"request"\|"cli"}` |
| `key_invalidated` | high | `{}` |
| `recovery_codes_created` | normal | `{}` |
| `password_set`, `password_changed`, `password_removed` | normal | `{}` |
| `password_throttled`, `password_compromised`, `totp_throttled`, `totp_compromised` | high | `{cause: "guessing"\|"report"\|"repudiation"\|"cancel"\|"admin"}` |
| `step_up_action` | normal | `{action, target_label}` |
| `repudiated` | high | `{event_id}` |
| `request_started`, `request_completed`, `request_cancelled`, `request_withdrawn` | high | `{request_id, purpose, cancelled_by?: {kind, user_id}}` |
| `request_ready_soon`, `request_ready`, `request_expired`, `request_void` | normal | `{request_id, purpose}` |
| `admin_setup` | high | `{mode: "setup"\|"recovery", admin_user_id, new_account}` |
| `admin_role_granted`, `admin_role_removed` | high | `{by_user_id}` |
| `recovery_contact_named`, `recovery_contact_confirmed`, `recovery_contact_changed` | normal (also to the contact) | `{contact_user_id\|null}` |
| `session_ended_reuse`, `session_ended_limit` | normal | `{session_id}` |
| `bootstrap_reminder` | normal | `{}` |

**`SecurityEvent`** = `{id, event_type, severity, created_at, subject_id, actor: {kind:
"self"|"admin"|"recovery_contact"|"system"|"cli", user_id|null, username|null}, client: {class,
label}|null, place|null, ip|null, session_id|null, authenticator: {id, type, label}|null,
request_id|null, details, actions: [Action], repudiated_at|null}`. `ip` and `place` appear only in
the owner's own log and the admin log, never in a contact's copy (Part C). `Action = {action:
"repudiate"|"cancel_request"|"remove_device"|"confirm_contact", via: "step_up"|"part2"|null,
warning: "voids_own_request"|"voids_own_bootstrap"|null}`, computed for the reader on every read and
listed only where usable (Part C); `warning` is the B.6.6 (rev 5) warning the screen must show
first.

**Security log.** `GET /api/me/security-events?before=<event id>&limit=<1..100, default 50>` (any
session) → `CursorPaginatedResponse<SecurityEvent>` (`{items, has_more, next_cursor}`), newest
first. Admins: `GET /api/admin/security-events?user_id=&before=&limit=` (elevated). No delete route.
"That wasn't me": `POST /api/me/security-events/{id}/repudiate {part2: {factor: {type:
"totp"|"recovery_code", code}}|null}` (part 1: full + `event.repudiate`/id; part 2 per B.2) → 200
`{event, ended_session_ids, revoked_authenticator_ids, compromised: ["password"|"totp"],
voided_request_ids}`; refused → 403 `REPUDIATE_NOT_ALLOWED`; already done → 409
`EVENT_ALREADY_REPUDIATED`.

**The "Kaiku" system account (L1a).** One user created by the migration: id
`6b61696b-7500-4000-8000-000000000001`, username `kaiku` (reserved, case-insensitive), display name
"Kaiku", no avatar (clients use a built-in icon). Clients recognise it **only** by `is_system: true`,
a new field on `AuthorProfile`, `DMParticipant`, `UserSummary` and the user profile views (false for
everyone else), never by id or name. Every auth path refuses it with that path's normal unknown-user
answer; it is hidden from user lists, search, presence, mentions, friend and bot paths.

- **Kaiku DM:** one DM per user with the system account, created on first notice. `DMListResponse`
  gains `read_only: bool` (true only here). Sending, editing, deleting, reacting, pinning, typing,
  leaving, hiding, muting or archiving → 403 `CHANNEL_READ_ONLY`.
- **Notices** are messages with `message_type: "security"` (new value; an older client shows
  `content`) and the new `MessageResponse` field `security_notice: SecurityNotice|null`; `content` is
  an English fallback only (C16). `SecurityNotice = {notice_id, subject: {user_id, username,
  display_name}, event_type, severity, count, first_at, last_at, events: [SecurityEvent] (newest
  first, ≤ 50), events_truncated: bool}`. Recipients: the subject's own DM; for `high`, also the DM of
  the confirmed recovery contact and of every admin other than the subject (each a separate notice
  with `subject` ≠ reader).
- **Coalescing (B.5):** per (recipient, subject, `event_type`), events within 10 min of the notice's
  first event update that notice (`count`, `events`, `last_at`, `edited_at`) instead of creating
  one; WS `security_notice_updated {channel_id, message_id, security_notice}` to the recipient;
  `security_events` rows stay individual. Clients load the Kaiku DM's latest page on every open and
  reconnect (not only `after` catch-up, C1), since coalescing edits older messages.
- **Push `t=security`** (instead of `t=dm` for Kaiku DM messages). FCM data (strings):
  `{"t":"security","notice_id":"<uuid>","event_type":"<type>","severity":"high"|"normal",
  "count":"<n>","subject_id":"<uuid>","actor_id":"<uuid>"}` (`actor_id` only when an admin or contact
  acted); UnifiedPush: the same JSON with `count` as a number. Always HIGH priority, collapse key
  `security-<notice_id>`, TTL `86400s`; not gated by the C20 coalescer or notification settings;
  `high` also ignores quiet hours. One push when a notice is created, then at most one per notice per
  60 s while it is updated. Sent to every push subscription of the recipient: lapsed key devices,
  devices named lost, and once to a device that "Remove device now" removed, before its subscription
  is deleted (C19 amendment 3 in C26). No content beyond these fields; the app renders text itself.
- **Text keys** (C15): web `security.notice.<event_type>` and `security.push.<event_type>`, Android
  `security_notice_<event_type>` and `security_push_<event_type>`, with placeholders `{subject}`,
  `{actor}`, `{device}`, `{count}` filled from the payload and the local user cache; an unknown
  `event_type` uses `security.notice.generic` / `security_push_generic`.

**Recovery contact (Part C, SM3 rev 5; L1b).** `RecoveryContact = {user_id, username, display_name,
named_at, confirmed_at|null, counts_from|null}`; `counts_from` = `confirmed_at` + the request delay:
a request that starts earlier does not record the contact as a canceller (the contact is still
notified and may cancel).

- `PUT /api/me/recovery-contact {user_id}` (full + `recovery_contact.set`/user_id) → 200
  `RecoveryContact`; an admin, the account itself or the system account → 400
  `RECOVERY_CONTACT_INVALID`. `DELETE /api/me/recovery-contact` (`recovery_contact.set`/`""`) → 204.
- `PUT /api/admin/users/{user_id}/recovery-contact {user_id}` (+ `admin.recovery_contact.set`;
  non-admin accounts only).
- Contact side: `GET /api/me/recovery-contact-for` → `{accounts: [{user_id, username, display_name,
  named_at, confirmed_at|null}]}`; `POST /api/me/recovery-contact-for/{user_id}/confirm` (full, no
  step-up; "I will help <name>") → 204. A new contact needs a new confirmation.
- The contact cancels a request through C27's cancel route (full + `request.cancel`).

## C29 — Passkeys (LP → LP clients on web, desktop, Android; L1a/L1b hooks)

**RP and origins (B.1, CC5).** RP ID = host of `PUBLIC_URL` (lowercase, punycode, no port; never a
parent domain); `PUBLIC_URL` is mandatory (https, or http only for `localhost`; no IP literal,
path, query, fragment or userinfo), and the RP ID is pinned at startup (LP DC5,
`PASSKEY_ALLOW_RP_ID_CHANGE`). Allowed origins, compared as exact strings, Android origins first:
the web origin, and `android:apk-key-hash:<b64u SHA-256 of the signing certificate>` for every
configured certificate. Never `tauri://localhost` or `tauri.localhost`, and no related-origins file
(§5.11). Config (C9): `PASSKEY_ANDROID_APP_IDS` (default `io.wolftown.kaiku,io.wolftown.kaiku.simple`;
`none` disables), `PASSKEY_ANDROID_CERT_SHA256` (comma list; colon hex as from `apksigner` or
`keytool`; default the release certificate). Both encodings of one certificate:

| Certificate SHA-256 | `assetlinks.json` | `clientDataJSON` origin |
|---|---|---|
| bytes `00 01 … 1F` | `00:01:02:…:1E:1F` | `android:apk-key-hash:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8` |
| SHA-256(`"kaiku-test-cert"`) | `18:6F:F0:75:BF:FB:D4:76:5E:1C:5C:D9:E1:22:C4:09:44:58:A3:58:BC:E5:34:F5:3E:49:D7:B2:C3:81:AB:C0` | `android:apk-key-hash:GG_wdb_71HZeHFzZ4SLECURYo1i85TT1PknXssOBq8A` |

A padded hash (`=`), another hash or upper/lower-case variants of the base64url form never match.

**`GET /.well-known/assetlinks.json`** (public, `Content-Type: application/json`): one statement per
app id, `{"relation": ["delegate_permission/common.get_login_creds",
"delegate_permission/common.handle_all_urls"], "target": {"namespace": "android_app",
"package_name": "<app id>", "sha256_cert_fingerprints": ["<every configured certificate, upper-case
colon hex>"]}}`; no Android config → `[]`. The reverse proxy routes `/.well-known/` to the server.
Asset links cannot name paths: the setup path is declared by the app. **App Links for setup links
(C27):** the manifest declares an `autoVerify` filter for `https://<host>/setup` only for hosts in
the Gradle property `kaikuAppLinkHosts` (comma list, default empty; the family build sets its host),
because an APK cannot know a self-hosted domain; every other server uses the `/setup` page's "Open in
Kaiku" (C27).

**WebAuthn JSON.** `options` = `{"publicKey": …}` in the WebAuthn L3 JSON form (b64u); `credential`
= `PublicKeyCredential.toJSON()`. Android passes exactly these strings to and from Credential
Manager. Registration: `residentKey: "required"`, `userVerification: "required"`, `attestation:
"none"`, `excludeCredentials`, `credProps`; challenges ≥ 16 bytes, single use, bound to the ceremony,
the session (or request, ticket or enrolment) and the account. Sign-in: `allowCredentials: []`, UV
required (UV=0 refused), BE must equal the stored value, the counter is a signal only.

| Route | Auth | Body → answer |
|---|---|---|
| `POST /auth/passkeys/login/start` | public | – → `{ceremony_id, options}` |
| `POST /auth/passkeys/login/finish` | public | `{ceremony_id, credential, username\|null}` → 200 `AuthResponse` (`assurance: "full"`, `auth_methods: ["passkey"]`; browser: refresh cookie; native: body) |
| `POST /auth/passkeys/step-up/start` | full | `{action, target}` → `{ceremony_id, options}` (`allowCredentials` = the caller's active passkeys) |
| `POST /auth/passkeys/step-up/finish` | full, same `sid` | `{ceremony_id, credential}` → 200 `StepUpGrant` (`via: "passkey"`) |
| `POST /auth/passkeys/register/start` | full + `passkey.add` | `{label\|null}` → `{ceremony_id, options}` |
| `POST /auth/passkeys/register/finish` | full, same `sid` | `{ceremony_id, credential}` → 201 `Passkey` (`active`; the grant is consumed here) |
| `POST /auth/account-requests/current/passkey/{start,finish}` | request secret, request `ready` | as register → 201 `Passkey` (`pending`; "Finish" activates it) |
| `POST /auth/registration/passkey/{start,finish}` | ticket | as register → 201 `Passkey` (`active`) |
| `POST /auth/enrollments/passkey/start` | public (setup link in a browser) | `{setup_token}` → `{ceremony_id, options, server_name, subject: {username, display_name}}` |
| `POST /auth/enrollments/passkey/finish` | public | `{setup_token, ceremony_id, credential, label\|null}` → 201 `{enrollment_id, state: "claimed", code, code_expires_at}` + the C26 claim-secret cookie; the browser shows the code, the issuer confirms (C26), the row is created at `done` (`active` or `pending`, SM2) |
| `GET /auth/passkeys` | any session | → `{rp_id, user_handle, passkeys: [Passkey]}` |
| `PATCH /auth/passkeys/{id}` | any session | `{label}` → `Passkey` |
| `DELETE /auth/passkeys/{id}` | full + `passkey.remove`/id | → 200 `{rp_id, user_handle, credential_id, remaining_credential_ids}` (for `signalUnknownCredential` and `signalAllAcceptedCredentials`) |

`Passkey = {id, label, state: "pending"|"active"|"revoked", created_at, last_used_at|null,
ready_at|null, backup_eligible, backup_state, transports, aaguid}`. Errors: `PASSKEY_CEREMONY_INVALID`
400, `PASSKEY_INVALID` 400 (origin, RP ID, UV, BE, signature), `PASSKEY_UNKNOWN` 400 (the client
signals the credential unknown and says it was removed), `PASSKEY_PENDING` 409 `{ready_at}` (never
signalled unknown), `PASSKEY_USER_MISMATCH` 400, `PASSKEY_NOT_DISCOVERABLE` 400,
`PASSKEY_ALREADY_REGISTERED` 409, `PASSKEY_LIMIT` 409 (10 active), `LAST_STRONG_AUTHENTICATOR` 409,
`PASSKEY_HANDOFF_INVALID` 400, `PASSKEYS_UNAVAILABLE` 503, `PASSKEY_CANCELLED` (loopback `error`
only), `FULL_SESSION_REQUIRED` / `STEP_UP_REQUIRED` 403, `CLIENT_OUTDATED` 400.

**Desktop: system browser and loopback (F1 rev 4).** Desktop purposes are `sign_in` and `step_up`
only; registration never runs from the desktop app (B.1 rev 3).

1. The app (Rust) binds `127.0.0.1:0` (the literal address, never `localhost`) **before** anything
   else and keeps port P; it draws `callback_path` (32 random bytes, b64u, 43 chars) and a verifier.
2. `POST /auth/passkeys/desktop/flows` with `X-Kaiku-Client: desktop` (bearer only for `step_up`):
   `{purpose: "sign_in"|"step_up", action|null, target|null, username|null, loopback_port: P,
   callback_path, client_challenge}` → 201 `{flow_id, url, expires_at}`, `url` =
   `{PUBLIC_URL}/auth/passkeys/desktop/{flow_id}`; flow ≤ 10 min (Valkey). No header → 400
   `CLIENT_OUTDATED`; `step_up` validates like `step-up/start`.
3. The app opens `url` in the system browser or the browser chosen in "Browser for passkey sign-in".
   The page names the purpose and the app and says "If someone sent you this link, close this page";
   it uses `POST …/{flow_id}/options` → `{ceremony: "get", options}`, `POST …/{flow_id}/finish`
   (form field `credential`) and `POST …/{flow_id}/cancel`. For `step_up`, an assertion by another
   account's passkey ends with `PASSKEY_USER_MISMATCH` and no result. For `sign_in` with a
   `username` (the name typed in the app, compared case-insensitively as in `login/finish`), an
   assertion by another account's passkey ends the same way; `username` is ignored for `step_up`.
4. `finish` and `cancel` answer 200 with the **hand-off page**: `Cache-Control: no-store`,
   `Referrer-Policy: no-referrer`, CSP `default-src 'none'; script-src 'sha256-<hash>'; form-action
   http://127.0.0.1:P/callback/<callback_path>; base-uri 'none'; frame-ancestors 'none'`, no
   third-party content; one form `POST` (`application/x-www-form-urlencoded`) to that URL with the
   hidden fields `flow_id` and either `code` (success: a fresh 256-bit one-time code, b64u, stored
   hashed as the flow's pending result for 60 s) or `error` (failure code; `PASSKEY_CANCELLED`),
   submitted by the hashed script, with a "Continue" button without script. The code is never
   rendered, logged, or put in an address bar, redirect or any API or polling response.
5. The listener answers exactly `POST /callback/<callback_path>` with `Host: 127.0.0.1:P` and a form
   body naming its `flow_id` with 200 `text/html` "You can close this page" (`no-store`, never a
   redirect), then stops. Any other path, method, Host, `flow_id` or body → 204, empty, nothing
   echoed, logged or shown, and it keeps listening until the flow ends or 10 min pass.
   `callback_path` is compared in constant time.
6. `POST /auth/passkeys/desktop/redeem {flow_id, code, verifier}` (`X-Kaiku-Client: desktop`; for
   `step_up` with the starting session's bearer) → `sign_in`: 200 `AuthResponse` with
   `refresh_token`, `assurance: "full"`, plus `purpose` and `user_id`; `step_up`: 200 `{purpose:
   "step_up", user_id, grant: StepUpGrant}`. Only now does the server create the session or grant,
   after re-checking the account, the passkey and (step-up) that session live and full. Any failure,
   including flow id + verifier without the code, → 400 `PASSKEY_HANDOFF_INVALID`, and every attempt
   spends the code. The app checks that `purpose` and (step-up) `user_id` are what it started; tokens
   stay in Rust.
7. **Spike fallback (F1):** if the form POST meets an insecure-form warning or Local Network Access
   blocks it, the hashed script sends the same form body by `fetch()` POST; the CSP then names the URL
   in `connect-src` instead of `form-action`, and the listener answers the preflight with
   `Access-Control-Allow-Origin: <PUBLIC_URL origin>`, `Access-Control-Allow-Methods: POST`,
   `Access-Control-Allow-Private-Network: true` and the POST as in step 5 plus that Allow-Origin.
   Fields and rules are unchanged; LP records which delivery ships.
8. **OIDC desktop hand-off (L1b):** desktop starts of `/auth/oidc/authorize/{provider}` (and of the
   C27 request OIDC factor) send `loopback_port`, `callback_path` and `client_challenge` instead of a
   loopback `redirect_uri`; the callback answers with the same hand-off page (`flow_id` = the OIDC
   flow id); the 307 `?code=` hand-back is removed. Redemption: C27.

**Android (Credential Manager).** `CreatePublicKeyCredentialRequest` / `GetPublicKeyCredentialOption`
with the `options` JSON above; requests carry `X-Kaiku-Client: android`; no Custom Tab fallback in
v1.0. Without a reachable `assetlinks.json` the app has no passkeys and is itself the key (B.1).

**Re-auth and step-up.** A passkey step-up (web page, the desktop flow, or Credential Manager) and a
C26 `device_stepup` are the only step-ups; `GET /auth/reauth/methods` → `{passkey: bool, device:
bool}` (C27). A passkey sign-in is never a step-up.

**Deltas from the LP plan's "C29 proposal"** (LP updates its plan before Task 5): the desktop result
travels only in the loopback form POST (no 303 `Location` with `code`), with one start
(`desktop/flows`, `loopback_port` + `callback_path` instead of `redirect_uri`) and one redemption
(`desktop/redeem`) replacing `desktop/login|step-up|register`, `exchange` and `exchange-session`;
no desktop registration; registration needs a full session and the grant `passkey.add` (first
passkeys come from a key device's full session, requests, tickets and enrolments; LP's interim
`reauth` body and DC1 gate may exist only until L1a merges, spec "Sequencing"); passkey state `pending` (not `pending_activation`); `LAST_STRONG_AUTHENTICATOR`
replaces `PASSKEY_LAST_STRONG_AUTHENTICATOR`, `PASSKEY_PENDING` is new, `PASSKEY_REAUTH_UNAVAILABLE`
and `REAUTH_PASSKEY_REQUIRED` are dropped; step-up needs a full session and returns `StepUpGrant`;
`POST /api/admin/elevate` takes only `{reason|null}` (same interim rule); the rev-2 hook points for "upgrading" a
probation session and owner evidence are void (CC4).

**C29 amendment (2026-11-03, LP review round 1, all three reviewers):** the desktop `sign_in` flow
takes an optional `username|null` (step 2) and refuses another account's passkey with
`PASSKEY_USER_MISMATCH` and no result (step 3), as web and Android already do, so a shared PC does
not sign in someone else by mistake. Additive; absent means no check.
