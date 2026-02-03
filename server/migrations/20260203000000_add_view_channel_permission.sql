-- Add VIEW_CHANNEL permission to existing roles (backward compatibility)
--
-- This migration adds the VIEW_CHANNEL permission (bit 24) to all existing guild roles.
-- This ensures backward compatibility - existing guilds continue to function as before,
-- with all roles able to see all channels. Guild admins can then opt-in to restricting
-- channel visibility by removing VIEW_CHANNEL from specific roles or using channel overrides.
--
-- The VIEW_CHANNEL permission controls whether a user can:
-- - See a channel in the channel list
-- - Read message history
-- - Send messages (in combination with SEND_MESSAGES permission)
-- - Perform any other channel operations
--
-- Security Note: This migration is idempotent (uses bitwise OR) and can be run multiple times safely.

-- Add VIEW_CHANNEL (bit 24 = 1 << 24 = 16777216) to all guild roles
UPDATE roles
SET permissions = permissions | (1::bigint << 24)
WHERE guild_id IS NOT NULL;

-- Ensure @everyone role in all guilds has VIEW_CHANNEL
-- (This is redundant with the above but explicit for clarity)
UPDATE roles
SET permissions = permissions | (1::bigint << 24)
WHERE name = '@everyone';

-- Migration Notes:
-- - This migration does NOT add VIEW_CHANNEL to system-level roles (where guild_id IS NULL)
-- - To rollback: UPDATE roles SET permissions = permissions & ~(1::bigint << 24)
-- - Expected execution time: <1 second for 1000 guilds
-- - No data loss occurs if this migration is rolled back
