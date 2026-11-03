-- v1.0 login (L1b Task 21): account requests (spec SM3; C27 "Account requests").
--
-- A delayed change to an account's authenticators: self-service recovery
-- (F6), bootstrap ("Secure my account", F8), or an admin setup/recovery of an
-- existing account (F5c). It records the factor generations it used and its
-- independent cancellers (routes: other admins, a confirmed recovery contact,
-- the account's own strong authenticators); without a route it cannot start,
-- and with none left it becomes `void`. One open request per account.

CREATE TABLE account_requests (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('self', 'admin')),
    purpose TEXT NOT NULL CHECK (purpose IN ('recovery', 'bootstrap', 'admin_setup', 'admin_recovery')),
    -- [{"factor": "password"|"totp"|"recovery_codes"|"oidc", "generation": n}]
    factors JSONB NOT NULL DEFAULT '[]',
    lost_device_ids UUID[] NOT NULL DEFAULT '{}',
    -- [{"route": "admin"|"contact"|"authenticator", "id": uuid, "type"?: "key_device"|"passkey"}]
    cancellers JSONB NOT NULL DEFAULT '[]',
    -- The acting admin of an `admin` request.
    started_by UUID REFERENCES users(id) ON DELETE SET NULL,
    -- The admin's issuing key device (`approval_devices.id`).
    started_by_device UUID,
    enrollment_id UUID,
    request_secret_hash BYTEA UNIQUE,
    -- Staged choices of the request client (Task 22) and the pending
    -- authenticator of an `admin` request: {"authenticator": {"type", "id"}, ...}.
    staged JSONB NOT NULL DEFAULT '{}',
    state TEXT NOT NULL CHECK (state IN ('waiting', 'ready', 'completed', 'cancelled', 'withdrawn',
                                         'expired', 'void')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ready_at TIMESTAMPTZ NOT NULL,
    -- The Finish deadline (24 h after `ready`); null while waiting.
    expires_at TIMESTAMPTZ,
    ready_soon_sent BOOLEAN NOT NULL DEFAULT false,
    cancelled_by JSONB,
    cancel_reason TEXT CHECK (char_length(cancel_reason) <= 500),
    ended_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX account_requests_one_open ON account_requests (user_id)
    WHERE state IN ('waiting', 'ready');
CREATE INDEX account_requests_cancellers ON account_requests USING GIN (cancellers);
CREATE INDEX account_requests_open_due ON account_requests (ready_at)
    WHERE state IN ('waiting', 'ready');
-- Start budget (B.5): `self` starts counted per account, every outcome.
CREATE TABLE account_request_starts (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX account_request_starts_user ON account_request_starts (user_id, started_at);
