# Rich Embeds Implementation Plan (feature #2, increment 1 of 2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Let bot-authored messages carry structured rich embeds (title/description/url/color/author/fields/image/thumbnail/footer/timestamp), validated server-side and rendered safely on the client.

**Architecture:** Two nullable JSONB columns (`embeds`, `components`) added to `messages`; this increment wires **`embeds`** only (the `components` column is added now but stays inert until increment 2). Typed serde structs (`Embed`, `EmbedField`, `EmbedAuthor`, `EmbedFooter`) with size/URL/color validation live in a new `chat/embeds.rs`. Embeds ride the existing message create/edit API, accepted only from bot authors (`users.is_bot`). The `Message` FromRow struct gains the columns (populated by the existing `RETURNING *`), and every `MessageResponse` build site maps them through. The client renders `<MessageEmbeds>` with the isolated DOMPurify sanitizer.

**Tech Stack:** Rust (axum, sqlx/PostgreSQL JSONB, serde, thiserror, utoipa, validator), Solid.js/TypeScript, DOMPurify (isolated instances per #631), `#[sqlx::test]` + `TestApp`, vitest.

**Scope note:** This is increment 1 of feature #2 (rich-embeds-and-components). Increment 2 (interactive components + the bot-gateway interaction loop) is a separate plan/PR; both ship before feature #3. The migration here provisions the `components` column so increment 2 needs no schema change.

---

## File Structure

**Server — create:**
- `server/migrations/20260707000001_message_embeds_components.sql` — add `embeds`, `components` JSONB columns.
- `server/src/chat/embeds.rs` — `Embed`/`EmbedField`/`EmbedAuthor`/`EmbedFooter` structs + `validate_embeds()` (size caps, https URLs, color clamp) + unit tests.

**Server — modify:**
- `server/src/db/models.rs` — add `embeds`/`components` to `Message` FromRow struct.
- `server/src/chat/mod.rs` — `pub mod embeds;`.
- `server/src/chat/types.rs` — `CreateMessageRequest` + `MessageResponse` gain `embeds`.
- `server/src/chat/messages.rs` — accept+validate+persist embeds on create/edit (bot-only); map embeds into every `MessageResponse`.
- `server/src/db/queries.rs` — `set_message_embeds()` helper; include `embeds` in read mapping (via `RETURNING *`/`SELECT *` already covers it once the struct has the field).
- `server/tests/integration/embeds_http.rs` — integration tests; register in `main.rs`.

**Client — create:**
- `client/src/components/messages/MessageEmbeds.tsx` — embed card renderer (sanitized).
- `client/src/components/messages/__tests__/messageEmbeds.test.tsx` — sanitization + render test.

**Client — modify:**
- `client/src/lib/types/*` — `Message` type gains `embeds?`.
- the message list item component — render `<MessageEmbeds>` when present.

**Docs:** `CHANGELOG.md` — Added entry.

---

## Task 1: Migration

**Files:** Create `server/migrations/20260707000001_message_embeds_components.sql`

- [ ] **Step 1: Write the migration**
```sql
-- Rich embeds + interactive components on messages (bot-authored).
-- embeds: Vec<Embed> (max 10). components: Vec<ActionRow> (max 5) — wired in a
-- later increment; the column is provisioned now so no second migration is needed.
ALTER TABLE messages ADD COLUMN embeds     JSONB;
ALTER TABLE messages ADD COLUMN components JSONB;
```

- [ ] **Step 2: Apply**
Run:
```bash
DATABASE_URL="postgresql://voicechat:voicechat_dev@localhost:5433/voicechat" sqlx migrate run --source server/migrations
```
Expected: `Applied 20260707000001/migrate message embeds components`.

- [ ] **Step 3: Commit**
```bash
git add server/migrations/20260707000001_message_embeds_components.sql
git commit -m "feat(db): message embeds/components JSONB columns"
```

---

## Task 2: Embed types + validation (pure, TDD)

**Files:** Create `server/src/chat/embeds.rs`; modify `server/src/chat/mod.rs`.

- [ ] **Step 1: Register the module** — in `server/src/chat/mod.rs` add near the other `pub mod` lines:
```rust
pub mod embeds;
```

- [ ] **Step 2: Write the types + validator + tests**
Create `server/src/chat/embeds.rs`:
```rust
//! Rich embed types (Discord-parity shape) and server-side validation.
//!
//! Embeds are semi-trusted (bot/webhook-authored). Every embed is validated
//! against Discord-parity size caps and URL rules before storage; text is
//! stored verbatim and sanitized at render time on the client.

use serde::{Deserialize, Serialize};

/// Max embeds per message.
pub const MAX_EMBEDS: usize = 10;
/// Combined character budget across one embed's text fields.
pub const MAX_EMBED_TOTAL_CHARS: usize = 6000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
pub struct Embed {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// 24-bit RGB color.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<EmbedAuthor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<EmbedField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer: Option<EmbedFooter>,
    /// RFC3339 timestamp string (rendered as-is).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
pub struct EmbedField {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub inline: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
pub struct EmbedAuthor {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
pub struct EmbedFooter {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
}

/// Reason an embed set was rejected.
#[derive(Debug, PartialEq)]
pub enum EmbedError {
    TooMany,
    TitleTooLong,
    DescriptionTooLong,
    TooManyFields,
    FieldNameTooLong,
    FieldValueTooLong,
    FooterTooLong,
    AuthorNameTooLong,
    TotalTooLong,
    NonHttpsUrl(&'static str),
}

impl std::fmt::Display for EmbedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooMany => write!(f, "too many embeds (max {MAX_EMBEDS})"),
            Self::TitleTooLong => write!(f, "embed title exceeds 256 chars"),
            Self::DescriptionTooLong => write!(f, "embed description exceeds 4096 chars"),
            Self::TooManyFields => write!(f, "embed has more than 25 fields"),
            Self::FieldNameTooLong => write!(f, "embed field name exceeds 256 chars"),
            Self::FieldValueTooLong => write!(f, "embed field value exceeds 1024 chars"),
            Self::FooterTooLong => write!(f, "embed footer exceeds 2048 chars"),
            Self::AuthorNameTooLong => write!(f, "embed author name exceeds 256 chars"),
            Self::TotalTooLong => write!(f, "embed total text exceeds {MAX_EMBED_TOTAL_CHARS} chars"),
            Self::NonHttpsUrl(field) => write!(f, "embed {field} must be an https:// URL"),
        }
    }
}

fn check_https(u: &Option<String>, field: &'static str) -> Result<(), EmbedError> {
    if let Some(u) = u {
        if !u.is_empty() && !u.starts_with("https://") {
            return Err(EmbedError::NonHttpsUrl(field));
        }
    }
    Ok(())
}

/// Validate + normalize a set of embeds. Clamps colors to 24-bit; rejects on any
/// size cap or non-https URL. Returns the (unchanged-but-validated) embeds.
pub fn validate_embeds(embeds: &mut [Embed]) -> Result<(), EmbedError> {
    if embeds.len() > MAX_EMBEDS {
        return Err(EmbedError::TooMany);
    }
    for e in embeds.iter_mut() {
        let mut total = 0usize;
        if let Some(t) = &e.title {
            if t.chars().count() > 256 {
                return Err(EmbedError::TitleTooLong);
            }
            total += t.chars().count();
        }
        if let Some(d) = &e.description {
            if d.chars().count() > 4096 {
                return Err(EmbedError::DescriptionTooLong);
            }
            total += d.chars().count();
        }
        if e.fields.len() > 25 {
            return Err(EmbedError::TooManyFields);
        }
        for f in &e.fields {
            if f.name.chars().count() > 256 {
                return Err(EmbedError::FieldNameTooLong);
            }
            if f.value.chars().count() > 1024 {
                return Err(EmbedError::FieldValueTooLong);
            }
            total += f.name.chars().count() + f.value.chars().count();
        }
        if let Some(fo) = &e.footer {
            if fo.text.chars().count() > 2048 {
                return Err(EmbedError::FooterTooLong);
            }
            total += fo.text.chars().count();
        }
        if let Some(a) = &e.author {
            if a.name.chars().count() > 256 {
                return Err(EmbedError::AuthorNameTooLong);
            }
            total += a.name.chars().count();
        }
        if total > MAX_EMBED_TOTAL_CHARS {
            return Err(EmbedError::TotalTooLong);
        }
        check_https(&e.url, "url")?;
        check_https(&e.image, "image")?;
        check_https(&e.thumbnail, "thumbnail")?;
        if let Some(a) = &e.author {
            check_https(&a.url, "author.url")?;
            check_https(&a.icon_url, "author.icon_url")?;
        }
        if let Some(fo) = &e.footer {
            check_https(&fo.icon_url, "footer.icon_url")?;
        }
        // Clamp color to 24-bit.
        if let Some(c) = e.color {
            e.color = Some(c & 0x00FF_FFFF);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Embed {
        Embed {
            title: None, description: None, url: None, color: None, author: None,
            fields: vec![], image: None, thumbnail: None, footer: None, timestamp: None,
        }
    }

    #[test]
    fn accepts_a_simple_embed() {
        let mut e = vec![Embed { title: Some("Hi".into()), description: Some("desc".into()), ..base() }];
        assert!(validate_embeds(&mut e).is_ok());
    }

    #[test]
    fn rejects_too_many_embeds() {
        let mut e = vec![base(); MAX_EMBEDS + 1];
        assert_eq!(validate_embeds(&mut e), Err(EmbedError::TooMany));
    }

    #[test]
    fn rejects_overlong_title() {
        let mut e = vec![Embed { title: Some("x".repeat(257)), ..base() }];
        assert_eq!(validate_embeds(&mut e), Err(EmbedError::TitleTooLong));
    }

    #[test]
    fn rejects_non_https_url() {
        let mut e = vec![Embed { url: Some("http://evil".into()), ..base() }];
        assert_eq!(validate_embeds(&mut e), Err(EmbedError::NonHttpsUrl("url")));
        let mut e2 = vec![Embed { image: Some("javascript:alert(1)".into()), ..base() }];
        assert_eq!(validate_embeds(&mut e2), Err(EmbedError::NonHttpsUrl("image")));
    }

    #[test]
    fn clamps_color_to_24bit() {
        let mut e = vec![Embed { color: Some(0xFF12_3456), ..base() }];
        validate_embeds(&mut e).unwrap();
        assert_eq!(e[0].color, Some(0x0012_3456));
    }

    #[test]
    fn rejects_total_over_budget() {
        let mut e = vec![Embed { description: Some("a".repeat(4096)), fields: vec![
            EmbedField { name: "n".repeat(256), value: "v".repeat(1024), inline: false },
            EmbedField { name: "n".repeat(256), value: "v".repeat(1024), inline: false },
        ], ..base() }];
        assert_eq!(validate_embeds(&mut e), Err(EmbedError::TotalTooLong));
    }
}
```

- [ ] **Step 3: Run the unit tests**
Run:
```bash
SQLX_OFFLINE=true cargo test -p vc-server --lib chat::embeds
```
Expected: all 6 pass.

- [ ] **Step 4: Commit**
```bash
git add server/src/chat/embeds.rs server/src/chat/mod.rs
git commit -m "feat(chat): embed types + validation"
```

---

## Task 3: Message struct + request/response wiring

**Files:** `server/src/db/models.rs`, `server/src/chat/types.rs`.

- [ ] **Step 1: Add columns to the `Message` FromRow struct**
In `server/src/db/models.rs`, in `pub struct Message`, after `pub parent_id: Option<Uuid>,` add:
```rust
    /// Rich embeds (bot-authored), JSONB. `RETURNING *` / `SELECT *` populate this.
    #[serde(default)]
    pub embeds: Option<serde_json::Value>,
    /// Interactive components (bot-authored), JSONB. Wired in a later increment.
    #[serde(default)]
    pub components: Option<serde_json::Value>,
```
(sqlx `FromRow` maps by column name; the new columns are picked up by every `SELECT *`/`RETURNING *`. Queries selecting explicit column lists that omit these still compile because the fields are `Option` — but verify in Step 3.)

- [ ] **Step 2: Add `embeds` to request + response types**
In `server/src/chat/types.rs`:
- In `CreateMessageRequest`, after `pub parent_id: Option<Uuid>,` add:
```rust
    /// Rich embeds — accepted from bot authors only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embeds: Option<Vec<crate::chat::embeds::Embed>>,
```
- In `MessageResponse`, after `pub nonce: Option<String>,` add:
```rust
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embeds: Option<Vec<crate::chat::embeds::Embed>>,
```
- If `UpdateMessageRequest` (edit) exists in this file, add the same `embeds` field to it.

- [ ] **Step 3: Verify the whole workspace still compiles**
Run:
```bash
SQLX_OFFLINE=true cargo check -p vc-server 2>&1 | grep -E "^error" | head
```
Expected: no errors. If a `SELECT`-explicit query errors on the new columns, it will surface here — fix by adding `embeds, components` to that query's column list or switch it to `SELECT *`.

- [ ] **Step 4: Commit**
```bash
git add server/src/db/models.rs server/src/chat/types.rs
git commit -m "feat(chat): thread embeds through message request/response types"
```

---

## Task 4: Persist + serve embeds (bot-only) on create

**Files:** `server/src/db/queries.rs`, `server/src/chat/messages.rs`.

- [ ] **Step 1: Add a persist helper**
In `server/src/db/queries.rs` add:
```rust
/// Store validated embeds JSON on a message (bot-authored). Pass `None` to clear.
pub async fn set_message_embeds(
    pool: &PgPool,
    message_id: Uuid,
    embeds: Option<&serde_json::Value>,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE messages SET embeds = $1 WHERE id = $2")
        .bind(embeds)
        .bind(message_id)
        .execute(pool)
        .await?;
    Ok(())
}
```

- [ ] **Step 2: Accept + validate + persist in the create handler**
In `server/src/chat/messages.rs` `create` handler, after the message row is created (the main path where `let message = ...; let author = ...; let mut response = MessageResponse { ... }` is built around line 627–665) and BEFORE building the response, insert:
```rust
    // Rich embeds: bot-authored only. Validate against size/URL caps, persist,
    // and reflect on the response. A human user sending embeds is rejected.
    let embeds_json = if let Some(mut embeds) = body.embeds.clone() {
        if !author.is_bot {
            return Err(ChatError::Forbidden);
        }
        crate::chat::embeds::validate_embeds(&mut embeds)
            .map_err(|e| ChatError::Validation(e.to_string()))?;
        let json = serde_json::to_value(&embeds).unwrap_or(serde_json::Value::Null);
        db::set_message_embeds(&state.db, message.id, Some(&json)).await?;
        Some(embeds)
    } else {
        None
    };
```
Then in the `MessageResponse { ... }` initializer add:
```rust
        embeds: embeds_json.clone(),
```
Note: `author` is an `AuthorProfile`; confirm it exposes `is_bot`. If it does not, fetch the flag: `let is_bot = db::find_user_by_id(&state.db, auth_user.id).await?.map(|u| u.is_bot).unwrap_or(false);` and gate on `is_bot`. (Check `AuthorProfile` in `chat/types.rs` in Step 4.)

- [ ] **Step 3: Map embeds on read paths**
In `build_message_responses` (the read/list builder), where each `MessageResponse` is constructed, set:
```rust
        embeds: msg.embeds.as_ref().and_then(|v| serde_json::from_value(v.clone()).ok()),
```
(using whatever the per-row binding is named — `msg`/`message`). Do the same in the edit-response builder and any other `MessageResponse { ... }` site so the field is always populated.

- [ ] **Step 4: Confirm `AuthorProfile`/bot flag**
Run:
```bash
grep -n "struct AuthorProfile" -A12 server/src/chat/types.rs
grep -n "ChatError::Forbidden\|Forbidden\|ChatError::Validation\|Validation(" server/src/chat/error.rs
```
Adjust Step 2/3 to the actual `is_bot` source and the actual `ChatError` variant names (use the existing Forbidden + a validation variant; if the validation variant differs, use it).

- [ ] **Step 5: Compile**
Run:
```bash
SQLX_OFFLINE=true cargo check -p vc-server 2>&1 | grep -E "^error" | head
```
Expected: clean.

- [ ] **Step 6: Commit**
```bash
git add server/src/db/queries.rs server/src/chat/messages.rs
git commit -m "feat(chat): store + serve bot embeds on message create"
```

---

## Task 5: Edit path + integration tests

**Files:** `server/src/chat/messages.rs`, `server/tests/integration/embeds_http.rs`, `server/tests/integration/main.rs`.

- [ ] **Step 1: Accept embeds on edit (bot-only)**
In the `update`/edit handler, after the ownership check and content update, mirror Task 4 Step 2: if `body.embeds` present, require the author (message owner) `is_bot`, validate, `set_message_embeds`, and include on the response. If the edit request type lacks `embeds`, add it (Task 3 Step 2 note).

- [ ] **Step 2: Write integration tests**
Create `server/tests/integration/embeds_http.rs`:
```rust
//! Integration tests for bot-authored message embeds.
//! Run: `cargo test --test integration embeds_http`

use axum::body::Body;
use axum::http::{Method, StatusCode};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::helpers::{
    add_guild_member, body_to_json, create_channel, create_guild_with_default_role,
    create_test_user, generate_access_token, TestApp,
};
use vc_server::permissions::GuildPermissions;

/// Make an existing user a bot (sets users.is_bot).
async fn make_bot(pool: &PgPool, user_id: Uuid) {
    sqlx::query("UPDATE users SET is_bot = true WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .expect("make bot");
}

fn post_message(app: &TestApp, token: &str, channel: Uuid, body: serde_json::Value) -> axum::http::Request<Body> {
    TestApp::request(Method::POST, &format!("/api/messages/channel/{channel}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[sqlx::test]
async fn bot_can_post_embed(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::SEND_MESSAGES | GuildPermissions::VIEW_CHANNEL).await;
    let channel = create_channel(&pool, guild, "general").await;
    make_bot(&pool, owner).await;
    let token = generate_access_token(&app.config, owner);

    let body = json!({
        "content": "see card",
        "encrypted": false,
        "embeds": [{ "title": "Hello", "description": "world", "color": 16711680 }]
    });
    let res = app.oneshot(post_message(&app, &token, channel, body)).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let j = body_to_json(res).await;
    assert_eq!(j["embeds"][0]["title"], "Hello");
}

#[sqlx::test]
async fn human_cannot_post_embed(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::SEND_MESSAGES | GuildPermissions::VIEW_CHANNEL).await;
    let channel = create_channel(&pool, guild, "general").await;
    let token = generate_access_token(&app.config, owner); // NOT a bot

    let body = json!({ "content": "x", "encrypted": false, "embeds": [{ "title": "nope" }] });
    let res = app.oneshot(post_message(&app, &token, channel, body)).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn oversized_embed_rejected(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::SEND_MESSAGES | GuildPermissions::VIEW_CHANNEL).await;
    let channel = create_channel(&pool, guild, "general").await;
    make_bot(&pool, owner).await;
    let token = generate_access_token(&app.config, owner);

    let body = json!({ "content": "x", "encrypted": false, "embeds": [{ "title": "t".repeat(300) }] });
    let res = app.oneshot(post_message(&app, &token, channel, body)).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn plain_message_has_null_embeds(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::SEND_MESSAGES | GuildPermissions::VIEW_CHANNEL).await;
    let channel = create_channel(&pool, guild, "general").await;
    let token = generate_access_token(&app.config, owner);

    let body = json!({ "content": "plain", "encrypted": false });
    let res = app.oneshot(post_message(&app, &token, channel, body)).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let j = body_to_json(res).await;
    assert!(j.get("embeds").is_none() || j["embeds"].is_null());
}
```
Note: `"t".repeat(300)` is invalid JSON via `json!` — replace with a pre-built `let big = "t".repeat(300);` then `json!({..., "embeds":[{"title": big}]})`. Fix inline when writing.

- [ ] **Step 3: Register the test module** — in `server/tests/integration/main.rs` add `mod embeds_http;` (alphabetical).

- [ ] **Step 4: Run**
Run:
```bash
SQLX_OFFLINE=true DATABASE_URL="postgresql://voicechat:voicechat_dev@localhost:5433/voicechat" cargo test -p vc-server --test integration embeds_http
```
Expected: 4 pass. If `human_cannot_post_embed` returns 400 instead of 403 (validation ordering), ensure the bot-check runs before validation.

- [ ] **Step 5: fmt + clippy + commit**
```bash
cargo +nightly fmt --all
SQLX_OFFLINE=true cargo clippy -p vc-server --all-targets 2>&1 | grep -cE "^warning|^error"
git add server/src/chat/messages.rs server/tests/integration/embeds_http.rs server/tests/integration/main.rs
git commit -m "feat(chat): embeds on message edit + integration tests"
```

---

## Task 6: Client embed renderer

**Files:** create `client/src/components/messages/MessageEmbeds.tsx` + test; modify the message type + message item.

- [ ] **Step 1: Locate the message renderer + sanitizer**
Run:
```bash
grep -rln "createIsolatedPurifier\|sanitizer" client/src/lib
grep -rln "MessageItem\|message.content\|renderMarkdown" client/src/components/messages | head
grep -n "interface Message\b" client/src/lib/types/*.ts
```
Note the sanitizer import path (from #631, `client/src/lib/sanitizer.ts`) and the message-item file + `Message` type file.

- [ ] **Step 2: Add `embeds` to the client Message type**
In the `Message` interface (found in Step 1), add:
```typescript
  embeds?: MessageEmbed[];
```
and define near it:
```typescript
export interface MessageEmbed {
  title?: string;
  description?: string;
  url?: string;
  color?: number;
  author?: { name: string; url?: string; icon_url?: string };
  fields?: { name: string; value: string; inline?: boolean }[];
  image?: string;
  thumbnail?: string;
  footer?: { text: string; icon_url?: string };
  timestamp?: string;
}
```

- [ ] **Step 3: Write the renderer**
Create `client/src/components/messages/MessageEmbeds.tsx`:
```tsx
import { Component, For, Show } from "solid-js";
import type { MessageEmbed } from "@/lib/types";
import { sanitizeMessageHtml } from "@/lib/sanitizer"; // adjust to the real export from #631

/** Render bot embed cards. All embed text is sanitized (attacker-influenced). */
const MessageEmbeds: Component<{ embeds: MessageEmbed[] }> = (props) => {
  const colorHex = (c?: number) =>
    c === undefined ? "var(--color-accent-primary)" : `#${(c & 0xffffff).toString(16).padStart(6, "0")}`;
  return (
    <div class="flex flex-col gap-2 mt-1">
      <For each={props.embeds}>
        {(e) => (
          <div
            class="rounded-md p-3 text-sm max-w-[520px]"
            style={{
              "background-color": "var(--color-surface-layer1)",
              "border-left": `4px solid ${colorHex(e.color)}`,
            }}
          >
            <Show when={e.author}>
              <div class="text-text-secondary text-xs mb-1">{e.author!.name}</div>
            </Show>
            <Show when={e.title}>
              <div class="font-semibold text-text-primary">
                <Show when={e.url} fallback={<span>{e.title}</span>}>
                  <a href={e.url} target="_blank" rel="noopener noreferrer nofollow" class="text-accent-primary hover:underline">{e.title}</a>
                </Show>
              </div>
            </Show>
            <Show when={e.description}>
              <div class="text-text-primary/90 mt-1" innerHTML={sanitizeMessageHtml(e.description!)} />
            </Show>
            <Show when={e.fields && e.fields.length > 0}>
              <div class="grid grid-cols-2 gap-2 mt-2">
                <For each={e.fields}>
                  {(f) => (
                    <div classList={{ "col-span-2": !f.inline }}>
                      <div class="text-text-primary font-medium text-xs">{f.name}</div>
                      <div class="text-text-secondary text-xs" innerHTML={sanitizeMessageHtml(f.value)} />
                    </div>
                  )}
                </For>
              </div>
            </Show>
            <Show when={e.image}>
              <img src={e.image} alt="" class="mt-2 rounded max-w-full" loading="lazy" />
            </Show>
            <Show when={e.footer}>
              <div class="text-text-muted text-xs mt-2">{e.footer!.text}</div>
            </Show>
          </div>
        )}
      </For>
    </div>
  );
};

export default MessageEmbeds;
```
(Adjust `sanitizeMessageHtml` to the actual export name from `client/src/lib/sanitizer.ts`. If embeds description should be markdown-rendered like message content, run it through the same markdown+sanitize pipeline the message body uses.)

- [ ] **Step 4: Render it in the message item**
In the message-item component (Step 1), after the message content block, add:
```tsx
<Show when={props.message.embeds && props.message.embeds.length > 0}>
  <MessageEmbeds embeds={props.message.embeds!} />
</Show>
```
and import `MessageEmbeds`.

- [ ] **Step 5: Write a sanitization test**
Create `client/src/components/messages/__tests__/messageEmbeds.test.tsx`:
```tsx
import { describe, it, expect } from "vitest";
import { render } from "@solidjs/testing-library";
import MessageEmbeds from "@/components/messages/MessageEmbeds";

describe("MessageEmbeds", () => {
  it("renders title and description, strips scripts", () => {
    const { getByText, container } = render(() => (
      <MessageEmbeds embeds={[{ title: "T", description: "hi <script>alert(1)</script>" }]} />
    ));
    expect(getByText("T")).toBeTruthy();
    expect(container.querySelector("script")).toBeNull();
  });
});
```

- [ ] **Step 6: Run client build + test**
Run:
```bash
cd client && bun run test:run messageEmbeds && bun run build
```
Expected: test passes, build clean. (If `@solidjs/testing-library` isn't a dep, assert on `sanitizeMessageHtml("...")` output directly instead of rendering.)

- [ ] **Step 7: Commit**
```bash
git add client/src/components/messages/MessageEmbeds.tsx client/src/components/messages/__tests__/messageEmbeds.test.tsx client/src/lib/types
git commit -m "feat(client): render bot message embeds (sanitized)"
```

---

## Task 7: Changelog, gate, PR

- [ ] **Step 1: Changelog** — under `[Unreleased] → ### Added` in `CHANGELOG.md`:
```markdown
- **Rich embeds**: bot-authored messages can now include rich embed cards (title, description, color, author, fields, image/thumbnail, footer). Embeds are validated server-side (Discord-parity size caps, https-only URLs) and rendered through the isolated HTML sanitizer.
```

- [ ] **Step 2: Full gate**
Run:
```bash
cargo +nightly fmt --all
SQLX_OFFLINE=true cargo clippy -p vc-server --all-targets 2>&1 | grep -cE "^warning|^error"   # expect 0
SQLX_OFFLINE=true DATABASE_URL="postgresql://voicechat:voicechat_dev@localhost:5433/voicechat" cargo test -p vc-server --test integration embeds_http
cd client && bun run test:run && bun run build
```

- [ ] **Step 3: Push + PR**
```bash
git add CHANGELOG.md && git commit -m "docs(changelog): rich embeds"
git push -u origin feature/rich-embeds
gh pr create --title "feat: rich embeds (bot-authored)" --body "<summary + test plan>"
```

---

## Self-Review

- **Spec coverage (embeds portion):** data model JSONB (Task 1); typed structs + all size caps + https URLs + color clamp (Task 2); bot-only gating + validation-on-write (Tasks 4/5); ride existing create/edit API, no new REST routes (Tasks 4/5); serialize on all read paths (Task 4 Step 3); client sanitized renderer (Task 6); `EMBED_LINKS` untouched (no permission added — matches spec). Components + interaction loop are explicitly increment 2 (separate plan) — the migration provisions the column.
- **Placeholder scan:** the two "adjust to actual name" notes (AuthorProfile.is_bot source, sanitizer export) are locate-then-match instructions with a concrete grep in the same task, not placeholders; every code step has real code.
- **Type consistency:** `Embed`/`EmbedField`/`EmbedAuthor`/`EmbedFooter` fields match between the Rust structs (Task 2), the request/response wiring (Task 3), and the client `MessageEmbed` type (Task 6). `validate_embeds(&mut [Embed])` signature is consistent across call sites. `set_message_embeds` name consistent (Task 4 def, Task 4/5 use).
