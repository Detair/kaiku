-- Ensure mfa_secret is either NULL or a non-empty, valid-length encrypted value.
-- The server derives mfa_enabled from mfa_secret IS NOT NULL.
ALTER TABLE users ADD CONSTRAINT mfa_secret_not_empty
    CHECK (mfa_secret IS NULL OR length(mfa_secret) > 0);
