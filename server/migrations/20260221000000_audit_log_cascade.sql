-- Add ON DELETE CASCADE to system_audit_log.actor_id foreign key.
--
-- Without CASCADE, deleting a user fails when audit log entries reference them,
-- which breaks test cleanup (and would break production user deletion).
-- This matches the existing CASCADE pattern on system_admins.user_id.
ALTER TABLE system_audit_log
  DROP CONSTRAINT system_audit_log_actor_id_fkey,
  ADD CONSTRAINT system_audit_log_actor_id_fkey
    FOREIGN KEY (actor_id) REFERENCES users(id) ON DELETE CASCADE;
