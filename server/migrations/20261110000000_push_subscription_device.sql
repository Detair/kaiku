-- Per-device push registration: one subscription per (user, device).
ALTER TABLE push_subscriptions ADD COLUMN device_id UUID;
CREATE UNIQUE INDEX idx_push_subscriptions_user_device
    ON push_subscriptions(user_id, device_id) WHERE device_id IS NOT NULL;
CREATE INDEX idx_push_subscriptions_device ON push_subscriptions(device_id);
