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
- G3 — Elevated and destructive actions need an explicit approval by the owner outside the
  agent's conversation, on one of the owner's enrolled devices. An approval from a desktop on
  the same host as the agent is weaker. It is therefore accepted only for elevation windows
  by default; destructive actions need the phone (maintainer, 4.4 step 5).
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

- System-admin API: `/api/admin/*` behind `require_auth`; everything except `/status` (own
  admin/elevation status, any authenticated user) also behind `require_system_admin`
  (`server/src/admin/mod.rs`). Mutations **and some reads** (reports, auth settings, OIDC
  provider list, page limits) additionally need `require_elevated`. Elevation lasts 15 min,
  is bound to a live `sessions` row via the JWT `sid` (`elevated_sessions.session_id` FK), and
  needs the password plus TOTP if MFA is enabled, or a fresh OIDC login (C24).
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
  page limits, report resolution, most guild actions. Some audit writes are best-effort
  after the mutation (content filters discard audit failures).
- Bot delivery ignores channel visibility: `MessageCreated` goes to every installed bot with
  the `messages` intent (`ws/bot_events.rs`, `ws/bot_gateway.rs`), without a `VIEW_CHANNEL`
  check (verified in review).
- No MCP code exists.

### 3.1 Gaps that block full parity (G1)

Parity is "every admin action", so admin actions that today exist **only** in the database or
as unused permission bits must first get an API (they are useful for the UI too):

| Gap | Today | v1.1 |
|---|---|---|
| Grant / revoke system admin | helpers exist (`permissions/queries.rs`), no exposed route; only the first-user setup grants | `POST/DELETE /api/admin/system-admins/{user_id}` (elevated, destructive tier) |
| Server name, terms URL, privacy URL | written once by first-user setup, no post-setup update route | extend `PUT /api/admin/auth-settings` → `PUT /api/admin/server-settings` |
| Optional features (forum, announcements, events, screening) | env `KAIKU_OPTIONAL_FEATURES` only | runtime setting in `server_config`, env stays as the operator's upper bound |
| Guild ban / unban | `BAN_MEMBERS` bit unused, no route | `PUT/DELETE /api/guilds/{id}/bans/{user_id}` + list |
| Member timeout | `TIMEOUT_MEMBERS` unused | `PUT/DELETE /api/guilds/{id}/members/{user_id}/timeout` |
| Transfer ownership | `TRANSFER_OWNERSHIP` unused | `POST /api/guilds/{id}/transfer-ownership` |
| Kick / invites | owner-only, permission bits ignored | honour `KICK_MEMBERS`, `CREATE_INVITE`, `MANAGE_INVITES` |
| Guild audit view | `VIEW_AUDIT_LOG` unused | `GET /api/guilds/{id}/audit-log` |
| Audit coverage | partial | every admin/guild-management mutation writes an audit entry |

These are ordinary API features with their own tests and UI; the MCP picks them up
automatically (section 4.2).

**v1.1 scope (coordinator call after the Fable review):** rows 1–3 (system admins, server
settings, optional features) and audit coverage ship in v1.1, because they are admin actions.
Rows 4–8 (guild ban, timeout, transfer, permission-bit kick/invites, guild audit view) are
new guild features that exist in neither the UI nor the API today. They follow in a later
step together with guild owners getting the MCP. When they land, they get MCP tools in the
same PR (G2).

## 4. Part 1 — Admin MCP

### 4.1 Shape and transport (coordinator call)

- The MCP endpoint is **built into the server** at `/mcp`, same host as the API. There is no
  sidecar to deploy or version separately.
- Transport: MCP **Streamable HTTP**.
  - **Revisions:** the newest published revision at build time (2026-07-28 when written:
    no GET stream, no protocol sessions), **plus** the 2025-06-18/2025-11-25 revisions with
    `Mcp-Session-Id`. Mainstream clients still negotiate those, so this compatibility is
    planned, not optional. Re-check the revision strings when building.
  - **Per request:** every request is authenticated on its own; requests with an unexpected
    `Origin` are rejected and the protocol version is validated.
  - **SSE:** responses set proxy-friendly headers (no buffering, keep-alive), with timeouts
    below the reverse proxy's.
- Local stdio clients use a small shim (`kaiku-mcp`) that forwards stdio to `/mcp` with the
  agent token. Clients that speak HTTP connect directly.
