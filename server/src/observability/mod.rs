//! Observability module — OpenTelemetry tracing, metrics, and logging.
//!
//! # Quick start
//!
//! ```rust,no_run
//! # use vc_server::{config::ObservabilityConfig, observability};
//! # let config = ObservabilityConfig {
//! #     enabled: false,
//! #     otlp_endpoint: String::new(),
//! #     service_name: String::new(),
//! #     trace_sample_ratio: 0.1,
//! #     log_level: String::new(),
//! # };
//! // In main(), before any logging:
//! let (_otel_guard, _meter_provider) = observability::init(&config);
//! // `_otel_guard` must stay alive until the end of `main`.
//! ```

pub mod metrics;
pub mod retention;
pub mod storage;
pub mod tracing;

use opentelemetry_sdk::metrics::SdkMeterProvider;
pub use tracing::OtelGuard;

use crate::config::ObservabilityConfig;

/// Initialise all observability subsystems.
///
/// This is a convenience wrapper that calls [`tracing::init`] and
/// [`metrics::init`] in the correct order (tracing first, so that log output
/// from metric initialisation is captured).
///
/// # Returns
/// * `OtelGuard` — drop-on-exit guard; keep it alive for the lifetime of
///   `main` so providers flush and shut down gracefully.
/// * `Option<SdkMeterProvider>` — `None` when `config.enabled == false`.
pub fn init(config: &ObservabilityConfig) -> (OtelGuard, Option<SdkMeterProvider>) {
    let guard = tracing::init(config);
    let meter_provider = metrics::init(config);
    if meter_provider.is_some() {
        metrics::register_metrics();
        metrics::register_process_memory_metric();
    }
    (guard, meter_provider)
}
