//! Guarantees that every machine-readable error code the server emits is
//! `UPPER_SNAKE` (`^[A-Z0-9_]+$`).
//!
//! Each error enum's variants are constructed here and rendered through
//! `IntoResponse`; the field that carries the code (`error`, or `code` for the
//! call API whose `error` field is free text) must match the pattern. Adding a
//! variant with a lower-case code makes the matching test fail.

use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde_json::Value;

use crate::admin::AdminError;
use crate::auth::AuthError;
use crate::chat::announcements::AnnouncementError;
use crate::chat::channel_pins::ChannelPinsError;
use crate::chat::forum::ForumError;
use crate::chat::pins::PinsError;
use crate::chat::reactions::ReactionsError;
use crate::chat::ChatError;
use crate::connectivity::ConnectivityError;
use crate::discovery::error::DiscoveryError;
use crate::favorites::FavoritesError;
use crate::governance::error::GovernanceError;
use crate::guild::categories::CategoryError;
use crate::guild::emojis::EmojiError;
use crate::guild::events::EventError;
use crate::guild::reaction_roles::ReactionRoleError;
use crate::guild::roles::RoleError;
use crate::guild::screening::ScreeningError;
use crate::guild::search::SearchError;
use crate::guild::GuildError;
use crate::incoming_webhooks::IncomingWebhookError;
use crate::moderation::filter_types::FilterError;
use crate::moderation::ModerationError;
use crate::pages::error::PagesError;
use crate::permissions::{GuildPermissions, PermissionError};
use crate::preferences::PreferencesError;
use crate::push::PushError;
use crate::ratelimit::error::RateLimitError;
use crate::ratelimit::RateLimitResult;
use crate::search::GlobalSearchError;
use crate::setup::SetupError;
use crate::social::SocialError;
use crate::voice::call_handlers::CallHandlerError;
use crate::voice::call_service::CallError;
use crate::voice::{ScreenShareError, VoiceError};
use crate::workspaces::error::WorkspaceError;
use crate::ws::handlers::OutboundSendError;

