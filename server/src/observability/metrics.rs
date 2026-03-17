//! OpenTelemetry meter provider initialization.
//!
//! Configures a periodic OTLP metric exporter and installs it as the global
//! meter provider so that any crate can call `opentelemetry::global::meter()`
//! to obtain an instrument without explicit provider wiring.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use opentelemetry::metrics::{Counter, Histogram, UpDownCounter};
use opentelemetry::{global, KeyValue};
use opentelemetry_otlp::WithExportConfig as _;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::Resource;
use sqlx::PgPool;

use crate::config::ObservabilityConfig;

// ============================================================================
// Existing metrics
// ============================================================================

static HTTP_REQUESTS_TOTAL: OnceLock<Counter<u64>> = OnceLock::new();
static HTTP_REQUEST_DURATION_MS: OnceLock<Histogram<f64>> = OnceLock::new();
static VOICE_JOINS_TOTAL: OnceLock<Counter<u64>> = OnceLock::new();
static WS_CONNECTIONS_TOTAL: OnceLock<Counter<u64>> = OnceLock::new();

// ============================================================================
// Renamed: auth_attempts_total -> auth_login_attempts_total
// ============================================================================

static AUTH_LOGIN_ATTEMPTS_TOTAL: OnceLock<Counter<u64>> = OnceLock::new();

// ============================================================================
// New metrics (Change 4)
// ============================================================================

static HTTP_ERRORS_TOTAL: OnceLock<Counter<u64>> = OnceLock::new();
static WS_CONNECTIONS_ACTIVE: OnceLock<UpDownCounter<i64>> = OnceLock::new();
/// Registered but not wired — needs reconnect token feature to distinguish
/// new connections from re-connections.
static WS_RECONNECTS_TOTAL: OnceLock<Counter<u64>> = OnceLock::new();
static WS_MESSAGES_TOTAL: OnceLock<Counter<u64>> = OnceLock::new();
static VOICE_SESSIONS_ACTIVE: OnceLock<UpDownCounter<i64>> = OnceLock::new();
static VOICE_SESSION_DURATION_SECONDS: OnceLock<Histogram<f64>> = OnceLock::new();

/// Hot-path atomic for RTP packet counting — avoids `OTel` overhead per packet.
static VOICE_RTP_PACKETS_FORWARDED: AtomicU64 = AtomicU64::new(0);
static VOICE_RTP_COUNTER: OnceLock<Counter<u64>> = OnceLock::new();

/// `SQLx` query execution time, wired via `SqlxMetricsLayer` tracing layer.
static DB_QUERY_DURATION_SECONDS: OnceLock<Histogram<f64>> = OnceLock::new();

static AUTH_TOKEN_REFRESH_TOTAL: OnceLock<Counter<u64>> = OnceLock::new();
static OTEL_EXPORT_FAILURES_TOTAL: OnceLock<Counter<u64>> = OnceLock::new();

/// Registered but not wired — `OTel` SDK 0.29 has no dropped-span callback.
static OTEL_DROPPED_SPANS_TOTAL: OnceLock<Counter<u64>> = OnceLock::new();

/// Build a [`Resource`] describing this service instance for metrics.
///
/// Uses the same attributes as the tracer resource so all telemetry signals
/// correlate under the same service identity.
fn build_resource(config: &ObservabilityConfig) -> Resource {
    let deployment_env =
        std::env::var("DEPLOYMENT_ENVIRONMENT").unwrap_or_else(|_| "local".to_owned());

    Resource::builder()
        .with_service_name(config.service_name.clone())
        .with_attributes([
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
            KeyValue::new("deployment.environment", deployment_env),
        ])
        .build()
}

