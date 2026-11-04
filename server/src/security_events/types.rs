//! Security event types (contract C28).

use std::net::IpAddr;

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::auth::client_class::ClientClass;
use crate::auth::strong::AuthenticatorRef;

/// Every event type of C28. The wire names are append-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EventType {
    SignIn,
    SignInNotFinished,
    SecondFactorFailed,
    Guessing,
    ApprovalReported,
    AuthenticatorAdded,
    AuthenticatorRemoved,
    KeyInvalidated,
    RecoveryCodesCreated,
    PasswordSet,
    PasswordChanged,
    PasswordRemoved,
    PasswordThrottled,
    PasswordCompromised,
    TotpThrottled,
    TotpCompromised,
    StepUpAction,
    Repudiated,
    RequestStarted,
    RequestCompleted,
    RequestCancelled,
    RequestWithdrawn,
    RequestReadySoon,
    RequestReady,
    RequestExpired,
    RequestVoid,
    AdminSetup,
    AdminRoleGranted,
    AdminRoleRemoved,
    RecoveryContactNamed,
    RecoveryContactConfirmed,
    RecoveryContactChanged,
    SessionEndedReuse,
    SessionEndedLimit,
    /// Sessions ended on the host (`kaiku-admin sessions end`; appended to
    /// C28's list, L1b Task 28).
    SessionsEnded,
    /// A compromised password or TOTP made active again (admin or CLI).
    FactorRestored,
    /// A throttled password or TOTP made active again (admin or CLI).
    ThrottleReset,
    /// `may_approve` turned on or off (admin or CLI).
    MayApproveChanged,
    BootstrapReminder,
}

impl EventType {
    /// Every type, in C28 order.
    pub const ALL: [Self; 39] = [
        Self::SignIn,
        Self::SignInNotFinished,
        Self::SecondFactorFailed,
        Self::Guessing,
        Self::ApprovalReported,
        Self::AuthenticatorAdded,
        Self::AuthenticatorRemoved,
        Self::KeyInvalidated,
        Self::RecoveryCodesCreated,
        Self::PasswordSet,
        Self::PasswordChanged,
        Self::PasswordRemoved,
        Self::PasswordThrottled,
        Self::PasswordCompromised,
        Self::TotpThrottled,
        Self::TotpCompromised,
        Self::StepUpAction,
        Self::Repudiated,
        Self::RequestStarted,
        Self::RequestCompleted,
        Self::RequestCancelled,
        Self::RequestWithdrawn,
        Self::RequestReadySoon,
        Self::RequestReady,
        Self::RequestExpired,
        Self::RequestVoid,
        Self::AdminSetup,
        Self::AdminRoleGranted,
        Self::AdminRoleRemoved,
        Self::RecoveryContactNamed,
        Self::RecoveryContactConfirmed,
        Self::RecoveryContactChanged,
        Self::SessionEndedReuse,
        Self::SessionEndedLimit,
        Self::SessionsEnded,
        Self::FactorRestored,
        Self::ThrottleReset,
        Self::MayApproveChanged,
        Self::BootstrapReminder,
    ];

