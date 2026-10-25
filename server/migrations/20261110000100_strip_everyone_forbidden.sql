-- Default (@everyone) roles can never hold any permission in EVERYONE_FORBIDDEN
-- (including the post, announcement, event and webhook management bits 26-29).
-- Clear them on existing rows.
UPDATE guild_roles
SET permissions = permissions & ~((1::bigint << 7) | (1::bigint << 8) | (1::bigint << 9) | (1::bigint << 10) | (1::bigint << 11) | (1::bigint << 12) | (1::bigint << 13) | (1::bigint << 14) | (1::bigint << 15) | (1::bigint << 16) | (1::bigint << 17) | (1::bigint << 18) | (1::bigint << 20) | (1::bigint << 21) | (1::bigint << 22) | (1::bigint << 23) | (1::bigint << 25) | (1::bigint << 26) | (1::bigint << 27) | (1::bigint << 28) | (1::bigint << 29))
WHERE is_default;

-- Channel overrides of default roles can not allow any forbidden permission either.
UPDATE channel_overrides
SET allow_permissions = allow_permissions & ~((1::bigint << 7) | (1::bigint << 8) | (1::bigint << 9) | (1::bigint << 10) | (1::bigint << 11) | (1::bigint << 12) | (1::bigint << 13) | (1::bigint << 14) | (1::bigint << 15) | (1::bigint << 16) | (1::bigint << 17) | (1::bigint << 18) | (1::bigint << 20) | (1::bigint << 21) | (1::bigint << 22) | (1::bigint << 23) | (1::bigint << 25) | (1::bigint << 26) | (1::bigint << 27) | (1::bigint << 28) | (1::bigint << 29))
WHERE role_id IN (SELECT id FROM guild_roles WHERE is_default);
