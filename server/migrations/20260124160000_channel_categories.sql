-- Channel Categories Enhancement: 2-level nesting and user collapse state
-- Migration: 20260124160000_channel_categories

-- ============================================================================
-- Add parent_id for 2-level nesting (Category -> Subcategory -> Channels)
-- ============================================================================

ALTER TABLE channel_categories
ADD COLUMN IF NOT EXISTS parent_id UUID REFERENCES channel_categories(id) ON DELETE CASCADE;

CREATE INDEX IF NOT EXISTS idx_categories_parent ON channel_categories(parent_id);

-- ============================================================================
-- Ensure guild_id index exists (may have been created in guilds migration)
-- ============================================================================

CREATE INDEX IF NOT EXISTS idx_categories_guild ON channel_categories(guild_id);

-- ============================================================================
-- Add constraint to prevent deep nesting (max 2 levels)
-- A category can only have a parent if that parent has no parent itself
-- ============================================================================

-- First, create a function to check nesting depth
CREATE OR REPLACE FUNCTION check_category_nesting_depth()
RETURNS TRIGGER AS $$
BEGIN
    -- If parent_id is being set, verify the parent has no parent itself
    IF NEW.parent_id IS NOT NULL THEN
        IF EXISTS (
            SELECT 1 FROM channel_categories
            WHERE id = NEW.parent_id AND parent_id IS NOT NULL
        ) THEN
            RAISE EXCEPTION 'Channel categories cannot be nested more than 2 levels deep';
        END IF;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create trigger to enforce nesting constraint on insert and update
DROP TRIGGER IF EXISTS check_category_nesting ON channel_categories;
CREATE TRIGGER check_category_nesting
    BEFORE INSERT OR UPDATE OF parent_id ON channel_categories
    FOR EACH ROW
    EXECUTE FUNCTION check_category_nesting_depth();

-- ============================================================================
-- User Category Collapse State
-- Stores per-user collapse preferences for categories
-- ============================================================================

CREATE TABLE IF NOT EXISTS user_category_collapse (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    category_id UUID NOT NULL REFERENCES channel_categories(id) ON DELETE CASCADE,
    collapsed BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (user_id, category_id)
);

-- Index for efficient lookup by user
CREATE INDEX IF NOT EXISTS idx_user_category_collapse_user ON user_category_collapse(user_id);
