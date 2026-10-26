//! Optional features behind a server flag (`KAIKU_OPTIONAL_FEATURES`).
//!
//! Forum channels, announcement channels, scheduled events and membership
//! screening are off unless the operator lists them. While a feature is off:
//! - its routes answer 404 `FEATURE_DISABLED` ([`feature_gate`]),
//! - new channels of that type are refused with 400 `FEATURE_DISABLED`,
//! - its background work (crossposts, event scheduler) does not run,
//! - new members are never placed in `pending` (screening).
//!
//! Existing rows are never deleted; turning a feature back on restores it.

use std::sync::atomic::{AtomicBool, Ordering};

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};

use crate::api::AppState;

/// Error code returned when a request touches a feature that is off.
pub const FEATURE_DISABLED: &str = "FEATURE_DISABLED";

/// Message returned with [`FEATURE_DISABLED`].
pub const FEATURE_DISABLED_MESSAGE: &str = "This feature is turned off on this server.";

/// One optional feature.
#[derive(Clone, Copy, Debug)]
pub enum OptionalFeature {
    Forum,
    Announcements,
    Events,
    Screening,
}

/// The set of optional features turned on for this server. Default: all off.
/// (Independent on/off flags by design, hence the bools.)
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OptionalFeatures {
    pub forum: bool,
    pub announcements: bool,
    pub events: bool,
    pub screening: bool,
}

impl OptionalFeatures {
    /// Parse a comma-separated list (`forum, announcements, events, screening`).
    ///
    /// Names are trimmed and matched case-insensitively; empty entries are
    /// skipped. Returns the parsed set and the unknown names (to be logged as
    /// warnings and otherwise ignored).
    #[must_use]
    pub fn parse(list: &str) -> (Self, Vec<String>) {
        let mut features = Self::default();
        let mut unknown = Vec::new();
        for name in list.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            match name.to_ascii_lowercase().as_str() {
                "forum" => features.forum = true,
                "announcements" => features.announcements = true,
                "events" => features.events = true,
                "screening" => features.screening = true,
                _ => unknown.push(name.to_string()),
            }
        }
        (features, unknown)
    }

    /// Whether `feature` is turned on.
    #[must_use]
    pub const fn is_enabled(&self, feature: OptionalFeature) -> bool {
        match feature {
            OptionalFeature::Forum => self.forum,
            OptionalFeature::Announcements => self.announcements,
            OptionalFeature::Events => self.events,
            OptionalFeature::Screening => self.screening,
        }
    }
}

/// Process-wide copy of the screening flag for the permission resolver, which
/// only receives a database pool (dozens of call sites). Set from the config
/// by [`crate::api::AppState::new`]; a server process has one config. Starts
/// `true` (screening enforced) until the state is built.
static SCREENING_ENFORCED: AtomicBool = AtomicBool::new(true);

/// Record whether membership screening is on for this process.
pub fn set_screening_enforced(on: bool) {
    SCREENING_ENFORCED.store(on, Ordering::Relaxed);
}

/// Whether `pending` members are held back. While screening is off they are
/// treated as active (rows are not changed, so turning screening back on
/// holds them again).
#[must_use]
pub fn screening_enforced() -> bool {
    SCREENING_ENFORCED.load(Ordering::Relaxed)
}

/// Whether new channels (or content in channels) of `channel_type` are
/// allowed: forum and announcement channels need their feature.
#[must_use]
pub const fn channel_type_enabled(
    features: &OptionalFeatures,
    channel_type: &crate::db::ChannelType,
) -> bool {
    match channel_type {
        crate::db::ChannelType::Forum => features.forum,
        crate::db::ChannelType::Announcement => features.announcements,
        crate::db::ChannelType::Text
        | crate::db::ChannelType::Voice
        | crate::db::ChannelType::Dm => true,
    }
}

/// Whether new content may be written into an existing channel of
/// `channel_type`: forum channels take nothing new while the forum feature is
/// off (user sends, bot messages, crossposts, pins, webhooks).
#[must_use]
pub fn channel_writable(
    features: &OptionalFeatures,
    channel_type: &crate::db::ChannelType,
) -> bool {
    *channel_type != crate::db::ChannelType::Forum || features.forum
}

/// JSON error response for a disabled feature with the given status.
#[must_use]
pub fn feature_disabled_response(status: StatusCode) -> Response {
    (
        status,
        Json(serde_json::json!({
            "error": FEATURE_DISABLED,
            "message": FEATURE_DISABLED_MESSAGE,
        })),
    )
        .into_response()
}

/// Route-layer middleware: answers 404 `FEATURE_DISABLED` when `feature` is
/// off, otherwise passes the request on.
pub async fn feature_gate(
    State((state, feature)): State<(AppState, OptionalFeature)>,
    req: Request,
    next: Next,
) -> Response {
    if state.config.optional_features.is_enabled(feature) {
        next.run(req).await
    } else {
        feature_disabled_response(StatusCode::NOT_FOUND)
    }
}

/// Apply [`feature_gate`] for `feature` to every route of `router`
/// (`route_layer`, so unmatched paths keep their normal 404/405).
pub fn gated(
    state: &AppState,
    feature: OptionalFeature,
    router: Router<AppState>,
) -> Router<AppState> {
    router.route_layer(from_fn_with_state((state.clone(), feature), feature_gate))
}

#[cfg(test)]
mod tests {
    use super::{OptionalFeature, OptionalFeatures};

    #[test]
    fn parse_known_unknown_and_empty() {
        let (f, unknown) = OptionalFeatures::parse("forum, events");
        assert_eq!(
            f,
            OptionalFeatures {
                forum: true,
                events: true,
                ..OptionalFeatures::default()
            }
        );
        assert!(unknown.is_empty());
        assert!(f.is_enabled(OptionalFeature::Forum));
        assert!(!f.is_enabled(OptionalFeature::Announcements));

        let (f, unknown) = OptionalFeatures::parse("nope");
        assert_eq!(f, OptionalFeatures::default());
        assert_eq!(unknown, vec!["nope".to_string()]);

        let (f, unknown) = OptionalFeatures::parse("");
        assert_eq!(f, OptionalFeatures::default());
        assert!(unknown.is_empty());

        let (f, unknown) = OptionalFeatures::parse(" Announcements ,,screening,forum");
        assert_eq!(
            f,
            OptionalFeatures {
                forum: true,
                announcements: true,
                screening: true,
                events: false,
            }
        );
        assert!(unknown.is_empty());
    }
}
