//! File serving endpoint — streams files from S3 with caching headers.
//!
//! `GET /api/files/{key...}` → streams public files (avatars, banners) from S3.
//! No auth required; only public key prefixes are served.

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;

use crate::api::AppState;

/// Convert an S3 key to an API file URL.
pub fn file_url(s3_key: &str) -> String {
    format!("/api/files/{s3_key}")
}

/// Transform an optional S3 key to an API file URL.
/// Returns None if input is None, or if it already starts with /api/ or http.
#[allow(clippy::single_option_map)]
pub fn maybe_file_url(s3_key: Option<String>) -> Option<String> {
    match s3_key {
        Some(key) if key.starts_with("/api/") || key.starts_with("http") => Some(key),
        Some(key) => Some(file_url(&key)),
        None => None,
    }
}

/// Infer MIME type from file extension.
fn content_type_from_key(key: &str) -> &'static str {
    // Extensions are matched case-insensitively so `.PNG` still renders under `nosniff`.
    match key
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

/// Whether `key` names an object that may be served without authentication.
///
/// Only user avatars (`avatars/...`, including DM icons under
/// `avatars/channels/...`) and server banners (`guilds/{uuid}/banner-...`) are
/// public. Every other prefix is served through authenticated routes only.
pub fn is_public_file_key(key: &str) -> bool {
    if key.starts_with('/') || key.contains('\\') {
        return false;
    }
    // Segment-based check: dots inside file names are fine, but empty,
    // `.` and `..` segments are never accepted.
    if key
        .split('/')
        .any(|seg| seg.is_empty() || seg == "." || seg == "..")
    {
        return false;
    }
    if key.starts_with("avatars/") {
        return true;
    }
    if let Some(rest) = key.strip_prefix("guilds/") {
        if let Some((guild_id, file)) = rest.split_once('/') {
            let canonical = uuid::Uuid::parse_str(guild_id)
                .map(|u| u.hyphenated().to_string() == guild_id)
                .unwrap_or(false);
            return canonical
                && file.starts_with("banner-")
                && file.len() > "banner-".len()
                && !file.contains('/');
        }
    }
    false
}

/// Serve a public file from S3 storage, streaming the body.
pub async fn serve(State(state): State<AppState>, Path(key): Path<String>) -> impl IntoResponse {
    if !is_public_file_key(&key) {
        return (StatusCode::NOT_FOUND, "File not found").into_response();
    }

    let s3 = match &state.s3 {
        Some(s3) => s3,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "File storage not configured",
            )
                .into_response()
        }
    };

    match s3.get_object_stream(&key).await {
        Ok(stream) => {
            let content_type = content_type_from_key(&key);
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
            headers.insert(
                header::CACHE_CONTROL,
                "public, max-age=86400".parse().unwrap(),
            );
            headers.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());

            let body = axum::body::Body::new(stream.into_inner());
            (StatusCode::OK, headers, body).into_response()
        }
        Err(e) => {
            tracing::warn!(key = %key, error = %e, "Failed to serve file from S3");
            (StatusCode::NOT_FOUND, "File not found").into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_type_ignores_extension_case() {
        assert_eq!(content_type_from_key("avatars/u/a.png"), "image/png");
        assert_eq!(content_type_from_key("avatars/u/a.PNG"), "image/png");
        assert_eq!(content_type_from_key("avatars/u/a.JpG"), "image/jpeg");
        assert_eq!(content_type_from_key("avatars/u/a.WEBP"), "image/webp");
        assert_eq!(
            content_type_from_key("avatars/u/a.bin"),
            "application/octet-stream"
        );
    }

    #[test]
    fn public_keys() {
        assert!(is_public_file_key("avatars/0190abcd/1700_me.png"));
        assert!(is_public_file_key("avatars/channels/0190abcd/0190abce.png"));
        assert!(is_public_file_key(
            "guilds/01900000-0000-7000-8000-000000000000/banner-1700_b.png"
        ));
        assert!(!is_public_file_key("attachments/c/m/f.png"));
        assert!(!is_public_file_key("exports/u/j.zip"));
        assert!(!is_public_file_key("emojis/g/e.png"));
        assert!(!is_public_file_key("avatars/../exports/u/j.zip"));
        assert!(!is_public_file_key("avatars//x.png"));
        assert!(!is_public_file_key("avatars/"));
        assert!(!is_public_file_key("guilds/not-a-uuid/banner-x.png"));
        assert!(!is_public_file_key(
            "guilds/01900000-0000-7000-8000-000000000000/other.png"
        ));
        assert!(!is_public_file_key("/avatars/x.png"));
        assert!(is_public_file_key("avatars/u/1_a..b.png"));
        assert!(is_public_file_key(
            "guilds/01900000-0000-7000-8000-000000000000/banner-1_x..y.png"
        ));
        assert!(!is_public_file_key("avatars/u/../x"));
        assert!(!is_public_file_key("avatars/./x"));
        assert!(!is_public_file_key(
            "guilds/0190000000007000800000000000000000/banner-x.png"
        ));
    }
}
