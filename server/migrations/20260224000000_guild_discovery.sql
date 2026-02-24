-- Guild Discovery: add columns for public guild browsing and full-text search

ALTER TABLE guilds ADD COLUMN discoverable BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE guilds ADD COLUMN tags TEXT[] NOT NULL DEFAULT '{}' CHECK (array_length(tags, 1) IS NULL OR array_length(tags, 1) <= 5);
ALTER TABLE guilds ADD COLUMN banner_url TEXT;
ALTER TABLE guilds ADD COLUMN member_count INTEGER NOT NULL DEFAULT 0 CHECK (member_count >= 0);
ALTER TABLE guilds ADD COLUMN search_vector tsvector
  GENERATED ALWAYS AS (
    setweight(to_tsvector('english', coalesce(name, '')), 'A') ||
    setweight(to_tsvector('english', coalesce(description, '')), 'B')
  ) STORED;

-- Backfill member_count from existing data
UPDATE guilds SET member_count = (
  SELECT COUNT(*) FROM guild_members WHERE guild_members.guild_id = guilds.id
);

-- Trigger to keep member_count in sync
CREATE OR REPLACE FUNCTION update_guild_member_count() RETURNS trigger AS $$
BEGIN
  IF TG_OP = 'INSERT' THEN
    UPDATE guilds SET member_count = member_count + 1 WHERE id = NEW.guild_id;
  ELSIF TG_OP = 'DELETE' THEN
    UPDATE guilds SET member_count = GREATEST(member_count - 1, 0) WHERE id = OLD.guild_id;
  END IF;
  RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_guild_member_count
  AFTER INSERT OR DELETE ON guild_members
  FOR EACH ROW EXECUTE FUNCTION update_guild_member_count();

-- Index for browsing discoverable guilds by member count (popular) and by created_at (newest)
CREATE INDEX idx_guilds_discoverable_members ON guilds (member_count DESC, created_at DESC)
  WHERE discoverable = true AND suspended_at IS NULL;
CREATE INDEX idx_guilds_discoverable_newest ON guilds (created_at DESC)
  WHERE discoverable = true AND suspended_at IS NULL;

-- GIN index for full-text search on name + description
CREATE INDEX idx_guilds_search_vector ON guilds USING gin (search_vector);

-- GIN index for tag-based filtering
CREATE INDEX idx_guilds_tags ON guilds USING gin (tags) WHERE discoverable = true;
