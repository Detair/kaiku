# Custom Status Backend Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add server-side custom status (text + emoji + expiry) to the presence system with real-time WebSocket broadcast and periodic expiry sweep.

**Architecture:** JSONB column on `users` table, dedicated `SetCustomStatus`/`CustomStatusUpdate` WebSocket events following the existing `SetActivity`/`RichPresenceUpdate` pattern, 60-second periodic sweep for expired statuses, holistic connect flow fix sending all presence data.

**Tech Stack:** Rust (axum, sqlx, tokio, fred, serde), PostgreSQL JSONB, Valkey pub/sub, Solid.js client, `unicode-segmentation` crate

**Design:** [Custom Status Backend Design](2026-03-07-custom-status-backend-design.md)

---

## Task 1: Database Migration

**Files:**
- Create: `server/migrations/20260307000000_add_custom_status.sql`

**Step 1: Create the migration file**

```sql
-- Add custom_status JSONB column to users table.
-- Structure: {"text": "...", "emoji": "...", "expires_at": "2026-..."}
ALTER TABLE users ADD COLUMN custom_status JSONB;

-- Partial index for periodic expiry sweep queries.
CREATE INDEX idx_users_custom_status_expires_at
  ON users ((custom_status->>'expires_at'))
  WHERE custom_status IS NOT NULL
    AND custom_status->>'expires_at' IS NOT NULL;
```

**Step 2: Run migration locally**

```bash
DATABASE_URL="postgresql://voicechat:voicechat_dev@localhost:5433/voicechat" sqlx migrate run --source server/migrations
```

Expected: migration applies successfully.

**Step 3: Commit**

```bash
git add server/migrations/20260307000000_add_custom_status.sql
git commit -m "feat(db): add custom_status JSONB column to users table"
```

---

## Task 2: Unicode Validation Helper + CustomStatus Type

**Files:**
- Modify: `server/src/presence/types.rs` (add `validate_unicode_text()`, `CustomStatus` struct, update `Activity::validate()`)
- Modify: `server/Cargo.toml` (add `unicode-segmentation` dependency)

**Step 1: Add `unicode-segmentation` to server Cargo.toml**

Add to `[dependencies]` in `server/Cargo.toml`:

```toml
unicode-segmentation = "1.12"
```

**Step 2: Write tests for unicode validation**

Add to the `#[cfg(test)] mod tests` block in `server/src/presence/types.rs` (after existing tests around line 68):

```rust
#[test]
fn test_validate_unicode_text_valid() {
    assert!(validate_unicode_text("Hello world", 128).is_ok());
    assert!(validate_unicode_text("Café ☕", 128).is_ok());
    assert!(validate_unicode_text("a\u{0301}", 128).is_ok()); // e with combining accent (1 combining mark)
}

#[test]
fn test_validate_unicode_text_too_long() {
    let long = "a".repeat(129);
    assert!(validate_unicode_text(&long, 128).is_err());
}

#[test]
fn test_validate_unicode_text_control_chars() {
    assert!(validate_unicode_text("hello\x00world", 128).is_err());
    assert!(validate_unicode_text("hello\x1Fworld", 128).is_err());
}

#[test]
fn test_validate_unicode_text_format_chars() {
    // Zero-width space
    assert!(validate_unicode_text("hello\u{200B}world", 128).is_err());
    // Zero-width joiner
    assert!(validate_unicode_text("hello\u{200D}world", 128).is_err());
    // Zero-width non-joiner
    assert!(validate_unicode_text("hello\u{200C}world", 128).is_err());
}

#[test]
fn test_validate_unicode_text_bidi_overrides() {
    assert!(validate_unicode_text("hello\u{202E}world", 128).is_err());
    assert!(validate_unicode_text("hello\u{202D}world", 128).is_err());
    assert!(validate_unicode_text("hello\u{202C}world", 128).is_err());
}

#[test]
fn test_validate_unicode_text_combining_mark_limit() {
    // 3 combining marks on one base: OK
    let ok = "a\u{0301}\u{0302}\u{0303}";
    assert!(validate_unicode_text(ok, 128).is_ok());

    // 4 combining marks on one base: rejected (Zalgo)
    let zalgo = "a\u{0301}\u{0302}\u{0303}\u{0304}";
    assert!(validate_unicode_text(zalgo, 128).is_err());
}

#[test]
fn test_custom_status_validate_valid() {
    let status = CustomStatus {
        text: "In a meeting".to_string(),
        emoji: Some("📅".to_string()),
        expires_at: Some(Utc::now() + chrono::Duration::hours(1)),
    };
    assert!(status.validate().is_ok());
}

#[test]
fn test_custom_status_validate_no_emoji() {
    let status = CustomStatus {
        text: "Busy".to_string(),
        emoji: None,
        expires_at: None,
    };
    assert!(status.validate().is_ok());
}

#[test]
fn test_custom_status_validate_empty_text() {
    let status = CustomStatus {
        text: "   ".to_string(),
        emoji: None,
        expires_at: None,
    };
    assert!(status.validate().is_err());
}

#[test]
fn test_custom_status_validate_text_too_long() {
    let status = CustomStatus {
        text: "a".repeat(129),
        emoji: None,
        expires_at: None,
    };
    assert!(status.validate().is_err());
}

#[test]
fn test_custom_status_validate_emoji_too_many_graphemes() {
    let status = CustomStatus {
        text: "hi".to_string(),
        emoji: Some("🎮🎵🎨🎭🎪🎫🎬🎤🎧🎼🎹".to_string()), // 11 emoji
        expires_at: None,
    };
    assert!(status.validate().is_err());
}

#[test]
fn test_custom_status_validate_expires_at_in_past() {
    let status = CustomStatus {
        text: "hi".to_string(),
        emoji: None,
        expires_at: Some(Utc::now() - chrono::Duration::hours(1)),
    };
    assert!(status.validate().is_err());
}

#[test]
fn test_custom_status_serialization() {
    let status = CustomStatus {
        text: "In queue".to_string(),
        emoji: Some("🎮".to_string()),
        expires_at: None,
    };
    let json = serde_json::to_string(&status).unwrap();
    assert!(json.contains("\"text\":\"In queue\""));
    assert!(json.contains("\"emoji\":\"🎮\""));
    assert!(!json.contains("\"expires_at\"")); // skipped when None
}
```

