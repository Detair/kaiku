-- Mobile push subscriptions (provider-agnostic; UnifiedPush default, FCM later).
CREATE TABLE push_subscriptions (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider     VARCHAR(32) NOT NULL,
    -- UnifiedPush: distributor endpoint URL. FCM: registration token.
    endpoint     TEXT NOT NULL,
    -- Optional webpush encryption keys (UnifiedPush).
    public_key   TEXT,
    auth_key     TEXT,
    device_label VARCHAR(64),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at TIMESTAMPTZ,
    UNIQUE (user_id, endpoint)
);
CREATE INDEX idx_push_subscriptions_user ON push_subscriptions(user_id);
