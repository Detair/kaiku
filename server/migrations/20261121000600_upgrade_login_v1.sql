-- F8 (L1b): the v1.0 login upgrade.
--
-- Sessions of accounts that have neither TOTP nor an active strong
-- authenticator end (a password alone is no longer a sign-in).
UPDATE sessions s
   SET ended_at = NOW(), end_reason = 'upgrade'
 WHERE s.ended_at IS NULL
   AND EXISTS (
       SELECT 1 FROM users u
        WHERE u.id = s.user_id
          AND NOT u.is_system
          AND u.mfa_secret IS NULL
          AND NOT EXISTS (SELECT 1 FROM approval_devices d
                           WHERE d.user_id = u.id AND d.state = 'active')
          AND NOT EXISTS (SELECT 1 FROM passkeys p
                           WHERE p.user_id = u.id AND p.state = 'active'));

-- Accounts with a first factor (password or OIDC identity) and TOTP but no
-- active strong authenticator may secure themselves ("Secure my account").
UPDATE users u
   SET bootstrap_open = true
 WHERE NOT u.is_system
   AND u.mfa_secret IS NOT NULL
   AND (u.password_hash IS NOT NULL
        OR EXISTS (SELECT 1 FROM user_identities i WHERE i.user_id = u.id))
   AND NOT EXISTS (SELECT 1 FROM approval_devices d
                    WHERE d.user_id = u.id AND d.state = 'active')
   AND NOT EXISTS (SELECT 1 FROM passkeys p
                    WHERE p.user_id = u.id AND p.state = 'active');

-- Temporary passwords are gone (C27): nobody is forced to change one.
UPDATE users SET must_change_password = false WHERE must_change_password;

-- The Kaiku DM repeats `bootstrap_reminder` every 30 days while
-- `bootstrap_open` (C28).
ALTER TABLE users ADD COLUMN bootstrap_reminded_at TIMESTAMPTZ;

-- Legacy backup codes (replaced by recovery codes) and e-mail reset tokens.
DROP TABLE IF EXISTS mfa_backup_codes;
DROP TABLE IF EXISTS password_reset_tokens;
