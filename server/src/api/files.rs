//! File serving endpoint — streams files from S3 with caching headers.
//!
//! `GET /api/files/{key...}` → serves file bytes from S3.
//! No auth required (URLs are shared in messages, profiles).

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;

use super::AppState;

/// Convert an S3 key to an API file URL.
pub fn file_url(s3_key: &str) -> String {
    format!("/api/files/{s3_key}")
}

/// Transform an optional S3 key to an API file URL.
/// Returns None if input is None, or if it already starts with /api/ or http.
pub fn maybe_file_url(s3_key: Option<String>) -> Option<String> {
    match s3_key {
        Some(key) if key.starts_with("/api/") || key.starts_with("http") => Some(key),
        Some(key) => Some(file_url(&key)),
        None => None,
    }
}

/// Infer MIME type from file extension.
fn content_type_from_key(key: &str) -> &'static str {
    match key.rsplit('.').next().unwrap_or("") {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

/// Serve a file from S3 storage.
pub async fn serve(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    let s3 = match &state.s3 {
        Some(s3) => s3,
        None => {
            return (StatusCode::SERVICE_UNAVAILABLE, "File storage not configured")
                .into_response()
        }
    };

    match s3.get_object_stream(&key).await {
        Ok(stream) => {
            let bytes = match stream.collect().await {
                Ok(data) => data.into_bytes(),
                Err(e) => {
                    tracing::warn!(key = %key, error = %e, "Failed to read file from S3");
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to read file")
                        .into_response();
                }
            };

            let content_type = content_type_from_key(&key);
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
            headers.insert(
                header::CACHE_CONTROL,
                "public, max-age=86400".parse().unwrap(),
            );

            (StatusCode::OK, headers, bytes).into_response()
        }
        Err(e) => {
            tracing::warn!(key = %key, error = %e, "Failed to serve file from S3");
            (StatusCode::NOT_FOUND, "File not found").into_response()
        }
    }
}