**Step 3: Run tests to verify they fail**

```bash
cd server && SQLX_OFFLINE=true cargo test -p vc-server --lib presence::types::tests -- --nocapture
```

Expected: compilation errors — `validate_unicode_text` and `CustomStatus` don't exist yet.

**Step 4: Implement `validate_unicode_text()` and `CustomStatus`**

Add to `server/src/presence/types.rs`, before the `Activity` struct (after line 10):

```rust
use unicode_segmentation::UnicodeSegmentation;

/// Maximum length for custom status text.
pub const MAX_CUSTOM_STATUS_TEXT_LEN: usize = 128;

/// Maximum grapheme clusters for custom status emoji.
pub const MAX_CUSTOM_STATUS_EMOJI_GRAPHEMES: usize = 10;

/// Maximum combining marks per base character.
const MAX_COMBINING_MARKS_PER_BASE: usize = 3;

/// Returns true if the character is an unsafe Unicode format or override character.
fn is_unsafe_unicode(c: char) -> bool {
    c.is_control() && c != ' '
        || matches!(c, '\u{200B}' | '\u{200C}' | '\u{200D}') // zero-width chars
        || matches!(c, '\u{202C}' | '\u{202D}' | '\u{202E}') // bidi overrides
}

/// Validate text for unsafe Unicode characters and combining mark abuse.
///
/// Reusable across custom status, activity names, display names, etc.
pub fn validate_unicode_text(text: &str, max_chars: usize) -> Result<(), &'static str> {
    if text.chars().count() > max_chars {
        return Err("Text too long");
    }

    if text.chars().any(is_unsafe_unicode) {
        return Err("Text contains invalid characters");
    }

    // Check for Zalgo-style combining mark abuse
    let mut combining_count: usize = 0;
    for c in text.chars() {
        if unicode_general_category_is_mark(c) {
            combining_count += 1;
            if combining_count > MAX_COMBINING_MARKS_PER_BASE {
                return Err("Too many combining marks on a single character");
            }
        } else {
            combining_count = 0;
        }
    }

    Ok(())
}

/// Check if a character is a Unicode combining mark (category M).
fn unicode_general_category_is_mark(c: char) -> bool {
    // Unicode general categories Mn (nonspacing), Mc (spacing combining), Me (enclosing)
    use std::char;
    matches!(
        unicode_general_category(c),
        UnicodeGeneralCategory::Mn | UnicodeGeneralCategory::Mc | UnicodeGeneralCategory::Me
    )
}

/// Minimal Unicode general category detection for combining marks.
///
/// Uses the `char` method from std — checking if a char is a combining mark
/// by testing if it is in the Unicode "Mark" category.
fn unicode_general_category_is_combining(c: char) -> bool {
    // Characters in the range U+0300..U+036F (Combining Diacritical Marks)
    // and other combining mark blocks.
    // Using a pragmatic approach: check common combining mark ranges.
    let cp = c as u32;
    matches!(cp,
        0x0300..=0x036F   // Combining Diacritical Marks
        | 0x0483..=0x0489 // Combining Cyrillic
        | 0x0591..=0x05BD // Hebrew combining
        | 0x05BF          // Hebrew
        | 0x05C1..=0x05C2 // Hebrew
        | 0x05C4..=0x05C5 // Hebrew
        | 0x05C7          // Hebrew
        | 0x0610..=0x061A // Arabic
        | 0x064B..=0x065F // Arabic combining
        | 0x0670          // Arabic
        | 0x06D6..=0x06DC // Arabic
        | 0x06DF..=0x06E4 // Arabic
        | 0x06E7..=0x06E8 // Arabic
        | 0x06EA..=0x06ED // Arabic
        | 0x0730..=0x074A // Syriac
        | 0x07A6..=0x07B0 // Thaana
        | 0x07EB..=0x07F3 // NKo
        | 0x0816..=0x0819 // Samaritan
        | 0x081B..=0x0823 // Samaritan
        | 0x0825..=0x0827 // Samaritan
        | 0x0829..=0x082D // Samaritan
        | 0x0859..=0x085B // Mandaic
        | 0x0898..=0x089F // Arabic extended
        | 0x08CA..=0x08E1 // Arabic extended
        | 0x08E3..=0x0903 // Arabic/Devanagari
        | 0x093A..=0x093C // Devanagari
        | 0x093E..=0x094F // Devanagari
        | 0x0951..=0x0957 // Devanagari
        | 0x0962..=0x0963 // Devanagari
        | 0x0981..=0x0983 // Bengali
        | 0x09BC          // Bengali
        | 0x09BE..=0x09C4 // Bengali
        | 0x0A01..=0x0A03 // Gurmukhi
        | 0x0A3C          // Gurmukhi
        | 0x0A3E..=0x0A42 // Gurmukhi
        | 0x0B01..=0x0B03 // Oriya
        | 0x0B3C          // Oriya
        | 0x0C00..=0x0C04 // Telugu
        | 0x0D00..=0x0D03 // Malayalam
        | 0x0D3B..=0x0D3C // Malayalam
        | 0x0D3E..=0x0D44 // Malayalam
        | 0x0E31          // Thai
        | 0x0E34..=0x0E3A // Thai
        | 0x0E47..=0x0E4E // Thai
        | 0x0EB1          // Lao
        | 0x0EB4..=0x0EBC // Lao
        | 0x0EC8..=0x0ECE // Lao
        | 0x0F18..=0x0F19 // Tibetan
        | 0x0F35          // Tibetan
        | 0x0F37          // Tibetan
        | 0x0F39          // Tibetan
        | 0x0F3E..=0x0F3F // Tibetan
        | 0x0F71..=0x0F84 // Tibetan
        | 0x0F86..=0x0F87 // Tibetan
        | 0x0F8D..=0x0F97 // Tibetan
        | 0x0F99..=0x0FBC // Tibetan
        | 0x0FC6          // Tibetan
        | 0x1DC0..=0x1DFF // Combining Diacritical Marks Supplement
        | 0x20D0..=0x20FF // Combining Diacritical Marks for Symbols
        | 0xFE00..=0xFE0F // Variation Selectors
        | 0xFE20..=0xFE2F // Combining Half Marks
    )
}
```

