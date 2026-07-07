-- Reaction-role bindings: bind an emoji on a message to a self-assignable role.
CREATE TABLE reaction_role_bindings (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    guild_id     UUID NOT NULL REFERENCES guilds(id)       ON DELETE CASCADE,
    channel_id   UUID NOT NULL REFERENCES channels(id)     ON DELETE CASCADE,
    message_id   UUID NOT NULL REFERENCES messages(id)     ON DELETE CASCADE,
    -- Emoji key: unicode grapheme ("🎨") or custom emoji ref ("<:name:uuid>"),
    -- the exact string form message_reactions already stores.
    emoji        VARCHAR(128) NOT NULL,
    role_id      UUID NOT NULL REFERENCES guild_roles(id)  ON DELETE CASCADE,
    -- Non-null groups bindings for `unique` (pick-one) behaviour.
    group_key    VARCHAR(64),
    mode         VARCHAR(16) NOT NULL DEFAULT 'toggle',
    created_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT reaction_role_unique_emoji UNIQUE (message_id, emoji),
    CONSTRAINT reaction_role_mode_valid   CHECK (mode IN ('toggle', 'unique'))
);

CREATE INDEX idx_reaction_role_message ON reaction_role_bindings(message_id);
CREATE INDEX idx_reaction_role_guild   ON reaction_role_bindings(guild_id);
