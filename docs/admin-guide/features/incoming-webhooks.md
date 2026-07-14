# Incoming Webhooks (Discord-Compatible)

Incoming webhooks let external services post messages into a channel by
calling a secret URL — no bot, no login. The API is **Discord-compatible**:
any tool that can post to a Discord webhook (game-server plugins, Grafana,
Uptime-Kuma, GitHub Actions notify steps, CI pipelines) works by pasting a
Kaiku webhook URL where it expects a Discord one.

## Creating a webhook

1. Open **Server Settings → Webhooks** (requires the **Manage Webhooks**
   permission or server ownership).
2. Pick a channel (text, announcement, or forum), give the webhook a name and
   optional avatar URL.
3. Copy the webhook URL:

```
https://<your-host>/api/webhooks/<id>/<token>
```

Treat the URL as a secret — anyone who has it can post to that channel.
Tokens are encrypted at rest (requires `MFA_ENCRYPTION_KEY` to be configured,
which the setup guide already mandates for MFA and bot webhooks).

## Posting messages

```bash
curl -X POST "https://<host>/api/webhooks/<id>/<token>?wait=true" \
  -H "Content-Type: application/json" \
  -d '{
    "content": "Match ended",
    "username": "Scoreboard",
    "avatar_url": "https://example.com/score.png",
    "embeds": [{
      "title": "Red team wins",
      "color": 5814783,
      "fields": [{ "name": "Score", "value": "16 : 9", "inline": true }]
    }]
  }'
```

Supported (Discord-format):

| Feature | Notes |
|---|---|
| `content` | Message text (Kaiku's 4000-char cap ⊇ Discord's 2000) |
| `username` / `avatar_url` | Per-message author override (https avatars only) |
| `embeds` | Up to 10, Discord embed object shape and size limits |
| `?wait=true` | Returns the created message (otherwise `204 No Content`) |
| `?thread_id=` | Post into an existing thread / forum post |
| `thread_name` | Create a forum post (forum channels only) |
| `GET/PATCH/DELETE …/messages/{id}` | Fetch/edit/delete a message this webhook created |
| `GET/PATCH/DELETE` on the webhook URL | Inspect/rename/delete via token |
| `POST …/slack` | Slack-format payloads (`text`, `attachments`) |

Accepted but ignored (v1): `tts`, `allowed_mentions`, `components`, `flags`,
`poll`, `applied_tags`. Unknown JSON fields never cause errors.
**Not supported:** file uploads (`files[n]`/multipart) and the `/github`
endpoint.

Webhook messages never ping `@everyone`/`@here`, and guild content filters
apply to them like any user message.

## Rate limits & abuse protection

- Per webhook: **5 posts per 2 seconds** (Discord's budget). Over-limit calls
  get a Discord-shaped `429` with `retry_after`, so Discord client libraries
  back off automatically. Tunable via `RATE_LIMIT_WEBHOOK_EXECUTE`
  (`requests,window_secs`).
- Invalid-token attempts feed the failed-auth IP blocker (same as password
  guessing: repeated failures block the IP for 15 minutes).
- The route also sits behind the general per-IP write limits.

## Configuration

| Env var | Purpose |
|---|---|
| `PUBLIC_BASE_URL` | Absolute base URL used in webhook objects' `url` field (falls back to request `Host`/`X-Forwarded-Proto` headers) |
| `RATE_LIMIT_WEBHOOK_EXECUTE` | Override the per-webhook execute budget, e.g. `10,2` |
| `MFA_ENCRYPTION_KEY` | Required to create webhooks (tokens are AES-256-GCM encrypted at rest) |

## Notes for integrators

- IDs are UUIDs (not numeric snowflakes). Tools that treat the webhook URL as
  an opaque string — practically all of them — are unaffected. Only helpers
  that *parse* Discord URLs (e.g. `discord.js` `parseWebhookURL`) reject
  non-Discord hosts anyway; use the raw HTTP endpoint in that case.
- Error responses carry Discord's numeric `code` values (`10015` unknown
  webhook, `50027` invalid token, `50006` empty message, `10008` unknown
  message) alongside Kaiku's string `error` codes.
