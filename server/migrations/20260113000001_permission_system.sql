-- Permission System Migration
-- Implements two-tier permission model: System Admin + Guild Roles
-- Migration: 20260113000000_permission_system

-- ============================================================================
-- System Admin Tables
-- ============================================================================

-- System-level admin users
CREATE TABLE system_admins (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    granted_by UUID REFERENCES users(id) ON DELETE SET NULL,
    granted_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Elevated admin sessions (sudo-style)
CREATE TABLE elevated_sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    session_id UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    ip_address INET NOT NULL,
    elevated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    reason VARCHAR(255),
    UNIQUE(session_id)
);

CREATE INDEX idx_elevated_sessions_user ON elevated_sessions(user_id);
CREATE INDEX idx_elevated_sessions_expires ON elevated_sessions(expires_at);

-- ============================================================================
-- Guild Role Tables
-- ============================================================================

-- Drop old guild_member_roles table (from 20240201000000_guilds.sql)
-- as it references the old roles table structure
DROP TABLE IF EXISTS guild_member_roles;

-- Guild roles (replaces simple roles for guild context)
CREATE TABLE guild_roles (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    name VARCHAR(64) NOT NULL,
    color VARCHAR(7),
    permissions BIGINT NOT NULL DEFAULT 0,
    position INTEGER NOT NULL DEFAULT 0,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(guild_id, name),
    CONSTRAINT guild_role_color_format CHECK (
        color IS NULL OR color ~ '^#[0-9A-Fa-f]{6}$'
    ),
    CONSTRAINT guild_role_position_valid CHECK (position >= 0)
);

CREATE INDEX idx_guild_roles_guild ON guild_roles(guild_id);
CREATE INDEX idx_guild_roles_position ON guild_roles(guild_id, position);

-- Guild member roles junction (new structure referencing guild_roles)
CREATE TABLE guild_member_roles (
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id UUID NOT NULL REFERENCES guild_roles(id) ON DELETE CASCADE,
    assigned_by UUID REFERENCES users(id) ON DELETE SET NULL,
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (guild_id, user_id, role_id)
);

CREATE INDEX idx_guild_member_roles_user ON guild_member_roles(user_id);
CREATE INDEX idx_guild_member_roles_role ON guild_member_roles(role_id);
CREATE INDEX idx_guild_member_roles_lookup ON guild_member_roles(guild_id, user_id);

-- Channel permission overrides
CREATE TABLE channel_overrides (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    role_id UUID NOT NULL REFERENCES guild_roles(id) ON DELETE CASCADE,
    allow_permissions BIGINT NOT NULL DEFAULT 0,
    deny_permissions BIGINT NOT NULL DEFAULT 0,
    UNIQUE(channel_id, role_id)
);

CREATE INDEX idx_channel_overrides_channel ON channel_overrides(channel_id);
CREATE INDEX idx_channel_overrides_role ON channel_overrides(role_id);

-- ============================================================================
-- System Settings & Audit
-- ============================================================================

-- System security settings
CREATE TABLE system_settings (
    key VARCHAR(64) PRIMARY KEY,
    value JSONB NOT NULL,
    updated_by UUID REFERENCES users(id) ON DELETE SET NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Insert default security settings
INSERT INTO system_settings (key, value) VALUES
    ('security.require_reauth_destructive', 'false'::jsonb),
    ('security.inactivity_timeout_minutes', 'null'::jsonb),
    ('security.dual_admin_approval', 'false'::jsonb),
    ('security.require_webauthn', 'false'::jsonb),
    ('security.cooling_off_hours', '4'::jsonb),
    ('break_glass.delay_minutes', '15'::jsonb),
    ('break_glass.cooldown_hours', '1'::jsonb),
    ('break_glass.max_per_admin_24h', '1'::jsonb),
    ('break_glass.max_system_24h', '3'::jsonb),
    ('break_glass.require_webauthn', 'false'::jsonb),
    ('break_glass.external_webhook', 'null'::jsonb),
    ('break_glass.review_due_hours', '48'::jsonb);

-- System audit log
CREATE TABLE system_audit_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    actor_id UUID NOT NULL REFERENCES users(id),
    action VARCHAR(64) NOT NULL,
    target_type VARCHAR(32),
    target_id UUID,
    details JSONB,
    ip_address INET,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_system_audit_actor ON system_audit_log(actor_id);
CREATE INDEX idx_system_audit_action ON system_audit_log(action, created_at DESC);
CREATE INDEX idx_system_audit_target ON system_audit_log(target_type, target_id);
CREATE INDEX idx_system_audit_created ON system_audit_log(created_at DESC);

-- System announcements
CREATE TABLE system_announcements (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    author_id UUID NOT NULL REFERENCES users(id),
    title VARCHAR(128) NOT NULL,
    content TEXT NOT NULL,
    severity VARCHAR(16) NOT NULL DEFAULT 'info',
    active BOOLEAN NOT NULL DEFAULT TRUE,
    starts_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ends_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT valid_severity CHECK (
        severity IN ('info', 'warning', 'critical', 'maintenance')
    )
);

CREATE INDEX idx_announcements_active ON system_announcements(active, starts_at, ends_at);
CREATE INDEX idx_announcements_author ON system_announcements(author_id);

-- ============================================================================
-- Approval & Break-Glass Tables
-- ============================================================================

-- Pending approvals (for dual approval flow)
CREATE TABLE pending_approvals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    action_type VARCHAR(64) NOT NULL,
    target_type VARCHAR(32) NOT NULL,
    target_id UUID NOT NULL,
    requested_by UUID NOT NULL REFERENCES users(id),
    approved_by UUID REFERENCES users(id) ON DELETE SET NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'pending',
    execute_after TIMESTAMPTZ,
    expires_at TIMESTAMPTZ NOT NULL,
    executed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT valid_approval_status CHECK (
        status IN ('pending', 'approved', 'rejected', 'expired', 'executed')
    )
);

