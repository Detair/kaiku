-- Read cursors point at the channel feed: a top-level message, never a thread
-- reply. Older "mark all read" requests could store a thread reply as the
-- cursor; clients compare the cursor with the channel's last top-level message,
-- so such a cursor never matched and the channel looked unread for good.
--
-- Move every reply cursor to the newest non-deleted top-level message of the
-- same channel created at or before the reply. Unread counts do not change:
-- no top-level message lies between that message and the reply. When there is
-- no such message the cursor becomes NULL, which counts from the user's join
-- time; as no top-level message exists up to the reply, the count is again
-- unchanged. `last_read_at` is kept.
--
-- Idempotent: only rows whose cursor is a thread reply are touched.

UPDATE channel_read_state rs
SET last_read_message_id = (
    SELECT t.id
    FROM messages t
    WHERE t.channel_id = rs.channel_id
      AND t.parent_id IS NULL
      AND t.deleted_at IS NULL
      AND t.created_at <= reply.created_at
    ORDER BY t.created_at DESC, t.id DESC
    LIMIT 1
)
FROM messages reply
WHERE reply.id = rs.last_read_message_id
  AND reply.parent_id IS NOT NULL;

UPDATE dm_read_state rs
SET last_read_message_id = (
    SELECT t.id
    FROM messages t
    WHERE t.channel_id = rs.channel_id
      AND t.parent_id IS NULL
      AND t.deleted_at IS NULL
      AND t.created_at <= reply.created_at
    ORDER BY t.created_at DESC, t.id DESC
    LIMIT 1
)
FROM messages reply
WHERE reply.id = rs.last_read_message_id
  AND reply.parent_id IS NOT NULL;
