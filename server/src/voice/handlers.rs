//! Voice HTTP Handlers
//!
//! HTTP endpoints for voice-related operations.
//! Voice signaling (join/leave/offer/answer/ice) is handled via WebSocket.

use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::api::AppState;
use crate::auth::AuthUser;

/// ICE server configuration.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct IceServer {
    /// Server URLs (e.g., "stun:stun.l.google.com:19302")
    pub urls: Vec<String>,
    /// Username for TURN servers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Credential for TURN servers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
}

/// Response containing ICE server configuration.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct IceServersResponse {
    /// List of ICE servers to use for WebRTC.
    pub ice_servers: Vec<IceServer>,
}

/// Get ICE server configuration.
///
/// GET /api/voice/ice-servers
///
/// Returns STUN and TURN server configuration for WebRTC connections.
/// Clients should use these servers for NAT traversal.
#[utoipa::path(
    get,
    path = "/api/voice/ice-servers",
    tag = "voice",
    responses(
        (status = 200, description = "ICE server configuration"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn get_ice_servers(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Json<IceServersResponse> {
    let mut servers = vec![IceServer {
        urls: vec![state.config.stun_server.clone()],
        username: None,
        credential: None,
    }];

    // One entry with every TURN URL (`TURN_SERVER` may list several, e.g.
    // UDP and TCP of the same server); one set of credentials for all.
    if !state.config.turn_servers.is_empty() {
        let urls = state.config.turn_servers.clone();
        // Prefer HMAC time-limited credentials when a shared secret is set.
        if let Some(secret) = &state.config.turn_shared_secret {
            let (username, credential) = super::turn_rest_credentials(
                secret,
                state.config.turn_credential_ttl,
                &auth_user.id.to_string(),
            );
            servers.push(IceServer {
                urls,
                username: Some(username),
                credential: Some(credential),
            });
        } else {
            // Fallback: static credentials for dev environments
            servers.push(IceServer {
                urls,
                username: state.config.turn_username.clone(),
                credential: state.config.turn_credential.clone(),
            });
        }
    }

    Json(IceServersResponse {
        ice_servers: servers,
    })
}
