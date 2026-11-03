//! `GET /.well-known/assetlinks.json` (C29): Digital Asset Links for the
//! Android apps, so Credential Manager accepts this domain as their RP.

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

use super::config::{assetlinks_fingerprint, PasskeyConfig};
use crate::api::AppState;

/// One statement per app id with every configured certificate (upper-case
/// colon hex); `[]` without Android apps.
#[must_use]
pub fn assetlinks_body(cfg: &PasskeyConfig) -> serde_json::Value {
    let fingerprints: Vec<String> = cfg
        .android_cert_sha256
        .iter()
        .map(assetlinks_fingerprint)
        .collect();
    if fingerprints.is_empty() {
        return serde_json::json!([]);
    }
    serde_json::Value::Array(
        cfg.android_app_ids
            .iter()
            .map(|id| {
                serde_json::json!({
                    "relation": [
                        "delegate_permission/common.get_login_creds",
                        "delegate_permission/common.handle_all_urls"
                    ],
                    "target": {
                        "namespace": "android_app",
                        "package_name": id,
                        "sha256_cert_fingerprints": fingerprints,
                    }
                })
            })
            .collect(),
    )
}

/// Public, no auth, no rate limit; cacheable for an hour.
pub async fn assetlinks(State(state): State<AppState>) -> Response {
    let body = state
        .config
        .passkeys
        .as_deref()
        .map_or_else(|| serde_json::json!([]), |rp| assetlinks_body(rp.config()));
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        body.to_string(),
    )
        .into_response()
}
