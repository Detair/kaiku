-- Per-device presence: a status change that has not been published yet.
-- `set_status` sets it together with `presence_seq`; a successful reconcile
-- clears it (only if no newer change happened). The presence sweeper on any
-- server reconciles users that stay marked, so the retry does not depend on
-- the socket that made the change.
ALTER TABLE users
    ADD COLUMN presence_dirty_since TIMESTAMPTZ;

CREATE INDEX idx_users_presence_dirty
    ON users (presence_dirty_since)
    WHERE presence_dirty_since IS NOT NULL;
