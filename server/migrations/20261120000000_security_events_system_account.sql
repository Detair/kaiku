-- v1.0 login (L1a, Task 3): the "Kaiku" system account, its read-only DMs
-- and the append-only security event log (spec Part C, contract C28).

-- The system account. Clients recognise it only by `is_system`.
ALTER TABLE users ADD COLUMN is_system BOOLEAN NOT NULL DEFAULT false;

-- The system account has no password; passwordless family accounts (F5d)
-- follow in L1b.
ALTER TABLE users DROP CONSTRAINT local_user_has_password;

-- `kaiku` is reserved. A pre-existing account with that name keeps its id,
-- data and sessions but is renamed (deterministic, matches username_format);
-- its owner signs in with the new name.
UPDATE users
   SET username = 'kaiku_' || left(replace(id::text, '-', ''), 8)
 WHERE username = 'kaiku'
   AND id <> '6b61696b-7500-4000-8000-000000000001';

INSERT INTO users (id, username, display_name, auth_method, is_system)
     VALUES ('6b61696b-7500-4000-8000-000000000001', 'kaiku', 'Kaiku', 'local', true);

-- At most one system account, and it is always the fixed id.
CREATE UNIQUE INDEX users_single_system_account ON users ((is_system)) WHERE is_system;
ALTER TABLE users ADD CONSTRAINT system_account_fixed_id
    CHECK (NOT is_system OR id = '6b61696b-7500-4000-8000-000000000001');

-- Kaiku DMs are read-only for the user (DC10). Only they are read-only.
ALTER TABLE channels ADD COLUMN read_only BOOLEAN NOT NULL DEFAULT false;

-- Security notices are messages of their own type.
ALTER TABLE messages DROP CONSTRAINT messages_message_type_check,
    ADD CONSTRAINT messages_message_type_check
        CHECK (message_type IN ('user', 'system', 'security'));

CREATE TABLE security_events (
    id UUID PRIMARY KEY,
    subject_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    severity TEXT NOT NULL CHECK (severity IN ('high', 'normal')),
    actor_kind TEXT NOT NULL CHECK (actor_kind IN ('self', 'admin', 'recovery_contact', 'system', 'cli')),
    actor_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    client_class TEXT,
    client_label TEXT,
    ip INET,
    place TEXT,
    session_id UUID,
    -- TOTP and password changes carry their type in `details.type`.
    authenticator_type TEXT CHECK (authenticator_type IN ('passkey', 'key_device')),
    authenticator_id UUID,
    request_id UUID,
    details JSONB NOT NULL DEFAULT '{}',
    repudiated_at TIMESTAMPTZ,
    delivered_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK ((authenticator_id IS NULL) = (authenticator_type IS NULL))
);

CREATE INDEX idx_security_events_subject ON security_events (subject_id, created_at DESC, id DESC);
CREATE INDEX idx_security_events_undelivered ON security_events (created_at) WHERE delivered_at IS NULL;

-- Append-only: only `repudiated_at` and `delivered_at` may change, each
-- only once (NULL → a time).
CREATE FUNCTION security_events_immutable() RETURNS trigger AS $$
BEGIN
    IF NEW.id IS DISTINCT FROM OLD.id
       OR NEW.subject_id IS DISTINCT FROM OLD.subject_id
       OR NEW.event_type IS DISTINCT FROM OLD.event_type
       OR NEW.severity IS DISTINCT FROM OLD.severity
       OR NEW.actor_kind IS DISTINCT FROM OLD.actor_kind
       OR (NEW.actor_user_id IS DISTINCT FROM OLD.actor_user_id AND NEW.actor_user_id IS NOT NULL)
       OR NEW.client_class IS DISTINCT FROM OLD.client_class
       OR NEW.client_label IS DISTINCT FROM OLD.client_label
       OR NEW.ip IS DISTINCT FROM OLD.ip
       OR NEW.place IS DISTINCT FROM OLD.place
       OR NEW.session_id IS DISTINCT FROM OLD.session_id
       OR NEW.authenticator_type IS DISTINCT FROM OLD.authenticator_type
       OR NEW.authenticator_id IS DISTINCT FROM OLD.authenticator_id
       OR NEW.request_id IS DISTINCT FROM OLD.request_id
       OR NEW.details IS DISTINCT FROM OLD.details
       OR NEW.created_at IS DISTINCT FROM OLD.created_at
       OR (OLD.repudiated_at IS NOT NULL AND NEW.repudiated_at IS DISTINCT FROM OLD.repudiated_at)
       OR (OLD.delivered_at IS NOT NULL AND NEW.delivered_at IS DISTINCT FROM OLD.delivered_at)
    THEN
        RAISE EXCEPTION 'security_events rows are append-only'
            USING ERRCODE = 'restrict_violation';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER security_events_immutable
    BEFORE UPDATE ON security_events
    FOR EACH ROW EXECUTE FUNCTION security_events_immutable();
