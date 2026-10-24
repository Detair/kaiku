-- Accounts created by a server admin start with a temporary password and must
-- set their own password before using anything beyond the password-change,
-- refresh, logout and profile-read routes.
ALTER TABLE users ADD COLUMN must_change_password BOOLEAN NOT NULL DEFAULT false;
