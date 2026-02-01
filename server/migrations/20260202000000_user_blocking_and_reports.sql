-- User Blocking Indexes & User Reports Table
--
-- Part 1: Indexes for efficient block lookups
-- Part 2: User reporting system with admin queue

-- ============================================================================
-- Part 1: Block Indexes
-- ============================================================================

-- Fast lookup: "who has this user blocked?"
CREATE INDEX idx_friendships_blocked_by
    ON friendships(requester_id) WHERE status = 'blocked';

-- Fast lookup: "is user A blocking user B?" (exact pair)
CREATE INDEX idx_friendships_block_pair
    ON friendships(requester_id, addressee_id) WHERE status = 'blocked';

-- ============================================================================
-- Part 2: User Reports
-- ============================================================================

CREATE TYPE report_category AS ENUM (
    'harassment',
    'spam',
    'inappropriate_content',
    'impersonation',
    'other'
);

CREATE TYPE report_status AS ENUM (
    'pending',
    'reviewing',
    'resolved',
    'dismissed'
);

CREATE TYPE report_target_type AS ENUM (
    'user',
    'message'
);

CREATE TABLE user_reports (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    reporter_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    target_type report_target_type NOT NULL,
    target_user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    target_message_id UUID REFERENCES messages(id) ON DELETE SET NULL,
    category report_category NOT NULL,
    description TEXT,
    status report_status NOT NULL DEFAULT 'pending',
    assigned_admin_id UUID REFERENCES users(id) ON DELETE SET NULL,
    resolution_action VARCHAR(32),
    resolution_note TEXT,
    resolved_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes for common queries
CREATE INDEX idx_reports_status ON user_reports(status, created_at DESC);
CREATE INDEX idx_reports_reporter ON user_reports(reporter_id, created_at DESC);
CREATE INDEX idx_reports_target_user ON user_reports(target_user_id);

-- Prevent duplicate active reports from same reporter for same target
CREATE UNIQUE INDEX idx_reports_no_duplicate_active
    ON user_reports(reporter_id, target_type, target_user_id)
    WHERE status IN ('pending', 'reviewing');

-- Auto-update updated_at timestamp
CREATE TRIGGER user_reports_updated_at
    BEFORE UPDATE ON user_reports FOR EACH ROW EXECUTE FUNCTION update_updated_at();
