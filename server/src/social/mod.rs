pub mod block_cache;
pub mod friends;
pub mod types;

use axum::{
    routing::{delete, get, post},
    Router,
};

use crate::api::AppState;

/// Create the social router with friend management endpoints
pub fn router() -> Router<AppState> {
    Router::new()
        // Friend requests
        .route("/friends/request", post(friends::send_friend_request))
        .route("/friends", get(friends::list_friends))
        .route("/friends/pending", get(friends::list_pending_requests))
        .route("/friends/blocked", get(friends::list_blocked))
        // Friend actions
        .route("/friends/{id}/accept", post(friends::accept_friend_request))
        .route("/friends/{id}/reject", post(friends::reject_friend_request))
        .route("/friends/{id}/block", post(friends::block_user).delete(friends::unblock_user))
        .route("/friends/{id}", delete(friends::remove_friend))
}
