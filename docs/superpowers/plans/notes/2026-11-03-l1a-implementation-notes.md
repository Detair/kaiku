# L1a implementation notes (v1.0 login, server)

Plan: `docs/superpowers/plans/2026-11-03-v1.0-login-server.md` (Tasks 1–14, Task 30 gates).
Spec rev 5 and contracts C26–C28 win over the plan; every deviation and judgment call
is listed here, per task. "Coordinator" marks items that need a decision.

## Task 1 — primitives and `PUBLIC_URL`

- `Config::auth_hmac_key` is a newtype `AuthHmacKey([u8; 32])` with a redacting `Debug`
  (the plan says a bare `[u8; 32]`). `Config` derives `Debug`; a bare array would print the
  key. `AuthCtx::hmac_key()` still returns `&[u8; 32]`.
- DC1 derivation input: the configured `JWT_PRIVATE_KEY` value (the base64 PEM string) is
  the HKDF input key material. Rotating the JWT key therefore rotates the derived key, as DC1
  states.
- An empty `AUTH_HMAC_KEY=` (compose templates) counts as unset, like every other optional
  variable (`env_opt`).

## Task 3 — system account, read-only DM, security events

- **Migration renames a pre-existing `kaiku` account (coordinator).** C28 reserves the
  username `kaiku`; an existing server may already have a user with that name, and the plain
  `INSERT` would fail the migration and keep the server from starting. The migration renames
  such an account to `kaiku_<first 8 hex of its id>` (id, data and sessions unchanged; the
  owner signs in with the new name). Alternative if the coordinator prefers: fail the
  migration with a clear message and let the operator rename the account by hand.
- Extra DB guards beyond the plan: a partial unique index allows only one `is_system` row and
  a CHECK pins it to the fixed id; `security_events` has a CHECK that `authenticator_type` and
  `authenticator_id` are both set or both null; the immutability trigger also refuses
  re-setting `delivered_at` / `repudiated_at` once set (only NULL → time), and allows the
  `actor_user_id ON DELETE SET NULL` action.
- `DMListResponse.read_only` is derived from the participants (a DM with the system account),
  while `ensure_writable` reads `channels.read_only`. Both hold by construction: only
  `kaiku_dm_tx` creates DMs with the system account, and it sets `read_only`.
- Hide/mute/archive: the server has no per-channel hide, mute or archive routes (they live in
  the client preference blob), so there is nothing to guard server-side. Guarded routes: send
  (incl. upload-with-message), edit, delete, react / unreact, pin / unpin (before the
  guild-only check), typing (WS; drops the hint and answers `CHANNEL_READ_ONLY`), leave,
  rename, icon upload, and `PATCH`/`DELETE /api/channels/{id}` (a DM participant passes the
  channel permission check there).
- Presence: the system account has no friends, guilds or sessions, so no presence audience
  ever includes it; no query change was needed. Mentions: `chat::mentions::resolve_usernames`
  excludes it. Bots: `add_bot_to_guild` already requires `is_bot`. OIDC: identity resolution
  joins `users` and excludes it. Admin: list, count, search, export, details and
  `user_exists` (ban, delete, …) exclude it.
- `UserSummary.is_system` (admin list) is always false because the system account is never
  listed; `UserProfile.is_system` (`GET /auth/me`) is false for every signed-in user.
