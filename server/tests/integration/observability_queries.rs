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

const ROUTE_DURATION: &str = vc_server::observability::metrics::HTTP_ROUTE_DURATION_METRIC;

/// The summary's latency p95 reads the unlabelled histogram only; per-route
/// rows (one slow route among fast ones) must not be averaged into it.
#[sqlx::test]
async fn summary_latency_p95_uses_unlabelled_total_series(pool: PgPool) {
    let total = json!({});
    insert_sample(&pool, "kaiku_http_request_duration_ms", &total, 1001, 5.0).await;
    let fast = json!({ "http.route": "/fast", "http.response.status_code": "200" });
    let slow = json!({ "http.route": "/slow", "http.response.status_code": "200" });
    insert_sample(&pool, ROUTE_DURATION, &fast, 1000, 5.0).await;
    insert_sample(&pool, ROUTE_DURATION, &slow, 1, 4000.0).await;

    let from = Utc::now() - Duration::hours(1);
    let to = Utc::now() + Duration::minutes(1);
    let p95 = vc_server::admin::queries::summary_latency_p95(&pool, from, to)
        .await
        .unwrap()
        .expect("p95");
    assert!((p95 - 5.0).abs() < f64::EPSILON, "p95 {p95}");

    let trend = storage::query_trends(&pool, "kaiku_http_request_duration_ms", from, to)
        .await
        .unwrap();
    assert_eq!(trend.len(), 1);
    assert_eq!(trend[0].value_p95, Some(5.0));
}

