-- Rich embeds + interactive components on messages (bot-authored).
-- embeds: Vec<Embed> (max 10). components: Vec<ActionRow> (max 5) — wired in a
-- later increment; the column is provisioned now so no second migration is needed.
ALTER TABLE messages ADD COLUMN embeds     JSONB;
ALTER TABLE messages ADD COLUMN components JSONB;
