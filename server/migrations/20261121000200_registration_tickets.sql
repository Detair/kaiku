-- v1.0 login (L1b Task 19): registration tickets (spec F8 "New accounts";
-- C27 "Registration tickets").
--
-- `POST /auth/register` creates the account and one ticket (256 bits, stored
-- only as a keyed hash, 30 minutes), never a session. Binding a passkey, a key
-- device or TOTP sets `bound_at`; the minute sweep deletes accounts whose
-- ticket bound nothing within 30 minutes.

CREATE TABLE registration_tickets (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    ticket_hash BYTEA NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    bound_at TIMESTAMPTZ,
    CONSTRAINT registration_tickets_ttl CHECK (expires_at <= created_at + INTERVAL '30 minutes')
);

CREATE INDEX registration_tickets_unbound ON registration_tickets (created_at)
    WHERE bound_at IS NULL;