**Wait — this approach is too complex and brittle.** Instead, use a simpler method: check if the character has Unicode general category "Mark" by leveraging the fact that combining marks have no width. A cleaner approach uses `char::is_alphanumeric()` inverted logic, but the most robust and maintainable approach is to use the `unicode-general-category` crate or simply check ranges pragmatically.

**Revised simpler approach** — replace the combining mark detection with:

```rust
/// Check if a Unicode character is a combining mark.
///
/// Covers the main combining mark blocks used in Zalgo text attacks.
/// Not exhaustive for all Unicode scripts, but catches the abuse vectors.
fn is_combining_mark(c: char) -> bool {
    let cp = c as u32;
    matches!(cp,
        0x0300..=0x036F   // Combining Diacritical Marks
        | 0x1AB0..=0x1AFF // Combining Diacritical Marks Extended
        | 0x1DC0..=0x1DFF // Combining Diacritical Marks Supplement
        | 0x20D0..=0x20FF // Combining Diacritical Marks for Symbols
        | 0xFE20..=0xFE2F // Combining Half Marks
    )
}

/// Validate text for unsafe Unicode characters and combining mark abuse.
pub fn validate_unicode_text(text: &str, max_chars: usize) -> Result<(), &'static str> {
    if text.chars().count() > max_chars {
        return Err("Text too long");
    }

    if text.chars().any(is_unsafe_unicode) {
        return Err("Text contains invalid characters");
    }

    // Check for Zalgo-style combining mark abuse
    let mut combining_count: usize = 0;
    for c in text.chars() {
        if is_combining_mark(c) {
            combining_count += 1;
            if combining_count > MAX_COMBINING_MARKS_PER_BASE {
                return Err("Too many combining marks on a single character");
            }
        } else {
            combining_count = 0;
        }
    }

    Ok(())
}
```

Add the `CustomStatus` struct and its validation after the helper functions:

```rust
/// Custom status set by a user (text + optional emoji + optional expiry).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct CustomStatus {
    /// Display text for the custom status.
    pub text: String,
    /// Optional emoji (max 10 grapheme clusters).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
    /// When the custom status expires (UTC).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

impl CustomStatus {
    /// Validate custom status data. Returns an error message if invalid.
    pub fn validate(&self) -> Result<(), &'static str> {
        let trimmed = self.text.trim();
        if trimmed.is_empty() {
            return Err("Custom status text cannot be empty");
        }
        validate_unicode_text(trimmed, MAX_CUSTOM_STATUS_TEXT_LEN)?;

        if let Some(ref emoji) = self.emoji {
            if emoji.graphemes(true).count() > MAX_CUSTOM_STATUS_EMOJI_GRAPHEMES {
                return Err("Emoji field too long (max 10 emoji)");
            }
            validate_unicode_text(emoji, MAX_CUSTOM_STATUS_TEXT_LEN)?;
        }

        if let Some(expires_at) = self.expires_at {
            if expires_at <= Utc::now() {
                return Err("Expiry time must be in the future");
            }
        }

        Ok(())
    }
}
```

**Step 5: Update `Activity::validate()` to use `validate_unicode_text()`**

Replace the body of `Activity::validate()` (lines 40-65 of `types.rs`):

```rust
impl Activity {
    /// Validate activity data. Returns an error message if invalid.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.name.is_empty() {
            return Err("Activity name cannot be empty");
        }
        validate_unicode_text(&self.name, MAX_ACTIVITY_NAME_LEN)?;
        if let Some(ref details) = self.details {
            validate_unicode_text(details, MAX_ACTIVITY_DETAILS_LEN)?;
        }
        Ok(())
    }
}
```

**Step 6: Run tests to verify they pass**

```bash
cd server && SQLX_OFFLINE=true cargo test -p vc-server --lib presence::types::tests -- --nocapture
```

Expected: all tests pass.

**Step 7: Commit**

