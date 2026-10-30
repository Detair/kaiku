# v1.1: Admin MCP and Agent Bots — Design

**Status:** draft, 2026-10-30 — open questions answered by the maintainer the same day
**Target:** v1.1, after the v1.0 family release
**Scope:** two features that share one foundation (agent tokens)

1. **Admin MCP:** a Model Context Protocol endpoint that exposes *every* administrative
   capability of a Kaiku server, so the operator can run the whole server from an AI
   assistant (Claude Code, Claude Desktop, any MCP client).
2. **Agent bots:** an AI agent (for example a Hermes agent) can live in chats as a bot and
   answer @mentions and slash commands.

Decisions marked **(maintainer)** came from the maintainer; decisions marked **(coordinator
call)** are the author's and are open to change during review.

---

## 1. Why

The maintainer does ~90 % of their work through an AI assistant. A Kaiku server they can
only administer through the web/desktop admin panel is a server they will not administer
well. The admin panel and the MCP must be **two equal front-ends to the same API**, not a
panel with a few convenience tools bolted on. **(maintainer)**

Agent bots follow from the same habit: an agent the family can ask questions in a channel
("@hermes when is the next match?") is more useful than one that only runs in a terminal.

## 2. Goals, non-goals, anti-goals

**Goals**
- G1 — **Full admin parity (maintainer):** every admin action available in the UI or the
  REST API is available as an MCP tool, with the same permission checks and audit trail.
- G2 — Parity stays true over time: a new admin route without an MCP tool fails CI.
- G3 — Destructive actions keep a human in the loop, equivalent to today's elevation (C24).
- G4 — Agents get their own revocable, scoped credentials; nothing runs on a user's
  password or a copied session token.
- G5 — An agent bot can be addressed in a channel (mention, reply, slash command) and
  answer in place, including long-running answers.

**Non-goals**
- Infrastructure operations (deploy, backups, restore, DNS, firewall). The Kaiku server does
  not control its host; those stay in the operator's infra tooling.
- A general "chat with your messages" assistant over all history. Agent bots see what a bot
  is allowed to see, nothing more.
- Federating agents across servers.

**Anti-goals (this would be a failure even if it works)**
- An agent that can ban/delete without the operator noticing.
- A chat bot that can be talked into admin actions by anyone in the channel
  (prompt injection reaching admin tools).
- A bot that silently reads channels its members believe are private.
- Parity that drifts: tools that call different code paths than the UI and therefore
  enforce different rules.

## 3. Current state (v1.0, verified on main 2026-10-30)

- System-admin API: `/api/admin/*` behind `require_auth` + `require_system_admin`
  (`server/src/admin/mod.rs`). Mutating routes additionally need `require_elevated`
  (elevation: 15 min, bound to the session `sid`, password + TOTP or fresh OIDC login, C24).
- Guild management lives in `/api/guilds/*`, `/api/channels/*` and feature routers, gated by
  guild permission bits (`server/src/permissions/guild.rs`) or by guild ownership.
- Tokens: user JWT (access 15 min / refresh 7 d) and bot tokens (`Bot <id>.<secret>`,
  Argon2id-hashed, **only** accepted on the bot gateway). There is no API-key / personal
  access token concept.
- Bot gateway `GET /api/gateway/bot`: events `CommandInvoked`, `ComponentInvoked`,
  `MessageCreated` (intent `messages`), `GuildJoined/Left`, `MemberJoined/Left`. Bot actions
  over the socket: `message_create {channel_id, content}` and
  `command_response {interaction_id, content, ephemeral}` (one response per interaction).
  Rate limit 120/60 s per bot.
- Audit log: `system_audit_log`, covering elevation, announcements, user and guild admin
  actions, content filters. Not covered: OIDC provider and auth-settings changes,
  page limits, report resolution, most guild actions.
- No MCP code exists.

### 3.1 Gaps that block full parity (G1)

Parity is "every admin action", so admin actions that today exist **only** in the database or
as unused permission bits must first get an API (they are useful for the UI too):

