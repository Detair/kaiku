-- Membership screening (rules gate).

-- Per-guild screening toggle + rules text.
ALTER TABLE guilds ADD COLUMN screening_enabled BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE guild_screening_rules (
    guild_id   UUID PRIMARY KEY REFERENCES guilds(id) ON DELETE CASCADE,
    rules_md   TEXT NOT NULL,
    updated_by UUID REFERENCES users(id) ON DELETE SET NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Membership state. Existing members default to 'active' (no backfill needed);
-- only new joins to a screening-enabled guild become 'pending'.
ALTER TABLE guild_members
    ADD COLUMN membership_state VARCHAR(16) NOT NULL DEFAULT 'active';
ALTER TABLE guild_members ADD COLUMN accepted_rules_at TIMESTAMPTZ;

ALTER TABLE guild_members
    ADD CONSTRAINT guild_member_state_valid
    CHECK (membership_state IN ('active', 'pending'));
