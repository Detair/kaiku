# Scheduled Events — Design

> Status: design, awaiting approval. 2026-07-07, gap #5a (split from membership
> screening per the scoping decision).

## Context

Communities schedule things — game nights, streams, meetings — and want members
to see upcoming events and RSVP. Kaiku has none of this. It has the pieces:
guilds/channels, the WebSocket event bus, and the background-task pattern
(`tokio::interval` in `observability/voice.rs` / `retention.rs`) needed to fire
reminders.

## Goals

- An admin/organizer creates a **guild event**: name, description, start (and
  optional end) time, and a location — either a voice channel in the guild or an
  external URL/text.
- Members **RSVP** (interested / going) and see a count; the event list shows
  upcoming events sorted by start time.
- A **reminder** fires shortly before start (in-app now; mobile push once gap #4
  lands — this feature degrades gracefully without it).

## Non-Goals (YAGNI)

- Recurring events — single occurrences for v1.
- Auto-start (moving RSVPs into the voice channel at start) — v2.
- Event cover images — optional later; schema leaves room.
- Cross-guild/discoverable events — guild-scoped only.

## Data Model

```sql
-- migration: 20260707000004_guild_events.sql
CREATE TABLE guild_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    -- Voice-channel event when set; external event when NULL + location text.
    channel_id UUID REFERENCES channels(id) ON DELETE SET NULL,
    name VARCHAR(100) NOT NULL,
    description VARCHAR(1000),
    location TEXT,                          -- external URL/text (when channel_id NULL)
    starts_at TIMESTAMPTZ NOT NULL,
    ends_at   TIMESTAMPTZ,
    status VARCHAR(16) NOT NULL DEFAULT 'scheduled', -- scheduled|active|completed|cancelled
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    reminder_sent BOOLEAN NOT NULL DEFAULT FALSE
);
CREATE INDEX idx_guild_events_upcoming ON guild_events(guild_id, starts_at)
    WHERE status = 'scheduled';

CREATE TABLE guild_event_rsvps (
    event_id UUID NOT NULL REFERENCES guild_events(id) ON DELETE CASCADE,
    user_id  UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    response VARCHAR(16) NOT NULL,           -- 'interested' | 'going'
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (event_id, user_id)
);
```

## Permissions

- Create/edit/cancel needs a new **`MANAGE_EVENTS`** bit (falls back to
  `MANAGE_GUILD` if we choose not to add a bit — but a dedicated bit is cleaner
  and matches the granular model).
- Any member with `VIEW_CHANNEL`/guild membership can view and RSVP.

## Behavior

- Create/edit validates `starts_at` in the future and `ends_at > starts_at`.
- RSVP is an upsert (change interested↔going, or delete to clear). Counts are
  aggregate queries; the event list returns `{ interested_count, going_count,
  my_response }`.
- A **scheduler task** (`spawn_event_reminder_task`, the existing
  `tokio::interval` pattern, ~60s tick):
  - Marks events `active` at `starts_at`, `completed` at `ends_at` (or
    `starts_at + default` when no end).
  - For events starting within the reminder window and `reminder_sent = false`,
    notifies RSVP'd members (in-app `ServerEvent`; a push wake-signal once gap
    #4 exists) and sets `reminder_sent`.

## Real-Time (WS)

New `ServerEvent`s broadcast to the guild: `GuildEventCreated`,
`GuildEventUpdated` (includes status transitions + RSVP counts),
`GuildEventCancelled`, and a per-user `GuildEventReminder`.

## API Surface

New module `server/src/guild/events.rs` (Tier-1 layout):
`GET /api/guilds/{id}/events` (upcoming; `?scope=past` for history),
`POST /api/guilds/{id}/events` (`MANAGE_EVENTS`),
`PATCH/DELETE /api/guilds/{id}/events/{event_id}`,
`PUT /api/guilds/{id}/events/{event_id}/rsvp {response}`,
`DELETE …/rsvp`.

## Client

- An **Events** panel in the guild view (and an entry in the guild header): a
  list of upcoming event cards (name, time, location, going count, RSVP button),
  a create/edit modal (`MANAGE_EVENTS`), and a reminder toast/notification
  handler in `stores/websocket`.
- Voice-channel events link to the channel; external events show the location
  link (through the existing safe-URL rendering).

## Edge Cases

- Event's voice channel deleted → `channel_id` SET NULL, event becomes an
  external/placeholder event (or auto-cancel — pick auto-cancel with a logged
  reason for clarity).
- Organizer leaves/deleted → `created_by` SET NULL, event persists.
- Server downtime across a reminder window → on restart the task still fires
  reminders for not-yet-started events (idempotent via `reminder_sent`); events
  whose start passed during downtime are marked `active`/`completed` without a
  late reminder.
- Timezone: all times are UTC in storage; the client localizes.

## Testing

- Create validates future start + end ordering; `MANAGE_EVENTS` gate.
- RSVP upsert/clear; counts correct.
- Scheduler: transitions scheduled→active→completed at the right times;
  reminder fires once (idempotent), suppressed for cancelled events.
- WS events broadcast on create/update/cancel.

## Rollout

Additive tables + one background task + endpoints + client panel. Reminders are
in-app until mobile push (#4) lands, at which point the reminder path gains a
push wake-signal with no schema change. No dependency ordering beyond that
graceful enhancement.
