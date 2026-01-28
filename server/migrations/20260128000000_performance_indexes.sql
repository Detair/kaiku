-- Performance Indexes Migration
-- Migration: 20260128000000_performance_indexes
-- Adds indexes to optimize common query patterns

-- ============================================================================
-- Active Messages Index
-- ============================================================================
-- Optimizes the common pattern: list messages where deleted_at IS NULL
-- Supports ORDER BY created_at DESC, id DESC for cursor-based pagination

CREATE INDEX idx_messages_active ON messages(channel_id, created_at DESC, id DESC)
    WHERE deleted_at IS NULL;

-- ============================================================================
-- Guild Member Lookup Index
-- ============================================================================
-- Optimizes permission checks that lookup by user_id first, then guild_id
-- The primary key (guild_id, user_id) is efficient for guild-first lookups
-- This index supports user-first lookups (e.g., "which guilds is user X in?")

CREATE INDEX idx_guild_members_user_guild ON guild_members(user_id, guild_id);

-- ============================================================================
-- Active Sessions Index
-- ============================================================================
-- Note: Partial indexes with NOW() comparisons are not supported in PostgreSQL
-- as the condition must be immutable. Instead, we add a covering index that
-- includes both token_hash and expires_at for efficient filtering.

CREATE INDEX idx_sessions_token_expires ON sessions(token_hash, expires_at);
