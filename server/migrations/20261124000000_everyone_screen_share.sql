-- Every member may share their screen in voice channels: SCREEN_SHARE (bit 22)
-- moves to the permissions the default (@everyone) role may hold and is part
-- of its default. Give it to the @everyone role of every existing server.
-- Admins can still take it away per channel with an override that denies it.
UPDATE guild_roles
SET permissions = permissions | (1::bigint << 22)
WHERE is_default;
