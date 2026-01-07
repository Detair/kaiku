-- VoiceChat Development Seed Data
-- Run with: make db-seed
--
-- This seeds channel structure only.
-- For test users, run: ./scripts/create-test-users.sh
-- (This ensures passwords are properly hashed with Argon2id)

-- =============================================================================
-- Channel Categories
-- =============================================================================

INSERT INTO channel_categories (id, name, position)
VALUES
    ('01903c8b-0001-7000-8000-000000000001', 'General', 0),
    ('01903c8b-0002-7000-8000-000000000002', 'Gaming', 1),
    ('01903c8b-0003-7000-8000-000000000003', 'Development', 2)
ON CONFLICT DO NOTHING;

-- =============================================================================
-- Text Channels
-- =============================================================================

INSERT INTO channels (id, name, channel_type, category_id, topic, position)
VALUES
    -- General category
    ('01903c8c-0001-7000-8000-000000000001', 'welcome', 'text',
     '01903c8b-0001-7000-8000-000000000001', 'Welcome to VoiceChat!', 0),
    ('01903c8c-0002-7000-8000-000000000002', 'general', 'text',
     '01903c8b-0001-7000-8000-000000000001', 'General discussion', 1),
    ('01903c8c-0003-7000-8000-000000000003', 'off-topic', 'text',
     '01903c8b-0001-7000-8000-000000000001', 'Anything goes', 2),

    -- Gaming category
    ('01903c8c-0004-7000-8000-000000000004', 'looking-for-group', 'text',
     '01903c8b-0002-7000-8000-000000000002', 'Find players for your games', 0),
    ('01903c8c-0005-7000-8000-000000000005', 'game-chat', 'text',
     '01903c8b-0002-7000-8000-000000000002', 'Discuss games', 1),

    -- Development category
    ('01903c8c-0006-7000-8000-000000000006', 'frontend', 'text',
     '01903c8b-0003-7000-8000-000000000003', 'Frontend development', 0),
    ('01903c8c-0007-7000-8000-000000000007', 'backend', 'text',
     '01903c8b-0003-7000-8000-000000000003', 'Backend development', 1),
    ('01903c8c-0008-7000-8000-000000000008', 'bugs', 'text',
     '01903c8b-0003-7000-8000-000000000003', 'Bug reports and fixes', 2)
ON CONFLICT DO NOTHING;

-- =============================================================================
-- Voice Channels
-- =============================================================================

INSERT INTO channels (id, name, channel_type, category_id, user_limit, position)
VALUES
    -- General category
    ('01903c8d-0001-7000-8000-000000000001', 'Lobby', 'voice',
     '01903c8b-0001-7000-8000-000000000001', NULL, 10),

    -- Gaming category
    ('01903c8d-0002-7000-8000-000000000002', 'Game Room 1', 'voice',
     '01903c8b-0002-7000-8000-000000000002', 5, 10),
    ('01903c8d-0003-7000-8000-000000000003', 'Game Room 2', 'voice',
     '01903c8b-0002-7000-8000-000000000002', 5, 11),
    ('01903c8d-0004-7000-8000-000000000004', 'Squad (4 max)', 'voice',
     '01903c8b-0002-7000-8000-000000000002', 4, 12),

    -- Development category
    ('01903c8d-0005-7000-8000-000000000005', 'Pair Programming', 'voice',
     '01903c8b-0003-7000-8000-000000000003', 2, 10),
    ('01903c8d-0006-7000-8000-000000000006', 'Team Meeting', 'voice',
     '01903c8b-0003-7000-8000-000000000003', 10, 11)
ON CONFLICT DO NOTHING;

-- =============================================================================
-- Summary
-- =============================================================================

DO $$
DECLARE
    category_count INTEGER;
    channel_count INTEGER;
BEGIN
    SELECT COUNT(*) INTO category_count FROM channel_categories;
    SELECT COUNT(*) INTO channel_count FROM channels;

    RAISE NOTICE 'Seed data loaded:';
    RAISE NOTICE '  - Categories: %', category_count;
    RAISE NOTICE '  - Channels: %', channel_count;
    RAISE NOTICE '';
    RAISE NOTICE 'To create test users, run:';
    RAISE NOTICE '  ./scripts/create-test-users.sh';
END $$;
