-- Per-device presence: every chosen-status change bumps this sequence. The
-- Valkey mirror of `invisible` (`presence:hidden:{user_id}`) stores the
-- sequence it reflects and only accepts writes that are not older, so
-- reordered or lost writes cannot leave a stale flag behind.
ALTER TABLE users
    ADD COLUMN presence_seq BIGINT NOT NULL DEFAULT 0;
