-- Personal Workspaces: user-owned cross-guild channel collections
-- Design doc: docs/plans/2026-02-15-phase-6-mobile-workspaces-design.md

-- ==========================================================================
-- 1. Workspaces table
-- ==========================================================================

CREATE TABLE workspaces (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name VARCHAR(100) NOT NULL,
    icon TEXT,
    sort_order INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_workspaces_owner ON workspaces(owner_user_id);

CREATE TRIGGER workspaces_updated_at
    BEFORE UPDATE ON workspaces
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at();

-- ==========================================================================
-- 2. Workspace entries table
-- ==========================================================================

CREATE TABLE workspace_entries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    position INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Prevent duplicate channel in same workspace
CREATE UNIQUE INDEX idx_workspace_entries_unique
    ON workspace_entries(workspace_id, guild_id, channel_id);

-- Efficient position-based ordering
CREATE INDEX idx_workspace_entries_position
    ON workspace_entries(workspace_id, position);

CREATE TRIGGER workspace_entries_updated_at
    BEFORE UPDATE ON workspace_entries
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at();
