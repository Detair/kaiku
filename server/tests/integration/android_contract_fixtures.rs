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
    AttachmentInfo, AuthorProfile, CursorPaginatedResponse, DMListResponse, DMParticipant,
    LastMessagePreview, MessageResponse,
};
use vc_server::db::{Channel, ChannelType, ChannelUnread, GuildUnreadSummary, UnreadAggregate};
use vc_server::guild::categories::{Category, CategoryType};
use vc_server::guild::settings::ChannelWithUnread;
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
        status: "offline".to_string(),
        is_system: false,
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
        security_notice: None,
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

const SESSION_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_d001);

#[test]
fn android_golden_auth_response() {
    let response = AuthResponse {
        access_token: "fixture-access-token".to_string(),
        refresh_token: Some("fixture-refresh-token".to_string()),
        expires_in: 900,
        token_type: "Bearer".to_string(),
        setup_required: false,
        password_change_required: false,
        session_id: SESSION_ID,
        assurance: vc_server::auth::assurance::Assurance::Probation,
        absolute_expires_at: ts("2026-12-03T12:00:00Z"),
    };

    check_golden(
        "auth_response.json",
        serde_json::to_value(&response).expect("serialize auth response"),
    );
}

/// Login response for an admin-created account still on its temporary
/// password: the client must ask for a new password before anything else.
#[test]
fn android_golden_auth_response_password_change() {
    let response = AuthResponse {
        access_token: "fixture-temporary-access-token".to_string(),
        refresh_token: Some("fixture-temporary-refresh-token".to_string()),
        expires_in: 900,
        token_type: "Bearer".to_string(),
        setup_required: false,
        password_change_required: true,
        session_id: SESSION_ID,
        assurance: vc_server::auth::assurance::Assurance::Probation,
        absolute_expires_at: ts("2026-12-03T12:00:00Z"),
    };

    check_golden(
        "auth_response_password_change.json",
        serde_json::to_value(&response).expect("serialize auth response"),
    );
}

/// Device-key sign-in (C26 `POST /auth/device-login`): a full session, the
/// refresh token in the body, the 90-day absolute cap of a native client.
#[test]
fn android_golden_auth_response_device_key() {
    let response = AuthResponse {
        access_token: "fixture-device-access-token".to_string(),
        refresh_token: Some("fixture-device-refresh-token".to_string()),
        expires_in: 900,
        token_type: "Bearer".to_string(),
        setup_required: false,
        password_change_required: false,
        session_id: SESSION_ID,
        assurance: vc_server::auth::assurance::Assurance::Full,
        absolute_expires_at: ts("2027-02-01T12:00:00Z"),
    };

    check_golden(
        "auth_response_device_key.json",
        serde_json::to_value(&response).expect("serialize auth response"),
    );
}

const GUILD_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_b001);

fn channel_with_unread(
    id: u128,
    name: &str,
    channel_type: ChannelType,
    position: i32,
) -> ChannelWithUnread {
    ChannelWithUnread {
        channel: Channel {
            id: Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_0000 + id),
            name: name.to_string(),
            channel_type,
            category_id: None,
            guild_id: Some(GUILD_ID),
            topic: None,
            icon_url: None,
            user_limit: None,
            position,
            max_screen_shares: 1,
            created_at: ts("2026-03-12T09:00:00Z"),
            updated_at: ts("2026-03-12T09:00:00Z"),
        },
        unread_count: 0,
        last_read_message_id: None,
        last_message_id: None,
    }
}

/// `GET /api/guilds/{id}/channels`: includes forum and announcement channels
/// that the Android client does not know and must hide instead of failing.
#[test]
fn android_golden_guild_channels() {
    let mut text = channel_with_unread(0xd001, "general", ChannelType::Text, 0);
    text.unread_count = 3;
    text.last_message_id = Some(NEWER_MESSAGE_ID);
    text.last_read_message_id = Some(OLDER_MESSAGE_ID);

    let mut voice = channel_with_unread(0xd002, "lounge", ChannelType::Voice, 1);
    voice.channel.user_limit = Some(10);

    let forum = channel_with_unread(0xd003, "ideas", ChannelType::Forum, 2);
    let announcement = channel_with_unread(0xd004, "news", ChannelType::Announcement, 3);

    let channels = vec![text, voice, forum, announcement];
    check_golden(
        "guild_channels.json",
        serde_json::to_value(&channels).expect("serialize guild channels"),
    );
}

