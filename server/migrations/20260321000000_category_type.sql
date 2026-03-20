-- Add category_type to channel_categories
-- Allows restricting a category to text-only or voice-only channels

CREATE TYPE category_type AS ENUM ('mixed', 'text', 'voice');

ALTER TABLE channel_categories
    ADD COLUMN category_type category_type NOT NULL DEFAULT 'mixed';
