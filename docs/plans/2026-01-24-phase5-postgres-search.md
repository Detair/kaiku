# Design: Postgres Native Full-Text Search (Phase 5)

## 1. Overview
Implement high-performance message search using PostgreSQL's `tsvector` column and `GIN` index. This leverages existing infrastructure without adding new services.

## 2. Database Schema
We will modify the existing `messages` table defined in `20240101000000_initial_schema.sql`.

### Migration SQL
File: `server/migrations/20260125000000_add_message_search.sql`

```sql
-- Add generated tsvector column
ALTER TABLE messages
ADD COLUMN content_search tsvector
GENERATED ALWAYS AS (to_tsvector('english', content)) STORED;

-- Create GIN index for fast search
CREATE INDEX idx_messages_content_search ON messages USING GIN (content_search);

-- Optional: Add trigger if we want to support multiple languages in the future
-- (For now, STORED generated column is sufficient and faster/simpler)
```

## 3. Backend Implementation (Rust)

### 3.1. Models (`server/src/chat/messages.rs`)
Update `Message` struct if necessary, though `content_search` is internal. We likely don't need to expose it to the API.

### 3.2. Repository (`server/src/chat/messages.rs`)
Add function `search_messages`:

```rust
pub async fn search_messages(
    pool: &PgPool,
    guild_id: Uuid,
    query: &str,
    limit: i64,
    offset: i64
) -> Result<Vec<Message>, Error> {
    sqlx::query_as!(
        Message,
        r#"
        SELECT m.* 
        FROM messages m
        JOIN channels c ON m.channel_id = c.id
        WHERE c.guild_id = $1
        AND m.content_search @@ websearch_to_tsquery('english', $2)
        ORDER BY m.created_at DESC
        LIMIT $3 OFFSET $4
        "#,
        guild_id,
        query,
        limit,
        offset
    )
    .fetch_all(pool)
    .await
}
```
*Note: We join `channels` to ensure we only search messages in the requested Guild.*

### 3.3. API Handler (`server/src/api/search.rs`)
*   New module `server/src/api/search.rs`.
*   Route: `GET /api/v1/guilds/:guild_id/search?q=...`
*   Permission Check: `permissions::check(user, guild, "view_channels")` (Basic check).
    *   *Advanced:* In the SQL, filter out channels where the user lacks `read_message_history`.

## 4. Frontend Implementation (Client)

### 4.1. Store (`client/src/stores/search.ts`)
*   State: `results: Message[]`, `isSearching: boolean`.
*   Action: `search(guildId, query)`.

### 4.2. UI Components
*   `client/src/components/home/HomeRightPanel.tsx`:
    *   Add "Search" tab or toggle.
    *   Reuse `MessageItem` but with reduced layout (hide avatars if compact).
    *   Highlight matches using a simple regex on the client side (Postgres `ts_headline` is expensive).

## 5. Step-by-Step Plan
1.  **DB:** Create migration file `server/migrations/20260125000000_add_message_search.sql`.
2.  **Server:** Implement `search_messages` query in `server/src/db/queries.rs` (or `messages.rs`).
3.  **Server:** Create `server/src/api/search.rs` handler and register in `server/src/lib.rs`.
4.  **Client:** Create `search.ts` store.
5.  **Client:** Build `SearchSidebar` component and integrate into `HomeView`.