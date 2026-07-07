-- Forum channels: posts (each a thread starter) + a per-channel tag set.
-- (Announcement follow/crosspost tables ship in a later migration.)

-- A forum post. Its root message lives in `messages` (parent_id NULL,
-- channel_id = the forum channel); thread replies hang off it as today.
CREATE TABLE forum_posts (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    channel_id       UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    root_message_id  UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    title            VARCHAR(128) NOT NULL,
    author_id        UUID REFERENCES users(id) ON DELETE SET NULL,
    pinned           BOOLEAN NOT NULL DEFAULT FALSE,
    locked           BOOLEAN NOT NULL DEFAULT FALSE,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_activity_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_forum_posts_channel ON forum_posts(channel_id, last_activity_at DESC);

-- A small per-channel tag set; posts reference them.
CREATE TABLE forum_tags (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    name       VARCHAR(32) NOT NULL,
    emoji      VARCHAR(64),
    UNIQUE (channel_id, name)
);

CREATE TABLE forum_post_tags (
    post_id UUID NOT NULL REFERENCES forum_posts(id) ON DELETE CASCADE,
    tag_id  UUID NOT NULL REFERENCES forum_tags(id)  ON DELETE CASCADE,
    PRIMARY KEY (post_id, tag_id)
);
