<!-- Parent: ../AGENTS.md -->
# Database Seeds

## Purpose

Development seed data for the Kaiku server. Provides realistic test data for local development and integration testing.

## Key Files

| File | Purpose |
|------|---------|
| `dev.sql` | Development seed data: test users, channels, sample messages |

## For AI Agents

### Running Seeds

```bash
# After running migrations
psql $DATABASE_URL -f server/seeds/dev.sql

# Or via sqlx
sqlx database setup --source server/migrations
psql $DATABASE_URL -f server/seeds/dev.sql
```

### Seed Data Categories

**`dev.sql` contains:**
- Test users with known credentials
- Sample guilds/servers
- Default channels (text, voice)
- Sample messages for UI testing
- Role assignments

### Test User Credentials

Standard test accounts (defined in `dev.sql`):
- `testuser@example.com` / `testpassword` - Regular user
- `admin@example.com` / `adminpassword` - Admin user

**Note:** These credentials are for development only. Never use in production.

### Writing New Seeds

```sql
-- Insert test user
INSERT INTO users (id, email, username, password_hash)
VALUES (
    '00000000-0000-0000-0000-000000000001',
    'test@example.com',
    'testuser',
    '$argon2id$...'  -- Pre-computed hash
);

-- Use deterministic UUIDs for reproducibility
-- Prefix with zeros to identify test data
```

### Best Practices

**DO:**
- Use deterministic UUIDs (easy to identify as test data)
- Pre-compute password hashes (don't rely on application code)
- Include variety of data states (empty channels, full channels, etc.)
- Document test credentials

**DON'T:**
- Include real user data
- Use production-like UUIDs
- Depend on auto-increment IDs
- Include sensitive configuration

### Integration Test Fixtures

For `#[sqlx::test]` fixtures, reference seeds by filename without extension:

```rust
#[sqlx::test(fixtures("dev"))]
async fn test_with_seed_data(pool: PgPool) {
    // dev.sql data is available
}
```

## Dependencies

- PostgreSQL with migrations already applied
- psql CLI or SQLx for execution