/// `GET /api/guilds/{id}/categories`: one top-level category and one nested
/// under it (`parent_id`); `category_type` is on the wire but ignored by Android.
#[test]
fn android_golden_guild_categories() {
    let top = Category {
        id: Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_9001),
        guild_id: GUILD_ID,
        name: "Text Channels".to_string(),
        position: 0,
        parent_id: None,
        category_type: CategoryType::Mixed,
        created_at: ts("2026-03-12T09:00:00Z"),
    };
    let nested = Category {
        id: Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_9002),
        guild_id: GUILD_ID,
        name: "Voice".to_string(),
        position: 1,
        parent_id: Some(top.id),
        category_type: CategoryType::Voice,
        created_at: ts("2026-03-12T09:05:00Z"),
    };

    let categories = vec![top, nested];
    check_golden(
        "guild_categories.json",
        serde_json::to_value(&categories).expect("serialize guild categories"),
    );
}

#[test]
fn android_golden_ws_ready() {
    let event = ServerEvent::Ready {
        user_id: AUTHOR_ID,
        server_time: ts("2026-03-12T10:03:00.25Z"),
    };

    check_golden(
        "ws_ready.json",
        serde_json::to_value(&event).expect("serialize ready"),
    );
}

/// `subscribed` ack: the client starts its gap-free catch-up for a channel
/// only after this arrives (see `server/src/ws/AGENTS.md`).
#[test]
fn android_golden_ws_subscribed() {
    let event = ServerEvent::Subscribed {
        channel_id: CHANNEL_ID,
    };

    check_golden(
        "ws_subscribed.json",
        serde_json::to_value(&event).expect("serialize subscribed"),
    );
}

const DM_CHANNEL_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_d101);
const GROUP_DM_CHANNEL_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_d102);
const BOB_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_a002);
const CAROL_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_a003);
const DM_MESSAGE_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_e101);

fn dm_channel(id: Uuid, name: &str, icon_url: Option<&str>) -> Channel {
    Channel {
        id,
        name: name.to_string(),
        channel_type: ChannelType::Dm,
        category_id: None,
        guild_id: None,
        topic: None,
        icon_url: icon_url.map(str::to_string),
        user_limit: None,
        position: 0,
        max_screen_shares: 1,
        created_at: ts("2026-03-12T09:30:00Z"),
        updated_at: ts("2026-03-12T09:30:00Z"),
    }
}

fn participant(user_id: Uuid, username: &str, display_name: &str) -> DMParticipant {
    DMParticipant {
        user_id,
        username: username.to_string(),
        display_name: display_name.to_string(),
        avatar_url: None,
        joined_at: ts("2026-03-12T09:30:00Z"),
        is_system: false,
    }
}

/// A brand-new 1:1 DM exactly as `announce_dm_created` builds it: no last
/// message, zero unread.
fn new_dm_entry() -> DMListResponse {
    DMListResponse {
        channel: dm_channel(DM_CHANNEL_ID, "", None).into(),
        participants: vec![
            participant(AUTHOR_ID, "alice", "Alice"),
            participant(BOB_ID, "bob", "Bob"),
        ],
        last_message: None,
        unread_count: 0,
        read_only: false,
    }
}

#[test]
fn android_golden_ws_dm_created() {
    let event = ServerEvent::DmCreated {
        channel: serde_json::to_value(new_dm_entry()).expect("serialize dm entry"),
    };

    check_golden(
        "ws_dm_created.json",
        serde_json::to_value(&event).expect("serialize dm_created"),
    );
}

/// `GET /api/dm`: a group DM with a last message and unread count, and a
/// fresh 1:1 DM.
#[test]
fn android_golden_dm_list() {
    let mut group = DMListResponse {
        channel: dm_channel(
            GROUP_DM_CHANNEL_ID,
            "Weekend plans",
            Some("/api/dm/icon.png"),
        )
        .into(),
        participants: vec![
            participant(AUTHOR_ID, "alice", "Alice"),
            participant(BOB_ID, "bob", "Bob"),
            participant(CAROL_ID, "carol", "Carol"),
        ],
        last_message: Some(LastMessagePreview {
            id: DM_MESSAGE_ID,
            content: "See you Saturday".to_string(),
            user_id: Some(CAROL_ID),
            username: Some("carol".to_string()),
            created_at: ts("2026-03-12T11:00:00Z"),
        }),
        unread_count: 2,
        read_only: false,
    };
    group.participants[2].avatar_url = Some("/api/users/carol/avatar".to_string());

    let list = vec![group, new_dm_entry()];
    check_golden(
        "dm_list.json",
        serde_json::to_value(&list).expect("serialize dm list"),
    );
}

