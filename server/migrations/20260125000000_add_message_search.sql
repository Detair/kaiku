-- Add full-text search capability to messages table
-- Uses PostgreSQL tsvector with GIN index for fast search

-- Add generated tsvector column for full-text search
ALTER TABLE messages
ADD COLUMN content_search tsvector
GENERATED ALWAYS AS (to_tsvector('english', content)) STORED;

-- Create GIN index for fast full-text search
CREATE INDEX idx_messages_content_search ON messages USING GIN (content_search);

-- Add index on channel_id + created_at for efficient channel-scoped searches
-- This supports the common query pattern: search within a channel, ordered by time
CREATE INDEX idx_messages_channel_created ON messages (channel_id, created_at DESC);
