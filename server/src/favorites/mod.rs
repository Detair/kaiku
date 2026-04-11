//! User Favorites API
//!
//! CRUD operations for user's cross-server channel favorites.

pub mod error;
pub mod handlers;
pub mod types;

pub use error::FavoritesError;
pub use types::{Favorite, FavoriteChannel, FavoritesResponse, ReorderChannelsRequest, ReorderGuildsRequest};

use axum::routing::{get, post, put};
use axum::Router;

use crate::api::AppState;

/// Create the favorites router, mounted at `/api/me/favorites`.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(handlers::list_favorites))
        .route("/reorder", put(handlers::reorder_channels))
        .route("/reorder-guilds", put(handlers::reorder_guilds))
        .route("/{channel_id}", post(handlers::add_favorite).delete(handlers::remove_favorite))
}

#[cfg(test)]
mod tests {
    use super::types::*;
    use axum::http::StatusCode;
    use uuid::Uuid;

    #[test]
    fn test_favorite_channel_from_row() {
        let channel_id = Uuid::new_v4();
        let guild_id = Uuid::new_v4();

        let row = FavoriteChannelRow {
            channel_id,
            channel_name: "general".to_string(),
            channel_type: "text".to_string(),
            guild_id,
            guild_name: "My Server".to_string(),
            guild_icon: Some("https://example.com/icon.png".to_string()),
            guild_position: 0,
            channel_position: 1,
        };

        let channel = FavoriteChannel::from(row);

        assert_eq!(channel.channel_id, channel_id.to_string());
        assert_eq!(channel.channel_name, "general");
        assert_eq!(channel.channel_type, "text");
        assert_eq!(channel.guild_id, guild_id.to_string());
        assert_eq!(channel.guild_name, "My Server");
        assert_eq!(
            channel.guild_icon,
            Some("https://example.com/icon.png".to_string())
        );
        assert_eq!(channel.guild_position, 0);
        assert_eq!(channel.channel_position, 1);
    }

    #[test]
    fn test_favorite_channel_from_row_no_icon() {
        let row = FavoriteChannelRow {
            channel_id: Uuid::new_v4(),
            channel_name: "voice".to_string(),
            channel_type: "voice".to_string(),
            guild_id: Uuid::new_v4(),
            guild_name: "Another Server".to_string(),
            guild_icon: None,
            guild_position: 2,
            channel_position: 0,
        };

        let channel = FavoriteChannel::from(row);

        assert_eq!(channel.guild_icon, None);
        assert_eq!(channel.channel_type, "voice");
    }

    #[test]
    fn test_favorites_error_status_codes() {
        use axum::response::IntoResponse;
        use super::error::FavoritesError;

        let test_cases = vec![
            (FavoritesError::ChannelNotFound, StatusCode::NOT_FOUND),
            (FavoritesError::InvalidChannel, StatusCode::BAD_REQUEST),
            (FavoritesError::LimitExceeded, StatusCode::BAD_REQUEST),
            (FavoritesError::AlreadyFavorited, StatusCode::CONFLICT),
            (FavoritesError::NotFavorited, StatusCode::NOT_FOUND),
            (FavoritesError::InvalidChannels, StatusCode::BAD_REQUEST),
            (FavoritesError::InvalidGuilds, StatusCode::BAD_REQUEST),
        ];

        for (error, expected_status) in test_cases {
            let response = error.into_response();
            assert_eq!(
                response.status(),
                expected_status,
                "Unexpected status for error"
            );
        }
    }

    #[test]
    fn test_max_favorites_constant() {
        assert_eq!(MAX_FAVORITES_PER_USER, 25);
    }

    #[test]
    fn test_reorder_request_deserialization() {
        let json = r#"{"guild_id": "abc123", "channel_ids": ["ch1", "ch2", "ch3"]}"#;
        let request: ReorderChannelsRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.guild_id, "abc123");
        assert_eq!(request.channel_ids, vec!["ch1", "ch2", "ch3"]);
    }

    #[test]
    fn test_reorder_guilds_request_deserialization() {
        let json = r#"{"guild_ids": ["g1", "g2"]}"#;
        let request: ReorderGuildsRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.guild_ids, vec!["g1", "g2"]);
    }
}
