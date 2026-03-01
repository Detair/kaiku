-- First User Setup Migration
-- Creates server_config table for platform-level configuration
-- Migration: 20260129000000_first_user_setup

-- ============================================================================
-- Server Configuration Table
-- ============================================================================

-- Server-level configuration (distinct from system_settings which is for security policies)
CREATE TABLE server_config (
    key VARCHAR(64) PRIMARY KEY,
    value JSONB NOT NULL,
    updated_by UUID REFERENCES users(id) ON DELETE SET NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Initialize with default values
INSERT INTO server_config (key, value) VALUES
    ('setup_complete', 'false'::jsonb),
    ('server_name', '"Kaiku Server"'::jsonb),
    ('registration_policy', '"open"'::jsonb),  -- 'open', 'invite_only', 'closed'
    ('terms_url', 'null'::jsonb),
    ('privacy_url', 'null'::jsonb);

-- Note: No additional index needed on key column since it's already the PRIMARY KEY

-- For existing installations: mark setup as complete if users exist
UPDATE server_config
SET value = 'true'::jsonb
WHERE key = 'setup_complete'
  AND EXISTS (SELECT 1 FROM users LIMIT 1);
