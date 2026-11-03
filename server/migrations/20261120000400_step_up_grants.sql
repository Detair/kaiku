-- v1.0 login (L1a, Task 9): action-bound step-up grants (F7, C27 common
-- rules). One action and target, ≤ 300 s, single use, bound to one full
-- session; produced by a passkey assertion (LP) or a `device_stepup`.

CREATE TABLE step_up_grants (
    id UUID PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    action TEXT NOT NULL,
    target_hash BYTEA NOT NULL,
    target TEXT NOT NULL,
    via TEXT NOT NULL CHECK (via IN ('passkey', 'device')),
    authenticator_type TEXT NOT NULL CHECK (authenticator_type IN ('passkey', 'key_device')),
    authenticator_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    voided_at TIMESTAMPTZ,
    CHECK (expires_at <= created_at + INTERVAL '300 seconds')
);

CREATE INDEX idx_step_up_open ON step_up_grants (session_id, action, target_hash)
    WHERE used_at IS NULL AND voided_at IS NULL;
CREATE INDEX idx_step_up_authenticator ON step_up_grants (authenticator_id)
    WHERE used_at IS NULL AND voided_at IS NULL;
