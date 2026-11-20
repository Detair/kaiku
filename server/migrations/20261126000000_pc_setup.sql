-- "Set up this PC" (spec 2026-11-06-this-pc-setup-design, SM5): a chat-only
-- probation session of a PC asks its owner's key phone for a passkey. The
-- phone scans a QR, approves with a `pc_setup` challenge (kind byte 6) and
-- gets a 6-digit code to type on the PC; the right code creates a `pc_setup`
-- enrolment that registers the passkey in the requesting session.
--
-- Only keyed hashes of the QR token and the code are stored (CC3). An attempt
-- is not part of the session-ending cascade (Locking 1): it keeps its row when
-- the session ends and is expired by the next transition or the minute sweep.

CREATE TABLE pc_setup_attempts (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- The requesting probation session; every attempt route is bound to it.
    -- No foreign key, like `enrollments.issuer_session_id`: logout and refresh
    -- delete session rows without the account lock and must not touch an
    -- attempt (Locking 1). Liveness is re-checked under the lock (Locking 2).
    session_id UUID NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('waiting', 'claimed', 'code', 'enrolled', 'completed',
                                         'refused', 'reported', 'failed', 'cancelled', 'expired')),
    -- Why the attempt ended, readable by the PC (for example `session_ended`).
    reason TEXT CHECK (reason IS NULL OR char_length(reason) BETWEEN 1 AND 64),
    -- Keyed hash of the current QR token; rotates every 60 s while `waiting`.
    qr_token_hash BYTEA CHECK (qr_token_hash IS NULL OR octet_length(qr_token_hash) = 32),
    qr_rotates_at TIMESTAMPTZ,
    -- The `pc_setup` challenge of the scan (TTL 2 min).
    challenge_id UUID REFERENCES approvals(id) ON DELETE SET NULL,
    -- `approval_devices.id` of the scanning key device. Device rows are only
    -- soft-revoked, never deleted (apart from the account's own cascade), so
    -- this key never fires in a path without the account lock.
    device_id UUID REFERENCES approval_devices(id) ON DELETE SET NULL,
    -- Keyed hash of the 6-digit code, 5 minutes from the approval, 3 tries.
    code_hash BYTEA CHECK (code_hash IS NULL OR octet_length(code_hash) = 32),
    code_expires_at TIMESTAMPTZ,
    tries SMALLINT NOT NULL DEFAULT 0 CHECK (tries BETWEEN 0 AND 3),
    enrollment_id UUID REFERENCES enrollments(id) ON DELETE SET NULL,
    -- How the right code arrived, and so the only way the passkey may be
    -- registered (spec Step 5, one path per attempt): `web` (the requesting
    -- session's own route and page) or `desktop_flow` (the one desktop flow
    -- `code_flow_id`, whose browser holds the browser secret).
    code_via TEXT CHECK (code_via IS NULL OR code_via IN ('web', 'desktop_flow')),
    code_flow_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- The 10 minutes bound `waiting` and `claimed`; the code and the
    -- enrolment run on their own clocks after that.
    expires_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT pc_setup_attempts_ttl CHECK (expires_at <= created_at + INTERVAL '10 minutes'),
    CONSTRAINT pc_setup_attempts_code_path CHECK (
        (code_via IS NULL AND code_flow_id IS NULL)
        OR (code_via = 'web' AND code_flow_id IS NULL)
        OR (code_via = 'desktop_flow' AND code_flow_id IS NOT NULL))
);

-- One live attempt per session: a new start ends the old one (Locking 6).
CREATE UNIQUE INDEX pc_setup_attempts_live_session ON pc_setup_attempts (session_id)
    WHERE state IN ('waiting', 'claimed', 'code', 'enrolled');
-- The scanned token finds its attempt.
CREATE UNIQUE INDEX pc_setup_attempts_qr_token ON pc_setup_attempts (qr_token_hash)
    WHERE qr_token_hash IS NOT NULL;
-- The minute sweep.
CREATE INDEX idx_pc_setup_attempts_due ON pc_setup_attempts (expires_at)
    WHERE state IN ('waiting', 'claimed', 'code', 'enrolled');
CREATE INDEX idx_pc_setup_attempts_user ON pc_setup_attempts (user_id, created_at);
-- The foreign keys' `ON DELETE SET NULL` (old challenges, enrolments and
-- devices are cleaned up) and the 30-day retention.
CREATE INDEX idx_pc_setup_attempts_challenge ON pc_setup_attempts (challenge_id)
    WHERE challenge_id IS NOT NULL;
CREATE INDEX idx_pc_setup_attempts_device ON pc_setup_attempts (device_id)
    WHERE device_id IS NOT NULL;
CREATE INDEX idx_pc_setup_attempts_enrollment ON pc_setup_attempts (enrollment_id)
    WHERE enrollment_id IS NOT NULL;
CREATE INDEX idx_pc_setup_attempts_created ON pc_setup_attempts (created_at);

-- The client's User-Agent when the session was created. Refresh rotation
-- updates `user_agent` with every refresh; the description the key phone shows
-- and signs for an attempt comes from this value, which never changes.
ALTER TABLE sessions ADD COLUMN created_user_agent TEXT
    CHECK (created_user_agent IS NULL OR char_length(created_user_agent) <= 512);
UPDATE sessions SET created_user_agent = left(user_agent, 512) WHERE user_agent IS NOT NULL;

-- The challenge kind and its requester.
ALTER TABLE approvals
    DROP CONSTRAINT approvals_kind_check,
    ADD CONSTRAINT approvals_kind_check
        CHECK (kind IN ('login', 'enroll', 'device_login', 'device_stepup', 'enroll_pop', 'pc_setup')),
    DROP CONSTRAINT approvals_requester_kind_check,
    ADD CONSTRAINT approvals_requester_kind_check
        CHECK (requester_kind IN ('login_attempt', 'enrollment', 'device', 'session',
                                  'pc_setup_attempt'));

-- SM2 variant `pc_setup`: issued by an attempt whose code was typed, recording
-- the requesting session (`issuer_session_id`) and the approving key device
-- (`issuer_device_id`, an `approval_devices` row id). There is no setup token
-- and no claim: a bearer token could be raced by anyone who saw it, so the
-- passkey is registered by the requesting session itself.
ALTER TABLE enrollments
    DROP CONSTRAINT enrollments_variant_check,
    ADD CONSTRAINT enrollments_variant_check
        CHECK (variant IN ('own', 'admin', 'request', 'registration', 'cli', 'pc_setup')),
    DROP CONSTRAINT enrollments_issuer_kind_check,
    ADD CONSTRAINT enrollments_issuer_kind_check
        CHECK (issuer_kind IN ('key_device', 'session', 'request', 'ticket', 'cli', 'pc_setup')),
    ALTER COLUMN setup_token_hash DROP NOT NULL,
    ADD CONSTRAINT enrollments_setup_token_required
        CHECK (variant = 'pc_setup' OR setup_token_hash IS NOT NULL),
    ADD CONSTRAINT enrollments_pc_setup_passkey_only
        CHECK (variant <> 'pc_setup' OR authenticator_type = 'passkey'),
    ADD CONSTRAINT enrollments_pc_setup_issuer
        CHECK ((variant = 'pc_setup') = (issuer_kind = 'pc_setup')),
    ADD CONSTRAINT enrollments_pc_setup_records_session_and_device
        CHECK (issuer_kind <> 'pc_setup'
               OR (issuer_session_id IS NOT NULL AND issuer_device_id IS NOT NULL));
