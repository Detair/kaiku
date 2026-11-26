-- A push subscription records the session that registered it.
--
-- A key phone's row (install of an active or pending key device) belongs to
-- the device; every other row lives as long as the session that registered it
-- (push/AGENTS.md "Ownership"). A chat-only session may overwrite or delete
-- only rows that are not a key phone's and are its own, without an owner, or
-- of a session that is no longer live. No foreign key, like `enrollments.issuer_session_id`: ended
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

-- A key backup remembers whether a chat-only session created it. A chat-only
-- session may create only the account's first backup, with version 1; a full
-- session may replace such a backup with any version. Existing backups count
-- as made by a full session (the version rule applies as before).
ALTER TABLE key_backups ADD COLUMN created_by_chat_only BOOLEAN NOT NULL DEFAULT false;
