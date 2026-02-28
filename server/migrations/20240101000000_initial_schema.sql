-- Initial Schema for Kaiku
-- Migration: 20240101000000_initial_schema

-- ============================================================================
-- Custom Types
-- ============================================================================

CREATE TYPE auth_method AS ENUM ('local', 'oidc');
CREATE TYPE user_status AS ENUM ('online', 'away', 'busy', 'offline');
CREATE TYPE channel_type AS ENUM ('text', 'voice', 'dm');

-- ============================================================================
-- Users
-- ============================================================================

CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username VARCHAR(32) UNIQUE NOT NULL,
    display_name VARCHAR(64) NOT NULL,
    email VARCHAR(255) UNIQUE,
    password_hash VARCHAR(255),
    auth_method auth_method NOT NULL DEFAULT 'local',
    external_id VARCHAR(255) UNIQUE,
    avatar_url TEXT,
    status user_status NOT NULL DEFAULT 'offline',
    mfa_secret VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    CONSTRAINT username_format CHECK (username ~ '^[a-z0-9_]{3,32}$'),
    CONSTRAINT local_user_has_password CHECK (
        auth_method != 'local' OR password_hash IS NOT NULL
    ),
    CONSTRAINT oidc_user_has_external_id CHECK (
        auth_method != 'oidc' OR external_id IS NOT NULL
    )
);

CREATE INDEX idx_users_username ON users(username);
CREATE INDEX idx_users_email ON users(email);
CREATE INDEX idx_users_external_id ON users(external_id);

-- ============================================================================
-- User E2EE Keys (for Olm/Megolm)
-- ============================================================================

CREATE TABLE user_keys (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    identity_key TEXT NOT NULL,
    signed_prekey TEXT NOT NULL,
    signed_prekey_signature TEXT NOT NULL,
    one_time_keys JSONB NOT NULL DEFAULT '[]',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================================
-- Sessions
-- ============================================================================

CREATE TABLE sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash VARCHAR(255) NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    ip_address INET,
    user_agent TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_sessions_user_id ON sessions(user_id);
CREATE INDEX idx_sessions_expires_at ON sessions(expires_at);

-- ============================================================================
-- Roles
-- ============================================================================

CREATE TABLE roles (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(64) NOT NULL UNIQUE,
    color VARCHAR(7),
    permissions JSONB NOT NULL DEFAULT '{}',
    position INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Insert default roles
INSERT INTO roles (name, color, permissions, position) VALUES
    ('admin', '#FF0000', '{"*": true}', 0),
    ('moderator', '#00FF00', '{"kick": true, "ban": true, "mute": true}', 1),
    ('member', '#808080', '{"send_messages": true, "connect": true}', 2);

-- ============================================================================
-- User Roles
-- ============================================================================

CREATE TABLE user_roles (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, role_id)
);

-- ============================================================================
-- Channel Categories
-- ============================================================================

CREATE TABLE channel_categories (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(64) NOT NULL,
    position INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================================
-- Channels
-- ============================================================================

CREATE TABLE channels (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(64) NOT NULL,
    channel_type channel_type NOT NULL,
    category_id UUID REFERENCES channel_categories(id) ON DELETE SET NULL,
    topic TEXT,
    user_limit INTEGER,
    position INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    CONSTRAINT voice_channel_limit CHECK (
        channel_type != 'voice' OR user_limit IS NULL OR user_limit > 0
    )
);

CREATE INDEX idx_channels_category ON channels(category_id);
CREATE INDEX idx_channels_type ON channels(channel_type);

-- ============================================================================
-- Channel Members
-- ============================================================================

CREATE TABLE channel_members (
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id UUID REFERENCES roles(id) ON DELETE SET NULL,
    joined_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (channel_id, user_id)
);

CREATE INDEX idx_channel_members_user ON channel_members(user_id);

-- ============================================================================
-- Messages
-- ============================================================================

CREATE TABLE messages (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    encrypted BOOLEAN NOT NULL DEFAULT FALSE,
    nonce VARCHAR(64),
    reply_to UUID REFERENCES messages(id) ON DELETE SET NULL,
    edited_at TIMESTAMPTZ,
    deleted_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_messages_channel ON messages(channel_id, created_at DESC);
CREATE INDEX idx_messages_user ON messages(user_id);
CREATE INDEX idx_messages_reply ON messages(reply_to);

-- ============================================================================
-- File Attachments
-- ============================================================================

CREATE TABLE file_attachments (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    message_id UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    filename VARCHAR(255) NOT NULL,
    mime_type VARCHAR(128) NOT NULL,
    size_bytes BIGINT NOT NULL,
    s3_key VARCHAR(512) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_attachments_message ON file_attachments(message_id);

-- ============================================================================
-- Megolm Sessions (for group E2EE)
-- ============================================================================

CREATE TABLE megolm_sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    session_id VARCHAR(255) NOT NULL,
    sender_key VARCHAR(255) NOT NULL,
    session_data TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    UNIQUE (channel_id, session_id)
);

CREATE INDEX idx_megolm_channel ON megolm_sessions(channel_id);

-- ============================================================================
-- Updated At Trigger
-- ============================================================================

CREATE OR REPLACE FUNCTION update_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER users_updated_at
    BEFORE UPDATE ON users
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at();

CREATE TRIGGER channels_updated_at
    BEFORE UPDATE ON channels
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at();

CREATE TRIGGER user_keys_updated_at
    BEFORE UPDATE ON user_keys
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at();
