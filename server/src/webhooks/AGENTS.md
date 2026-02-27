<!-- Parent: ../AGENTS.md -->
# Webhooks Module

## Purpose
HTTP POST delivery of platform events to bot endpoints. Covers the full lifecycle: CRUD management, event dispatch, HMAC-signed delivery with exponential backoff, and dead-letter handling.

## Key Files

- `types.rs` — Core structs. `Webhook` includes `signing_secret` (encrypted at rest). `WebhookResponse` omits it. `WebhookCreatedResponse` returns it once at creation. `WebhookDeliveryItem` is the Redis queue payload — signing secret is intentionally absent (fetched from DB at delivery time).
- `events.rs` — `BotEventType` enum (`message.created`, `member.joined`, `member.left`, `command.invoked`). Maps to the `webhook_event_type` PostgreSQL enum via `#[sqlx(type_name = "webhook_event_type")]`. `GatewayIntent` groups event types; `CommandInvoked` is always permitted regardless of declared intents.
- `handlers.rs` — REST CRUD (`POST/GET/PATCH/DELETE` under `/api/applications/{app_id}/webhooks`). All handlers call `verify_ownership` first. URL validation runs `ssrf::is_blocked_host` at registration time. Signing secrets are encrypted with `MFA_ENCRYPTION_KEY` (AES-256-GCM via `auth::mfa_crypto`) before DB insert; plaintext is returned once at creation only.
- `queries.rs` — Uses runtime `sqlx::query` / `sqlx::query_as` (not compile-time macros) to avoid requiring a live DB at compile time. `get_webhook_full` returns the signing secret; `get_webhook` does not. `find_guild_webhooks_for_event` joins `guild_bot_installations` to scope delivery to installed bots.
- `dispatch.rs` — Non-blocking entry points called from other modules. `dispatch_guild_event` fans out to all matching webhooks for a guild. `dispatch_command_event` targets a specific application. Both enqueue to Redis and swallow errors with `warn!` (never block the caller).
- `delivery.rs` — Background worker (`spawn_delivery_worker`). Pulls from `webhook:delivery:queue` (Redis list, BRPOP). Retries go into `webhook:delivery:retry` (sorted set, score = Unix timestamp). A Lua script atomically promotes due retries to avoid double-delivery. Max 5 attempts; delays: 5s, 30s, 120s, 600s, 1800s. SSRF-blocked deliveries are NOT retried.
- `signing.rs` — HMAC-SHA256. `sign_payload` returns hex. `verify_signature` uses constant-time comparison (manual XOR fold, not `==`). `generate_signing_secret` produces 32 random bytes as 64-char hex.
- `ssrf.rs` — Two-layer protection. `is_blocked_host` checks at registration (static: hostname blocklist + IP parse). `verify_resolved_ip` checks at delivery (dynamic: DNS resolution + IP validation). Returns `VerifiedUrl` with a pinned `SocketAddr`; the delivery worker builds a per-request `reqwest::Client` with `.resolve()` to pin the IP and prevent DNS rebinding between check and send.

## For AI Agents

### Webhook Lifecycle
```
Create → validate URL (ssrf::is_blocked_host) → encrypt secret → store
Dispatch → find matching webhooks → enqueue WebhookDeliveryItem to Redis
Deliver → BRPOP → ssrf::verify_resolved_ip → fetch+decrypt secret → sign → POST
Retry → schedule_retry into sorted set → promote_due_retries (Lua) → re-enqueue
Dead-letter → after 5 attempts → insert_dead_letter
```

### Security Rules
- **Signing secret never in Redis.** `WebhookDeliveryItem` has no `signing_secret` field. The worker fetches it from DB at delivery time via `queries::get_signing_secret`.
- **Signing secret encrypted at rest.** Uses `auth::mfa_crypto::{encrypt_mfa_secret, decrypt_mfa_secret}` (AES-256-GCM). Legacy plaintext secrets are handled with a `warn!` fallback, not an error.
- **SSRF is two-phase.** Registration blocks known-bad hostnames/IPs. Delivery resolves DNS and pins the IP. Both checks must pass. SSRF failures at delivery skip retry entirely.
- **Constant-time signature comparison.** `verify_signature` uses XOR fold, not string equality. Don't replace with `==`.
- **Limit: 5 webhooks per application.** Enforced in `create_webhook` handler via `config.max_webhooks_per_app`.

### Adding a New Event Type
1. Add variant to `BotEventType` in `events.rs` with matching `#[serde(rename)]` and `#[sqlx(rename)]`.
2. Add a migration to extend the `webhook_event_type` PostgreSQL enum.
3. Wire up a `dispatch_guild_event` or `dispatch_command_event` call at the relevant site.
4. Update `GatewayIntent::event_types()` if the event belongs to an existing intent group.

### Anti-Patterns
- Don't call `queries::get_webhook_full` in list/read handlers — it exposes the signing secret.
- Don't store `signing_secret` in `WebhookDeliveryItem` or any Redis payload.
- Don't use `==` for signature comparison — always use `verify_signature`.
- Don't retry SSRF-blocked deliveries — the URL is the problem, not a transient failure.
- Don't block the caller in `dispatch_*` functions — they must remain fire-and-forget.
- Don't use compile-time `sqlx::query!` macros here — this module intentionally uses runtime queries.