| Gap | Today | v1.1 |
|---|---|---|
| Grant / revoke system admin | only the first-user setup insert | `POST/DELETE /api/admin/system-admins/{user_id}` (elevated, destructive tier) |
| Server name, terms URL, privacy URL | seeded, no write route | extend `PUT /api/admin/auth-settings` → `PUT /api/admin/server-settings` |
| Optional features (forum, announcements, events, screening) | env `KAIKU_OPTIONAL_FEATURES` only | runtime setting in `server_config`, env stays as the operator's upper bound |
| Guild ban / unban | `BAN_MEMBERS` bit unused, no route | `PUT/DELETE /api/guilds/{id}/bans/{user_id}` + list |
| Member timeout | `TIMEOUT_MEMBERS` unused | `PUT/DELETE /api/guilds/{id}/members/{user_id}/timeout` |
| Transfer ownership | `TRANSFER_OWNERSHIP` unused | `POST /api/guilds/{id}/transfer-ownership` |
| Kick / invites | owner-only, permission bits ignored | honour `KICK_MEMBERS`, `CREATE_INVITE`, `MANAGE_INVITES` |
| Guild audit view | `VIEW_AUDIT_LOG` unused | `GET /api/guilds/{id}/audit-log` |
| Audit coverage | partial | every admin/guild-management mutation writes an audit entry |

These are ordinary API features with their own tests and UI; the MCP picks them up
automatically (section 4.2).

## 4. Part 1 — Admin MCP

### 4.1 Shape (coordinator call)

- The MCP endpoint is **built into the server**: `POST/GET /mcp` (MCP "Streamable HTTP"
  transport), same host as the API. No sidecar to deploy or version separately.
- Local stdio clients use a tiny shim (`kaiku-mcp`, part of the CLI/desktop bundle) that
  forwards stdio ↔ `/mcp` with the agent token. Claude Code can also connect to the HTTP
  endpoint directly.
- Rust implementation with the official MCP Rust SDK (`rmcp`, license to be checked against
  the allow-list before adoption) or a minimal hand-written JSON-RPC layer if the SDK does not
  fit axum cleanly — builder's choice after a spike.
- Behind a reverse proxy / MCP gateway the endpoint is just another path; no special
  routing.

### 4.2 Parity by construction (coordinator call)

An MCP tool **does not re-implement** an admin action. Each tool call is turned into an
internal request to the **same axum router** (`tower::ServiceExt::oneshot`) with the agent's
credential attached. Consequences:

- identical validation, permission checks, elevation checks, rate limits and audit logging;
- a bug fixed in a handler is fixed for the MCP too;
- tool definitions can be **derived from the OpenAPI spec** (utoipa), which already
  describes path, parameters and request schemas.

Mechanics:
- Admin-relevant operations carry an OpenAPI extension `x-mcp` with
  `{ tool, tier, summary }` (tier: `read` | `write` | `elevated` | `destructive`, section 4.4).
- At startup the server builds the tool list from the spec: tool name from `x-mcp.tool`
  (for example `admin_users_ban`), input schema = path params + query + JSON body schema,
  description = `summary` + the operation's doc comment.
- **CI guard (G2):** a test walks the OpenAPI spec; every operation under `/api/admin/*` and
  every operation guarded by a management permission bit (MANAGE_*, KICK/BAN/TIMEOUT,
  VIEW_AUDIT_LOG, TRANSFER_OWNERSHIP) must have `x-mcp` or an explicit
  `x-mcp: { exclude: "<reason>" }`. Missing → test fails. The same test checks that every
  `x-mcp.tool` name is unique and every tier is set.
- A handful of **composite tools** exist where one API call is not the natural unit
  (for example `server_overview`: stats + diagnostics + pending reports + recent audit
  entries in one call). Composites only call other tools' routes.

### 4.3 Agent tokens (shared foundation)

A new credential type, also used by agent bots (section 5):

- **Owned by a user**, created by that user in User Settings → Security → *Agent tokens*
  (and by the MCP itself only with elevation, never silently).
- Format `kaiku_at_<id>_<secret>`, secret Argon2id-hashed like bot tokens, shown once.
- Fields: name ("Claude Code on my PC"), scopes, optional expiry (default 90 days, maximum
  1 year), created/last-used timestamps, last-used IP.
