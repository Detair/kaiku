-- Information Pages Migration
-- Implements platform and guild-level information pages with acceptance tracking
-- Migration: 20260118000000_information_pages

-- ============================================================================
-- Pages Table
-- ============================================================================

-- Pages table (platform pages have guild_id = NULL)
-- Platform admin checks use existing system_admins table
CREATE TABLE pages (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    guild_id UUID REFERENCES guilds(id) ON DELETE CASCADE,

    title VARCHAR(100) NOT NULL,
    slug VARCHAR(100) NOT NULL
        CONSTRAINT slug_format CHECK (slug ~ '^[a-z0-9]([a-z0-9\-]*[a-z0-9])?$'),

    content TEXT NOT NULL,
    content_hash VARCHAR(64) NOT NULL,  -- SHA-256 for version tracking

    position INT NOT NULL DEFAULT 0,
    requires_acceptance BOOLEAN NOT NULL DEFAULT FALSE,

    created_by UUID NOT NULL REFERENCES users(id),
    updated_by UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMPTZ
);

-- Unique slug per guild (or platform)
-- Platform pages use a nil UUID as the discriminator
CREATE UNIQUE INDEX idx_pages_unique_slug
    ON pages(COALESCE(guild_id, '00000000-0000-0000-0000-000000000000'::uuid), slug)
    WHERE deleted_at IS NULL;

-- Fast lookup by position for listing
CREATE INDEX idx_pages_guild_position ON pages(guild_id, position) WHERE deleted_at IS NULL;
CREATE INDEX idx_pages_platform_position ON pages(position) WHERE guild_id IS NULL AND deleted_at IS NULL;

-- ============================================================================
-- Page Audit Log
-- ============================================================================

CREATE TABLE page_audit_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    page_id UUID NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    action VARCHAR(20) NOT NULL,  -- 'create', 'update', 'delete', 'restore'
    actor_id UUID NOT NULL REFERENCES users(id),
    previous_content_hash VARCHAR(64),
    ip_address INET,
    user_agent TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT valid_action CHECK (action IN ('create', 'update', 'delete', 'restore'))
);

CREATE INDEX idx_page_audit_log_page ON page_audit_log(page_id);
CREATE INDEX idx_page_audit_log_actor ON page_audit_log(actor_id);
CREATE INDEX idx_page_audit_log_created ON page_audit_log(created_at DESC);

-- ============================================================================
-- User Acceptance Tracking
-- ============================================================================

CREATE TABLE page_acceptances (
    user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    page_id UUID REFERENCES pages(id) ON DELETE CASCADE,
    content_hash VARCHAR(64) NOT NULL,  -- Hash at time of acceptance
    accepted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, page_id)
);

CREATE INDEX idx_page_acceptances_page ON page_acceptances(page_id);

-- ============================================================================
-- Role-Based Visibility for Guild Pages
-- ============================================================================

CREATE TABLE page_visibility (
    page_id UUID REFERENCES pages(id) ON DELETE CASCADE,
    role_id UUID REFERENCES guild_roles(id) ON DELETE CASCADE,
    PRIMARY KEY (page_id, role_id)
);

CREATE INDEX idx_page_visibility_role ON page_visibility(role_id);

-- Trigger: Ensure role belongs to same guild as page
CREATE OR REPLACE FUNCTION check_page_visibility_guild()
RETURNS TRIGGER AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pages p
        JOIN guild_roles gr ON gr.guild_id = p.guild_id
        WHERE p.id = NEW.page_id AND gr.id = NEW.role_id
    ) THEN
        RAISE EXCEPTION 'Role must belong to same guild as page';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER page_visibility_guild_check
    BEFORE INSERT OR UPDATE ON page_visibility
    FOR EACH ROW EXECUTE FUNCTION check_page_visibility_guild();

-- ============================================================================
-- Update Timestamp Trigger
-- ============================================================================

CREATE OR REPLACE FUNCTION update_pages_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER pages_updated_at
    BEFORE UPDATE ON pages
    FOR EACH ROW EXECUTE FUNCTION update_pages_updated_at();