/// Initialise the global `OTel` [`SdkMeterProvider`].
///
/// Returns `None` when `config.enabled` is `false` — the global meter provider
/// is left as the no-op default installed by the `opentelemetry` crate.
///
/// When enabled, a periodic OTLP/gRPC exporter is created (default flush
/// interval: 60 s, overridable via the `OTEL_METRIC_EXPORT_INTERVAL`
/// environment variable in milliseconds) and the provider is registered as the
/// global meter provider.
///
/// The caller should retain the returned `SdkMeterProvider` and call
/// [`SdkMeterProvider::shutdown`] during graceful shutdown.  Dropping the
/// provider without calling `shutdown` triggers shutdown in the `Drop` impl,
/// but explicit shutdown during the orderly teardown phase is preferred.
pub fn init(
    config: &ObservabilityConfig,
    metric_tx: tokio::sync::mpsc::Sender<super::ingestion::CapturedMetricSample>,
) -> Option<SdkMeterProvider> {
    // Always add the native metric exporter so samples are captured to DB
    // even when OTLP export is disabled.
    let native_exporter = super::ingestion::NativeMetricExporter::new(metric_tx);

    if !config.enabled {
        // No OTLP export — only the native exporter
        let provider = SdkMeterProvider::builder()
            .with_periodic_exporter(native_exporter)
            .build();
        global::set_meter_provider(provider.clone());
        return Some(provider);
    }

    let resource = build_resource(config);

    let exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_tonic()
        .with_endpoint(&config.otlp_endpoint)
        .build()
        .expect("Failed to build OTLP metric exporter");

    // Wrap the exporter to count export failures.
    let exporter = FailureCountingMetricExporter::new(exporter);

    // `with_periodic_exporter` defaults to a 60-second interval.
    // Override by setting `OTEL_METRIC_EXPORT_INTERVAL` (milliseconds).
    let provider = SdkMeterProvider::builder()
        .with_resource(resource)
        .with_periodic_exporter(exporter)
        .with_periodic_exporter(native_exporter)
        .build();

    global::set_meter_provider(provider.clone());

    Some(provider)
}

/// Return the global [`opentelemetry::metrics::Meter`] scoped to `name`.
///
/// Convenience wrapper so callers don't need to import `opentelemetry::global`.
#[must_use]
pub fn meter(name: &'static str) -> opentelemetry::metrics::Meter {
    global::meter(name)
}

/// Registers all application metrics. Call once at startup after `init()`.
pub fn register_metrics() {
    let meter = meter("vc-server");

    // -- Existing metrics --

    HTTP_REQUESTS_TOTAL.get_or_init(|| {
        meter
            .u64_counter("kaiku_http_requests_total")
            .with_description("Total HTTP requests")
            .build()
    });

    HTTP_REQUEST_DURATION_MS.get_or_init(|| {
        meter
            .f64_histogram("kaiku_http_request_duration_ms")
            .with_description("HTTP request latency in milliseconds")
            .with_unit("ms")
            .build()
    });

    VOICE_JOINS_TOTAL.get_or_init(|| {
        meter
            .u64_counter("kaiku_voice_joins_total")
            .with_description("Total voice join attempts")
            .build()
    });

    WS_CONNECTIONS_TOTAL.get_or_init(|| {
        meter
            .u64_counter("kaiku_ws_connections_total")
            .with_description("Total WebSocket connection lifecycle events")
            .build()
    });

    // -- Renamed metric (was kaiku_auth_attempts_total) --

    AUTH_LOGIN_ATTEMPTS_TOTAL.get_or_init(|| {
        meter
            .u64_counter("kaiku_auth_login_attempts_total")
            .with_description("Total login attempts by outcome")
            .build()
    });

    // -- New metrics --

    HTTP_ERRORS_TOTAL.get_or_init(|| {
        meter
            .u64_counter("kaiku_http_errors_total")
            .with_description("HTTP 4xx/5xx responses")
            .build()
    });

    WS_CONNECTIONS_ACTIVE.get_or_init(|| {
        meter
            .i64_up_down_counter("kaiku_ws_connections_active")
            .with_description("Current open WebSocket connections")
            .build()
    });

    WS_RECONNECTS_TOTAL.get_or_init(|| {
        meter
            .u64_counter("kaiku_ws_reconnects_total")
            .with_description("WebSocket reconnection attempts")
            .build()
    });

    WS_MESSAGES_TOTAL.get_or_init(|| {
        meter
            .u64_counter("kaiku_ws_messages_total")
            .with_description("WebSocket messages dispatched by event type")
            .build()
    });

    VOICE_SESSIONS_ACTIVE.get_or_init(|| {
        meter
            .i64_up_down_counter("kaiku_voice_sessions_active")
            .with_description("Current active voice sessions")
            .build()
    });

    VOICE_SESSION_DURATION_SECONDS.get_or_init(|| {
        meter
            .f64_histogram("kaiku_voice_session_duration_seconds")
            .with_description("Duration of completed voice sessions")
            .with_unit("s")
            .build()
    });

    VOICE_RTP_COUNTER.get_or_init(|| {
        meter
            .u64_counter("kaiku_voice_rtp_packets_forwarded_total")
            .with_description("RTP packets forwarded by the SFU")
            .build()
    });

    DB_QUERY_DURATION_SECONDS.get_or_init(|| {
        meter
            .f64_histogram("kaiku_db_query_duration_seconds")
            .with_description("SQLx query execution time")
            .with_unit("s")
            .build()
    });

    AUTH_TOKEN_REFRESH_TOTAL.get_or_init(|| {
        meter
            .u64_counter("kaiku_auth_token_refresh_total")
            .with_description("Token refresh operations by outcome")
            .build()
    });

    OTEL_EXPORT_FAILURES_TOTAL.get_or_init(|| {
        meter
            .u64_counter("kaiku_otel_export_failures_total")
            .with_description("OTLP export failures from the SDK")
            .build()
    });

    OTEL_DROPPED_SPANS_TOTAL.get_or_init(|| {
        meter
            .u64_counter("kaiku_otel_dropped_spans_total")
            .with_description("Spans dropped due to queue overflow")
            .build()
    });
}

