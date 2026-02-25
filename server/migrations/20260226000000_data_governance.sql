-- Data Governance: export pipeline, account deletion lifecycle, FK fixes
-- Phase 5: SaaS Trust & Data Governance

-- ============================================================================
-- 1. Data Export Jobs
-- ============================================================================

CREATE TABLE data_export_jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status VARCHAR(20) NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'processing', 'completed', 'failed', 'expired')),
    s3_key TEXT,
    file_size_bytes BIGINT,
    error_message TEXT,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ
);

CREATE INDEX idx_data_export_jobs_user_status ON data_export_jobs(user_id, status);
CREATE INDEX idx_data_export_jobs_pending ON data_export_jobs(status) WHERE status = 'pending';
CREATE UNIQUE INDEX idx_data_export_jobs_one_active
    ON data_export_jobs(user_id) WHERE status IN ('pending', 'processing');

-- ============================================================================
-- 2. Account Deletion Columns
-- ============================================================================

ALTER TABLE users
    ADD COLUMN deletion_requested_at TIMESTAMPTZ,
    ADD COLUMN deletion_scheduled_at TIMESTAMPTZ;

-- ============================================================================
-- 3. Messages FK: CASCADE → SET NULL (preserve messages, anonymize author)
-- ============================================================================

ALTER TABLE messages ALTER COLUMN user_id DROP NOT NULL;
ALTER TABLE messages DROP CONSTRAINT messages_user_id_fkey;
ALTER TABLE messages ADD CONSTRAINT messages_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE SET NULL;

-- ============================================================================
-- 4. Fix bare FKs that default to RESTRICT (would block user deletion)
-- ============================================================================

-- guild_filter_patterns.created_by → SET NULL
ALTER TABLE guild_filter_patterns ALTER COLUMN created_by DROP NOT NULL;
ALTER TABLE guild_filter_patterns DROP CONSTRAINT guild_filter_patterns_created_by_fkey;
ALTER TABLE guild_filter_patterns ADD CONSTRAINT guild_filter_patterns_created_by_fkey
    FOREIGN KEY (created_by) REFERENCES users(id) ON DELETE SET NULL;

-- moderation_actions.user_id → SET NULL (preserve mod action records)
ALTER TABLE moderation_actions ALTER COLUMN user_id DROP NOT NULL;
ALTER TABLE moderation_actions DROP CONSTRAINT moderation_actions_user_id_fkey;
ALTER TABLE moderation_actions ADD CONSTRAINT moderation_actions_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE SET NULL;

-- guild_emojis.uploaded_by → SET NULL
ALTER TABLE guild_emojis ALTER COLUMN uploaded_by DROP NOT NULL;
ALTER TABLE guild_emojis DROP CONSTRAINT guild_emojis_uploaded_by_fkey;
ALTER TABLE guild_emojis ADD CONSTRAINT guild_emojis_uploaded_by_fkey
    FOREIGN KEY (uploaded_by) REFERENCES users(id) ON DELETE SET NULL;

-- pages.created_by → SET NULL
ALTER TABLE pages ALTER COLUMN created_by DROP NOT NULL;
ALTER TABLE pages DROP CONSTRAINT pages_created_by_fkey;
ALTER TABLE pages ADD CONSTRAINT pages_created_by_fkey
    FOREIGN KEY (created_by) REFERENCES users(id) ON DELETE SET NULL;

-- pages.updated_by → SET NULL
ALTER TABLE pages ALTER COLUMN updated_by DROP NOT NULL;
ALTER TABLE pages DROP CONSTRAINT pages_updated_by_fkey;
ALTER TABLE pages ADD CONSTRAINT pages_updated_by_fkey
    FOREIGN KEY (updated_by) REFERENCES users(id) ON DELETE SET NULL;

-- page_audit_log.actor_id → SET NULL
ALTER TABLE page_audit_log ALTER COLUMN actor_id DROP NOT NULL;
ALTER TABLE page_audit_log DROP CONSTRAINT page_audit_log_actor_id_fkey;
ALTER TABLE page_audit_log ADD CONSTRAINT page_audit_log_actor_id_fkey
    FOREIGN KEY (actor_id) REFERENCES users(id) ON DELETE SET NULL;

-- system_audit_log.actor_id → SET NULL
ALTER TABLE system_audit_log ALTER COLUMN actor_id DROP NOT NULL;
ALTER TABLE system_audit_log DROP CONSTRAINT system_audit_log_actor_id_fkey;
ALTER TABLE system_audit_log ADD CONSTRAINT system_audit_log_actor_id_fkey
    FOREIGN KEY (actor_id) REFERENCES users(id) ON DELETE SET NULL;

-- system_announcements.author_id → SET NULL
ALTER TABLE system_announcements ALTER COLUMN author_id DROP NOT NULL;
ALTER TABLE system_announcements DROP CONSTRAINT system_announcements_author_id_fkey;
ALTER TABLE system_announcements ADD CONSTRAINT system_announcements_author_id_fkey
    FOREIGN KEY (author_id) REFERENCES users(id) ON DELETE SET NULL;

-- pending_approvals.requested_by → CASCADE (approval meaningless without requester)
ALTER TABLE pending_approvals DROP CONSTRAINT pending_approvals_requested_by_fkey;
ALTER TABLE pending_approvals ADD CONSTRAINT pending_approvals_requested_by_fkey
    FOREIGN KEY (requested_by) REFERENCES users(id) ON DELETE CASCADE;

-- break_glass_requests.admin_id → SET NULL (preserve audit trail)
ALTER TABLE break_glass_requests ALTER COLUMN admin_id DROP NOT NULL;
ALTER TABLE break_glass_requests DROP CONSTRAINT break_glass_requests_admin_id_fkey;
ALTER TABLE break_glass_requests ADD CONSTRAINT break_glass_requests_admin_id_fkey
    FOREIGN KEY (admin_id) REFERENCES users(id) ON DELETE SET NULL;

-- break_glass_cooldowns.admin_id → CASCADE (PK, cooldown meaningless without user)
ALTER TABLE break_glass_cooldowns DROP CONSTRAINT break_glass_cooldowns_admin_id_fkey;
ALTER TABLE break_glass_cooldowns ADD CONSTRAINT break_glass_cooldowns_admin_id_fkey
    FOREIGN KEY (admin_id) REFERENCES users(id) ON DELETE CASCADE;

-- guild_bot_installations.installed_by → CASCADE
ALTER TABLE guild_bot_installations DROP CONSTRAINT guild_bot_installations_installed_by_fkey;
ALTER TABLE guild_bot_installations ADD CONSTRAINT guild_bot_installations_installed_by_fkey
    FOREIGN KEY (installed_by) REFERENCES users(id) ON DELETE CASCADE;

-- prekeys.claimed_by → SET NULL (already nullable)
ALTER TABLE prekeys DROP CONSTRAINT IF EXISTS prekeys_claimed_by_fkey;
ALTER TABLE prekeys ADD CONSTRAINT prekeys_claimed_by_fkey
    FOREIGN KEY (claimed_by) REFERENCES users(id) ON DELETE SET NULL;
