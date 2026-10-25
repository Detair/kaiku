-- Default (@everyone) roles can never hold the post, announcement, event or
-- webhook management permissions (bits 26-29). Clear them on existing rows.
UPDATE guild_roles
SET permissions = permissions & ~((1::bigint << 26) | (1::bigint << 27) | (1::bigint << 28) | (1::bigint << 29))
WHERE is_default;