```bash
git add server/Cargo.toml server/Cargo.lock server/src/presence/types.rs
git commit -m "feat(api): add CustomStatus type with unicode validation helper

Shared validate_unicode_text() rejects control chars, format chars,
bidi overrides, and Zalgo-style combining mark abuse. Activity::validate()
updated to use the shared helper."
```

---

## Task 3: Display Name Validation Hardening

**Files:**
- Modify: `server/src/auth/handlers.rs` (add unicode validation to display name updates)

**Step 1: Read current display name validation**

Read `server/src/auth/handlers.rs` around lines 200-215 (`UpdateProfileRequest`) and lines 1127-1157 (the `update_profile` handler) to understand the current validation.

**Step 2: Add unicode validation to display name**

In the `update_profile` handler (around line 1127), after the existing `body.validate()?` call, add validation for display_name:

```rust
if let Some(ref display_name) = body.display_name {
    crate::presence::validate_unicode_text(display_name, 64)
        .map_err(|e| {
            (
                axum::http::StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({ "error": e })),
            )
        })?;
}
```

**Step 3: Run existing tests**

```bash
cd server && SQLX_OFFLINE=true cargo test -p vc-server -- --nocapture
```

Expected: all existing tests pass (the validation is stricter but existing test data should be clean).

**Step 4: Commit**

```bash
git add server/src/auth/handlers.rs
git commit -m "fix(auth): add unicode safety validation to display name updates

Rejects control chars, format chars, bidi overrides, and Zalgo-style
combining mark abuse in display names using shared validate_unicode_text()."
```

---

## Task 4: WebSocket Events — SetCustomStatus + CustomStatusUpdate

**Files:**
- Modify: `server/src/ws/mod.rs` (add events to `ClientEvent` and `ServerEvent` enums)

**Step 1: Add `SetCustomStatus` to `ClientEvent`**

In `server/src/ws/mod.rs`, add after `SetStatus` (line 197), before `AdminSubscribe` (line 199):

```rust
/// Set or clear custom status (text + emoji + optional expiry).
SetCustomStatus {
    custom_status: Option<crate::presence::CustomStatus>,
},
```

**Step 2: Add `CustomStatusUpdate` to `ServerEvent`**

In `server/src/ws/mod.rs`, add after `RichPresenceUpdate` (line 579), before `Patch` (line 583):

```rust
/// Custom status update for a user.
CustomStatusUpdate {
    user_id: Uuid,
    custom_status: Option<crate::presence::CustomStatus>,
},
```

**Step 3: Update blocked-user filtering in pub/sub handler**

In the presence event handler (around line 1668-1686), add `CustomStatusUpdate` to the match arm:

```rust
let should_filter = match &event {
    ServerEvent::PresenceUpdate { user_id: uid, .. }
    | ServerEvent::RichPresenceUpdate { user_id: uid, .. }
    | ServerEvent::CustomStatusUpdate { user_id: uid, .. } => {
        params.blocked_users.read().await.contains(uid)
    }
    _ => false,
};
```

**Step 4: Verify compilation**

```bash
cd server && SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
```

