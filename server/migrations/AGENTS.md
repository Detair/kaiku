<!-- Parent: ../AGENTS.md -->
# Database Migrations

## Purpose

SQLx database migrations for the Kaiku server. Defines the PostgreSQL schema evolution from initial setup through feature additions.

## Key Files

Migrations are timestamp-prefixed (`YYYYMMDDHHmmss_description.sql`) and
self-documenting via their header comments. They are forward-only (no
down-migrations). Rather than maintain a hand-curated list here (which drifts),
discover them directly:

```bash
ls server/migrations/*.sql                  # all migrations, in apply order
sqlx migrate info --source server/migrations # applied vs pending
```

Foundational ones to know: `20240101000000_initial_schema.sql` (core tables:
users, channels, messages, sessions) and `20240201000000_guilds.sql` (guilds,
memberships, roles).

## For AI Agents

### Migration Commands

```bash
# Run pending migrations
sqlx migrate run --source server/migrations

# Create new migration (reversible)
sqlx migrate add -r <name> --source server/migrations

# Revert last migration
sqlx migrate revert --source server/migrations

# Check migration status
sqlx migrate info --source server/migrations
```

### Migration Naming Convention

Format: `YYYYMMDDHHMMSS_description.sql`

- Timestamp ensures ordering
- Description uses snake_case
- Keep descriptions concise but clear

### Writing Migrations

**Reversible migrations (`-r` flag):**
```sql
-- up.sql
CREATE TABLE new_table (...);

-- down.sql
DROP TABLE new_table;
```

**Single file (irreversible):**
```sql
-- Only contains forward migration
ALTER TABLE users ADD COLUMN new_field TEXT;
```

### Best Practices

**DO:**
- Use `IF NOT EXISTS` for idempotency where appropriate
- Add indices for foreign keys
- Include `created_at`, `updated_at` timestamps
- Use UUIDv7 for primary keys (time-sortable)
- Add meaningful constraints

**DON'T:**
- Drop columns in production without data migration plan
- Modify existing migration files after they've been run
- Use raw SQL strings for user data (parameterize)

### Schema Conventions

**Primary Keys:**
```sql
id UUID PRIMARY KEY DEFAULT gen_random_uuid()
```

**Timestamps:**
```sql
created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
```

**Foreign Keys:**
```sql
user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE
```

### Critical Migrations

- `initial_schema.sql` - Core data model
- `permission_system.sql` - Authorization framework
- `guilds.sql` - Multi-server architecture

Changes to these require careful review.

## Dependencies

- PostgreSQL 15+
- SQLx CLI (`cargo install sqlx-cli`)
- `uuid-ossp` or `pgcrypto` extension for UUID generation
