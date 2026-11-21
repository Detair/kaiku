-- A push subscription belongs to the session that registered it.
--
-- A chat-only session may overwrite or delete only its own rows, rows without
-- an owner and rows whose owning session is no longer live; whether a row is
-- deleted when its session ends depends on why it ended (push/AGENTS.md
-- "Ownership"). No foreign key, like `enrollments.issuer_session_id`: ended
-- session rows are cleaned up after 30 days, and a missing session counts as
-- "no longer live".
ALTER TABLE push_subscriptions ADD COLUMN session_id UUID;

CREATE INDEX idx_push_subscriptions_session
    ON push_subscriptions (session_id) WHERE session_id IS NOT NULL;

-- Back-fill: a row of an install that is an active key device of the same
-- account belongs to that device's newest live session. Every other row has
-- no owner; the next registration from that install takes it over. Nothing
-- is deleted, so every phone keeps its pushes.
UPDATE push_subscriptions p
   SET session_id = (
       SELECT s.id
         FROM approval_devices d
         JOIN sessions s
           ON s.user_id = d.user_id
          AND s.authenticator_type = 'key_device'
          AND s.authenticator_id = d.id
        WHERE d.user_id = p.user_id
          AND d.device_id = p.device_id
          AND d.state = 'active'
          AND s.ended_at IS NULL
          AND s.expires_at > NOW()
          AND s.absolute_expires_at > NOW()
        ORDER BY s.created_at DESC, s.id DESC
        LIMIT 1)
 WHERE p.device_id IS NOT NULL;
