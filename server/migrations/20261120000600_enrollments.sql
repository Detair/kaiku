-- v1.0 login (L1a, Task 11): enrolment of one key device (spec SM2, C26).
-- The setup token (256-bit, 10 min, single use), the 6-digit code and the
-- claim secret are stored only as keyed hashes (B.2 "Data model").

CREATE TABLE enrollments (
    id UUID PRIMARY KEY,
    variant TEXT NOT NULL CHECK (variant IN ('own', 'admin', 'request', 'registration', 'cli')),
    subject_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    issuer_kind TEXT NOT NULL CHECK (issuer_kind IN ('key_device', 'session', 'request', 'ticket', 'cli')),
    issuer_session_id UUID,
    issuer_device_id UUID,
    issuer_passkey_id UUID,
    issuer_grant_id UUID,
    issuer_request_id UUID,
    issuer_ticket_id UUID,
    setup_token_hash BYTEA UNIQUE NOT NULL CHECK (octet_length(setup_token_hash) = 32),
    nonce BYTEA NOT NULL CHECK (octet_length(nonce) = 32),
    code_hash BYTEA,
    code_expires_at TIMESTAMPTZ,
    tries SMALLINT NOT NULL DEFAULT 0 CHECK (tries BETWEEN 0 AND 3),
    claim_secret_hash BYTEA,
    authenticator_type TEXT CHECK (authenticator_type IN ('key_device', 'passkey')),
    claimed_device_id UUID,
    claimed_public_key BYTEA,
    claimed_key_hash BYTEA,
    claimed_label TEXT CHECK (claimed_label IS NULL OR char_length(claimed_label) BETWEEN 1 AND 64),
    claimed_attestation BYTEA[],
    claimed_properties JSONB,
    claimed_uv_class TEXT CHECK (claimed_uv_class IS NULL OR claimed_uv_class IN ('per_use', 'window_10s')),
    claimed_api_level INT,
    -- Filled by L1b Task 23 (passkey claims); no passkey code in L1a.
    claimed_passkey JSONB,
    claimant_check TEXT NOT NULL DEFAULT 'none' CHECK (claimant_check IN ('none', 'in_person', 'called_back')),
    claimant_note TEXT,
    lost_device_ids UUID[] NOT NULL DEFAULT '{}',
    request_id UUID,
    state TEXT NOT NULL CHECK (state IN ('issued', 'claimed', 'done', 'cancelled', 'expired', 'failed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    claimed_at TIMESTAMPTZ,
    done_at TIMESTAMPTZ,
    ended_at TIMESTAMPTZ,
    CHECK (expires_at <= created_at + INTERVAL '10 minutes'),
    CHECK (code_expires_at IS NULL OR claimed_at IS NOT NULL)
);

CREATE INDEX idx_enrollments_open_issuer ON enrollments (issuer_kind, issuer_session_id, issuer_device_id)
    WHERE state IN ('issued', 'claimed');
CREATE INDEX idx_enrollments_subject ON enrollments (subject_id) WHERE state IN ('issued', 'claimed');
