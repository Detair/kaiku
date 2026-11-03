-- v1.0 login (L1a, Task 8): the key-device table (spec B.2 "Data model",
-- SM4) and `may_approve` (B.1). The table exists from here on so grants,
-- counts and cascades can check device state; the routes come in Tasks 10–11.

-- Off for accounts created by admin setup (L1b), on for others.
ALTER TABLE users ADD COLUMN may_approve BOOLEAN NOT NULL DEFAULT true;

-- One table for every approval device; v1.1 adds desktop rows via
-- `platform`. Rows are created when an enrolment is confirmed, so failed
-- enrolments leave nothing; revoked rows are kept (audit).
CREATE TABLE approval_devices (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    device_id UUID NOT NULL,
    platform TEXT NOT NULL DEFAULT 'android' CHECK (platform IN ('android')),
    public_key BYTEA NOT NULL,
    public_key_hash BYTEA NOT NULL CHECK (octet_length(public_key_hash) = 32),
    attestation_chain BYTEA[] NOT NULL DEFAULT '{}',
    attestation_properties JSONB NOT NULL DEFAULT '{}',
    uv_class TEXT NOT NULL CHECK (uv_class IN ('per_use', 'window_10s')),
    api_level INT,
    label TEXT NOT NULL CHECK (char_length(label) BETWEEN 1 AND 64),
    state TEXT NOT NULL CHECK (state IN ('pending', 'active', 'revoked')),
    enrolled_via TEXT NOT NULL CHECK (enrolled_via IN ('own', 'admin', 'request', 'registration', 'cli')),
    enrollment_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    activated_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    revoke_reason TEXT CHECK (revoke_reason IN
        ('owner', 'admin', 'request', 'repudiated', 'key_invalidated', 'account_deleted', 'cli')),
    CHECK ((state = 'revoked') = (revoked_at IS NOT NULL)),
    CHECK ((state = 'revoked') = (revoke_reason IS NOT NULL)),
    CHECK (state <> 'active' OR activated_at IS NOT NULL)
);

-- One live (pending or active) row per install and user.
CREATE UNIQUE INDEX approval_devices_live ON approval_devices (user_id, device_id) WHERE state <> 'revoked';
CREATE INDEX idx_approval_devices_user ON approval_devices (user_id) WHERE state = 'active';
