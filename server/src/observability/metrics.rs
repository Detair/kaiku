//! OpenTelemetry meter provider initialization.
//!
//! Configures a periodic OTLP metric exporter and installs it as the global
//! meter provider so that any crate can call `opentelemetry::global::meter()`
//! to obtain an instrument without explicit provider wiring.

use std::sync::OnceLock;

use opentelemetry::metrics::{Counter, Histogram};
use opentelemetry::{global, KeyValue};
use opentelemetry_otlp::WithExportConfig as _;
use opentelemetry_sdk::{metrics::SdkMeterProvider, Resource};

use crate::config::ObservabilityConfig;

static HTTP_REQUESTS_TOTAL: OnceLock<Counter<u64>> = OnceLock::new();
static HTTP_REQUEST_DURATION_MS: OnceLock<Histogram<f64>> = OnceLock::new();
static VOICE_JOINS_TOTAL: OnceLock<Counter<u64>> = OnceLock::new();
static WS_CONNECTIONS_TOTAL: OnceLock<Counter<u64>> = OnceLock::new();
static AUTH_ATTEMPTS_TOTAL: OnceLock<Counter<u64>> = OnceLock::new();

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
pub fn init(config: &ObservabilityConfig) -> Option<SdkMeterProvider> {
    if !config.enabled {
        return None;
    }

    let resource = build_resource(config);

    let exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_tonic()
        .with_endpoint(&config.otlp_endpoint)
        .build()
        .expect("Failed to build OTLP metric exporter");

    // `with_periodic_exporter` defaults to a 60-second interval.
    // Override by setting `OTEL_METRIC_EXPORT_INTERVAL` (milliseconds).
    let provider = SdkMeterProvider::builder()
        .with_resource(resource)
        .with_periodic_exporter(exporter)
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

    AUTH_ATTEMPTS_TOTAL.get_or_init(|| {
        meter
            .u64_counter("kaiku_auth_attempts_total")
            .with_description("Total authentication attempts")
            .build()
    });
}

pub fn record_voice_join(success: bool) {
    let result = if success { "success" } else { "error" };
    if let Some(counter) = VOICE_JOINS_TOTAL.get() {
        counter.add(1, &[KeyValue::new("result", result)]);
    }
}