Expected: compiles with no errors (unused variant warnings are acceptable for now since the handler isn't written yet; clippy should be clean).

**Step 5: Commit**

```bash
git add server/src/ws/mod.rs
git commit -m "feat(ws): add SetCustomStatus and CustomStatusUpdate WebSocket events"
```

---

## Task 5: SetCustomStatus Handler

**Files:**
- Modify: `server/src/ws/mod.rs` (add handler in the `ClientEvent` match block, add `CustomStatusState`)

**Step 1: Add `CustomStatusState` struct**

Add near `ActivityState` (around line 49):

```rust
/// State for custom status rate limiting and deduplication.
#[derive(Default)]
pub struct CustomStatusState {
    /// Last custom status update timestamp.
    last_update: Option<Instant>,
    /// Last custom status data for deduplication.
    last_custom_status: Option<Option<crate::presence::CustomStatus>>,
}
```

**Step 2: Add `custom_status_state` variable in the WebSocket connection handler**

Find where `ActivityState` is created (search for `ActivityState::default()` or `let mut activity_state`) and add alongside it:

```rust
let mut custom_status_state = CustomStatusState::default();
```

**Step 3: Add the handler in the `ClientEvent` match block**

Add after the `SetStatus` handler (after line 1483), before `AdminSubscribe`:

```rust
ClientEvent::SetCustomStatus { custom_status } => {
    // Validate if setting (not clearing)
    if let Some(ref cs) = custom_status {
        cs.validate()
            .map_err(|e| format!("Invalid custom status: {e}"))?;
    }

    // Rate limiting
    let now = Instant::now();
    if let Some(last_update) = custom_status_state.last_update {
        let elapsed = now.duration_since(last_update);
        if elapsed < ACTIVITY_UPDATE_INTERVAL {
            let remaining = ACTIVITY_UPDATE_INTERVAL.saturating_sub(elapsed);
            return Err(format!(
                "Rate limited: wait {} seconds before next custom status update",
                remaining.as_secs() + 1
            ).into());
        }
    }

    // Deduplication
    if custom_status_state.last_custom_status.as_ref() == Some(&custom_status) {
        debug!("Skipping custom status update: unchanged for user={}", user_id);
        return Ok(());
    }

    // Persist to database
    let json_value = custom_status
        .as_ref()
        .and_then(|cs| serde_json::to_value(cs).ok());
    sqlx::query("UPDATE users SET custom_status = $1 WHERE id = $2")
        .bind(&json_value)
        .bind(user_id)
        .execute(&state.db)
        .await
        .map_err(|e| format!("Failed to update custom status: {e}"))?;

    // Update rate limiting state
    custom_status_state.last_update = Some(now);
    custom_status_state.last_custom_status = Some(custom_status.clone());

    // Broadcast to presence subscribers
    let event = ServerEvent::CustomStatusUpdate {
        user_id,
        custom_status,
    };
    broadcast_presence_update(state, user_id, &event).await;
    debug!("User {} updated custom status", user_id);
}
```

**Step 4: Verify compilation and clippy**

```bash
cd server && SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
```

Expected: clean compilation. If the new sqlx query needs offline data, run:

```bash
DATABASE_URL="postgresql://voicechat:voicechat_dev@localhost:5433/voicechat" cargo sqlx prepare --workspace
```

**Step 5: Commit**

```bash
git add server/src/ws/mod.rs
# If .sqlx files changed:
git add .sqlx/
git commit -m "feat(ws): implement SetCustomStatus handler with validation and rate limiting"
```

---

## Task 6: Hide Custom Status on Offline/Invisible

**Files:**
- Modify: `server/src/ws/mod.rs` (extend `SetStatus` handler)

**Step 1: Extend `SetStatus` handler to hide custom status**

In the `SetStatus` handler (line 1468-1483), after the existing `broadcast_presence_update` call (line 1481), add:

```rust
// Hide custom status when going offline/invisible
if matches!(status, crate::db::UserStatus::Offline) {
    let hide_event = ServerEvent::CustomStatusUpdate {
        user_id,
        custom_status: None,
    };
    broadcast_presence_update(state, user_id, &hide_event).await;
}
```

**Step 2: Verify compilation**

```bash
cd server && SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
```

**Step 3: Commit**

```bash
git add server/src/ws/mod.rs
git commit -m "feat(ws): hide custom status from friends when user goes offline/invisible"
```

---

## Task 7: Holistic Connect Flow — FriendPresenceSnapshot

**Files:**
- Modify: `server/src/ws/mod.rs` (replace `get_friends_presence()`, update connect flow)

**Step 1: Replace `get_friends_presence` with `get_friends_presence_full`**

Replace the `get_friends_presence` function (around lines 1743-1769) with:

```rust
/// Snapshot of a friend's full presence state for the connect flow.
#[derive(Debug, sqlx::FromRow)]
struct FriendPresenceSnapshot {
    user_id: Uuid,
    status: String,
    activity: Option<serde_json::Value>,
    custom_status: Option<serde_json::Value>,
}

async fn get_friends_presence(
    db: &sqlx::PgPool,
    user_id: Uuid,
) -> Result<Vec<FriendPresenceSnapshot>, sqlx::Error> {
    let rows: Vec<FriendPresenceSnapshot> = sqlx::query_as(
        r"
        SELECT
            CASE
                WHEN f.requester_id = $1 THEN f.addressee_id
                ELSE f.requester_id
            END as user_id,
            u.status::text as status,
            u.activity,
            u.custom_status
        FROM friendships f
        JOIN users u ON u.id = CASE
            WHEN f.requester_id = $1 THEN f.addressee_id
            ELSE f.requester_id
        END
        WHERE (f.requester_id = $1 OR f.addressee_id = $1)
          AND f.status = 'accepted'
        ",
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;

    Ok(rows)
}
```

**Step 2: Update the connect flow to send all presence data**

In the connect flow (around lines 1120-1138), replace the loop that sends initial friend presence:

```rust
match get_friends_presence(&state.db, user_id).await {
    Ok(snapshots) => {
        for snap in snapshots {
            // Always send base presence
            let presence_event = ServerEvent::PresenceUpdate {
                user_id: snap.user_id,
                status: snap.status.clone(),
            };
            if tx.send(presence_event).await.is_err() {
                break;
            }

            let is_offline = snap.status == "offline";

            // Send activity if present and user is not offline
            if !is_offline {
                if let Some(activity_json) = snap.activity {
                    if let Ok(activity) = serde_json::from_value::<crate::presence::Activity>(activity_json) {
                        let activity_event = ServerEvent::RichPresenceUpdate {
                            user_id: snap.user_id,
                            activity: Some(activity),
                        };
                        if tx.send(activity_event).await.is_err() {
                            break;
                        }
                    }
                }

                // Send custom status if present and user is not offline
                if let Some(cs_json) = snap.custom_status {
                    if let Ok(cs) = serde_json::from_value::<crate::presence::CustomStatus>(cs_json) {
                        let cs_event = ServerEvent::CustomStatusUpdate {
                            user_id: snap.user_id,
                            custom_status: Some(cs),
                        };
                        if tx.send(cs_event).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    }
    Err(e) => {
        warn!(
            "Failed to fetch initial friend presence for {}: {}",
            user_id, e
        );
    }
}
```

**Step 3: Update sqlx offline data if needed**

```bash
DATABASE_URL="postgresql://voicechat:voicechat_dev@localhost:5433/voicechat" cargo sqlx prepare --workspace
```

**Step 4: Verify compilation**

```bash
cd server && SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
```

**Step 5: Commit**

```bash
git add server/src/ws/mod.rs
# If .sqlx files changed:
git add .sqlx/
git commit -m "feat(ws): send full presence snapshot on connect (status + activity + custom_status)

Fixes pre-existing gap where friends' activities and custom statuses
were not sent during the initial connect flow."
```

---

## Task 8: Expiry Sweep Background Task

**Files:**
- Modify: `server/src/ws/mod.rs` (add `spawn_custom_status_sweep` function)
- Modify: `server/src/main.rs` (spawn the task, add to shutdown cleanup)

**Step 1: Add sweep function to `ws/mod.rs`**

Add a public function at module level (near the bottom of the file, before private helpers):

```rust
/// Spawn a background task that periodically clears expired custom statuses.
///
/// Runs every 60 seconds. For each expired status:
/// 1. Clears `custom_status` to NULL in the database
/// 2. Broadcasts `CustomStatusUpdate { custom_status: None }` to friends
pub fn spawn_custom_status_sweep(
    db: PgPool,
    redis: fred::clients::Client,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;

            // Find expired custom statuses
            let expired: Vec<(Uuid,)> = match sqlx::query_as(
                r"
                SELECT id FROM users
                WHERE custom_status IS NOT NULL
                  AND custom_status->>'expires_at' IS NOT NULL
                  AND (custom_status->>'expires_at')::timestamptz <= NOW()
                ",
            )
            .fetch_all(&db)
            .await
            {
                Ok(rows) => rows,
                Err(e) => {
                    tracing::warn!(error = %e, "Custom status sweep: query failed");
                    continue;
                }
            };

            if expired.is_empty() {
                continue;
            }

            let user_ids: Vec<Uuid> = expired.into_iter().map(|(id,)| id).collect();
            tracing::debug!(count = user_ids.len(), "Clearing expired custom statuses");

            // Clear in database
            if let Err(e) = sqlx::query(
                "UPDATE users SET custom_status = NULL WHERE id = ANY($1)",
            )
            .bind(&user_ids)
            .execute(&db)
            .await
            {
                tracing::warn!(error = %e, "Custom status sweep: clear failed");
                continue;
            }

            // Broadcast to friends
            for uid in &user_ids {
                let event = ServerEvent::CustomStatusUpdate {
                    user_id: *uid,
                    custom_status: None,
                };
                let json = match serde_json::to_string(&event) {
                    Ok(j) => j,
                    Err(_) => continue,
                };
                let channel = format!("presence:{uid}");
                let _: Result<(), _> = redis.publish(&channel, &json).await;
            }
        }
    })
}
```

**Step 2: Spawn the sweep in `main.rs`**

In `server/src/main.rs`, after the existing background task spawns (after the `db_cleanup_handle` block around line 239), add:

```rust
// Start custom status expiry sweep (every 60 seconds)
let custom_status_sweep_handle = vc_server::ws::spawn_custom_status_sweep(
    db_pool.clone(),
    redis.clone(),
);
```

**Step 3: Add to shutdown cleanup**

In the graceful shutdown section (around line 368-374), add:

```rust
custom_status_sweep_handle.abort();
```

And in the await block (around line 375-380), add:

```rust
let _ = custom_status_sweep_handle.await;
```

**Step 4: Update sqlx offline data**

```bash
DATABASE_URL="postgresql://voicechat:voicechat_dev@localhost:5433/voicechat" cargo sqlx prepare --workspace
```

**Step 5: Verify compilation**

```bash
cd server && SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
```

**Step 6: Commit**

```bash
git add server/src/ws/mod.rs server/src/main.rs
git add .sqlx/
git commit -m "feat(ws): add 60-second custom status expiry sweep

Background task clears expired custom statuses from the database
and broadcasts CustomStatusUpdate to friends via Redis pub/sub."
```

---

## Task 9: Client Type Fix — `expiresAt` to `expires_at`

**Files:**
- Modify: `client/src/lib/types.ts` (rename field)
- Modify: `client/src/stores/presence.ts` (update references)
- Modify: `client/src/components/ui/CustomStatusModal.tsx` (update references)

**Step 1: Rename in `types.ts`**

In `client/src/lib/types.ts` (line 72), change `expiresAt` to `expires_at`:

```typescript
export interface CustomStatus {
  /** Display text for the custom status. */
  text: string;
  /** Optional emoji to show with the status. */
  emoji?: string;
  /** ISO timestamp when the custom status expires. */
  expires_at?: string;
}
```

**Step 2: Update `CustomStatusModal.tsx`**

In `client/src/components/ui/CustomStatusModal.tsx`, in the `handleSave` function (around line 30-44), update the property name from `expiresAt` to `expires_at`:

Find: `expiresAt:`
Replace with: `expires_at:`

Also update `currentStatus?.expiresAt` references if any exist in the component.

**Step 3: Update `presence.ts`**

In `client/src/stores/presence.ts`, update references to `status.expiresAt` (around line 427) to `status.expires_at`:

Find all: `expiresAt`
Replace with: `expires_at`

**Step 4: Run client tests**

```bash
cd client && bun run test:run
```

Expected: existing custom status tests may need updating for the renamed field.

**Step 5: Update test file**

In `client/src/stores/__tests__/presence.test.ts`, update any test data using `expiresAt` to use `expires_at`.

**Step 6: Run tests again**

```bash
cd client && bun run test:run
```

Expected: all tests pass.

**Step 7: Commit**

```bash
git add client/src/lib/types.ts client/src/stores/presence.ts client/src/components/ui/CustomStatusModal.tsx client/src/stores/__tests__/presence.test.ts
git commit -m "refactor(client): rename CustomStatus.expiresAt to expires_at

Aligns with snake_case convention used across all server-client
serialization boundaries."
```

---

## Task 10: Client WebSocket Integration — Send + Receive

**Files:**
- Modify: `client/src/stores/presence.ts` (rewrite `setMyCustomStatus()`, add event handler)
- Modify: `client/src/stores/websocket.ts` (add `custom_status_update` dispatch + Tauri listener)
- Modify: `client/src/lib/tauri.ts` (remove `updateCustomStatus()` workaround)

**Step 1: Add `updateUserCustomStatus` function to presence store**

In `client/src/stores/presence.ts`, add a new exported function near `updateUserActivity()`:

```typescript
/** Handle incoming CustomStatusUpdate from server. */
export function updateUserCustomStatus(
  userId: string,
  customStatus: CustomStatus | null,
): void {
  setPresenceState(
    produce((state) => {
      if (!state.users[userId]) {
        state.users[userId] = { status: "offline" };
      }
      state.users[userId].customStatus = customStatus;
    }),
  );
}
```

**Step 2: Rewrite `setMyCustomStatus()` to use WebSocket**

Replace the body of `setMyCustomStatus()` (around lines 399-441) with:

```typescript
export async function setMyCustomStatus(
  status: CustomStatus | null,
): Promise<void> {
  const user = currentUser();
  if (!user) return;

  try {
    // Send via WebSocket
    const ws = getBrowserWebSocket();
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({
        type: "set_custom_status",
        custom_status: status,
      }));
    }

    // Update local state immediately for responsive UX
    setPresenceState(
      produce((state) => {
        if (!state.users[user.id]) {
          state.users[user.id] = { status: user.status };
        }
        state.users[user.id].customStatus = status;
      }),
    );

    // Update status_message in auth store for fallback display
    const statusMessage = status
      ? `${status.emoji ? `${status.emoji} ` : ""}${status.text}`.trim()
      : null;
    updateUser({ status_message: statusMessage });

    // Manage client-side expiry timer
    if (customStatusClearTimer) {
      clearTimeout(customStatusClearTimer);
      customStatusClearTimer = null;
    }

    if (status?.expires_at) {
      const expiresAtMs = Date.parse(status.expires_at);
      if (Number.isFinite(expiresAtMs)) {
        const delayMs = expiresAtMs - Date.now();
        if (delayMs > 0) {
          customStatusClearTimer = setTimeout(() => {
            void setMyCustomStatus(null);
          }, delayMs);
        }
      }
    }
  } catch (e) {
    console.error("[Presence] Failed to update custom status:", e);
  }
}
```

Note: Check how `getBrowserWebSocket` is imported/accessed. If the WebSocket is sent via a different pattern (e.g., Tauri invoke), adapt accordingly. Read `updateStatus()` in `tauri.ts` (lines 722-741) for the pattern — it sends via Tauri `invoke("update_status")` in Tauri mode or via WebSocket in browser mode. Follow the same pattern.

**Step 3: Add `custom_status_update` to browser WebSocket dispatch**

In `client/src/stores/websocket.ts`, in the `handleServerEvent` switch block (around line 957), add after `rich_presence_update`:

```typescript
case "custom_status_update":
  updateUserCustomStatus(event.user_id, event.custom_status);
  break;
```

Import `updateUserCustomStatus` from `@/stores/presence` at the top of the file.

**Step 4: Add Tauri event listener**

In `client/src/stores/websocket.ts`, in the Tauri listeners section (around line 311), add after `rich_presence_update`:

```typescript
// Custom status events
pending.push(
  listen<{ user_id: string; custom_status: CustomStatus | null }>("ws:custom_status_update", (event) => {
    updateUserCustomStatus(event.payload.user_id, event.payload.custom_status);
  }),
);
```

**Step 5: Remove `updateCustomStatus()` from `tauri.ts`**

In `client/src/lib/tauri.ts`, delete the `updateCustomStatus()` function (around lines 743-782). Also remove any imports of it in other files (presence.ts used to call it).

**Step 6: Run client tests**

```bash
cd client && bun run test:run
```

Fix any test failures from the refactored `setMyCustomStatus()` — the mock for `updateCustomStatus` should be removed and replaced with WebSocket send verification.

**Step 7: Commit**

```bash
git add client/src/stores/presence.ts client/src/stores/websocket.ts client/src/lib/tauri.ts client/src/stores/__tests__/presence.test.ts
git commit -m "feat(client): wire custom status to WebSocket events

setMyCustomStatus() now sends SetCustomStatus via WebSocket instead of
HTTP profile workaround. Handles incoming CustomStatusUpdate events
in both Tauri and browser modes."
```

---

## Task 11: Wire UserPanel handleCustomStatusSave

**Files:**
- Modify: `client/src/components/layout/UserPanel.tsx`

**Step 1: Connect `handleCustomStatusSave` to `setMyCustomStatus`**

In `client/src/components/layout/UserPanel.tsx` (lines 64-67), replace the no-op:

```typescript
const handleCustomStatusSave = async (status: CustomStatus | null) => {
  await setMyCustomStatus(status);
  setShowCustomStatusModal(false);
};
```

Add the import at the top:

```typescript
import { setMyCustomStatus } from "@/stores/presence";
```

**Step 2: Run client tests**

```bash
cd client && bun run test:run
```

Expected: all tests pass.

**Step 3: Commit**

```bash
git add client/src/components/layout/UserPanel.tsx
git commit -m "feat(client): connect custom status modal to presence system

handleCustomStatusSave now calls setMyCustomStatus() instead of being
a no-op. Closes the custom status backend support feature."
```

---

## Task 12: Server Integration Tests

**Files:**
- Create: `server/tests/integration/custom_status.rs`
- Modify: `server/tests/integration/mod.rs` (add module if needed)

**Step 1: Write integration tests**

Create `server/tests/integration/custom_status.rs` with tests covering:

1. **Set custom status** — send `SetCustomStatus` via WS, verify `custom_status` column in DB is not null
2. **Clear custom status** — send `SetCustomStatus { custom_status: None }`, verify DB column is null
3. **Validation: empty text** — send status with empty/whitespace text, expect error
4. **Validation: text too long** — send 129-char text, expect error
5. **Validation: expires_at in past** — send past timestamp, expect error
6. **Rate limiting** — send two updates within 10 seconds, expect rate limit error on second

Follow the existing test patterns from `server/tests/integration/websocket_integration.rs` — use `TestApp`, `CleanupGuard`, and the helper functions.

**Step 2: Run integration tests**

```bash
cd server && cargo test --test integration custom_status -- --nocapture
```

Expected: all tests pass.

**Step 3: Commit**

```bash
git add server/tests/integration/custom_status.rs server/tests/integration/mod.rs
git commit -m "test(ws): add custom status integration tests

Tests set/clear via WS, validation errors (empty text, too long,
past expiry), and rate limiting."
```

---

## Task 13: Expiry Sweep Integration Test

**Files:**
- Modify: `server/tests/integration/custom_status.rs` (add sweep test)

**Step 1: Write sweep test**

Add a test that:
1. Directly inserts a user with an already-expired `custom_status` (expired 5 minutes ago)
2. Calls the sweep logic (or triggers it)
3. Verifies the `custom_status` column is now NULL

Since the sweep runs in a background task, the test should either:
- Call the sweep query directly (extract the query into a testable function), or
- Insert expired data and verify via a short `tokio::time::sleep` + DB query

**Step 2: Run test**

```bash
cd server && cargo test --test integration custom_status::test_expiry_sweep -- --nocapture
```

**Step 3: Commit**

```bash
git add server/tests/integration/custom_status.rs
git commit -m "test(ws): add custom status expiry sweep integration test"
```

---

## Task 14: Update sqlx Offline Cache + Final Verification

**Files:**
- Modify: `.sqlx/*.json` (regenerated)

**Step 1: Regenerate sqlx offline cache**

```bash
DATABASE_URL="postgresql://voicechat:voicechat_dev@localhost:5433/voicechat" cargo sqlx prepare --workspace
```

**Step 2: Full clippy check**

```bash
SQLX_OFFLINE=true cargo clippy -- -D warnings
```

**Step 3: Full test suite**

```bash
cd server && cargo test
cd ../client && bun run test:run
```

**Step 4: Commit any remaining sqlx changes**

```bash
git add .sqlx/
git commit -m "chore: update sqlx offline query cache for custom status"
```

---

## Task 15: Update CHANGELOG and Roadmap

**Files:**
- Modify: `CHANGELOG.md` (add entry under `[Unreleased]`)
- Modify: `docs/developer-guide/project/roadmap.md` (mark item complete)

**Step 1: Add CHANGELOG entry**

Add under `[Unreleased]` → `### Added`:

```markdown
- Custom status backend support: users can set a text + emoji status with optional expiry. Server enforces validation (128-char text, 10-emoji limit, Unicode safety), broadcasts via WebSocket, and runs a 60-second expiry sweep. Friends see custom statuses on connect and in real time. Custom status is hidden when the user is offline/invisible.
```

**Step 2: Update roadmap**

In `docs/developer-guide/project/roadmap.md`, mark the custom status item as complete:

```markdown
- [x] **[Social] Custom Status Backend Support** `Priority: Medium` ✅
```

Update the Phase 6 completion percentage.

**Step 3: Commit**

```bash
git add CHANGELOG.md docs/developer-guide/project/roadmap.md
git commit -m "docs: mark custom status backend support complete in roadmap and changelog"
```

---

## Summary

| Task | Description | Key Files |
|------|-------------|-----------|
| 1 | Database migration | `server/migrations/20260307000000_add_custom_status.sql` |
| 2 | Unicode validation + CustomStatus type | `server/src/presence/types.rs`, `server/Cargo.toml` |
| 3 | Display name validation hardening | `server/src/auth/handlers.rs` |
| 4 | WS event definitions | `server/src/ws/mod.rs` |
| 5 | SetCustomStatus handler | `server/src/ws/mod.rs` |
| 6 | Hide on offline/invisible | `server/src/ws/mod.rs` |
| 7 | Connect flow — FriendPresenceSnapshot | `server/src/ws/mod.rs` |
| 8 | Expiry sweep task | `server/src/ws/mod.rs`, `server/src/main.rs` |
| 9 | Client type rename `expiresAt` → `expires_at` | `client/src/lib/types.ts`, `presence.ts`, `CustomStatusModal.tsx` |
| 10 | Client WS send + receive | `presence.ts`, `websocket.ts`, `tauri.ts` |
| 11 | Wire UserPanel handler | `UserPanel.tsx` |
| 12 | Server integration tests | `server/tests/integration/custom_status.rs` |
| 13 | Expiry sweep test | `server/tests/integration/custom_status.rs` |
| 14 | sqlx offline cache + final verification | `.sqlx/`, full test suite |
| 15 | CHANGELOG + roadmap | `CHANGELOG.md`, `roadmap.md` |
