-- v1.0 login (L1a, Task 5): session assurance fixed at creation and the
-- committed session row as the authority (spec B.4 rev 5, CC4, C27).

ALTER TABLE sessions
    ADD COLUMN assurance TEXT NOT NULL DEFAULT 'probation' CHECK (assurance IN ('full', 'probation')),
    ADD COLUMN auth_methods TEXT[] NOT NULL DEFAULT '{legacy}',
    ADD COLUMN auth_time TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ADD COLUMN client_class TEXT NOT NULL DEFAULT 'web' CHECK (client_class IN ('web', 'desktop', 'android')),
    ADD COLUMN authenticator_type TEXT CHECK (authenticator_type IN ('passkey', 'key_device')),
    ADD COLUMN authenticator_id UUID,
    ADD COLUMN absolute_expires_at TIMESTAMPTZ NOT NULL DEFAULT NOW() + INTERVAL '30 days',
    ADD COLUMN last_used_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ADD COLUMN ended_at TIMESTAMPTZ,
    ADD COLUMN end_reason TEXT,
    ADD CONSTRAINT session_authenticator_pair
        CHECK ((authenticator_id IS NULL) = (authenticator_type IS NULL)),
    ADD CONSTRAINT full_needs_authenticator
        CHECK (assurance = 'probation' OR authenticator_id IS NOT NULL),
    ADD CONSTRAINT ended_has_reason CHECK ((ended_at IS NULL) = (end_reason IS NULL));

-- DC3: every session from before the upgrade is probation, `{legacy}`, web,
-- with the strictest cap (30 days from its creation).
UPDATE sessions
   SET auth_time = created_at,
       last_used_at = created_at,
       absolute_expires_at = created_at + INTERVAL '30 days';
UPDATE sessions
   SET ended_at = NOW(), end_reason = 'absolute_cap'
 WHERE absolute_expires_at <= NOW();

-- Elevations from before the upgrade (password + TOTP or OIDC re-auth) do
-- not survive it: elevation now needs a full session and a step-up (F7),
-- and every pre-upgrade session is probation.
DELETE FROM elevated_sessions;

CREATE INDEX idx_sessions_user_live ON sessions (user_id) WHERE ended_at IS NULL;
CREATE INDEX idx_sessions_authenticator ON sessions (authenticator_id) WHERE ended_at IS NULL;

-- B.0 "new account": set once at the first password, TOTP, recovery code,
-- authenticator or session, never cleared. Every account that exists at the
-- upgrade is an existing account: it had a password (local accounts needed
-- one) or a session (OIDC accounts are created at their first sign-in) --
-- also when the old cleanup already deleted its expired session rows.
ALTER TABLE users ADD COLUMN first_activity_at TIMESTAMPTZ;
UPDATE users SET first_activity_at = created_at WHERE NOT is_system;