/// `channel_read`: the user read a guild channel on another session; Android
/// clears that channel's message notification.
#[test]
fn android_golden_ws_channel_read() {
    let event = ServerEvent::ChannelRead {
        channel_id: CHANNEL_ID,
        last_read_message_id: Some(NEWER_MESSAGE_ID),
    };

    check_golden(
        "ws_channel_read.json",
        serde_json::to_value(&event).expect("serialize channel_read"),
    );
}

/// `dm_read` without a read position (`last_read_message_id` absent or null).
#[test]
fn android_golden_ws_dm_read() {
    let event = ServerEvent::DmRead {
        channel_id: DM_CHANNEL_ID,
        last_read_message_id: None,
    };

    check_golden(
        "ws_dm_read.json",
        serde_json::to_value(&event).expect("serialize dm_read"),
    );
}

/// `GET /api/me/unread`: Android's wake fetch reads the server's unread count
/// of the woken channel (only messages from others, without thread replies).
#[test]
fn android_golden_unread_aggregate() {
    let aggregate = UnreadAggregate {
        guilds: vec![GuildUnreadSummary {
            guild_id: GUILD_ID,
            guild_name: "Family".to_string(),
            channels: vec![ChannelUnread {
                channel_id: CHANNEL_ID,
                channel_name: "general".to_string(),
                unread_count: 3,
                last_read_message_id: Some(OLDER_MESSAGE_ID),
            }],
            total_unread: 3,
        }],
        dms: vec![ChannelUnread {
            channel_id: DM_CHANNEL_ID,
            channel_name: String::new(),
            unread_count: 2,
            last_read_message_id: None,
        }],
        total: 5,
    };

    check_golden(
        "unread_aggregate.json",
        serde_json::to_value(&aggregate).expect("serialize unread aggregate"),
    );
}

// ============================================================================
// Security notices (C28)
// ============================================================================

const NOTICE_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_5001);
const NOTICE_MESSAGE_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_5002);
const KAIKU_DM_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_5003);
const EVENT_ID_1: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_5011);
const EVENT_ID_2: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_5012);

fn security_event(id: Uuid, at: &str) -> vc_server::security_events::SecurityEvent {
    use vc_server::security_events::types::{EventActor, EventClient};
    vc_server::security_events::SecurityEvent {
        id,
        event_type: "sign_in".to_string(),
        severity: "normal".to_string(),
        created_at: ts(at),
        subject_id: AUTHOR_ID,
        actor: EventActor {
            kind: "self".to_string(),
            user_id: Some(AUTHOR_ID),
            username: Some("alice".to_string()),
        },
        client: Some(EventClient {
            class: "desktop".to_string(),
            label: "Kaiku desktop on Windows".to_string(),
        }),
        place: None,
        ip: Some("203.0.113.7".to_string()),
        session_id: None,
        authenticator: None,
        request_id: None,
        details: serde_json::json!({"auth_methods": ["qr"], "assurance": "probation"}),
        actions: Vec::new(),
        repudiated_at: None,
    }
}

fn security_notice() -> vc_server::security_events::SecurityNotice {
    use vc_server::security_events::notices::NoticeSubject;
    vc_server::security_events::SecurityNotice {
        notice_id: NOTICE_ID,
        subject: NoticeSubject {
            user_id: AUTHOR_ID,
            username: "alice".to_string(),
            display_name: "Alice".to_string(),
        },
        event_type: "sign_in".to_string(),
        severity: "normal".to_string(),
        count: 2,
        first_at: ts("2026-11-03T12:00:00Z"),
        last_at: ts("2026-11-03T12:04:00Z"),
        events: vec![
            security_event(EVENT_ID_2, "2026-11-03T12:04:00Z"),
            security_event(EVENT_ID_1, "2026-11-03T12:00:00Z"),
        ],
        events_truncated: false,
    }
}

/// A notice message in the Kaiku DM (`GET /api/messages/channel/{id}` item).
#[test]
fn android_golden_security_notice_message() {
    let mut m = message(
        NOTICE_MESSAGE_ID,
        "Security notice: sign in (2)",
        "2026-11-03T12:00:00Z",
    );
    m.channel_id = KAIKU_DM_ID;
    m.author = AuthorProfile {
        id: vc_server::system_account::SYSTEM_USER_ID,
        username: "kaiku".to_string(),
        display_name: "Kaiku".to_string(),
        avatar_url: None,
        status: "offline".to_string(),
        is_system: true,
    };
    m.message_type = "security".to_string();
    m.edited_at = Some(ts("2026-11-03T12:04:00Z"));
    m.security_notice = Some(security_notice());
    check_golden(
        "security_notice_message.json",
        serde_json::to_value(&m).expect("serialize message"),
    );
}

