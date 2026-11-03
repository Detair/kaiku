-- v1.0 login (L1a, Task 10): the approval challenges (spec B.2, C26).
-- Every challenge is single use and completes within 10 minutes (DB CHECK);
-- the kind CHECK lists the v1.0 kinds only (v1.1 adds its own migration).

CREATE TABLE approvals (
    id UUID PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('login', 'enroll', 'device_login', 'device_stepup', 'enroll_pop')),
    variant TEXT,
    approver_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    subject_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    device_id UUID NOT NULL,
    requester_kind TEXT NOT NULL CHECK (requester_kind IN ('login_attempt', 'enrollment', 'device', 'session')),
    requester_id UUID NOT NULL,
    nonce BYTEA NOT NULL CHECK (octet_length(nonce) = 32),
    context_hash BYTEA NOT NULL CHECK (octet_length(context_hash) = 32),
    context JSONB NOT NULL,
    display JSONB NOT NULL,
    transfer_hash BYTEA,
    state TEXT NOT NULL CHECK (state IN ('issued', 'approved', 'consumed', 'denied', 'reported', 'expired')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    decided_at TIMESTAMPTZ,
    consumed_at TIMESTAMPTZ,
    limit_class TEXT NOT NULL DEFAULT 'login' CHECK (limit_class IN ('login')),
    CHECK (expires_at <= created_at + INTERVAL '10 minutes')
);

CREATE INDEX idx_approvals_open ON approvals (requester_kind, requester_id)
    WHERE state IN ('issued', 'approved');
CREATE INDEX idx_approvals_due ON approvals (expires_at) WHERE state IN ('issued', 'approved');
