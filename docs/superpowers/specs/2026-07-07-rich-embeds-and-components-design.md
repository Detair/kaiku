# Rich Embeds & Interactive Components — Design

> Status: design, awaiting approval. 2026-07-07, gap #2 of the feature-gap
> analysis. Scope decision: embeds **and** interactive components together, so
> bots gain rich cards and buttons/menus in one coherent surface.

## Context

Bots and webhooks can only post plain markdown today, so third-party
integrations look bare next to Discord's rich cards and interactive controls.
The load-bearing enabler already exists: the **bot gateway already implements an
interaction round-trip** — `BotServerEvent::CommandInvoked` carries an
`interaction_id`, the bot replies over its gateway connection, and the server
checks interaction ownership and expiry (`server/src/ws/bot_gateway.rs`). This
was built for slash commands. Component interactions (button clicks, select
choices) extend that exact model rather than inventing a new one.

## Goals

- **Embeds:** structured rich cards on a message — title, description, URL,
  color, author, fields (name/value/inline), image/thumbnail, footer, timestamp.
  Set by bots (message API) and, later, incoming webhooks.
- **Components:** action rows containing buttons (styles: primary/secondary/
  success/danger/link) and select menus. A user interaction routes to the owning
  bot, which can reply (new message / ephemeral / update the source message).
- Rendered safely — embeds are semi-trusted, attacker-influenced content.

## Non-Goals (YAGNI)

- Modals (component-triggered forms) — a fast follow once buttons ship.
- Embed authoring by regular users in the composer — bots/webhooks only for v1.
- Rich embeds from *incoming* webhooks — depends on adding incoming webhooks
  (separate feature); the schema supports it from day one.

## Data Model

Two nullable JSONB columns on `messages` (which today is just
`content`/`parent_id`/…):

```sql
-- migration: 20260707000001_message_embeds_components.sql
ALTER TABLE messages ADD COLUMN embeds     JSONB;  -- Vec<Embed>, max 10
ALTER TABLE messages ADD COLUMN components JSONB;  -- Vec<ActionRow>, max 5 rows
```

Typed server structs mirror Discord's shape so existing bot libraries feel
familiar (`Embed`, `EmbedField`, `EmbedAuthor`, `EmbedFooter`, `ActionRow`,
`Button`, `SelectMenu`, `SelectOption`). serde-(de)serialized, validated on
write. JSONB (not a sidecar table) because embeds/components are always fetched
with the message, are bounded, and are opaque to SQL queries.

Component interaction state reuses the **existing interaction table/registry**
that slash commands already use (interaction_id, bot_id, context, expiry) — a
new `interaction_type` discriminator (`command` | `component`) and a
`custom_id` field distinguish component clicks.

## Validation & Safety (server-side, on message write)

Bots/webhooks are semi-trusted, so every embed/component is validated before
storage:

- **Size caps** (Discord-parity): ≤10 embeds; embed total ≤6000 chars; title
  ≤256; description ≤4096; ≤25 fields; field name ≤256 / value ≤1024; ≤5 action
  rows; ≤5 buttons per row; ≤25 select options.
- **URL fields** (`url`, `image`, `thumbnail`, `author.icon`, `footer.icon`)
  must be `https://` and are stored verbatim but rendered through the existing
  isolated DOMPurify link-hardening; no `javascript:`/`data:`.
- **`custom_id`** ≤100 chars, opaque to the server (bot-defined routing key).
- **Colors** are ints clamped to 24-bit.

Only **bots** and (later) incoming webhooks may set `components` — a human user's
message cannot carry buttons (prevents impersonating bot UIs). Embeds from bots
only in v1.

**Note on the existing `EMBED_LINKS` permission (verified `permissions/guild.rs`
bit 1<<1):** that bit governs *user link-unfurling* (auto-previewing pasted
URLs) and is unrelated to bot-authored rich embeds. Bot rich embeds need **no
new permission** — bot identity + the channel's `SEND_MESSAGES` suffice. This
spec does not touch `EMBED_LINKS`, and the distinction should be called out in
docs so it isn't conflated.

**Interaction model reuse (verified `ws/bot_gateway.rs`):** the gateway already
has `BotServerEvent::CommandInvoked { interaction_id, … }`, a bot `respond`
path keyed by `interaction_id` with ownership + expiry guards, and an
intent filter (`intent_permits_event`, intents `messages`/`members`/`commands`).
Components add exactly one server event (`ComponentInvoked`) and one intent
(`components`) to that machinery — no new interaction subsystem.

## Interaction Loop (components)

Reuses the slash-command machinery:

1. User clicks a button / picks a select option → client sends a WS
   `ComponentInteraction { message_id, custom_id, values? }` frame.
2. Server validates the component exists on that message, mints an
   `interaction_id` (type `component`, short TTL), and routes a
   `BotServerEvent::ComponentInvoked { interaction_id, custom_id, user, message,
   values }` to the owning bot over its gateway (respecting `components` intent).
3. Bot replies with the existing interaction-response `BotClientEvent`
   (`respond`) — reply variants: `channel_message`, `ephemeral`
   (visible only to the invoker), `update_message` (edit the source message's
   embeds/components), `deferred` (ack now, edit later).
4. Ownership + expiry are enforced by the existing guards ("Bot attempted to
   respond to interaction it does not own", "Interaction not found or expired").

Ephemeral replies are delivered only to the invoking user's WS session(s) and
are not persisted as channel messages.

## Client

- **`components/messages/`** gains `<MessageEmbeds>` and `<MessageComponents>`
  renderers. Embeds render as cards (all text through the **isolated DOMPurify
  sanitizer** from #631 — this is exactly the attacker-influenced content that
  motivated instance isolation). Components render as disabled-until-hydrated
  buttons/selects; clicking dispatches the `ComponentInteraction` frame.
- **`stores/websocket`** handles the ephemeral-reply and update-message events.
- Message list already re-renders on `MessageEdit`; `update_message`
  interactions reuse that path (embeds/components are part of the message body).

## API Surface

No new REST routes — embeds/components ride the existing message-create/edit API
(`POST /api/messages/channel/{id}`, `PATCH /api/messages/{id}`) with optional
`embeds` / `components` in the body, accepted only from bot-authenticated
callers. The bot gateway gains the `ComponentInvoked` server event and the
component branch of the interaction-response handler.

## Edge Cases

- Message with components edited to remove them → client drops the controls;
  in-flight interactions expire harmlessly.
- Bot offline when a button is clicked → interaction times out; client shows a
  transient "interaction failed" and re-enables the control.
- Source message deleted mid-interaction → response is dropped (message gone).
- Oversized/invalid embed → `400` at message-create, nothing stored.
- Select menu returning stale option values → bot validates; server only
  guarantees `custom_id` integrity.

## Testing

- Serde round-trip + validation rejects (each size cap, non-https URL, human
  user attempting `components`).
- Interaction routing: `ComponentInvoked` reaches only the owning bot with
  `components` intent; ownership/expiry rejects.
- Response variants: ephemeral goes only to the invoker; `update_message`
  rewrites the message embeds/components and broadcasts `MessageEdit`.
- Client: embed renderer sanitizes a malicious embed; component click dispatches
  the correct frame.

## Rollout

Additive columns + one WS server event + one interaction subtype; existing
plain-text messages are unaffected (`embeds`/`components` NULL). Ship server +
client together so clients can render what bots send. Bot-library authors get a
short "embeds & components" doc mirroring Discord field names for easy porting.
