-- v1.0 login (L1b Task 15): credential states, factor generations and
-- recovery codes (spec B.1 "Credential states", "Recovery codes"; B.7 rule 2).
--
-- A password or TOTP secret is `active`, `throttled` or `compromised`. Each
-- factor has a generation that changes when it is replaced, removed or marked
-- compromised (a throttle starts none); login attempts and account requests
-- record the generation they used. `bootstrap_open` is the one-time right of a
-- migrated account to bind its first strong authenticator through a request
-- (F8; set by the upgrade migration and by registration with TOTP).
--
-- Legacy `mfa_backup_codes` stay until the upgrade migration drops them; the
-- server no longer reads them.

ALTER TABLE users
    ADD COLUMN password_state TEXT NOT NULL DEFAULT 'active'
        CONSTRAINT users_password_state CHECK (password_state IN ('active', 'throttled', 'compromised')),
    ADD COLUMN totp_state TEXT NOT NULL DEFAULT 'active'
        CONSTRAINT users_totp_state CHECK (totp_state IN ('active', 'throttled', 'compromised')),
    ADD COLUMN password_compromised_at TIMESTAMPTZ,
    ADD COLUMN totp_compromised_at TIMESTAMPTZ,
    ADD COLUMN password_generation BIGINT NOT NULL DEFAULT 1,
    ADD COLUMN totp_generation BIGINT NOT NULL DEFAULT 1,
    ADD COLUMN recovery_codes_generation BIGINT NOT NULL DEFAULT 1,
    ADD COLUMN bootstrap_open BOOLEAN NOT NULL DEFAULT false;

CREATE TABLE recovery_codes (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    generation BIGINT NOT NULL,
    -- Argon2id PHC string of the 13 base32 characters (upper case, no dashes).
    code_hash TEXT NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX recovery_codes_unused ON recovery_codes (user_id, generation) WHERE used_at IS NULL;
