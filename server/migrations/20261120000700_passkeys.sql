-- Passkeys (LP, C29; spec B.1, SM4). One row per WebAuthn credential.
-- `state` follows SM4: `pending` until "Finish" (L1b account requests, with
-- `ready_at`), `active`, `revoked` (kept for audit, DC8). `enrollment_id`
-- links a row created by a setup-link enrolment (SM2, L1b).
CREATE TABLE passkeys (
    id              UUID PRIMARY KEY,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    credential_id   BYTEA NOT NULL UNIQUE,
    credential      JSONB NOT NULL,
    label           TEXT NOT NULL CHECK (char_length(label) BETWEEN 1 AND 64),
    aaguid          UUID,
    backup_eligible BOOLEAN NOT NULL,
    backup_state    BOOLEAN NOT NULL,
    transports      TEXT[] NOT NULL DEFAULT '{}',
    state           TEXT NOT NULL DEFAULT 'active'
                    CHECK (state IN ('pending', 'active', 'revoked')),
    ready_at        TIMESTAMPTZ,
    enrollment_id   UUID REFERENCES enrollments(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at    TIMESTAMPTZ,
    revoked_at      TIMESTAMPTZ,
    revoke_reason   TEXT,
    CHECK ((state = 'revoked') = (revoked_at IS NOT NULL)),
    CHECK ((state = 'pending') = (ready_at IS NOT NULL))
);

CREATE INDEX idx_passkeys_user_live ON passkeys (user_id) WHERE state <> 'revoked';

-- The WebAuthn user handle: random v4, set at first use, never the user id.
ALTER TABLE users ADD COLUMN webauthn_user_handle UUID UNIQUE;