/// Register database pool metrics as observable gauges with callbacks.
///
/// Call once at startup after pool creation and `register_metrics()`.
pub fn register_db_pool_metrics(pool: PgPool) {
    let meter = meter("vc-server");

    let pool_active = pool.clone();
    meter
        .u64_observable_gauge("kaiku_db_pool_connections_active")
        .with_description("Active database pool connections")
        .with_callback(move |observer| {
            observer.observe(u64::from(pool_active.size()), &[]);
        })
        .build();

    meter
        .u64_observable_gauge("kaiku_db_pool_connections_idle")
        .with_description("Idle database pool connections")
        .with_callback(move |observer| {
            observer.observe(pool.num_idle() as u64, &[]);
        })
        .build();
}

/// Register process memory gauge (Linux only, reads `/proc/self/status`).
///
/// Call once at startup after `register_metrics()`.
pub fn register_process_memory_metric() {
    let meter = meter("vc-server");

    meter
        .u64_observable_gauge("kaiku_process_memory_bytes")
        .with_description("Process resident set size (RSS) from /proc/self/status")
        .with_unit("bytes")
        .with_callback(|observer| {
            if let Ok(rss) = read_rss_bytes() {
                observer.observe(rss, &[]);
            }
        })
        .build();
}

// ============================================================================
// Recording functions
// ============================================================================

/// Record a voice join attempt with the correct `outcome` label.
pub fn record_voice_join(success: bool) {
    let outcome = if success { "success" } else { "failure" };
    if let Some(counter) = VOICE_JOINS_TOTAL.get() {
        counter.add(1, &[KeyValue::new("outcome", outcome)]);
    }
}

/// Record a login attempt with `outcome` label.
pub fn record_auth_login_attempt(success: bool) {
    let outcome = if success { "success" } else { "failure" };
    if let Some(counter) = AUTH_LOGIN_ATTEMPTS_TOTAL.get() {
        counter.add(1, &[KeyValue::new("outcome", outcome)]);
    }
}

/// Record an HTTP error response (status >= 400).
pub fn record_http_error(status: u16) {
    if let Some(counter) = HTTP_ERRORS_TOTAL.get() {
        counter.add(
            1,
            &[KeyValue::new(
                "http.response.status_code",
                i64::from(status),
            )],
        );
    }
}

/// Record a new WebSocket connection.
pub fn record_ws_connect() {
    if let Some(counter) = WS_CONNECTIONS_ACTIVE.get() {
        counter.add(1, &[]);
    }
}

/// Record a WebSocket disconnection.
pub fn record_ws_disconnect() {
    if let Some(counter) = WS_CONNECTIONS_ACTIVE.get() {
        counter.add(-1, &[]);
    }
}

/// Record a dispatched WebSocket message by event type.
pub fn record_ws_message(event_type: &'static str) {
    if let Some(counter) = WS_MESSAGES_TOTAL.get() {
        counter.add(1, &[KeyValue::new("ws.event_type", event_type)]);
    }
}

/// Record a voice session start.
pub fn record_voice_session_start() {
    if let Some(counter) = VOICE_SESSIONS_ACTIVE.get() {
        counter.add(1, &[]);
    }
}

/// Record a voice session end with duration.
pub fn record_voice_session_end(duration_s: f64) {
    if let Some(counter) = VOICE_SESSIONS_ACTIVE.get() {
        counter.add(-1, &[]);
    }
    if let Some(histogram) = VOICE_SESSION_DURATION_SECONDS.get() {
        histogram.record(duration_s, &[]);
    }
}

/// Increment the RTP packet counter atomically (hot path, no `OTel` overhead).
pub fn record_rtp_packet_forwarded() {
    VOICE_RTP_PACKETS_FORWARDED.fetch_add(1, Ordering::Relaxed);
}

