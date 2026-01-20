-- Add activity column for rich presence data
ALTER TABLE users ADD COLUMN activity JSONB;

-- Index for efficient NULL checks (users with active activity)
CREATE INDEX idx_users_activity_not_null ON users ((activity IS NOT NULL)) WHERE activity IS NOT NULL;

COMMENT ON COLUMN users.activity IS 'Rich presence activity data (game, music, etc). NULL = no activity.';