CREATE INDEX idx_pending_approvals_status ON pending_approvals(status, expires_at);
CREATE INDEX idx_pending_approvals_requester ON pending_approvals(requested_by);
CREATE INDEX idx_pending_approvals_target ON pending_approvals(target_type, target_id);

-- Break-glass requests
CREATE TABLE break_glass_requests (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    admin_id UUID NOT NULL REFERENCES users(id),
    action_type VARCHAR(64) NOT NULL,
    target_type VARCHAR(32) NOT NULL,
    target_id UUID NOT NULL,
    justification TEXT NOT NULL CHECK (length(justification) >= 50),
    incident_ticket VARCHAR(64),
    status VARCHAR(16) NOT NULL DEFAULT 'waiting',
    execute_at TIMESTAMPTZ NOT NULL,
    blocked_by UUID REFERENCES users(id) ON DELETE SET NULL,
    block_reason TEXT,
    executed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT valid_break_glass_status CHECK (
        status IN ('waiting', 'blocked', 'executed', 'cancelled', 'expired')
    )
);

CREATE INDEX idx_break_glass_status ON break_glass_requests(status, execute_at);
CREATE INDEX idx_break_glass_admin ON break_glass_requests(admin_id);
CREATE INDEX idx_break_glass_target ON break_glass_requests(target_type, target_id);

-- Break-glass cooldowns (per admin)
CREATE TABLE break_glass_cooldowns (
    admin_id UUID PRIMARY KEY REFERENCES users(id),
    last_used_at TIMESTAMPTZ NOT NULL,
    uses_last_24h INTEGER NOT NULL DEFAULT 1
);

-- Break-glass reviews
CREATE TABLE break_glass_reviews (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    break_glass_id UUID NOT NULL REFERENCES break_glass_requests(id) ON DELETE CASCADE,
    reviewer_id UUID REFERENCES users(id) ON DELETE SET NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'pending',
    notes TEXT,
    due_at TIMESTAMPTZ NOT NULL,
    reviewed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT valid_review_status CHECK (
        status IN ('pending', 'reviewed', 'overdue', 'waived')
    )
);

CREATE INDEX idx_bg_reviews_pending ON break_glass_reviews(status, due_at)
    WHERE status = 'pending';
CREATE INDEX idx_bg_reviews_break_glass ON break_glass_reviews(break_glass_id);

-- ============================================================================
-- Guild Extensions
-- ============================================================================

-- Guild security settings
ALTER TABLE guilds ADD COLUMN IF NOT EXISTS security_settings JSONB NOT NULL DEFAULT '{
    "require_dual_owner_delete": false,
    "require_webauthn_transfer": false,
    "cooling_off_hours": 4
}'::jsonb;

-- Guild suspension fields
ALTER TABLE guilds ADD COLUMN IF NOT EXISTS suspended_at TIMESTAMPTZ;
ALTER TABLE guilds ADD COLUMN IF NOT EXISTS suspended_by UUID REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE guilds ADD COLUMN IF NOT EXISTS suspension_reason TEXT;

-- ============================================================================
-- Global User Bans
-- ============================================================================

CREATE TABLE global_bans (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    banned_by UUID REFERENCES users(id) ON DELETE SET NULL,
    reason TEXT NOT NULL,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_global_bans_expires ON global_bans(expires_at) WHERE expires_at IS NOT NULL;
CREATE INDEX idx_global_bans_banned_by ON global_bans(banned_by);
