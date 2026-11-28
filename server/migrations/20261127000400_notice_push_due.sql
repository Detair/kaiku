-- A security notice whose push was held back (another event joined it
-- within the push interval) gets one follow-up push from the minute sweep
-- once the interval has passed. `push_due_at` is when that follow-up is
-- due; NULL when nothing is held back.

ALTER TABLE security_notices ADD COLUMN push_due_at TIMESTAMPTZ;

CREATE INDEX idx_security_notices_push_due
    ON security_notices (push_due_at)
    WHERE push_due_at IS NOT NULL;