    /// The C28 `event_type` name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SignIn => "sign_in",
            Self::SignInNotFinished => "sign_in_not_finished",
            Self::SecondFactorFailed => "second_factor_failed",
            Self::Guessing => "guessing",
            Self::ApprovalReported => "approval_reported",
            Self::AuthenticatorAdded => "authenticator_added",
            Self::AuthenticatorRemoved => "authenticator_removed",
            Self::KeyInvalidated => "key_invalidated",
            Self::RecoveryCodesCreated => "recovery_codes_created",
            Self::PasswordSet => "password_set",
            Self::PasswordChanged => "password_changed",
            Self::PasswordRemoved => "password_removed",
            Self::PasswordThrottled => "password_throttled",
            Self::PasswordCompromised => "password_compromised",
            Self::TotpThrottled => "totp_throttled",
            Self::TotpCompromised => "totp_compromised",
            Self::StepUpAction => "step_up_action",
            Self::Repudiated => "repudiated",
            Self::RequestStarted => "request_started",
            Self::RequestCompleted => "request_completed",
            Self::RequestCancelled => "request_cancelled",
            Self::RequestWithdrawn => "request_withdrawn",
            Self::RequestReadySoon => "request_ready_soon",
            Self::RequestReady => "request_ready",
            Self::RequestExpired => "request_expired",
            Self::RequestVoid => "request_void",
            Self::AdminSetup => "admin_setup",
            Self::AdminRoleGranted => "admin_role_granted",
            Self::AdminRoleRemoved => "admin_role_removed",
            Self::RecoveryContactNamed => "recovery_contact_named",
            Self::RecoveryContactConfirmed => "recovery_contact_confirmed",
            Self::RecoveryContactChanged => "recovery_contact_changed",
            Self::SessionEndedReuse => "session_ended_reuse",
            Self::SessionEndedLimit => "session_ended_limit",
            Self::SessionsEnded => "sessions_ended",
            Self::FactorRestored => "factor_restored",
            Self::ThrottleReset => "throttle_reset",
            Self::MayApproveChanged => "may_approve_changed",
            Self::BootstrapReminder => "bootstrap_reminder",
        }
    }

    /// Parse a stored `event_type`.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == s)
    }

    /// The C28 severity. `authenticator_added` is high for a key device or
    /// passkey and normal for TOTP (`details.type`).
    #[must_use]
    pub fn severity(self, details: &serde_json::Value) -> Severity {
        match self {
            Self::AuthenticatorAdded => match details.get("type").and_then(|v| v.as_str()) {
                Some("totp") => Severity::Normal,
                _ => Severity::High,
            },
            Self::AuthenticatorRemoved
            | Self::KeyInvalidated
            | Self::PasswordThrottled
            | Self::PasswordCompromised
            | Self::TotpThrottled
            | Self::TotpCompromised
            | Self::Repudiated
            | Self::RequestStarted
            | Self::RequestCompleted
            | Self::RequestCancelled
            | Self::RequestWithdrawn
            | Self::AdminSetup
            | Self::AdminRoleGranted
            | Self::AdminRoleRemoved
            | Self::SessionsEnded
            | Self::FactorRestored
            | Self::MayApproveChanged => Severity::High,
            Self::SignIn
            | Self::SignInNotFinished
            | Self::SecondFactorFailed
            | Self::Guessing
            | Self::ApprovalReported
            | Self::RecoveryCodesCreated
            | Self::PasswordSet
            | Self::PasswordChanged
            | Self::PasswordRemoved
            | Self::StepUpAction
            | Self::RequestReadySoon
            | Self::RequestReady
            | Self::RequestExpired
            | Self::RequestVoid
            | Self::RecoveryContactNamed
            | Self::RecoveryContactConfirmed
            | Self::RecoveryContactChanged
            | Self::SessionEndedReuse
            | Self::SessionEndedLimit
            | Self::ThrottleReset
            | Self::BootstrapReminder => Severity::Normal,
        }
    }
}

/// `high` events also reach the confirmed recovery contact and all admins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    High,
    Normal,
}

impl Severity {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Normal => "normal",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "high" => Some(Self::High),
            "normal" => Some(Self::Normal),
            _ => None,
        }
    }
}

/// Who caused an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Actor {
    /// The subject themselves (`self`).
    User(Uuid),
    Admin(Uuid),
    RecoveryContact(Uuid),
    System,
    Cli,
}

impl Actor {
    /// The stored `actor_kind`.
    #[must_use]
    pub const fn kind(self) -> &'static str {
        match self {
            Self::User(_) => "self",
            Self::Admin(_) => "admin",
            Self::RecoveryContact(_) => "recovery_contact",
            Self::System => "system",
            Self::Cli => "cli",
        }
    }

    /// The acting user, if a user acted.
    #[must_use]
    pub const fn user_id(self) -> Option<Uuid> {
        match self {
            Self::User(id) | Self::Admin(id) | Self::RecoveryContact(id) => Some(id),
            Self::System | Self::Cli => None,
        }
    }
}

