-- Thread support: parent_id on messages + thread read state

ALTER TABLE messages ADD COLUMN parent_id UUID REFERENCES messages(id) ON DELETE CASCADE;
ALTER TABLE messages ADD COLUMN thread_reply_count INT NOT NULL DEFAULT 0;
ALTER TABLE messages ADD COLUMN thread_last_reply_at TIMESTAMPTZ;

-- Thread replies lookup (chronological)
CREATE INDEX idx_messages_parent_id ON messages(parent_id, created_at ASC)
    WHERE parent_id IS NOT NULL;

-- Top-level messages only (for channel feed)
CREATE INDEX idx_messages_channel_toplevel ON messages(channel_id, created_at DESC, id DESC)
    WHERE parent_id IS NULL AND deleted_at IS NULL;

-- Per-user per-thread read tracking
CREATE TABLE thread_read_state (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    thread_parent_id UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    last_read_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_read_message_id UUID REFERENCES messages(id) ON DELETE SET NULL,
    PRIMARY KEY (user_id, thread_parent_id)
);

CREATE INDEX idx_thread_read_state_user ON thread_read_state(user_id);
CREATE INDEX idx_thread_read_state_thread ON thread_read_state(thread_parent_id);
