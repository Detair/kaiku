//! OpenTelemetry meter provider initialization.
//!
//! Configures a periodic OTLP metric exporter and installs it as the global
//! meter provider so that any crate can call `opentelemetry::global::meter()`
//! to obtain an instrument without explicit provider wiring.

use opentelemetry::{global, KeyValue};
use opentelemetry_otlp::WithExportConfig as _;
use opentelemetry_sdk::{metrics::SdkMeterProvider, Resource};

use crate::config::ObservabilityConfig;

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