/// An event to record.
#[derive(Clone, Debug)]
pub struct NewSecurityEvent {
    pub subject_id: Uuid,
    pub event_type: EventType,
    pub actor: Actor,
    /// The client class and its (already bounded) label.
    pub client: Option<(ClientClass, String)>,
    /// Display and audit only (CC3).
    pub ip: Option<IpAddr>,
    pub session_id: Option<Uuid>,
    pub authenticator: Option<AuthenticatorRef>,
    pub request_id: Option<Uuid>,
    pub details: serde_json::Value,
}

impl NewSecurityEvent {
    /// An event with no client, IP, session, authenticator or request.
    #[must_use]
    pub const fn new(
        subject_id: Uuid,
        event_type: EventType,
        actor: Actor,
        details: serde_json::Value,
    ) -> Self {
        Self {
            subject_id,
            event_type,
            actor,
            client: None,
            ip: None,
            session_id: None,
            authenticator: None,
            request_id: None,
            details,
        }
    }
}

/// `SecurityEvent.actor` (C28).
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct EventActor {
    /// `self`, `admin`, `recovery_contact`, `system` or `cli`.
    pub kind: String,
    pub user_id: Option<Uuid>,
    pub username: Option<String>,
}

/// `SecurityEvent.client` (C28).
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct EventClient {
    /// `web`, `desktop` or `android`.
    pub class: String,
    pub label: String,
}

/// `SecurityEvent.authenticator` (C28).
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct EventAuthenticator {
    pub id: Uuid,
    /// `passkey` or `key_device`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Always a string (C28): the current label, else the label the event
    /// recorded, else a neutral name (a pending passkey that went with its
    /// request has no row).
    pub label: String,
}

/// An action the reader can take on an event (C28; computed per reader).
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct Action {
    /// `repudiate`, `cancel_request`, `remove_device` or `confirm_contact`.
    pub action: String,
    /// `step_up`, `part2` or null.
    pub via: Option<String>,
    /// `voids_own_request`, `voids_own_bootstrap` or null.
    pub warning: Option<String>,
}

/// The C28 wire type of one event.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct SecurityEvent {
    pub id: Uuid,
    pub event_type: String,
    /// `high` or `normal`.
    pub severity: String,
    pub created_at: DateTime<Utc>,
    pub subject_id: Uuid,
    pub actor: EventActor,
    pub client: Option<EventClient>,
    /// Only in the owner's own log and the admin log.
    pub place: Option<String>,
    /// Only in the owner's own log and the admin log.
    pub ip: Option<String>,
    pub session_id: Option<Uuid>,
    pub authenticator: Option<EventAuthenticator>,
    pub request_id: Option<Uuid>,
    #[schema(value_type = Object)]
    pub details: serde_json::Value,
    pub actions: Vec<Action>,
    pub repudiated_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for t in EventType::ALL {
            assert!(seen.insert(t.as_str()));
            assert_eq!(EventType::parse(t.as_str()), Some(t));
        }
        assert_eq!(EventType::parse("nope"), None);
    }

    #[test]
    fn severity_follows_c28() {
        let none = serde_json::json!({});
        assert_eq!(
            EventType::AuthenticatorAdded.severity(&serde_json::json!({"type": "totp"})),
            Severity::Normal
        );
        assert_eq!(
            EventType::AuthenticatorAdded.severity(&serde_json::json!({"type": "key_device"})),
            Severity::High
        );
        assert_eq!(EventType::SignIn.severity(&none), Severity::Normal);
        assert_eq!(EventType::KeyInvalidated.severity(&none), Severity::High);
        assert_eq!(EventType::RequestReady.severity(&none), Severity::Normal);
        assert_eq!(EventType::AdminRoleRemoved.severity(&none), Severity::High);
    }
}
