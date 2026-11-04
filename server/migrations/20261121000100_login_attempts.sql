-- v1.0 login (L1b Task 17): login attempts (spec SM1; C27 "Login attempts").
--
-- A PC that wants to be signed in creates an attempt and keeps its collection
-- secret: a browser holds a cookie (stored here only as a keyed hash), a native
-- client holds a verifier whose S256 challenge is stored. The QR token (256
-- bits, rotated every 60 s) is stored only as a keyed hash. A key device claims
-- the attempt with the scanned token, which creates a `login` challenge; the PC
-- collects the session after the approval.

CREATE TABLE login_attempts (
    id UUID PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('qr', 'password', 'oidc')),
    -- cookie: keyed hash of the secret; s256: the decoded S256 challenge.
    collection_hash BYTEA NOT NULL,
    collection_kind TEXT NOT NULL CHECK (collection_kind IN ('cookie', 's256')),
    -- The `collection_hash` of the signed `login` context (C26): the S256
    -- challenge, or SHA-256 of the cookie secret.
    collection_digest BYTEA NOT NULL,
    qr_token_hash BYTEA,
    qr_token_at TIMESTAMPTZ,
    qr_token_used BOOLEAN NOT NULL DEFAULT false,
    -- QR: the claimer (set at the claim, cleared at a release); password and
    -- OIDC: the account of the first factor.
    user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    first_factor TEXT CHECK (first_factor IN ('password', 'oidc')),
    first_factor_generation BIGINT,
    client_class TEXT NOT NULL CHECK (client_class IN ('web', 'desktop', 'android')),
    client_label TEXT NOT NULL CHECK (char_length(client_label) BETWEEN 1 AND 64),
    ip INET,
    user_agent TEXT,
    place TEXT NOT NULL DEFAULT '',
    state TEXT NOT NULL CHECK (state IN ('open', 'claimed', 'approved', 'consumed', 'denied',
                                         'reported', 'cancelled', 'expired')),
    -- `approval_devices.id` of the claiming key device.
    claimed_by_device UUID,
    challenge_id UUID,
    approved_at TIMESTAMPTZ,
    -- The session the attempt was collected into (repudiation finds the
    -- first factor through it, Task 24).
    session_id UUID,
    -- An F3/F4 attempt that ended without a session recorded its
    -- `sign_in_not_finished` event (Task 18).
    unfinished_recorded BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    ended_at TIMESTAMPTZ,
    CONSTRAINT login_attempts_ttl CHECK (expires_at <= created_at + INTERVAL '10 minutes'),
    CONSTRAINT login_attempts_first_factor CHECK (
        (kind = 'qr' AND first_factor IS NULL)
        OR (kind <> 'qr' AND first_factor = kind AND user_id IS NOT NULL
            AND first_factor_generation IS NOT NULL))
);

CREATE UNIQUE INDEX login_attempts_qr_token ON login_attempts (qr_token_hash)
    WHERE qr_token_hash IS NOT NULL;
CREATE INDEX login_attempts_collection ON login_attempts (collection_hash);
CREATE INDEX login_attempts_live ON login_attempts (expires_at)
    WHERE state IN ('open', 'claimed', 'approved');
CREATE INDEX login_attempts_user ON login_attempts (user_id) WHERE user_id IS NOT NULL;
