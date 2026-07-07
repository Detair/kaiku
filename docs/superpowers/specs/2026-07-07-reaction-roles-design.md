# Self-Assignable (Reaction) Roles — Design

> Status: design, awaiting approval. Synthesized 2026-07-07 from the feature-gap
> analysis (Kaiku vs Discord/TeamSpeak). This is gap #1 of the top-5, chosen as
> highest value-per-effort because it reuses the existing roles, reactions,
> permissions, and WebSocket subsystems end-to-end.

## Context

Communities expect members to grant themselves opt-in roles (pronouns, regions,
game pings, notification opt-ins, a color) without a moderator doing it by hand.
Kaiku has roles, per-member role assignment, message reactions, and a permission
system — but nothing binds a reaction to a role grant. This feature adds that
binding.

A secondary, load-bearing discovery shaped the design: **role assignment is
currently silent over WebSocket.** `guild/roles.rs` calls
`assign_role_to_member` / `remove_role_from_member` and returns, with no
`ServerEvent` emitted, so other clients only see role changes on the next
refetch. Self-assign must feel live, so we introduce a `MemberRolesUpdated`
event and retrofit it into the existing admin assign/remove path too — closing
that latent gap for all role changes.

## Goals

- Admins bind an emoji on a specific message to a role; members react to
  grant/revoke that role themselves.
- Two modes at launch:
  - **`toggle`** — reacting grants the role, removing the reaction revokes it.
  - **`unique`** — bindings sharing a `group_key` are mutually exclusive:
    granting one revokes the others in the group (pick-one, e.g. a color).
- Safe by construction: a member self-assigning can never gain a role the
  binding's creator could not themselves grant.
- Live updates: members and admins see role changes without refetching.

## Non-Goals (YAGNI)

- `add_only` / `remove_only` modes — deferred until asked for.
- Button/dropdown "role panel" UX — deferred to gap #2b (interactive message
  components). When that lands, a panel is a second frontend over the **same**
  binding table; nothing here needs to change.
- Auto-role-on-join, role icons, role-mention pings — separate features.

## Data Model

One new table, TimescaleDB-agnostic (plain table), following the existing
migration and FK-cascade conventions.

```sql
-- migration: 20260707000000_reaction_role_bindings.sql
CREATE TABLE reaction_role_bindings (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    guild_id     UUID NOT NULL REFERENCES guilds(id)   ON DELETE CASCADE,
    channel_id   UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    message_id   UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    -- Emoji key: unicode grapheme ("🎨") or custom emoji ref ("<:name:uuid>"),
    -- the exact string form message_reactions already stores.
    emoji        VARCHAR(128) NOT NULL,
    role_id      UUID NOT NULL REFERENCES guild_roles(id) ON DELETE CASCADE,
    -- Non-null groups bindings for `unique` (pick-one) behaviour.
    group_key    VARCHAR(64),
    mode         VARCHAR(16) NOT NULL DEFAULT 'toggle',  -- 'toggle' | 'unique'
    created_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- At most one binding per emoji per message.
    CONSTRAINT reaction_role_unique_emoji UNIQUE (message_id, emoji),
    CONSTRAINT reaction_role_mode_valid   CHECK (mode IN ('toggle', 'unique'))
);

CREATE INDEX idx_reaction_role_message ON reaction_role_bindings(message_id);
CREATE INDEX idx_reaction_role_guild   ON reaction_role_bindings(guild_id);
```

Cascades mean deleting a guild, channel, message, or role removes its bindings
automatically — no orphan-cleanup code. `created_by` is `SET NULL` (consistent
with `oidc_providers`/`server_config`) so a bound message survives the admin's
account deletion.

## Permissions & Safety

All enforcement is at **binding-creation** time; self-assign at reaction time is
intentionally unprivileged (that is the feature).

Creating or editing a binding requires `MANAGE_ROLES` and reuses the existing
`permissions::resolver::can_manage_role(actor_perms, actor_highest_position,
target_role_position, None)`:

1. Actor must hold `MANAGE_ROLES`.
2. **Role-hierarchy guard** — the target role must be strictly below the actor's
   highest role. This blocks the headline exploit: an admin binding `@Owner` (or
   any role ≥ their own) to an emoji everyone can click.
3. **Dangerous-permission guard** — reject binding a role whose permission
   bitfield intersects a deny-list (`ADMINISTRATOR`, `MANAGE_GUILD`,
   `MANAGE_ROLES`, `MANAGE_CHANNELS`, `KICK_MEMBERS`, `BAN_MEMBERS`). Even a
   low-position role must not be self-grantable if it carries moderator power.
   Returned as a distinct `ReactionRoleError::RoleNotSelfAssignable`.

The `@everyone`/default role and managed/bot roles are not bindable.

## Reaction-Time Behavior

Hook the existing handlers in `server/src/chat/reactions.rs`:

- **`add_reaction`** — after the reaction row is written, in the **same
  transaction**, look up `reaction_role_bindings` for `(message_id, emoji)`:
  - `toggle` / `unique` → `assign_role_to_member(guild_id, user_id, role_id,
    assigned_by = user_id)` (idempotent; `ON CONFLICT DO NOTHING` already in the
    query).
  - `unique` additionally: for every sibling binding in the same `group_key`,
    `remove_role_from_member` for that role, and remove the user's stored
    reaction for the sibling emoji so the UI reflects the swap.
  - Emit `MemberRolesUpdated`.
- **`remove_reaction`** — symmetric: `toggle`/`unique` → `remove_role_from_member`
  and emit `MemberRolesUpdated`. (Removing a `unique` reaction leaves the user
  with no role in that group — acceptable; groups are opt-in, not mandatory.)

Bindings only act on reactions to messages in a guild channel; DM reactions are
ignored (no roles in DMs). If the reacting user is not a guild member (edge
case: reacting to a bound message they can see but haven't joined), the grant is
skipped.

## WebSocket Event

New variant in `server/src/ws/events.rs`:

```rust
MemberRolesUpdated {
    guild_id: Uuid,
    user_id: Uuid,
    role_ids: Vec<Uuid>,   // the member's full role set after the change
},
```

Broadcast to the guild's subscribers via the existing Redis pub-sub path. Sending
the **full role set** (not a delta) makes the client update idempotent and
tolerant of missed events.

Emitted from:
1. The reaction-role grant/revoke path above.
2. **Retrofit:** `guild/roles.rs` `assign_role`/`remove_role` admin handlers,
   which are silent today. This is the bonus latent-gap fix — after this,
   *every* role change is live.

Client (`stores/websocket`) handles `MemberRolesUpdated` by updating the cached
member's `role_ids`, which drives member-list role chips and permission-derived
UI.

## API Surface

New Tier-1 module `server/src/guild/reaction_roles.rs` (error / types / queries /
handlers split, per the codebase consistency standards). All routes nested under
the guild router and gated by `MANAGE_ROLES`:

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/api/guilds/{id}/reaction-roles` | List bindings (optionally `?message_id=`) |
| `POST` | `/api/guilds/{id}/reaction-roles` | Create a binding (validates hierarchy + self-assignable) |
| `DELETE` | `/api/guilds/{id}/reaction-roles/{binding_id}` | Remove a binding |

`POST` body: `{ channel_id, message_id, emoji, role_id, mode, group_key? }`.
Creating a binding also adds the bot/system reaction to the message so members
have something to click (best-effort; failure to pre-seed the reaction is
non-fatal and logged).

## Client Changes

- **`GuildSettingsModal`** → new "Reaction Roles" section: pick a channel +
  message (or paste a message link), then add emoji↔role rows with a mode
  selector and optional group label. Reuses the existing emoji picker and role
  list components. Only roles the actor can manage and that pass the
  self-assignable check are offered.
- **`stores/websocket`** → handle `MemberRolesUpdated`.
- Member-list role chips already render from `role_ids`; they simply become live.

## Edge Cases

- **Role deleted** → binding cascade-removed; existing grants of that role are
  independently removed by the role's own delete cascade.
- **Bound message deleted** → binding cascade-removed.
- **Emoji un-bound while reactions exist** → future reactions no longer grant;
  existing role grants are left untouched (no retroactive strip — least
  surprise).
- **User leaves guild** → membership cascade removes their roles; stale
  reactions are cosmetic and cleared on message refetch.
- **Rapid toggle spam** → the existing per-user reaction rate limit applies;
  grant/revoke is idempotent so worst case is redundant no-op writes.
- **`unique` group swap** → done in one transaction so a member is never
  momentarily in two group roles or none.

## Testing

Integration tests (`#[sqlx::test]`, per the project's isolation pattern):

- `toggle`: react grants role + emits event; un-react revokes.
- `unique`: reacting a second emoji in a group revokes the first role and clears
  the first reaction; single transaction.
- Hierarchy guard: creating a binding for a role ≥ actor position → `403`.
- Self-assignable guard: binding a role with `MANAGE_GUILD` → rejected.
- Cascade: deleting the role / message / guild removes bindings.
- Non-member reacting to a bound message → no grant.
- `MemberRolesUpdated` fires on both reaction-role and admin assign/remove paths.

Client: unit test the `MemberRolesUpdated` reducer updates the cached member's
role set.

## Rollout

Additive migration + new endpoints + one new WS variant; no changes to existing
message/reaction wire formats. The `MemberRolesUpdated` retrofit is backward-safe
(clients that don't yet handle it ignore unknown variants, as the WS layer
already tolerates). Ship server + client together so the live-update path is
consistent, but there is no hard ordering requirement.