- Implementation: the official MCP Rust SDK (`rmcp`, Apache-2.0 upstream; confirm with
  `cargo deny`) or a minimal JSON-RPC layer if the SDK does not fit axum. The builder decides after a
  spike.

### 4.2 Parity by construction (coordinator call)

An MCP tool **does not re-implement** an admin action. A tool call becomes an internal HTTP
request dispatched through the **fully layered API service** (`tower::ServiceExt::oneshot`
on the same `Router` the listener serves, not on a sub-router), carrying:

- the agent's own `Authorization` header, so `require_auth`, scopes (4.3), elevation and
  permission checks run unchanged. An `AuthUser` extension inserted by hand would bypass them.
- the outer request's trusted connection context (`ConnectInfo`, proxy-derived client IP),
  because elevation and rate limiting read it;
- a child request id linked to the MCP request id (`x-request-id: <mcp-id>/<n>`;
  `SetRequestIdLayer` keeps an existing id), so logs and audit entries correlate. HTTP
  metrics and tracing count child requests as well, which is accepted;
- body-size limits on the outer MCP request **and** on each generated request.

Implementation notes (verified against `api/mod.rs` and `main.rs`):
- **`ConnectInfo` is not a layer.** It is inserted per connection by
  `into_make_service_with_connect_info`, so the MCP handler must copy the outer
  `ConnectInfo<SocketAddr>` extension **and** the forwarding headers (`X-Forwarded-For`,
  `X-Real-IP`) into the child request. Otherwise `ban_user`, rate limiting and
  `extract_client_ip` fail.
- **Router handle:** the finished `Router` is stored in `AppState` through a `OnceLock` (or
  `ArcSwap`) after `create_router`.
- **No recursion:** child requests carry a marker extension. A child request to `/mcp`, or a
  depth greater than 1, is refused.
- **Compression:** `Accept-Encoding` is stripped from child requests, so the handler never
  has to parse gzip.
- **Rate limits:** per-user route rate limits also apply to child requests (keyed by the
  owner); this is intended.

**Rate limits:** most admin routes have no general limiter today. The MCP gets its own
per-token limit (calls per minute, plus a lower one for `elevated`/`destructive` tiers),
independent of the routes.

**Tool definitions** come from the OpenAPI spec (utoipa). Each admin-relevant operation carries
an `x-mcp` extension `{ tool, tier, summary }`. The input schema is built from path and query
parameters plus the request body, with `$ref`s resolved and parameter serialisation (arrays,
enums) matching what the handler expects. Non-JSON bodies use explicit adapters. For example,
emoji upload is multipart, and its tool takes base64 content plus a filename.

**Completeness guard (G2) (coordinator call after the Fable review):** walking the OpenAPI
document is not enough, because paths are registered by hand (`openapi.rs`). Rebuilding every
router from one table would be a large refactor right after the family release. So:

- **Source-scanning CI test** in the style of `server/src/error_code_tests.rs`: every mounted
  **HTTP method + full path** in the management modules (including chained methods such as
  `get(..).post(..)` and nested router prefixes) must match an OpenAPI **operation** with an
  `x-mcp` extension, or an entry in an explicit exclusion list that gives a reason. A new POST
  or DELETE on an already documented path therefore fails CI too. The
  management modules are `admin/`, and the guild, channel, category, role, override, invite,
  emoji, webhook, filter, bot-install, screening, event, reaction-role and page modules.
- **Same test, two more checks:** tool names must be unique, and the tier must be set.
- **New guild-management features** fail CI until they declare their MCP tool.

`x-mcp` uses utoipa's path `extensions(...)`, which utoipa-gen 5.4 supports.

**Composite tools** exist where one API call is not the natural unit (for example
`server_overview`: stats, diagnostics, pending reports and recent audit entries). Composite
tools only call routes that other tools already cover.

### 4.3 Agent tokens (shared foundation)

A new credential type for the admin MCP. Agent bots keep using their existing bot tokens
(section 5), so admin and bot credentials are separate by construction.

- **Owned by a user.** The user creates it in User Settings → Security → *Agent tokens*. The
  MCP never creates one silently.
