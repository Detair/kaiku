-- Create user_preferences table for syncing settings across devices
CREATE TABLE user_preferences (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    preferences JSONB NOT NULL DEFAULT '{}',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Index for querying by update time (useful for future sync operations)
CREATE INDEX idx_user_preferences_updated ON user_preferences(updated_at);

-- Trigger to auto-update updated_at timestamp
CREATE TRIGGER user_preferences_updated_at
    BEFORE UPDATE ON user_preferences
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at();

-- Comment for documentation
COMMENT ON TABLE user_preferences IS 'Stores user preferences (theme, sound, notifications) for cross-device sync';
