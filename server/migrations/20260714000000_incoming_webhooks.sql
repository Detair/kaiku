-- Incoming (Discord-compatible) webhooks: external services POST to
-- /api/webhooks/{id}/{token} to create messages in a channel.
-- Distinct from the existing outgoing `webhooks` table (bot event delivery).

CREATE TABLE incoming_webhooks (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    guild_id    UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    channel_id  UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    name        VARCHAR(80) NOT NULL,
    avatar_url  TEXT,
    -- Encrypted at rest (AES-256-GCM via MFA_ENCRYPTION_KEY, same scheme as
    -- webhooks.signing_secret) and decrypted on authenticated management
    -- reads — Discord parity requires returning the token (copy-URL UX).
    token       TEXT NOT NULL UNIQUE,
    created_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_incoming_webhooks_channel ON incoming_webhooks(channel_id);
CREATE INDEX idx_incoming_webhooks_guild ON incoming_webhooks(guild_id);

-- Per-message author snapshot (Discord semantics: a message keeps the
-- name/avatar — and webhook_id, for client badging — it was posted with,
-- even after the webhook is renamed or deleted; hence no FK on webhook_id).
ALTER TABLE messages
    ADD COLUMN webhook_id UUID,
    ADD COLUMN webhook_username VARCHAR(80),
    ADD COLUMN webhook_avatar_url TEXT;

CREATE INDEX idx_messages_webhook ON messages(webhook_id) WHERE webhook_id IS NOT NULL;
