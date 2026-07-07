-- Extend the channel_type ENUM with 'forum' and 'announcement'.
--
-- IMPORTANT: `ALTER TYPE ... ADD VALUE` cannot run in the same transaction that
-- later USES the new value. sqlx wraps each migration file in a transaction, so
-- this enum extension MUST be its own migration, committed before any migration
-- or query references 'forum'/'announcement'. The forum tables live in the next
-- migration (…0003_forum_channels.sql).
ALTER TYPE channel_type ADD VALUE IF NOT EXISTS 'forum';
ALTER TYPE channel_type ADD VALUE IF NOT EXISTS 'announcement';