/// `security_notice_updated` (WS, to the recipient only).
#[test]
fn android_golden_ws_security_notice_updated() {
    let event = ServerEvent::SecurityNoticeUpdated {
        channel_id: KAIKU_DM_ID,
        message_id: NOTICE_MESSAGE_ID,
        security_notice: serde_json::to_value(security_notice()).expect("serialize notice"),
    };
    check_golden(
        "ws_security_notice_updated.json",
        serde_json::to_value(&event).expect("serialize event"),
    );
}

/// FCM `data` of a `t=security` push (all strings), with an admin actor.
#[test]
fn android_golden_push_security_fcm() {
    use vc_server::push::PushPayload;
    use vc_server::security_events::push::SecurityPush;
    use vc_server::security_events::{EventType, Severity};
    let push = SecurityPush {
        notice_id: NOTICE_ID,
        event_type: EventType::PasswordCompromised,
        severity: Severity::High,
        count: 3,
        subject_id: AUTHOR_ID,
        actor_id: Some(BOB_ID),
    };
    check_golden(
        "push_security_fcm.json",
        serde_json::json!({
            "data": push.fcm_data(),
            "priority": if push.high_priority() { "HIGH" } else { "NORMAL" },
            "collapse_key": push.collapse_key(),
            "ttl": push.ttl(),
        }),
    );
}

/// Passkey sign-in start (C29 `POST /auth/passkeys/login/start`): the
/// `options` JSON is passed to Credential Manager unchanged.
#[test]
fn android_golden_passkey_get_start() {
    let start = vc_server::auth::passkeys::types::CeremonyStart {
        ceremony_id: Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_d001),
        options: serde_json::json!({
            "publicKey": {
                "challenge": "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
                "timeout": 300_000,
                "rpId": "chat.example.com",
                "allowCredentials": [],
                "userVerification": "required"
            }
        }),
    };
    check_golden(
        "passkey_get_start.json",
        serde_json::to_value(&start).expect("serialize ceremony start"),
    );
}

const PASSKEY_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_7000_8000_0000_0000_e001);

/// C29 `Passkey` (a pending one, so `ready_at` is set).
#[test]
fn android_golden_passkey() {
    let passkey = vc_server::auth::passkeys::types::PasskeyView {
        id: PASSKEY_ID,
        label: "Pixel 8".to_string(),
        state: vc_server::auth::passkeys::queries::PasskeyState::Pending,
        created_at: ts("2026-11-01T10:00:00Z"),
        last_used_at: None,
        ready_at: Some(ts("2026-11-03T10:00:00Z")),
        backup_eligible: true,
        backup_state: true,
        transports: vec!["internal".to_string(), "hybrid".to_string()],
        aaguid: Some(Uuid::from_u128(0xea9b_8d66_4d01_1d21_3ce4_b6b4_8cb5_75d4)),
    };
    check_golden(
        "passkey.json",
        serde_json::to_value(&passkey).expect("serialize passkey"),
    );
}

/// C29 `DELETE /auth/passkeys/{id}` answer (the signal data).
#[test]
fn android_golden_passkey_removed() {
    let removed = vc_server::auth::passkeys::types::PasskeyRemovedResponse {
        rp_id: "chat.example.com".to_string(),
        user_handle: "AAECAwQFBgcICQoLDA0ODw".to_string(),
        credential_id: "Y3JlZGVudGlhbC0x".to_string(),
        remaining_credential_ids: vec!["Y3JlZGVudGlhbC0y".to_string()],
    };
    check_golden(
        "passkey_removed.json",
        serde_json::to_value(&removed).expect("serialize removal"),
    );
}

/// C26–C29 `StepUpGrant` (`via: "passkey"`).
#[test]
fn android_golden_step_up_grant() {
    let grant = vc_server::auth::step_up::StepUpGrant {
        action: vc_server::auth::step_up::StepUpAction::PasskeyRemove,
        target: PASSKEY_ID.to_string(),
        via: vc_server::auth::step_up::StepUpVia::Passkey,
        expires_at: ts("2026-11-01T10:05:00Z"),
    };
    check_golden(
        "step_up_grant.json",
        serde_json::to_value(&grant).expect("serialize grant"),
    );
}