/// `true` for `^[A-Z0-9_]+$` (non-empty).
fn is_upper_snake(code: &str) -> bool {
    !code.is_empty()
        && code
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

/// Render a response and return the string in `key` (None if absent).
async fn code_of(resp: axum::response::Response, key: &str) -> (StatusCode, Option<String>) {
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .expect("body");
    let body: Value = serde_json::from_slice(&bytes).expect("JSON error body");
    (status, body[key].as_str().map(str::to_owned))
}

async fn assert_codes<I, E>(label: &str, key: &str, errors: I)
where
    I: IntoIterator<Item = E>,
    E: IntoResponse + std::fmt::Debug,
{
    for e in errors {
        let dbg = format!("{e:?}");
        let (status, code) = code_of(e.into_response(), key).await;
        let code = code.unwrap_or_else(|| panic!("{label}::{dbg}: no string `{key}` field"));
        assert!(
            is_upper_snake(&code),
            "{label}::{dbg} ({status}) has non-UPPER_SNAKE code {code:?}"
        );
    }
}

fn db() -> sqlx::Error {
    sqlx::Error::RowNotFound
}

fn perms() -> Vec<PermissionError> {
    vec![
        PermissionError::MissingPermission(GuildPermissions::empty()),
        PermissionError::RoleHierarchy {
            actor_position: 1,
            target_position: 2,
        },
        PermissionError::CannotEscalate(GuildPermissions::empty()),
        PermissionError::CannotModerateOwner,
        PermissionError::NotGuildMember,
        PermissionError::ElevationRequired,
        PermissionError::NotSystemAdmin,
        PermissionError::DatabaseError("x".into()),
        PermissionError::NotFound,
        PermissionError::InvalidChannel,
        PermissionError::Forbidden,
    ]
}

fn s() -> String {
    "x".to_string()
}

#[test]
fn upper_snake_matcher() {
    assert!(is_upper_snake("VALIDATION_ERROR"));
    assert!(is_upper_snake("MFA2_REQUIRED"));
    assert!(!is_upper_snake("validation"));
    assert!(!is_upper_snake("Not_Found"));
    assert!(!is_upper_snake("NOT-FOUND"));
    assert!(!is_upper_snake(""));
}

#[tokio::test]
async fn admin_and_auth_codes_are_upper_snake() {
    let mut admin = vec![
        AdminError::NotAdmin,
        AdminError::ElevationRequired,
        AdminError::MfaRequired,
        AdminError::InvalidMfaCode,
        AdminError::NotFound(s()),
        AdminError::Conflict(s()),
        AdminError::Validation(s()),
        AdminError::UnknownServer,
        AdminError::InvalidRequest(s()),
        AdminError::Database(db()),
        AdminError::Reauth(AuthError::ElevationRequired),
        AdminError::Internal(s()),
    ];
    admin.extend(perms().into_iter().map(AdminError::Permission));
    assert_codes("AdminError", "error", admin).await;

    let auth = vec![
        AuthError::InvalidCredentials,
        AuthError::UserNotFound,
        AuthError::NotFound(s()),
        AuthError::Forbidden,
        AuthError::UserAlreadyExists,
        AuthError::EmailTaken,
        AuthError::InvalidToken,
        AuthError::TokenExpired,
        AuthError::MissingAuthHeader,
        AuthError::InvalidAuthHeader,
        AuthError::MfaRequired,
        AuthError::InvalidMfaCode,
        AuthError::EmailNotConfigured,
        AuthError::Validation(s()),
        AuthError::PasswordHash,
        AuthError::Database(db()),
        AuthError::Jwt(jsonwebtoken::errors::ErrorKind::InvalidToken.into()),
        AuthError::OidcProviderNotFound,
        AuthError::OidcNotConfigured,
        AuthError::OidcStateMismatch,
        AuthError::OidcCodeExchangeFailed(s()),
        AuthError::RegistrationDisabled,
        AuthError::AuthMethodDisabled,
        AuthError::PasswordChangeRequired,
        AuthError::AccountBanned,
        AuthError::ElevationRequired,
        AuthError::CannotUnlinkLastIdentity,
        AuthError::ServiceUnavailable,
        AuthError::MfaCodeRequired,
        AuthError::ReauthUnavailable,
        AuthError::ReauthOidcRequired,
        AuthError::ReauthProviderUnavailable,
        AuthError::TooManyAttempts { retry_after: 1 },
        AuthError::Internal(s()),
    ];
    assert_codes("AuthError", "error", auth).await;
}

#[tokio::test]
async fn chat_codes_are_upper_snake() {
    let mut chat = vec![
        ChatError::ChannelNotFound,
        ChatError::Forbidden,
        ChatError::PasswordChangeRequired,
        ChatError::SessionEnded,
        ChatError::ServiceUnavailable,
        ChatError::Validation(s()),
        ChatError::LimitExceeded(s()),
        ChatError::FeatureDisabled,
        ChatError::MessageNotFound,
        ChatError::Blocked,
        ChatError::ContentFiltered,
        ChatError::StorageNotConfigured,
        ChatError::FileNotFound,
        ChatError::FileTooLarge { max_size: 1 },
        ChatError::InvalidMimeType { mime_type: s() },
        ChatError::NoFile,
        ChatError::InvalidFilename,
        ChatError::Storage(s()),
        ChatError::RoleNotFound,
        ChatError::NotMember,
        ChatError::InvalidQuery(s()),
        ChatError::Database(db()),
    ];
    chat.extend(perms().into_iter().map(ChatError::Permission));
    assert_codes("ChatError", "error", chat).await;

    assert_codes(
        "PinsError",
        "error",
        [
            PinsError::NotFound,
            PinsError::LimitExceeded,
            PinsError::ContentTooLong,
            PinsError::TitleTooLong,
            PinsError::Database(db()),
        ],
    )
    .await;
    assert_codes(
        "ReactionsError",
        "error",
        [
            ReactionsError::MessageNotFound,
            ReactionsError::ChannelNotFound,
            ReactionsError::InvalidEmoji,
            ReactionsError::Forbidden,
            ReactionsError::Database(db()),
        ],
    )
    .await;
    assert_codes(
        "ChannelPinsError",
        "error",
        [
            ChannelPinsError::MessageNotFound,
            ChannelPinsError::ChannelNotFound,
            ChannelPinsError::NotGuildChannel,
            ChannelPinsError::PinLimitReached,
            ChannelPinsError::Forbidden,
            ChannelPinsError::FeatureDisabled,
            ChannelPinsError::Database(db()),
        ],
    )
    .await;

    let mut ann = vec![
        AnnouncementError::ChannelNotFound,
        AnnouncementError::NotAnnouncement,
        AnnouncementError::AlreadyFollowing,
        AnnouncementError::FollowNotFound,
        AnnouncementError::SourceForbidden,
        AnnouncementError::FeatureDisabled,
        AnnouncementError::Database(db()),
    ];
    ann.extend(perms().into_iter().map(AnnouncementError::Permission));
    assert_codes("AnnouncementError", "error", ann).await;

    let mut forum = vec![
        ForumError::NotForum,
        ForumError::ChannelNotFound,
        ForumError::PostNotFound,
        ForumError::TagNotFound,
        ForumError::NotMember,
        ForumError::Validation(s()),
        ForumError::Database(db()),
    ];
    forum.extend(perms().into_iter().map(ForumError::Permission));
    assert_codes("ForumError", "error", forum).await;
}

#[tokio::test]
async fn guild_codes_are_upper_snake() {
    let mut guild = vec![
        GuildError::NotFound,
        GuildError::Forbidden,
        GuildError::ForbiddenMsg(s()),
        GuildError::Validation(s()),
        GuildError::LimitExceeded(s()),
        GuildError::Database(db()),
        GuildError::Internal(s()),
    ];
    guild.extend(perms().into_iter().map(GuildError::Permission));
    assert_codes("GuildError", "error", guild).await;

    let mut cat = vec![
        CategoryError::NotFound,
        CategoryError::NotMember,
        CategoryError::Validation(s()),
        CategoryError::Database(db()),
    ];
    cat.extend(perms().into_iter().map(CategoryError::Permission));
    assert_codes("CategoryError", "error", cat).await;

    let mut role = vec![
        RoleError::NotFound,
        RoleError::NotMember,
        RoleError::Validation(s()),
        RoleError::LimitExceeded(s()),
        RoleError::Database(db()),
    ];
    role.extend(perms().into_iter().map(RoleError::Permission));
    assert_codes("RoleError", "error", role).await;

    assert_codes(
        "EmojiError",
        "error",
        [
            EmojiError::GuildNotFound,
            EmojiError::EmojiNotFound,
            EmojiError::Forbidden,
            EmojiError::InvalidFilename,
            EmojiError::FileTooLarge { max_size: 1 },
            EmojiError::InvalidFileType,
            EmojiError::NoFile,
            EmojiError::Storage(s()),
            EmojiError::Validation(s()),
            EmojiError::LimitExceeded(s()),
            EmojiError::Database(db()),
        ],
    )
    .await;

    let mut ev = vec![
        EventError::NotFound,
        EventError::NotMember,
        EventError::StartInPast,
        EventError::EndBeforeStart,
        EventError::InvalidRsvp,
        EventError::Validation(s()),
        EventError::Database(db()),
    ];
    ev.extend(perms().into_iter().map(EventError::Permission));
    assert_codes("EventError", "error", ev).await;

    let mut rr = vec![
        ReactionRoleError::NotFound,
        ReactionRoleError::NotMember,
        ReactionRoleError::RoleNotFound,
        ReactionRoleError::MessageNotFound,
        ReactionRoleError::ChannelNotFound,
        ReactionRoleError::RoleNotSelfAssignable,
        ReactionRoleError::DefaultRoleNotBindable,
        ReactionRoleError::DuplicateBinding,
        ReactionRoleError::Validation(s()),
        ReactionRoleError::Database(db()),
    ];
    rr.extend(perms().into_iter().map(ReactionRoleError::Permission));
    assert_codes("ReactionRoleError", "error", rr).await;

    let mut scr = vec![
        ScreeningError::NotMember,
        ScreeningError::NotEnabled,
        ScreeningError::NotPending,
        ScreeningError::Validation(s()),
        ScreeningError::Database(db()),
    ];
    scr.extend(perms().into_iter().map(ScreeningError::Permission));
    assert_codes("ScreeningError", "error", scr).await;

    assert_codes(
        "SearchError",
        "error",
        [
            SearchError::GuildNotFound,
            SearchError::NotMember,
            SearchError::InvalidQuery(s()),
            SearchError::Database(db()),
        ],
    )
    .await;
    assert_codes(
        "GlobalSearchError",
        "error",
        [
            GlobalSearchError::InvalidQuery(s()),
            GlobalSearchError::Forbidden,
            GlobalSearchError::Database(db()),
        ],
    )
    .await;
}

#[tokio::test]
async fn platform_codes_are_upper_snake() {
    assert_codes(
        "FavoritesError",
        "error",
        [
            FavoritesError::ChannelNotFound,
            FavoritesError::InvalidChannel,
            FavoritesError::LimitExceeded,
            FavoritesError::AlreadyFavorited,
            FavoritesError::NotFavorited,
            FavoritesError::InvalidChannels,
            FavoritesError::InvalidGuilds,
            FavoritesError::Database(db()),
        ],
    )
    .await;
    assert_codes(
        "ConnectivityError",
        "error",
        [
            ConnectivityError::SessionNotFound,
            ConnectivityError::Database(db()),
        ],
    )
    .await;
    assert_codes(
        "DiscoveryError",
        "error",
        [
            DiscoveryError::Disabled,
            DiscoveryError::NotFound,
            DiscoveryError::Forbidden(s()),
            DiscoveryError::Validation(s()),
            DiscoveryError::LimitExceeded(s()),
            DiscoveryError::Database(db()),
        ],
    )
    .await;
    assert_codes(
        "ModerationError",
        "error",
        [
            ModerationError::Database(db()),
            ModerationError::NotFound,
            ModerationError::Validation(s()),
            ModerationError::RateLimited,
            ModerationError::Duplicate,
        ],
    )
    .await;
    assert_codes(
        "FilterError",
        "error",
        [
            FilterError::NotFound,
            FilterError::Forbidden,
            FilterError::Validation(s()),
            FilterError::Database(db()),
        ],
    )
    .await;
    assert_codes(
        "PreferencesError",
        "error",
        [
            PreferencesError::Database(db()),
            PreferencesError::Validation(s()),
        ],
    )
    .await;
    assert_codes(
        "PushError",
        "error",
        [
            PushError::NotFound,
            PushError::Validation(s()),
            PushError::ProviderUnavailable,
            PushError::Database(db()),
        ],
    )
    .await;
    assert_codes(
        "SetupError",
        "error",
        [
            SetupError::SetupAlreadyComplete,
            SetupError::Unauthorized,
            SetupError::Validation(s()),
            SetupError::Database(db()),
        ],
    )
    .await;
    assert_codes(
        "SocialError",
        "error",
        [
            SocialError::Database(db()),
            SocialError::UserNotFound,
            SocialError::SelfFriendRequest,
            SocialError::AlreadyExists,
            SocialError::Blocked,
            SocialError::FriendshipNotFound,
            SocialError::Unauthorized,
            SocialError::Validation(s()),
        ],
    )
    .await;
    assert_codes(
        "WorkspaceError",
        "error",
        [
            WorkspaceError::NotFound,
            WorkspaceError::EntryNotFound,
            WorkspaceError::WorkspaceLimitExceeded,
            WorkspaceError::EntryLimitExceeded,
            WorkspaceError::Validation(s()),
            WorkspaceError::ChannelNotFound,
            WorkspaceError::DuplicateEntry,
            WorkspaceError::InvalidEntries,
            WorkspaceError::InvalidWorkspaces,
            WorkspaceError::Database(db()),
        ],
    )
    .await;
    assert_codes(
        "GovernanceError",
        "error",
        [
            GovernanceError::ExportNotFound,
            GovernanceError::ExportAlreadyPending,
            GovernanceError::ExportExpired,
            GovernanceError::DeletionAlreadyScheduled,
            GovernanceError::NoDeletionPending,
            GovernanceError::PasswordInvalid,
            GovernanceError::OwnsGuilds(s()),
            GovernanceError::OidcPasswordNotSupported,
            GovernanceError::StorageNotConfigured,
            GovernanceError::Validation(s()),
            GovernanceError::Database(db()),
            GovernanceError::Internal(s()),
        ],
    )
    .await;

    let mut pages = vec![
        PagesError::NotFound,
        PagesError::RevisionNotFound,
        PagesError::CategoryNotFound,
        PagesError::Forbidden,
        PagesError::NotGuildMember,
        PagesError::AdminRequired,
        PagesError::Validation(s()),
        PagesError::SlugConflict(s()),
        PagesError::LimitExceeded(s()),
        PagesError::CategoryConflict(s()),
        PagesError::InvalidReorder(s()),
        PagesError::NotPlatformPage,
        PagesError::NotGuildPage,
        PagesError::CorruptRevision(s()),
        PagesError::Database(db()),
        PagesError::Internal(s()),
    ];
    pages.extend(perms().into_iter().map(PagesError::Permission));
    assert_codes("PagesError", "error", pages).await;

    // The Discord-shaped webhook 429 has no `error` field by design; its
    // numeric `code` is Discord's, not ours.
    let mut hooks = vec![
        IncomingWebhookError::Database(db()),
        IncomingWebhookError::UnknownWebhook,
        IncomingWebhookError::InvalidToken,
        IncomingWebhookError::UnknownMessage,
        IncomingWebhookError::UnknownChannel,
        IncomingWebhookError::EmptyMessage,
        IncomingWebhookError::Validation(s()),
        IncomingWebhookError::ThreadRequired,
        IncomingWebhookError::ThreadLocked,
        IncomingWebhookError::ContentFiltered,
        IncomingWebhookError::MaxWebhooksReached,
        IncomingWebhookError::FeatureDisabled,
    ];
    hooks.extend(perms().into_iter().map(IncomingWebhookError::Permission));
    assert_codes("IncomingWebhookError", "error", hooks).await;
}

#[tokio::test]
async fn rate_limit_voice_and_call_codes_are_upper_snake() {
    assert_codes(
        "RateLimitError",
        "error",
        [
            RateLimitError::RedisUnavailable,
            RateLimitError::LimitExceeded(RateLimitResult {
                allowed: false,
                limit: 1,
                remaining: 0,
                reset_at: 0,
                retry_after: 1,
            }),
            RateLimitError::IpBlocked { retry_after: 1 },
        ],
    )
    .await;

    assert_codes(
        "VoiceError",
        "error",
        [
            VoiceError::RoomNotFound(uuid::Uuid::nil()),
            VoiceError::ParticipantNotFound(uuid::Uuid::nil()),
            VoiceError::WebRtc(s()),
            VoiceError::Signaling(s()),
            VoiceError::DuplicateStreamId,
            VoiceError::IceConnectionFailed,
            VoiceError::ChannelFull {
                max_participants: 1,
            },
            VoiceError::Unauthorized,
            VoiceError::ChannelNotFound(uuid::Uuid::nil()),
            VoiceError::AlreadyJoined,
            VoiceError::NotInChannel,
            VoiceError::RateLimited("x"),
            VoiceError::Internal(s()),
            VoiceError::Database(db()),
            VoiceError::Outbound(OutboundSendError::Closed),
        ],
    )
    .await;

    assert_codes(
        "ScreenShareError",
        "error",
        [
            ScreenShareError::NoPermission,
            ScreenShareError::LimitReached,
            ScreenShareError::NotInChannel,
            ScreenShareError::QualityNotAllowed,
            ScreenShareError::RenegotiationFailed,
            ScreenShareError::InternalError,
            ScreenShareError::InvalidSourceLabel,
            ScreenShareError::AlreadySharing,
        ],
    )
    .await;

    // The call API carries its machine code in `code`; `error` is free text.
    assert_codes(
        "CallError",
        "code",
        [
            CallError::CallNotFound,
            CallError::CallAlreadyExists,
            CallError::Contention,
            CallError::Redis(s()),
            CallError::InvalidEvent(s()),
            CallError::StateTransition(s()),
            CallError::Serialization(s()),
            CallError::Internal(s()),
        ],
    )
    .await;
    assert_codes(
        "CallHandlerError",
        "code",
        [
            CallHandlerError::Call(CallError::CallNotFound),
            CallHandlerError::NotFound,
            CallHandlerError::Forbidden,
            CallHandlerError::Blocked,
            CallHandlerError::Database(s()),
        ],
    )
    .await;
}

/// WebSocket `error` events carry their code in handler code rather than in an
/// enum, so scan the sources for literal codes: `code: "..."` fields and the
/// `send_err("...", ...)` helper must be `UPPER_SNAKE`.
#[test]
fn ws_event_and_close_codes_are_upper_snake() {
    let sources = [
        ("ws/handlers.rs", include_str!("ws/handlers.rs")),
        ("ws/bot_gateway.rs", include_str!("ws/bot_gateway.rs")),
    ];
    let mut checked = 0;
    for (name, src) in sources {
        for line in src.lines() {
            let line = line.trim();
            if line.starts_with("//") {
                continue;
            }
            let literal = ["code: \"", "send_err(\"", "CLOSE_REASON_RESYNC: &str = \""]
                .iter()
                .find_map(|p| line.split_once(p).map(|(_, rest)| rest));
            if let Some(rest) = literal {
                let code = rest.split('"').next().unwrap_or_default();
                // `code: u16` close codes and `code: "..."` HTTP-less test
                // fixtures never reach this branch (they have no string literal).
                assert!(
                    is_upper_snake(code),
                    "{name}: non-UPPER_SNAKE WS code {code:?} in `{line}`"
                );
                checked += 1;
            }
        }
    }
    assert!(checked >= 10, "expected to scan WS codes, saw {checked}");
}