- **Storage:** format `kaiku_at_<id>_<secret>`, where the secret is 256 bits of CSPRNG output.
  - **Hash:** only `SHA-256(secret)` is stored, looked up by id and compared in constant
    time. A slow hash protects low-entropy passwords; for random tokens it would only turn
    per-request verification into a CPU DoS (each MCP tool call authenticates twice).
  - **Display:** the token is shown once.
  - **Last-used updates:** `last_used_at` and IP are written at most once per minute per
    token.
- **Fields:** name ("Claude Code on my PC"), scopes, expiry (default 90 days, maximum 1 year),
  created and last-used timestamps, last-used IP.
- **Scopes (coordinator call):** `admin:read`, `admin:write`, `admin:elevated`, and
  `guild:manage` (acts with the owner's guild permissions; agent tokens are issued to system
  admins only in v1.1, see section 8). A token never exceeds its owner's rights; scopes only
  narrow them.
- **Default deny.** `require_auth` accepts `Authorization: Bearer kaiku_at_…` and puts a
  credential kind into `AuthUser` (`Session { sid }` or `AgentToken { id, scopes }`). A
  middleware that runs directly after `require_auth` then **rejects the `AgentToken` kind with
  403 on every route that does not declare a scope requirement**. Only these accept agent
  tokens:
  - the management routes carrying `x-mcp` (4.2);
  - `/mcp`;
  - `GET /api/admin/status`;
  - the approval execute/status endpoints (4.4);
  - `DELETE /api/agent-tokens/self` (self-revocation, see below).

  Agent tokens can therefore never reach:
  - user routes (sending messages, DMs, profile);
  - `/auth/*` (password, MFA, sessions);
  - the user WebSocket `/ws`;
  - push subscriptions;
  - agent-token management other than revoking **their own** token;
  - device enrollment;
  - approvals answering;
  - `POST`/`DELETE /api/admin/elevate`.

  A CI test calls a sample of non-management routes with an agent token and expects 403.
- **Scope enforcement covers REST, not just MCP.** Scope and tier checks live in the same
  shared middleware, so calling REST directly cannot bypass what the MCP enforces.
- **Single execution path for `destructive`.** A destructive route called with an agent token
  (REST or MCP) **always** answers `202 Accepted` with an `approval_id` and stores the
  request. Execution happens **only** server-side, from the stored request, via
  `POST /api/approvals/{id}/execute`, which is what the `get_approval` tool calls. An
  approval id is never accepted on the original route, so arguments cannot be swapped
  between approval and execution.
- **Revocation is durable and checked on every request.** It uses its own `agent_tokens`
  table with `revoked_at`, cached in Valkey with immediate invalidation. The C25 denylist is
  not used, because its entries expire with the access-token lifetime. Revocation is
  triggered from:
  - the owner's settings;
  - the admin panel (system admins can revoke anyone's token);
  - the MCP tool `agent_token_revoke_self` (`DELETE /api/agent-tokens/self`). This is the **only** agent-token-management
    action an agent token may call, and only for its own token; it is an explicit exception
    to the default deny;
  - banning or deleting the owner, which revokes all their tokens and closes their streams.

  Revocation sets `revoked_at`, so the FK cascade does not fire. The revocation transaction
  therefore **explicitly deletes the token's `elevated_sessions` row and moves its pending and
  approved-but-unconsumed approvals to `expired`** (audited)
  and invalidates the cached elevation (`admin:elevated:at:{token_id}`) and token entries. The
  cascade only covers hard deletion of the token row.
- **Elevation storage:** `elevated_sessions.session_id` becomes nullable, and a nullable
  `agent_token_id` (FK `agent_tokens(id) ON DELETE CASCADE`) is added.
  - A check constraint requires exactly one of the two.
  - Two partial unique indexes allow one active elevation per session and one per token.
  - `require_elevated` branches on the credential kind. For `AgentToken` it looks up
    `elevated_sessions.agent_token_id`; the cache key becomes `admin:elevated:at:{token_id}`.
  - Token elevations come **only** from an approved elevation request (4.4). They appear in
    the admin panel next to session elevations, and the owner can end one early there or in
    the token list.
- **Audit:** every entry made through an agent token records `via: "agent_token"` and the
  token id, which are authenticated. It also records the MCP client name, which is
  self-reported and labelled as such.

### 4.4 Tiers and human approval (G3)

| Tier | Examples | Behaviour |
|---|---|---|
| `read` | list users and guilds, stats, diagnostics, audit log, observability | runs directly |
| `write` | create/edit channels and categories, create roles without new permissions, edit guild settings, emojis, reaction roles, pages | runs directly (same as an admin clicking in the UI) |
| `elevated` | reports (including reads), auth settings and OIDC provider list (including reads), page limits, create account, announcements, guild suspend, **create/update webhooks, create invites, change role permissions or channel overrides, edit content filters** | needs an **elevation window** for this token |
| `destructive` | ban/unban, bulk ban, delete user or guild, grant/revoke system admin, transfer ownership, delete an OIDC provider | needs a **per-action approval**, even inside an elevation window |

Routes that require elevation today keep requiring it. The tier never weakens an existing
check.

- **Highest tier wins.** A route's tier is the highest of its own tier and the tier of
  anything it can create. Webhooks, invites and permission changes are `elevated` because
  they let data or people out: a prompt-injected report text must not be able to create
  them without an approval.

**Approval happens outside the chat, on the owner's own Kaiku device (maintainer: a notification
on mobile or desktop that is simply confirmed; coordinator call on the mechanism).** MCP forbids
collecting credentials through form elicitation. A code typed into the conversation would also
reach the model, which could apply it to a different pending action. So:

