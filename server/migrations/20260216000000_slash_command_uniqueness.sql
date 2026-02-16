-- Enforce uniqueness for global commands (guild_id IS NULL) per application.
-- Guild-scoped commands already have UNIQUE(application_id, guild_id, name)
-- via the existing table constraint, but that doesn't cover the NULL case.
CREATE UNIQUE INDEX IF NOT EXISTS idx_slash_commands_global_app_name
    ON slash_commands (application_id, name)
    WHERE guild_id IS NULL;
