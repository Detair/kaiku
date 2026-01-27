-- Add read state tracking for guild channels
-- Migration: 20260127000000_add_channel_read_state
-- Purpose: Enable unread message tracking and cross-device sync for guild channels

-- ============================================================================
-- Channel Read State
-- ============================================================================

CREATE TABLE channel_read_state (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    last_read_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_read_message_id UUID REFERENCES messages(id) ON DELETE SET NULL,
    PRIMARY KEY (user_id, channel_id)
);

-- Index for fast lookups by user (when loading all guild channels)
CREATE INDEX idx_channel_read_state_user ON channel_read_state(user_id);

-- Index for fast lookups by channel
CREATE INDEX idx_channel_read_state_channel ON channel_read_state(channel_id);
