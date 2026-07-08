-- Scheduled guild events + RSVPs.
CREATE TABLE guild_events (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    guild_id      UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    -- Voice-channel event when set; external event when NULL + `location` text.
    channel_id    UUID REFERENCES channels(id) ON DELETE SET NULL,
    name          VARCHAR(100) NOT NULL,
    description   VARCHAR(1000),
    location      TEXT,
    starts_at     TIMESTAMPTZ NOT NULL,
    ends_at       TIMESTAMPTZ,
    status        VARCHAR(16) NOT NULL DEFAULT 'scheduled',
    created_by    UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    reminder_sent BOOLEAN NOT NULL DEFAULT FALSE,
    CONSTRAINT guild_event_status_valid
        CHECK (status IN ('scheduled', 'active', 'completed', 'cancelled'))
);
CREATE INDEX idx_guild_events_upcoming ON guild_events(guild_id, starts_at)
    WHERE status = 'scheduled';

CREATE TABLE guild_event_rsvps (
    event_id   UUID NOT NULL REFERENCES guild_events(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    response   VARCHAR(16) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (event_id, user_id),
    CONSTRAINT guild_event_rsvp_response_valid
        CHECK (response IN ('interested', 'going'))
);