/// Flush the atomic RTP counter into the `OTel` counter. Called periodically.
pub fn flush_rtp_counter() {
    let count = VOICE_RTP_PACKETS_FORWARDED.swap(0, Ordering::Relaxed);
    if count > 0 {
        if let Some(counter) = VOICE_RTP_COUNTER.get() {
            counter.add(count, &[]);
        }
    }
}

/// Record a token refresh attempt with `outcome` label.
pub fn record_token_refresh(success: bool) {
    let outcome = if success { "success" } else { "failure" };
    if let Some(counter) = AUTH_TOKEN_REFRESH_TOTAL.get() {
        counter.add(1, &[KeyValue::new("outcome", outcome)]);
    }
}

/// Record an `OTel` export failure.
pub fn record_otel_export_failure() {
    if let Some(counter) = OTEL_EXPORT_FAILURES_TOTAL.get() {
        counter.add(1, &[]);
    }
}

/// Record a database query duration in seconds.
pub fn record_db_query_duration(duration_s: f64) {
    if let Some(histogram) = DB_QUERY_DURATION_SECONDS.get() {
        histogram.record(duration_s, &[]);
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Read the process RSS in bytes from `/proc/self/status` (Linux only).
///
/// Parses the `VmRSS:` line which reports resident set size in kB.
pub fn read_rss_bytes() -> std::io::Result<u64> {
    let status = std::fs::read_to_string("/proc/self/status")?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb: u64 = rest
                .trim()
                .trim_end_matches(" kB")
                .trim()
                .parse()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            return Ok(kb * 1024);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "VmRSS not found in /proc/self/status",
    ))
}

// ============================================================================
// OTel metric exporter wrapper for failure counting
// ============================================================================

use opentelemetry_sdk::metrics::data::ResourceMetrics;
use opentelemetry_sdk::metrics::exporter::PushMetricExporter;

/// Wraps an OTLP `MetricExporter` to count export failures.
#[derive(Debug)]
pub struct FailureCountingMetricExporter<E> {
    inner: E,
}

impl<E> FailureCountingMetricExporter<E> {
    pub const fn new(inner: E) -> Self {
        Self { inner }
    }
}

impl<E: PushMetricExporter> PushMetricExporter for FailureCountingMetricExporter<E> {
    async fn export(
        &self,
        metrics: &ResourceMetrics,
    ) -> opentelemetry_sdk::error::OTelSdkResult {
        let result = self.inner.export(metrics).await;
        if result.is_err() {
            record_otel_export_failure();
        }
        result
    }

    fn force_flush(&self) -> opentelemetry_sdk::error::OTelSdkResult {
        self.inner.force_flush()
    }

    fn shutdown_with_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> opentelemetry_sdk::error::OTelSdkResult {
        self.inner.shutdown_with_timeout(timeout)
    }

    fn temporality(&self) -> opentelemetry_sdk::metrics::Temporality {
        self.inner.temporality()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_metrics_does_not_panic() {
        register_metrics();
        // Calling a second time should also not panic (OnceLock)
        register_metrics();
    }

    #[test]
    fn record_functions_without_init_do_not_panic() {
        // Before register_metrics is called, OnceLocks are empty — these should be no-ops
        record_voice_join(true);
        record_voice_join(false);
        record_auth_login_attempt(true);
        record_auth_login_attempt(false);
        record_http_error(500);
        record_ws_connect();
        record_ws_disconnect();
        record_ws_message("ping");
        record_voice_session_start();
        record_voice_session_end(10.5);
        record_rtp_packet_forwarded();
        flush_rtp_counter();
        record_token_refresh(true);
        record_otel_export_failure();
        record_db_query_duration(0.5);
    }

    #[test]
    fn rtp_atomic_flush_works() {
        // Reset counter
        VOICE_RTP_PACKETS_FORWARDED.store(0, Ordering::Relaxed);

        record_rtp_packet_forwarded();
        record_rtp_packet_forwarded();
        record_rtp_packet_forwarded();

        assert_eq!(VOICE_RTP_PACKETS_FORWARDED.load(Ordering::Relaxed), 3);

        // Flush resets to 0
        flush_rtp_counter();
        assert_eq!(VOICE_RTP_PACKETS_FORWARDED.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn read_rss_bytes_returns_nonzero_on_linux() {
        if cfg!(target_os = "linux") {
            let rss = read_rss_bytes().expect("/proc/self/status should be readable");
            assert!(rss > 0, "RSS should be non-zero for a running process");
        }
    }
}
