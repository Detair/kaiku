-- Create user_pins table for global pins/scratchpad
CREATE TABLE user_pins (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    pin_type VARCHAR(20) NOT NULL CHECK (pin_type IN ('note', 'link', 'message')),
    content TEXT NOT NULL,
    title VARCHAR(255),
    metadata JSONB DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    position INT NOT NULL DEFAULT 0
);

-- Index for efficient user queries
CREATE INDEX idx_user_pins_user ON user_pins(user_id, position);

-- Limit pins per user (enforced at application level, but add comment)
COMMENT ON TABLE user_pins IS 'User pins for global scratchpad. Max 50 pins per user enforced in API.';
