-- Sessions remember what made them: the key device that confirmed a
-- probation sign-in, and the password and TOTP generations it used.
--
-- * `approved_by_type`/`approved_by_id`: a QR sign-in, or a password or OIDC
--   sign-in confirmed on a phone. The session ends when that key device is
--   revoked. `approved_by_type` set with `approved_by_id` NULL means "some key
--   device of the account, unknown which": such a session ends when any key
--   device of the account is revoked.
-- * `password_generation`/`totp_generation`: the factor generations the
--   sign-in used (NULL = that factor was not used). The session ends when that
--   generation ends (replaced, removed or marked compromised).
--
-- Only live sessions are filled; ended rows stay as they are. Live sessions
-- whose confirming device or factor generation had already ended before the
-- upgrade end with it (see the end of this file).

ALTER TABLE sessions
    ADD COLUMN approved_by_type TEXT
        CONSTRAINT sessions_approved_by_type CHECK (approved_by_type IN ('key_device')),
    ADD COLUMN approved_by_id UUID,
    ADD COLUMN password_generation BIGINT,
    ADD COLUMN totp_generation BIGINT,
    ADD CONSTRAINT session_approver_pair
        CHECK (approved_by_type IS NOT NULL OR approved_by_id IS NULL);

-- The approver, exactly, where the attempt still exists (attempts are deleted
-- a day after they ended).
UPDATE sessions s
   SET approved_by_type = 'key_device', approved_by_id = a.claimed_by_device
  FROM login_attempts a
 WHERE a.session_id = s.id
   AND a.user_id = s.user_id
   AND a.state = 'consumed'
   AND a.claimed_by_device IS NOT NULL
   AND s.ended_at IS NULL
   AND s.auth_methods && ARRAY['qr', 'phone_confirm']::text[];

-- Every other phone-confirmed session: a key device of the account, unknown
-- which.
UPDATE sessions
   SET approved_by_type = 'key_device'
 WHERE ended_at IS NULL
   AND approved_by_type IS NULL
   AND auth_methods && ARRAY['qr', 'phone_confirm']::text[];

-- The generations each sign-in recorded in its `sign_in` event (a value that
-- is not a whole number is skipped; the next step then fills it).
UPDATE sessions s
   SET password_generation = (f->>'generation')::bigint
  FROM security_events e, jsonb_array_elements(
           CASE WHEN jsonb_typeof(e.details->'factors') = 'array' THEN e.details->'factors' END) f
 WHERE e.event_type = 'sign_in'
   AND e.subject_id = s.user_id
   AND e.session_id = s.id
   AND f->>'factor' = 'password'
   AND jsonb_typeof(f->'generation') = 'number'
   AND f->>'generation' ~ '^[0-9]{1,18}$'
   AND s.ended_at IS NULL;

UPDATE sessions s
   SET totp_generation = (f->>'generation')::bigint
  FROM security_events e, jsonb_array_elements(
           CASE WHEN jsonb_typeof(e.details->'factors') = 'array' THEN e.details->'factors' END) f
 WHERE e.event_type = 'sign_in'
   AND e.subject_id = s.user_id
   AND e.session_id = s.id
   AND f->>'factor' = 'totp'
   AND jsonb_typeof(f->'generation') = 'number'
   AND f->>'generation' ~ '^[0-9]{1,18}$'
   AND s.ended_at IS NULL;

-- A session that used a factor without a recorded generation (none
-- expected), and sessions from before v1.0: the account's current
-- generation of every such factor it has.
UPDATE sessions s
   SET password_generation = u.password_generation
  FROM users u
 WHERE u.id = s.user_id
   AND s.ended_at IS NULL
   AND s.password_generation IS NULL
   AND ('password' = ANY(s.auth_methods)
        OR ('legacy' = ANY(s.auth_methods) AND u.password_hash IS NOT NULL));

UPDATE sessions s
   SET totp_generation = u.totp_generation
  FROM users u
 WHERE u.id = s.user_id
   AND s.ended_at IS NULL
   AND s.totp_generation IS NULL
   AND ('totp' = ANY(s.auth_methods)
        OR ('legacy' = ANY(s.auth_methods) AND u.mfa_secret IS NOT NULL));

-- What was already over before the upgrade ends now, with the reason the
-- rules give from now on: a session confirmed by a key device that is no
-- longer active, and a session that signed in with a password or TOTP
-- generation the account has left behind (replaced, removed or marked
-- compromised). Every other session stays signed in.
UPDATE sessions s
   SET ended_at = NOW(), end_reason = 'authenticator_revoked'
 WHERE s.ended_at IS NULL
   AND s.approved_by_id IS NOT NULL
   AND NOT EXISTS (SELECT 1 FROM approval_devices d
                    WHERE d.id = s.approved_by_id AND d.user_id = s.user_id
                      AND d.state = 'active');

UPDATE sessions s
   SET ended_at = NOW(), end_reason = 'factor_ended'
  FROM users u
 WHERE u.id = s.user_id
   AND s.ended_at IS NULL
   AND (s.password_generation < u.password_generation
        OR s.totp_generation < u.totp_generation);

CREATE INDEX idx_sessions_approved_by ON sessions (user_id, approved_by_id)
    WHERE ended_at IS NULL AND approved_by_type IS NOT NULL;
CREATE INDEX idx_sessions_factor_generation ON sessions (user_id)
    WHERE ended_at IS NULL
      AND (password_generation IS NOT NULL OR totp_generation IS NOT NULL);
