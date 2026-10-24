//! Integration tests for the public file endpoint.
//! Run: `cargo test --test integration files_http`

use axum::body::Body;
use axum::http::{Method, StatusCode};
use sqlx::PgPool;

use crate::helpers::TestApp;

#[sqlx::test]
async fn test_files_endpoint_rejects_private_prefix_without_s3(pool: PgPool) {
    let app = TestApp::with_pool(pool).await;

    let req = TestApp::request(Method::GET, "/api/files/exports/x/y.zip")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // An allowed prefix reaches the storage check (no S3 configured).
    let req = TestApp::request(Method::GET, "/api/files/avatars/x.png")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}