1. For an elevation or a destructive action, the server stores the **exact pending request**:
   - token;
   - route and arguments;
   - a precondition snapshot (for example the target's `updated_at` or version);
   - a 5-minute expiry.

   It returns an approval id. The tool result tells the agent "waiting for approval on your
   devices".
2. **Notification to the owner's devices.**
   - **Android:** a high-priority push using the C20 wake path.
     - **Payload:** new kind `t=approval` with only the approval id. C20 and the Android
       parser are extended, since `channel_id` is not required for this kind.
     - **Delivery:** TTL 5 minutes, collapse key `approval-<id>`. Approval wakes get their
       **own budget** like `ringing` (capped by the anti-fatigue limits in step 7). They
       bypass the message coalescer and read gate, so they can never be dropped under
       message load. They go only to the push subscriptions of enrolled approval devices.
     - **Parsers:** both Android parsers change: `WakeSignalParser.build()`, which drops
       payloads without `channel_id` today, and the UnifiedPush body path.
     - **Display:** the app fetches the pending request **only from its configured server**,
       authenticated, and shows "*Claude Code on my PC* wants to: ban user *max* from the
       whole server – Approve / Deny". A forged push can at most cause one authenticated
       fetch that returns nothing.
   - **Desktop app:** a system notification (`tauri-plugin-notification`, already a
     dependency) plus an in-app dialog with the same text.
     - **Trigger:** an internal message on `user:{id}` (the pattern from
       `auth/revocation.rs`) becomes a WebSocket event `approval_pending { approval_id }`.
     - **Fetch:** the app fetches the request authenticated, the same way as Android.
   - The text is built from the **server-stored** request with **fixed wording per
     action**:
     - immutable identifiers next to display names ("user *max* · id 7f3…");
     - the server's own name and origin;
     - the token's name and creation date.

     Free-text labels (agent token names, usernames, guild names) are escaped, length-capped
     and visibly marked as labels. Agent-supplied free text (for example a ban reason) is
     shown as a quoted "reason given by the agent", never as the action description.
3. **One tap approves.** The approval is signed with a **per-install device key**.
   - **Signature:** **ECDSA P-256 / SHA-256 on every platform.** Android Keystore and
     StrongBox support P-256, not Ed25519.
     - **Signing input:** the **server** builds the exact bytes:
       `"kaiku-approval-v1" ‖ approval_id ‖ request_hash ‖ decision ‖ expires_at ‖ origin ‖
       owner_id ‖ device_id`, fixed-length or length-prefixed fields.
     - **Device side:** the device receives these bytes together with the display text and
       signs them after showing the text. It never canonicalises JSON.
     - **Request hash:** `request_hash` is computed server-side over the stored request.
     - **Origin:** comes from `config.public_url`. Approvals and the TOTP page **fail
       closed** when it is unset.
   - **Acceptance:** the server accepts only signatures from a non-revoked enrolled device of
     the token's owner.
   - **Enrollment:** only from an interactive user session, never with an agent token. It
     needs a fresh re-authentication (C24: password+TOTP or a fresh OIDC login) and proof of
     key possession (the device signs a server challenge).
   - **What agent tokens can never do:** enroll, replace or revoke devices, open the TOTP
     approval page, or answer approvals. These endpoints reject the `AgentToken` credential
     kind outright.
   - **Enrollment request:** reauth factors go inline, like `/elevate` (password +
     `mfa_code`), or through a new OIDC reauth purpose `OidcFlowPurpose::EnrollDevice`.
   - **Device table:** `approval_devices(id, user_id, device_id, session_id, public_key,
     platform, enrolled_at, last_used_at, revoked_at)`. `device_id` is the same per-install
     id as `push_subscriptions.device_id`, so approval pushes go to enrolled devices only.
   - **Visibility:** enrolled devices are listed under Security with last use, and enrolling
     a new device notifies all existing ones.
   - **Lifecycle:** a device key is revoked when its session ends (logout or expiry). The app
     then shows a visible "re-enable approvals on this device" prompt at the next sign-in, so
     the owner never finds out only when an approval has no approver.
4. **Android: fresh user authentication for every approval.** This is new work: there is no
   biometric or Keystore code in the app yet, and it adds `androidx.biometric`. The app's
   `minSdk` is 26.
   - **Key:** a non-exportable Android Keystore key (StrongBox where available), required to
     authenticate on every use.
   - **API 30+:** `setUserAuthenticationParameters(0, BIOMETRIC_STRONG | DEVICE_CREDENTIAL)`.
     Fingerprint, face or device PIN.
   - **API 26–29:** `setUserAuthenticationValidityDurationSeconds(-1)`, which allows
     **strong biometrics only**. A `CryptoObject` cannot use the PIN there. Devices without
     strong biometrics cannot enroll (they fail closed) and use the TOTP page.
   - **Approval:** each signature needs its own `BiometricPrompt` with a `CryptoObject`, even
     on an already unlocked phone. That one touch is the "simple confirmation".
   - **Result:** a stolen phone cannot approve without the owner's biometric or PIN. A phone
     whose PIN was shoulder-surfed can, which is the same limit as the phone itself.
5. **Desktop is the convenient path, not the strongest.**
   - **Weakness:** if the agent runs on the same PC, local malware or automation could click
     the dialog or misuse the desktop's key, which is stored in the OS keychain.
   - **Accepted (maintainer):** simple confirmation on desktop or mobile.
   - **Hardening:** the dialog ignores synthetic input where the OS allows detecting it and
     never auto-focuses Approve.
   - **Owner setting "Destructive actions only from my phone": on by default (maintainer).**
     The server rejects desktop signatures for the `destructive` tier. Desktop approval
     stays available for the `elevated` tier (elevation windows). The owner can switch the
     setting off, which needs a fresh re-authentication and is audit-logged. If no phone is
     enrolled, destructive actions fall back to the TOTP page.
6. **Fallback:** if no device is reachable, the tool result and a URL elicitation point to
   `https://<server>/approve/<approval_id>`.
   - **Access:** the page is served by Kaiku, needs a signed-in owner session (never an agent
     token) and a **current TOTP code**.
   - **Protection:** CSRF protection, attempt limits and the existing TOTP replay protection.
   - **Remaining risk:** like any web login, it stays exposed to real-time phishing and a
     compromised browser. Device approval is therefore the primary path.
7. **Execution.** The agent calls `get_approval { approval_id }`, which polls the status and,
   once approved, calls `POST /api/approvals/{id}/execute`. That is the only execution path.
   - **Caller:** must be the same agent token that created the request.
   - **States:** approvals follow `pending → approved → consumed`, `pending → denied` and
     `pending → expired`.
     - **Transitions:** each one is a single conditional
       `UPDATE … WHERE state = <from> AND expires_at > now()`.
     - **Expiry:** `pending` and `approved` requests both have `expires_at` (5 minutes from
       creation; approving does not extend it). Both move to `expired` in one of two ways:
       - when touched after `expires_at` (`UPDATE … SET state='expired' WHERE state IN
         ('pending','approved') AND expires_at <= now()`);
       - through a small periodic sweep (every minute, the same statement), so abandoned
         requests are audited as `expired` and release their slot.
     - **Open-approval limit:** only unexpired requests in either `pending` or `approved`
       state count toward the limit of 3 open approvals, so abandoned ones cannot exhaust
       it.
     - **No races:** approve, deny, expiry and consumption cannot overlap.
     - **Elevation:** consuming an elevation approval creates the 10-minute window.
     - **Audit:** every transition is audited, with the device id for approve/deny.
   - **Execution:** the server re-checks token validity, scopes, permissions and elevation,
     then executes exactly the stored request.
   - **Precondition:** declared per route next to its `x-mcp` entry.
     - **Default `None`:** the request hash alone binds the arguments.
     - **Named checks** for the few routes where state matters:
       - ban: user not already banned;
       - grant admin: not already admin;
       - revoke admin: still admin and not the last one;
       - transfer ownership: current owner id unchanged;
       - delete guild/user: target still exists.
     - **Transaction:** the check runs inside the same database transaction as the mutation,
       with a row lock or a conditional update. If it fails, the request is refused and a new
       approval is needed.
   - **Failure:** the approval stays consumed and the error is returned; a retry needs a new
     approval. Deny is final for that approval id.
   - **Anti-fatigue limits:** pending requests and notifications are rate limited per token,
     per owner and per device, including unanswered ones and requests created via direct
     REST. At most 3 open approvals per owner.
8. **Elevation windows** for agent tokens last **10 minutes** (maintainer). Elevation and every
   destructive action each need their own approval. An approval never covers more than the
   one stored request, or the one elevation window.
9. The `elevated` and `destructive` tiers need at least one registered device or TOTP. Admin
   accounts with neither cannot use these tiers through the MCP.

### 4.5 Untrusted content and audit durability

- Some admin tools return text written by other users: report contents, usernames, guild
  names, message excerpts. Tool results mark these fields as untrusted, and a fixed note in
  the tool description says so. Combined with 4.4, a malicious report text cannot cause a ban
  without the operator approving it on the Kaiku page.
- **Audit durability** (coordinator call after the Fable review: no outbox worker), for MCP
  and direct REST alike:
  - Every admin mutation writes its audit row **in the same transaction as the mutation,
    before commit**. Today `ban_user` writes it after commit, and content filters discard
    audit errors; both change in v1.1-a.
  - Side effects after commit (Valkey session revocation, pushes) that fail return an error,
    as they do now, and add a second audit row `<action>.failed`.
  - No path reports success without a committed audit record.

### 4.6 Resources and prompts

- MCP *resources* for read-mostly context:
  - `kaiku://server/overview`
  - `kaiku://admin/audit-log/recent`
  - `kaiku://docs/admin-guide/<page>`: bundled admin guide pages, so the agent can answer
    "how do I…" from the shipped docs.
- MCP *prompts* for frequent workflows: "onboard a family member" (create an account with a
  temporary password and add it to guilds), "weekly health check".

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
offers bot tools when called with the bot's **existing bot token** (`Authorization: Bot …`),
never with an agent token:
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
- **REST for bots:** the same actions over REST with `Authorization: Bot …` (bot tokens get
  the same fast-hash verification or an in-memory verified-token cache, since Argon2 per
  request would be a CPU DoS) for runtimes that
  cannot hold a WebSocket. Rate limits per bot as today.
- **Visible identity:** bots show a *Bot* badge (exists), agent bots additionally an *AI*
  badge and the owner's name; a channel shows which bots can read it.

### 5.4 Safety rules for agent bots

- Admin and bot access are **separate identities by construction**:
  - agent tokens carry only admin and guild-management scopes;
  - bots authenticate with bot tokens, and bot tools authorise **as the bot user**, never as
    its owner.

  No token can combine both. (Anti-goal: prompt injection from chat reaching admin or
  guild-management powers.)
- The server cannot see what runs inside an agent process. The admin guide therefore says:
  never connect the admin MCP and a chat bot in the same agent context.
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
| v1.1-c | `elevated` + `destructive` tiers: device approvals (push to Android, desktop dialog, device keys), TOTP approval page as fallback, resources/prompts | b, web client (RC2) |
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
2. Approval for `elevated` and `destructive` MCP tools: **a notification on mobile or
   desktop that is simply confirmed** (signed by the owner's device; Android also needs the
   phone unlocked). TOTP on the Kaiku approval page is the fallback. A password never
   counts. **(maintainer; replaces the earlier "TOTP only" answer the same evening)**
3. v1.1 ships the admin MCP for **system admins first**; guild owners managing their own
   guilds through the MCP is a later step. **(maintainer)**
4. **Destructive actions only from the phone**, on by default; desktop approvals cover
   elevation windows only. **(maintainer)**