#[sqlx::test]
async fn observability_aggregate_queries_decode_with_data(pool: PgPool) {
    let route = json!({ "http.route": "/api/x", "http.response.status_code": "200" });
    let route_err = json!({ "http.route": "/api/x", "http.response.status_code": "500" });
    let err_typed = json!({ "http.response.status_code": "500", "error.type": "500" });

    insert_sample(&pool, ROUTE_DURATION, &route, 10, 12.5).await;
    insert_sample(&pool, ROUTE_DURATION, &route_err, 3, 40.0).await;
    insert_sample(&pool, "kaiku_http_requests_total", &json!({}), 100, 0.0).await;
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

    // top_errors: per-route error rows from the route histogram must decode.
    // The (newer, `error.type`-carrying) counter row is the same requests and
    // must not be added on top.
    let errors = storage::query_top_errors(&pool, from, to, 10)
        .await
        .expect("query_top_errors must decode with data present");
    assert_eq!(errors.len(), 1, "expected one error row: {errors:?}");
    assert_eq!(errors[0].error_type.as_deref(), Some("500"));
    assert_eq!(errors[0].route.as_deref(), Some("/api/x"));
    assert_eq!(errors[0].error_count, Some(3));
    assert_eq!(errors[0].avg_p95, Some(40.0));

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

/// End-to-end regression for "error rate shows a peak but Top Routes / Top
/// Errors say No data" with the OTLP exporter disabled.
///
/// Real requests go through the full router (`http_metrics` middleware), the
/// native-only meter provider (`OBSERVABILITY_ENABLED=false`) exports them,
/// the samples are stored, and the admin queries must then agree: the error
/// counts behind the error-rate vital sign also show up per route and per
/// error type, labelled with the matched route template (not the raw URI).
#[sqlx::test]
async fn top_routes_and_errors_have_data_for_4xx_without_otlp(pool: PgPool) {
    use axum::body::Body;
    use axum::http::Method;
    use vc_server::config::ObservabilityConfig;
    use vc_server::observability::{ingestion, metrics};

    use crate::helpers::TestApp;

    // Native exporter only — exactly what a deployment without OTLP runs.
    let (metric_tx, mut metric_rx) = ingestion::metric_channel();
    let provider = metrics::init(
        &ObservabilityConfig {
            enabled: false,
            otlp_endpoint: String::new(),
            service_name: "vc-server-test".into(),
            trace_sample_ratio: 0.0,
            log_level: String::new(),
        },
        metric_tx,
    )
    .expect("native meter provider");
    metrics::register_metrics();

    let app = TestApp::with_pool(pool.clone()).await;
    let guild_id = uuid::Uuid::new_v4();
    // Unauthenticated: rejected by `require_auth` with 401 before the handler.
    for _ in 0..3 {
        let resp = app
            .oneshot(
                TestApp::request(Method::GET, &format!("/api/guilds/{guild_id}/members"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(resp.status(), 401);
    }
    // Unmatched path: 404 with a fixed placeholder route label.
    let resp = app
        .oneshot(
            TestApp::request(Method::GET, &format!("/no-such-route/{guild_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), 404);

    provider.force_flush().expect("flush native exporter");
    while let Ok(s) = metric_rx.try_recv() {
        storage::insert_metric_sample(
            &pool,
            &InsertMetricSample {
                ts: s.ts,
                metric_name: &s.metric_name,
                scope: &s.scope,
                labels: &s.labels,
                value_count: s.value_count,
                value_sum: s.value_sum,
                value_p50: s.value_p50,
                value_p95: s.value_p95,
                value_p99: s.value_p99,
            },
        )
        .await
        .expect("store exported sample");
    }

    let from = Utc::now() - Duration::hours(1);
    let to = Utc::now() + Duration::minutes(1);
    let template = "/api/guilds/{id}/members";

    let counts = vc_server::admin::queries::summary_error_and_request_counts(&pool, from, to)
        .await
        .unwrap()
        .expect("counts row");
    assert!(
        counts.0.unwrap_or(0) >= 4,
        "error-rate source saw the errors"
    );

    let routes = storage::query_top_routes(&pool, from, to, true, 10)
        .await
        .unwrap();
    assert!(
        routes.iter().all(|r| r
            .route
            .as_deref()
            .is_some_and(|p| !p.contains(&guild_id.to_string()))),
        "routes must be templates, not raw URIs: {routes:?}"
    );
    let guild_route = routes
        .iter()
        .find(|r| r.route.as_deref() == Some(template))
        .unwrap_or_else(|| panic!("top routes must list {template}: {routes:?}"));
    assert!(guild_route.request_count.unwrap_or(0) >= 3);
    assert!(
        guild_route.error_count.unwrap_or(0) >= 3,
        "4xx must count as errors, like the error-rate vital sign"
    );
    assert!(routes
        .iter()
        .any(|r| r.route.as_deref() == Some(metrics::UNMATCHED_ROUTE)));

    let errors = storage::query_top_errors(&pool, from, to, 10)
        .await
        .unwrap();
    let unauthorized = errors
        .iter()
        .find(|e| e.error_type.as_deref() == Some("401") && e.route.as_deref() == Some(template))
        .unwrap_or_else(|| panic!("top errors must list 401 on {template}: {errors:?}"));
    assert!(unauthorized.error_count.unwrap_or(0) >= 3);
}

/// Samples stored before the error counter carried `error.type` / `http.route`
/// (only `http.response.status_code`) must still appear in Top Errors.
#[sqlx::test]
async fn top_errors_include_legacy_status_only_samples(pool: PgPool) {
    let legacy = json!({ "http.response.status_code": "429" });
    insert_sample(&pool, "kaiku_http_errors_total", &legacy, 7, 0.0).await;

    let from = Utc::now() - Duration::hours(1);
    let to = Utc::now() + Duration::minutes(1);
    let errors = storage::query_top_errors(&pool, from, to, 10)
        .await
        .unwrap();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].error_type.as_deref(), Some("429"));
    assert_eq!(errors[0].route, None);
    assert_eq!(errors[0].error_count, Some(7));
}

/// 7d/30d trends read daily rollups: counters are summed per day, but level
/// metrics (gauges) must be averaged — summing 1440 memory samples a day
/// would chart ~1440x the real RSS.
#[sqlx::test]
async fn rollup_trends_average_gauges_and_sum_counters(pool: PgPool) {
    let day_ts = Utc::now() - Duration::days(2);
    for (metric, value) in [
        ("kaiku_process_memory_bytes", 100_i64),
        ("kaiku_process_memory_bytes", 300),
        ("kaiku_http_errors_total", 4),
        ("kaiku_http_errors_total", 6),
    ] {
        storage::insert_metric_sample(
            &pool,
            &InsertMetricSample {
                ts: day_ts,
                metric_name: metric,
                scope: "test",
                labels: &json!({}),
                value_count: Some(value),
                value_sum: None,
                value_p50: None,
                value_p95: None,
                value_p99: None,
            },
        )
        .await
        .unwrap();
    }
    // f64 gauge (CPU %): stored in value_sum.
    for cpu in [2.0_f64, 4.0] {
        storage::insert_metric_sample(
            &pool,
            &InsertMetricSample {
                ts: day_ts,
                metric_name: "kaiku_process_cpu_percent",
                scope: "test",
                labels: &json!({}),
                value_count: None,
                value_sum: Some(cpu),
                value_p50: None,
                value_p95: None,
                value_p99: None,
            },
        )
        .await
        .unwrap();
    }
    sqlx::query("REFRESH MATERIALIZED VIEW telemetry_trend_rollups")
        .execute(&pool)
        .await
        .unwrap();

    let from = Utc::now() - Duration::days(7);
    let to = Utc::now();
    let mem = storage::query_trends(&pool, "kaiku_process_memory_bytes", from, to)
        .await
        .unwrap();
    assert_eq!(mem.len(), 1);
    assert_eq!(mem[0].value_count, Some(200), "gauge: daily mean");

    let errs = storage::query_trends(&pool, "kaiku_http_errors_total", from, to)
        .await
        .unwrap();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].value_count, Some(10), "counter: daily total");

    let cpu = storage::query_trends(&pool, "kaiku_process_cpu_percent", from, to)
        .await
        .unwrap();
    assert_eq!(cpu.len(), 1);
    assert_eq!(
        cpu[0].value_sum,
        Some(3.0),
        "f64 gauge: daily mean of value_sum"
    );
}

/// Several label sets exported in one cycle share a `ts`; the trend must
/// collapse them into a single point per cycle.
#[sqlx::test]
async fn trends_collapse_label_sets_per_export_cycle(pool: PgPool) {
    let ts = Utc::now();
    for (route, count) in [("/a", 2_i64), ("/b", 5)] {
        storage::insert_metric_sample(
            &pool,
            &InsertMetricSample {
                ts,
                metric_name: "kaiku_http_errors_total",
                scope: "test",
                labels: &json!({ "http.route": route, "error.type": "401" }),
                value_count: Some(count),
                value_sum: None,
                value_p50: None,
                value_p95: None,
                value_p99: None,
            },
        )
        .await
        .unwrap();
    }
    let trend = storage::query_trends(
        &pool,
        "kaiku_http_errors_total",
        ts - Duration::hours(1),
        ts + Duration::minutes(1),
    )
    .await
    .unwrap();
    assert_eq!(trend.len(), 1, "one point per export cycle: {trend:?}");
    assert_eq!(trend[0].value_count, Some(7));
}
