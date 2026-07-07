# Membership Screening (Rules Gate) — Design

> Status: design, awaiting approval. 2026-07-07, gap #5b (split from scheduled
> events).

## Context

Public/discoverable guilds want a gate: a new member must read and accept rules
(and optionally clear a verification step) before they can participate. Kaiku
joins a guild by inserting into `guild_members` in three places — invite redeem
(`guild/queries/invites.rs`), discovery join (`discovery/queries.rs`), and guild
create (`guild/queries/core.rs`) — with no notion of a pending member. This
feature adds a **pending** state gated by rules acceptance, integrating with the
existing permission system (a pending member gets no `VIEW_CHANNEL`) and the
client onboarding UI.

## Goals

- A guild can enable **screening**: newly-joined members enter a **pending**
  state and see only a rules/acceptance screen until they accept.
- Pending members cannot read channels, send messages, join voice, or DM guild
  members via the guild — enforced server-side, not just hidden in the UI.
- Admins configure the rules text and toggle screening; they see and can approve
  or kick pending members.

## Non-Goals (YAGNI)

- Application questions / manual approval queues — acceptance is
  self-service-on-accept for v1 (approval-required is a later mode).
- CAPTCHA / anti-bot verification — the structure allows a `verify` step later;
  none shipped now.
- Per-channel screening — guild-level only.

## Data Model

```sql
-- migration: 20260707000005_membership_screening.sql

-- Screening config + rules on the guild.
ALTER TABLE guilds ADD COLUMN screening_enabled BOOLEAN NOT NULL DEFAULT FALSE;
CREATE TABLE guild_screening_rules (
    guild_id UUID PRIMARY KEY REFERENCES guilds(id) ON DELETE CASCADE,
    rules_md TEXT NOT NULL,                 -- markdown, rendered via the safe sanitizer
    updated_by UUID REFERENCES users(id) ON DELETE SET NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Membership state. Default 'active' preserves all existing members.
ALTER TABLE guild_members
    ADD COLUMN membership_state VARCHAR(16) NOT NULL DEFAULT 'active'; -- 'active' | 'pending'
ALTER TABLE guild_members ADD COLUMN accepted_rules_at TIMESTAMPTZ;
```

Existing members default to `active` (no backfill migration needed). Only new
joins, when `screening_enabled`, become `pending`.

## Join Flow Integration

The three `INSERT INTO guild_members` sites converge on a single
`add_guild_member(guild_id, user_id)` helper (small refactor to remove the
duplication — a targeted consistency improvement while we're here). That helper
sets `membership_state = 'pending'` when the guild has `screening_enabled`,
else `'active'`.

## Permission Enforcement

The permission resolver already computes a member's effective permissions. Add a
short-circuit: **a `pending` member has an empty permission set** (no
`VIEW_CHANNEL`, no `SEND_MESSAGES`, no voice), regardless of roles. This is one
guard in `permissions::helpers::get_member_permission_context`, so every existing
`require_*` check inherits it — no per-endpoint changes. The only things a
pending member can do: fetch the guild's rules and `POST` acceptance.

**WS coverage is automatic (verified):** the user WebSocket channel-subscribe
handler calls `permissions::require_channel_access` (`ws/handlers.rs:528`), which
resolves through `get_member_permission_context` — so the same short-circuit
blocks a pending member from subscribing to guild channels; no separate WS gate
is needed. And because permission context is **not cached** (verified this
session), acceptance takes effect immediately on the next request/WS check.

## Acceptance & Approval

- `POST /api/guilds/{id}/screening/accept` → sets `membership_state = 'active'`,
  `accepted_rules_at = now`, emits `MemberUpdated`. Requires the member to be
  `pending` in a screening-enabled guild.
- Admin view: `GET /api/guilds/{id}/members?state=pending`; admins may kick a
  pending member (existing kick). (Manual approval mode — admin flips
  pending→active — is a later toggle; not v1.)

## Real-Time (WS)

New `MemberUpdated { guild_id, user_id, membership_state }` broadcast to the
guild so admin member lists reflect pending→active live.

**Cross-spec coordination:** the reaction-roles spec introduces
`MemberRolesUpdated { guild_id, user_id, role_ids }` for the same "member
changed, update the list" purpose. Whichever ships **second** should extend the
first's event into a single `MemberUpdated { guild_id, user_id, role_ids?,
membership_state? }` (optional fields, full-value not delta) rather than add a
parallel event — one reducer on the client, no overlap. This spec assumes that
unification if reaction-roles landed first.

## API Surface

New module `server/src/guild/screening.rs` (Tier-1 layout):
`GET /api/guilds/{id}/screening` (config + rules; readable by a pending member),
`PUT /api/guilds/{id}/screening` (`MANAGE_GUILD`: toggle + rules text),
`POST /api/guilds/{id}/screening/accept` (the pending member).

## Client

- On entering a guild where the user is `pending`, the client shows a **rules
  gate** screen (reusing the onboarding-wizard visual language) instead of the
  channel view: rendered rules (safe markdown) + an "I agree" button →
  `accept` → transitions into the normal guild view.
- Guild settings gains a **Screening** section (toggle + rules editor) and a
  **Pending Members** list (approve-by-nothing / kick).

## Edge Cases

- Screening enabled *after* members joined → existing members stay `active`
  (only new joins are gated). Intentional; a "re-screen everyone" action is out
  of scope.
- Screening disabled while members are pending → pending members are promoted to
  `active` on disable (one UPDATE), so nobody is stuck.
- Owner/first member is always `active` (guild create path bypasses screening).
- A pending member's WS connection must also be gated (can't subscribe to guild
  channels) — the permission short-circuit covers WS subscription auth too.
- Rules text is markdown → rendered through the isolated DOMPurify sanitizer
  (no arbitrary classes), same as wiki pages.

## Testing

- New join to a screening guild is `pending`; pending member gets 403 on
  channel read/send/voice; can read rules + accept.
- Accept flips to active and unlocks access immediately (no cache staleness).
- Disabling screening promotes pending members.
- Owner bypass on guild create.
- `MemberUpdated` fires on accept.
- The unified `add_guild_member` helper is used by all three join paths.

## Rollout

Additive columns (defaulting existing members to `active`) + one resolver
short-circuit + endpoints + client gate. The resolver change is the sensitive
bit — it must be covered by the permission tests above so an active member is
never accidentally gated. Ship server + client together so gated users get the
acceptance screen rather than an empty guild.
