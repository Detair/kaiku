-- v1.0 login (L1a, Task 4): coalesced security notices in the Kaiku DM
-- (contract C28, spec B.5). One notice per (recipient, subject, event type)
-- and 10-minute window; the events stay individual in `security_events`.

CREATE TABLE security_notices (
    id UUID PRIMARY KEY,
    recipient_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    subject_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    severity TEXT NOT NULL CHECK (severity IN ('high', 'normal')),
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    message_id UUID NOT NULL UNIQUE REFERENCES messages(id) ON DELETE CASCADE,
    count INT NOT NULL DEFAULT 1 CHECK (count >= 1),
    first_at TIMESTAMPTZ NOT NULL,
    last_at TIMESTAMPTZ NOT NULL,
    last_pushed_at TIMESTAMPTZ
);

CREATE TABLE security_notice_events (
    notice_id UUID NOT NULL REFERENCES security_notices(id) ON DELETE CASCADE,
    event_id UUID NOT NULL REFERENCES security_events(id) ON DELETE CASCADE,
    PRIMARY KEY (notice_id, event_id)
);

CREATE INDEX idx_security_notices_coalesce
    ON security_notices (recipient_id, subject_id, event_type, first_at DESC);
CREATE INDEX idx_security_notice_events_event ON security_notice_events (event_id);