- **Scopes (coordinator call):** `admin:read`, `admin:write`, `admin:elevated`,
  `guild:manage` (acts with the owner's guild permissions), `bot:<application_id>`
  (section 5). A token never exceeds its owner's rights; scopes only narrow them.
- Accepted by `require_auth` as `Authorization: Bearer kaiku_at_…`; it produces an
  `AuthUser` with a synthetic session id (`sid = token id`) so revocation, banned-user checks
  and the session denylist (C25) apply unchanged. Banning or deleting the owner revokes
  their tokens.
- Revocable instantly from the UI, the admin panel (system admins can revoke anyone's) and
  via an MCP tool.
- Every audit entry made through an agent token records `via: "agent_token"`, the token id
  and the MCP client name (from the MCP `initialize` handshake).

### 4.4 Tiers and the human in the loop (G3)

| Tier | Examples | Behaviour |
|---|---|---|
| `read` | list users/guilds, stats, diagnostics, audit log, reports, observability | runs directly |
| `write` | create channel/category/role, edit settings, manage invites, emojis, webhooks, filters | runs directly (same as an admin clicking in the UI) |
| `elevated` | create account, auth settings, OIDC providers, page limits, announcements, guild suspend | needs an **elevation window** for this token |
| `destructive` | ban/unban, bulk ban, delete user/guild, revoke/grant system admin, transfer ownership, delete OIDC provider | needs a **per-call confirmation**, even inside an elevation window |

**Elevation for agent tokens (maintainer: 10 min, TOTP only):** the token cannot elevate with a stored
secret. The MCP asks the human for a **current TOTP code**:

1. The tool call returns a structured "elevation required" result, or, if the client supports
   MCP *elicitation*, the server sends an elicitation request "Enter your authenticator code
   to unlock admin actions for 10 minutes".
2. The code is verified with the existing C24 re-auth path (TOTP replay protection included).
   The resulting elevation is bound to the **token id** for 10 minutes and shown in the admin
   panel next to session elevations.
3. Admin accounts without TOTP cannot use `elevated`/`destructive` tools through the MCP
   (the MCP tells them to enable MFA). This deliberately makes TOTP a prerequisite for
   AI-driven administration.

**Per-call confirmation for `destructive` (coordinator call):** a two-step pattern that works
in every MCP client, with elicitation as nicer UX when available:

1. The tool call returns `confirmation_required` with a human-readable summary
   ("Ban user *max* (id …) from the whole server and end all their sessions") and a
   short-lived `confirmation_id` (2 min, single use, bound to token + exact arguments).
2. The agent calls `confirm_action { confirmation_id, totp }`; the server re-verifies the
   TOTP and executes exactly the stored request.

This keeps the property the maintainer relies on today: nothing irreversible happens without
them typing a fresh code.

### 4.5 Untrusted content

Some admin tools return text written by other users (report contents, usernames, guild
names, message excerpts in reports). Tool results mark these fields as untrusted
(`"untrusted": true` per field, plus a fixed note in the tool description) so the agent
treats them as data. Combined with 4.4, a malicious report text cannot cause a ban without
the operator's TOTP.

### 4.6 Resources and prompts

- MCP *resources* for read-mostly context: `kaiku://server/overview`,
  `kaiku://admin/audit-log/recent`, `kaiku://docs/admin-guide/<page>` (bundled admin guide
  pages, so the agent can answer "how do I…" from the shipped docs).
- MCP *prompts* for frequent workflows: "onboard a family member" (create account with a
  temporary password, add to guilds), "weekly health check".

## 5. Part 2 — Agent bots (Hermes and others)

### 5.1 The constraint

MCP is request/response from the agent's side: an MCP server cannot start an agent turn when
someone writes `@hermes` in a channel. Something must turn chat events into agent turns.

### 5.2 Two integration paths (coordinator call: support both, recommend A)

**A — Event-driven via a platform adapter (recommended).** The agent runtime connects to the
Kaiku bot gateway as a bot and turns each relevant event into an agent turn, the same way
agent frameworks handle Telegram/Discord/Slack. For Hermes this means a Kaiku adapter for its
messaging gateway. **To verify in the spike:** Hermes' adapter interface and whether it can be
an out-of-tree plugin; if Hermes only supports built-in platforms, path A uses a small
generic bridge service instead (bot gateway ↔ the agent's HTTP/API entry point).

**B — Agent-driven via a Bot MCP (fallback for agents without adapters).** The `/mcp` endpoint
offers bot tools when called with a `bot:<application_id>` token:
`bot_wait_for_events { timeout_s, types }` (long-poll, max 60 s, server buffers up to N events
per bot), `bot_send_message`, `bot_reply`, `bot_respond_to_command`, `bot_update_response`,
`bot_set_typing`, `bot_get_context { channel_id, around, limit }`. The agent runs a loop. Works
everywhere, costs more and answers slower.

Both paths use the same server-side additions (5.3) and the same bot identity.

### 5.3 Server additions for agent bots

- **Intent `mentions`:** deliver only messages that @mention the bot, reply to a bot message,
  or are in a thread the bot started. Default for agent bots; the existing `messages` intent
  (everything) stays for classic bots.
- **Respect channel visibility (must-fix, also for v1.0 bots):** `MessageCreated` today goes to
  every bot with the `messages` intent installed in the guild. Delivery must be filtered by the
  bot's effective `VIEW_CHANNEL` in that channel (role + overrides), exactly like a member.
  Private channels stay private unless the bot is explicitly allowed in.
- **Richer bot actions:** `message_create` gains `reply_to` and `thread_id`; new
  `message_edit` (own messages), `typing_start`, `reaction_add`; `command_response` gains
  a *deferred* mode ("Hermes is thinking…") plus one or more follow-ups/edits within 15 min,
  because agent answers take longer than the current single immediate response.
- **DMs to bots:** a user can DM an agent bot; gated by a per-bot setting (default off).
- **REST for bots:** the same actions over REST with `Authorization: Bot …` for runtimes that
  cannot hold a WebSocket. Rate limits per bot as today.
- **Visible identity:** bots show a *Bot* badge (exists), agent bots additionally an *AI*
  badge and the owner's name; a channel shows which bots can read it.

### 5.4 Safety rules for agent bots

- An agent bot token never carries `admin:*` scopes. The admin MCP and chat bots are
  **separate identities with separate tokens**; the server refuses a token that combines
  `bot:*` with `admin:*`. (Anti-goal: prompt injection reaching admin tools.)
- Everything a bot receives from chat is untrusted input.
- Bots are installed per guild by someone with `MANAGE_GUILD` and see only the channels they
  are allowed in (5.3).
- E2EE is paused in 1.0; when it returns, bots never receive encrypted content (already the
  rule in the dispatcher).

## 6. Phasing

| Phase | Content | Depends on |
|---|---|---|
| v1.1-a | Parity gaps API (3.1), audit coverage, agent tokens (4.3) | — |
| v1.1-b | `/mcp` endpoint, OpenAPI-derived tools, `read` + `write` tiers, CI parity guard | a |
| v1.1-c | `elevated` + `destructive` tiers (TOTP elevation, confirmations), resources/prompts | b |
| v1.1-d | Bot additions (5.3) incl. channel-visibility fix — the visibility fix may be pulled into a 1.0.x patch | a |
| v1.1-e | Agent bot path A (Hermes adapter or bridge, after spike) and path B (bot MCP tools) | b, d |

## 7. Left to the builder

- `rmcp` vs a hand-written JSON-RPC layer (license and axum fit decide).
- Exact tool naming scheme beyond the `area_object_verb` convention.
- Event buffer size and retention for `bot_wait_for_events`.
- How the stdio shim is shipped (desktop bundle, separate CLI, or both).
- UI layout of the Agent tokens page (follow existing Security settings patterns).

## 8. Maintainer decisions (2026-10-30)

1. Elevation window for agent tokens: **10 minutes**. **(maintainer)**
2. `elevated` and `destructive` MCP tools require **TOTP only**; a password entered via
   elicitation does not count, and admin accounts without TOTP cannot use these tiers.
   **(maintainer)**
3. v1.1 ships the admin MCP for **system admins first**; guild owners managing their own
   guilds through the MCP is a later step. **(maintainer)**
