-- Deleting a message deletes its files.
--
-- 1. `storage_pending_deletes` is an outbox of object keys whose rows are
--    gone. Keys are written in the transaction that removes the rows; the
--    object-cleanup worker (`chat::storage_cleanup`) deletes the objects
--    after commit, retrying failures with backoff (`attempts`,
--    `next_attempt_at`). A key leaves the table only once the object delete
--    succeeded (or the object was already gone).
-- 2. Every removed `file_attachments` row queues its original, thumbnail and
--    medium keys. Row-level triggers also fire for foreign-key cascades, so
--    channel, server and forum-post deletion are covered.
-- 3. A message that becomes soft-deleted loses its attachment rows in the
--    same transaction (author delete, moderator delete, webhook message
--    delete).
-- 4. Attachments of messages deleted before this migration are removed by
--    the worker in small batches (`storage_cleanup_backfill` records when it
--    is finished). This migration only creates tables, indexes and triggers:
--    it changes no existing row and holds its locks only briefly.

CREATE TABLE storage_pending_deletes (
    s3_key TEXT PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_attempt_at TIMESTAMPTZ
);

-- Claiming due keys (`next_attempt_at <= now()`).
CREATE INDEX idx_storage_pending_deletes_due
    ON storage_pending_deletes (next_attempt_at);

-- Single-row state of the historical clean-up.
CREATE TABLE storage_cleanup_backfill (
    id BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    finished_at TIMESTAMPTZ
);
INSERT INTO storage_cleanup_backfill DEFAULT VALUES;

CREATE OR REPLACE FUNCTION queue_attachment_objects() RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO storage_pending_deletes (s3_key)
    SELECT k FROM unnest(ARRAY[OLD.s3_key, OLD.thumbnail_s3_key, OLD.medium_s3_key]) AS k
    WHERE k IS NOT NULL AND k <> ''
    ON CONFLICT (s3_key) DO NOTHING;
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER file_attachments_queue_objects
    AFTER DELETE ON file_attachments
    FOR EACH ROW EXECUTE FUNCTION queue_attachment_objects();

CREATE OR REPLACE FUNCTION delete_attachments_of_deleted_message() RETURNS TRIGGER AS $$
BEGIN
    DELETE FROM file_attachments WHERE message_id = NEW.id;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER messages_soft_delete_attachments
    AFTER UPDATE OF deleted_at ON messages
    FOR EACH ROW
    WHEN (OLD.deleted_at IS NULL AND NEW.deleted_at IS NOT NULL)
    EXECUTE FUNCTION delete_attachments_of_deleted_message();
