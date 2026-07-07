# Mobile Push Notifications — Design

> Status: design, awaiting approval. 2026-07-07, gap #4. Provider decision:
> **provider-agnostic dispatcher, UnifiedPush/ntfy as the default backend**, FCM
> as a pluggable backend later. Content-free pushes preserve E2EE.

## Context

The Android app has no push path today (no FCM in `mobile/android`), so it can
only notify while foregrounded — users won't return to an app that can't reach
them. Two forces shape the design: Kaiku's **E2EE DMs** (a push provider must
never see message content) and its **sovereignty thesis** (avoid a hard Google
dependency). The answer to both: send a **content-free wake signal**, let the
app fetch and decrypt locally, and abstract the provider so a self-hostable
transport (UnifiedPush/ntfy) is the default with FCM as an opt-in backend.

## Goals

- Deliver a push when a user should be notified while the app is
  backgrounded/closed: DM message, @mention, and incoming DM voice call.
- **Content-free**: the push carries only a type + minimal routing (e.g.
  channel_id), never message text — the app wakes, syncs over its normal
  authenticated channel, and renders the (decrypted) notification locally.
- Respect the user's existing DND / quiet-hours / focus-engine settings and
  per-guild mute state — those already gate in-app notifications; reuse them.
- Provider-agnostic: one dispatcher, pluggable backends (UnifiedPush first).

## Non-Goals (YAGNI)

- iOS push (APNs) — arrives with the iOS client (deferred).
- Rich/actionable notifications (reply-from-notification) — v2.
- Web push — separate track; the abstraction leaves room.

## Data Model

```sql
-- migration: 20260707000003_push_subscriptions.sql
CREATE TABLE push_subscriptions (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider   VARCHAR(32) NOT NULL,       -- 'unifiedpush' | 'fcm' | ...
    -- UnifiedPush: the distributor endpoint URL. FCM: the registration token.
    endpoint   TEXT NOT NULL,
    -- Optional client public key for payload encryption (UnifiedPush webpush).
    public_key TEXT,
    auth_key   TEXT,
    device_label VARCHAR(64),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at TIMESTAMPTZ,
    UNIQUE (user_id, endpoint)
);
```

Multiple rows per user (multi-device). `ON DELETE CASCADE` drops subscriptions
when an account is deleted (GDPR-consistent).

## Provider Abstraction

A `push` module with a `PushProvider` trait:

```rust
#[async_trait]
trait PushProvider {
    fn name(&self) -> &'static str;
    async fn send(&self, sub: &PushSubscription, signal: &WakeSignal)
        -> Result<Delivery, PushError>; // Delivery::Gone => prune the sub
}
```

- **UnifiedPushProvider** (default): POST the (optionally webpush-encrypted)
  wake signal to the subscription `endpoint` (the user's distributor, e.g. a
  self-hosted ntfy). Endpoints are user-supplied URLs → **reuse the existing
  `webhooks::ssrf` guard** (private-IP/DNS-rebind protection) before every send.
- **FcmProvider** (later): POST a `data`-only message (no `notification` block,
  so the OS doesn't render provider-side content) to FCM with the token.

The dispatcher selects the provider per subscription row, sends concurrently,
and prunes subscriptions a backend reports as `Gone`.

## Wake Signal (content-free)

```
{ "t": "dm" | "mention" | "call", "channel_id": "...", "count": 3 }
```

No sender name, no text. For E2EE DMs the app must decrypt locally; for
non-E2EE guild mentions the app still fetches over its authenticated API rather
than trusting push contents (uniform path, minimal push surface, no content
leak to the transport).

## Trigger & Gating

Hook the points that already emit in-app notifications (message create →
`MessageNew`, mention detection, DM call ring). For each recipient who is **not
currently connected via WebSocket** (or is connected but backgrounded, signaled
by the client), the notification service:

1. Checks the user's existing notification gates: DND, quiet-hours, focus mode,
   per-guild/channel mute. If suppressed, no push.
2. Loads the user's `push_subscriptions` and dispatches the wake signal.

Runs on the existing Redis-backed async worker pattern (like webhook delivery)
so push latency never blocks message handling.

## Android Client

- On login / notification-permission grant, the app registers with its
  UnifiedPush distributor and `POST`s the endpoint to
  `POST /api/me/push-subscriptions`; unregisters on logout
  (`DELETE …/{id}`).
- On receiving a wake signal, a background worker syncs the referenced channel
  over the normal authenticated (and E2EE-decrypting) path and posts a local
  Android notification. Tapping deep-links into the channel.
- Reuses the existing `VoiceCallService` foreground-service pattern for call
  pushes (full-screen incoming-call intent).

## API Surface

`POST /api/me/push-subscriptions {provider, endpoint, public_key?, device_label?}`,
`GET /api/me/push-subscriptions`, `DELETE /api/me/push-subscriptions/{id}` — all
under the authenticated user; a user only manages their own devices.

## Edge Cases

- Stale endpoint (distributor uninstalled) → backend returns `Gone` → prune.
- User on 5 devices, 2 unreachable → send to all, prune failures, don't fail the
  batch.
- User actively connected on desktop but phone backgrounded → still push the
  phone (per-subscription, not per-user online state).
- SSRF: a malicious `endpoint` pointing at internal infra → blocked by the
  shared SSRF guard, identical to webhooks.
- Rate/dedupe: collapse a burst of mentions in one channel into a single wake
  signal with a `count`.

## Testing

- Dispatcher selects the right provider, sends concurrently, prunes `Gone`.
- SSRF guard rejects an internal endpoint.
- Gating: DND/quiet-hours/mute suppress the push; a connected+foreground session
  suppresses; backgrounded does not.
- Wake signal carries no message content (assert the payload shape).
- Subscription CRUD is user-scoped (can't register/delete another user's device).

## Rollout

Server dispatcher + `push_subscriptions` table + endpoints first (inert without
clients). Android client second. UnifiedPush ships as default; FCM backend lands
behind config when/if Play-Store reach is prioritized. iOS/APNs slots into the
same trait later. This is the highest-infra feature of the five and the one with
an external-service tail — worth shipping the server abstraction early so client
work can proceed in parallel.
