//! Regression tests for the admin Command Center observability queries.
//!
//! Several of these queries `SUM(value_count)` over a `bigint` column, which
//! `PostgreSQL` returns as `NUMERIC`. Without a `::bigint` cast the row fails to
//! decode into `i64` — but only once there is data, because `SUM` over zero
//! rows is `NULL`, which decodes fine. The bug was therefore invisible in an
//! empty test database and only surfaced in production once telemetry
//! accumulated (summary → 500, top-routes/top-errors → 500). These tests seed
//! samples first, then assert the queries succeed and decode.

use chrono::{Duration, Utc};
use serde_json::json;
use sqlx::PgPool;
use vc_server::observability::storage::{self, InsertMetricSample};

async fn insert_sample(
    pool: &PgPool,
    metric: &str,
    labels: &serde_json::Value,
    count: i64,
    p95: f64,
) {
    storage::insert_metric_sample(
        pool,
        &InsertMetricSample {
            ts: Utc::now(),
            metric_name: metric,
            scope: "test",
            labels,
            value_count: Some(count),
            value_sum: None,
            value_p50: None,
            value_p95: Some(p95),
            value_p99: None,
        },
    )
    .await
    .expect("insert metric sample");
}

#[sqlx::test]
async fn observability_aggregate_queries_decode_with_data(pool: PgPool) {
    let route = json!({ "http.route": "/api/x", "http.response.status_code": "200" });
    let route_err = json!({ "http.route": "/api/x", "http.response.status_code": "500" });
    let err_typed = json!({ "error.type": "timeout" });

    insert_sample(&pool, "kaiku_http_request_duration_ms", &route, 10, 12.5).await;
    insert_sample(&pool, "kaiku_http_request_duration_ms", &route_err, 3, 40.0).await;
    insert_sample(&pool, "kaiku_http_requests_total", &route, 100, 0.0).await;
    insert_sample(&pool, "kaiku_http_errors_total", &err_typed, 5, 0.0).await;

    let from = Utc::now() - Duration::hours(1);
    let to = Utc::now() + Duration::minutes(1);

    // top_routes: SUM(value_count)::bigint and the 500-status error SUM must decode.
    let routes = storage::query_top_routes(&pool, from, to, false, 10)
        .await
        .expect("query_top_routes must decode with data present");
    assert_eq!(routes.len(), 1, "expected one route row");
    assert_eq!(routes[0].request_count, Some(13)); // 10 + 3
    assert_eq!(routes[0].error_count, Some(3)); // the 500 sample

    // top_errors: SUM(value_count)::bigint grouped by error.type must decode.
    let errors = storage::query_top_errors(&pool, from, to, 10)
        .await
        .expect("query_top_errors must decode with data present");
    assert_eq!(errors.len(), 1, "expected one error-type row");
    assert_eq!(errors[0].error_count, Some(5));

    // trends (<=24h reads raw samples directly).
    let trend = storage::query_trends(&pool, "kaiku_http_requests_total", from, to)
        .await
        .expect("query_trends must decode");
    assert!(!trend.is_empty(), "expected trend datapoints");

    // summary error/request counts: SUM(...)::bigint must decode.
    let counts = vc_server::admin::queries::summary_error_and_request_counts(&pool, from, to)
        .await
        .expect("summary_error_and_request_counts must decode")
        .expect("counts row present");
    assert_eq!(counts.0, Some(5), "error count");
    assert_eq!(counts.1, Some(100), "request count");
}
