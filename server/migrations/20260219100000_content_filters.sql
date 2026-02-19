-- Content Filters for Guild Moderation
-- Provides guild-configurable content filtering with built-in categories
-- and custom patterns (keyword + regex).

-- Enum for filter categories
CREATE TYPE filter_category AS ENUM (
    'slurs', 'hate_speech', 'spam', 'abusive_language', 'custom'
);

-- Enum for filter actions
CREATE TYPE filter_action AS ENUM ('block', 'log', 'warn');

-- Guild filter configuration: which built-in categories are enabled
CREATE TABLE guild_filter_configs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    category filter_category NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT true,
    action filter_action NOT NULL DEFAULT 'block',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(guild_id, category)
);

-- Custom guild-specific filter patterns
CREATE TABLE guild_filter_patterns (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    pattern TEXT NOT NULL,
    is_regex BOOLEAN NOT NULL DEFAULT false,
    description TEXT,
    enabled BOOLEAN NOT NULL DEFAULT true,
    created_by UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Moderation action log
CREATE TABLE moderation_actions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id),
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    action filter_action NOT NULL,
    category filter_category,
    matched_pattern TEXT NOT NULL,
    original_content TEXT NOT NULL,
    custom_pattern_id UUID REFERENCES guild_filter_patterns(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_guild_filter_configs_guild ON guild_filter_configs(guild_id);
CREATE INDEX idx_guild_filter_patterns_guild ON guild_filter_patterns(guild_id);
CREATE INDEX idx_moderation_actions_guild ON moderation_actions(guild_id, created_at DESC);
CREATE INDEX idx_moderation_actions_user ON moderation_actions(user_id, created_at DESC);
