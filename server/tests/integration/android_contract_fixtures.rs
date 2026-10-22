//! Golden JSON fixtures shared with the Android client.
//!
//! Each test builds a fixed (UUID + timestamp) instance of a real server
//! response type, serializes it with the server's own `Serialize` impl and
//! compares it to a JSON file under
//! `mobile/android/app/src/test/resources/golden/`. The Android unit tests
//! decode the same files, so a wire-format change on either side fails CI.
//!
//! Regenerate the files after an intentional wire change with:
//!
//! ```text
//! KAIKU_UPDATE_GOLDEN=1 SQLX_OFFLINE=true \
//!     cargo test -p vc-server --test integration android_contract_fixtures
//! ```
//!
//! These are plain `#[test]`s: no database or network needed.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use uuid::Uuid;
use vc_server::auth::types::AuthResponse;
use vc_server::chat::types::{
    AttachmentInfo, AuthorProfile, CursorPaginatedResponse, MessageResponse,
};
use vc_server::ws::ServerEvent;

/// Environment variable that switches [`check_golden`] from compare to write mode.
const UPDATE_ENV: &str = "KAIKU_UPDATE_GOLDEN";

/// Directory holding the golden files (inside the Android test resources).
fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../mobile/android/app/src/test/resources/golden")
}

/// Compare `value` against the golden file `name`, or (re)write the file when
/// `KAIKU_UPDATE_GOLDEN=1` is set.
///
/// Compare mode asserts `serde_json::Value` equality, so key order and
/// whitespace in the file do not matter.
pub fn check_golden(name: &str, value: serde_json::Value) {
    let path = golden_dir().join(name);

    if std::env::var(UPDATE_ENV).as_deref() == Ok("1") {
        std::fs::create_dir_all(golden_dir()).expect("create golden dir");
        let mut pretty = serde_json::to_string_pretty(&value).expect("serialize golden value");
        pretty.push('\n');
        std::fs::write(&path, pretty).expect("write golden file");
        return;
    }

    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "golden file {} is missing ({e}); run with {UPDATE_ENV}=1 to create it",
            path.display()
        )
    });
    let expected: serde_json::Value = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("golden file {} is not valid JSON: {e}", path.display()));
    assert_eq!(
        expected, value,
        "golden file {name} is out of date; if the wire change is intentional, \
         rerun with {UPDATE_ENV}=1 and update the Android client"
    );
}

/// Parse a fixed RFC 3339 timestamp.
fn ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .expect("valid fixture timestamp")
        .with_timezone(&Utc)
}

const CHANNEL_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_c001);
const AUTHOR_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_a001);
/// Newer message (first in the server's newest-first page).
const NEWER_MESSAGE_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_e002);
/// Older message (last in the page; also the `next_cursor`).
const OLDER_MESSAGE_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_e001);
const ATTACHMENT_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_f001);
const LIVE_MESSAGE_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_e003);

fn author() -> AuthorProfile {
    AuthorProfile {
        id: AUTHOR_ID,
        username: "alice".to_string(),
        display_name: "Alice".to_string(),
        avatar_url: None,
        status: "online".to_string(),
    }
}

fn message(id: Uuid, content: &str, created_at: &str) -> MessageResponse {
    MessageResponse {
        id,
        channel_id: CHANNEL_ID,
        author: author(),
        content: content.to_string(),
        encrypted: false,
        attachments: Vec::new(),
        reply_to: None,
        parent_id: None,
        thread_reply_count: 0,
        thread_last_reply_at: None,
        edited_at: None,
        created_at: ts(created_at),
        mention_type: None,
        reactions: None,
        thread_info: None,
        pinned: false,
        message_type: "user".to_string(),
        nonce: None,
        embeds: None,
        components: None,
        webhook_id: None,
    }
}

fn attachment() -> AttachmentInfo {
    let url = format!("/api/messages/attachments/{ATTACHMENT_ID}/download");
    AttachmentInfo {
        id: ATTACHMENT_ID,
        filename: "photo.png".to_string(),
        mime_type: "image/png".to_string(),
        size: 204_800,
        thumbnail_url: Some(format!("{url}?variant=thumbnail")),
        url,
        width: Some(1280),
        height: Some(720),
        blurhash: None,
        medium_url: None,
    }
}

#[test]
fn android_golden_messages_page() {
    // Newer message: no fractional seconds, carries an attachment.
    let mut newer = message(NEWER_MESSAGE_ID, "Second message", "2026-03-12T10:01:00Z");
    newer.attachments = vec![attachment()];
    // Older message: 6 fractional digits.
    let older = message(
        OLDER_MESSAGE_ID,
        "First message",
        "2026-03-12T10:00:00.123456Z",
    );

    // The real server returns newest first; next_cursor is the oldest item's id.
    let page = CursorPaginatedResponse {
        items: vec![newer, older],
        has_more: true,
        next_cursor: Some(OLDER_MESSAGE_ID),
    };

    check_golden(
        "messages_page.json",
        serde_json::to_value(&page).expect("serialize page"),
    );
}

#[test]
fn android_golden_ws_message_new() {
    let live = message(
        LIVE_MESSAGE_ID,
        "Hello from WebSocket!",
        "2026-03-12T10:02:00.5Z",
    );
    let event = ServerEvent::MessageNew {
        channel_id: CHANNEL_ID,
        message: serde_json::to_value(&live).expect("serialize message"),
    };

    check_golden(
        "ws_message_new.json",
        serde_json::to_value(&event).expect("serialize event"),
    );
}

#[test]
fn android_golden_auth_response() {
    let response = AuthResponse {
        access_token: "fixture-access-token".to_string(),
        refresh_token: Some("fixture-refresh-token".to_string()),
        expires_in: 900,
        token_type: "Bearer".to_string(),
        setup_required: false,
    };

    check_golden(
        "auth_response.json",
        serde_json::to_value(&response).expect("serialize auth response"),
    );
}
