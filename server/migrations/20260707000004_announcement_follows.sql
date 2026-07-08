-- Announcement follows: a follower guild subscribes one of its channels to a
-- source announcement channel, so new announcements crosspost into the target.
CREATE TABLE announcement_follows (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    target_channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    created_by        UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (source_channel_id, target_channel_id)
);
CREATE INDEX idx_announcement_follows_source ON announcement_follows(source_channel_id);

-- Marks a message that was produced by a crosspost fan-out, so it is never
-- itself re-crossposted (loop guard for A-follows-B-follows-A).
ALTER TABLE messages ADD COLUMN is_crosspost BOOLEAN NOT NULL DEFAULT FALSE;
