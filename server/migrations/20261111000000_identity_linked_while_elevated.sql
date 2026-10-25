-- C24 re-authentication: an identity linked after the current session began
-- may confirm elevation only if it was linked from an already elevated
-- session. This flag records that case.
ALTER TABLE user_identities
    ADD COLUMN linked_while_elevated BOOLEAN NOT NULL DEFAULT FALSE;
