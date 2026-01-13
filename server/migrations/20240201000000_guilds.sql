-- Phase 3: Guild Architecture & Social Features
-- Migration: 20240201000000_guilds

-- ============================================================================
-- Guilds (Servers)
-- ============================================================================

CREATE TABLE guilds (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(100) NOT NULL,
    owner_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    icon_url TEXT,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_guilds_owner ON guilds(owner_id);

-- ============================================================================
-- Guild Members
-- ============================================================================

CREATE TABLE guild_members (
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    nickname VARCHAR(64),
    joined_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (guild_id, user_id)
);

CREATE INDEX idx_guild_members_user ON guild_members(user_id);

-- ============================================================================
-- Add guild_id to existing tables
-- ============================================================================

ALTER TABLE channels ADD COLUMN guild_id UUID REFERENCES guilds(id) ON DELETE CASCADE;
ALTER TABLE roles ADD COLUMN guild_id UUID REFERENCES guilds(id) ON DELETE CASCADE;
ALTER TABLE channel_categories ADD COLUMN guild_id UUID REFERENCES guilds(id) ON DELETE CASCADE;

-- ============================================================================
-- Guild-scoped roles
-- ============================================================================

CREATE TABLE guild_member_roles (
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    PRIMARY KEY (guild_id, user_id, role_id)
);

-- ============================================================================
-- Friendships
-- ============================================================================

CREATE TYPE friendship_status AS ENUM ('pending', 'accepted', 'blocked');

CREATE TABLE friendships (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    requester_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    addressee_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status friendship_status NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (requester_id, addressee_id),
    CONSTRAINT no_self_friendship CHECK (requester_id != addressee_id)
);

CREATE INDEX idx_friendships_users ON friendships(requester_id, addressee_id);
CREATE INDEX idx_friendships_addressee ON friendships(addressee_id);

-- ============================================================================
-- User status extensions
-- ============================================================================

ALTER TABLE users ADD COLUMN status_message VARCHAR(128);
ALTER TABLE users ADD COLUMN invisible BOOLEAN NOT NULL DEFAULT FALSE;

-- ============================================================================
-- DM Participants (for group DMs)
-- ============================================================================

CREATE TABLE dm_participants (
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    joined_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (channel_id, user_id)
);

CREATE INDEX idx_dm_participants_user ON dm_participants(user_id);

-- ============================================================================
-- Updated At Trigger for new tables
-- ============================================================================

CREATE TRIGGER guilds_updated_at
    BEFORE UPDATE ON guilds
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at();

CREATE TRIGGER friendships_updated_at
    BEFORE UPDATE ON friendships
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at();
