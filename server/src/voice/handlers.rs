//! Voice HTTP Handlers
//!
//! HTTP endpoints for voice-related operations.
//! Voice signaling (join/leave/offer/answer/ice) is handled via WebSocket.

use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::api::AppState;

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
pub async fn get_ice_servers(State(state): State<AppState>) -> Json<IceServersResponse> {
    let mut servers = vec![IceServer {
        urls: vec![state.config.stun_server.clone()],
        username: None,
        credential: None,
    }];

    // Add TURN server if configured
    if let Some(turn) = &state.config.turn_server {
        servers.push(IceServer {
            urls: vec![turn.clone()],
            username: state.config.turn_username.clone(),
            credential: state.config.turn_credential.clone(),
        });
    }

    Json(IceServersResponse {
        ice_servers: servers,
    })
}
