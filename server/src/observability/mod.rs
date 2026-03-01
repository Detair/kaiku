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
//! let (_otel_guard, _meter_provider, _ingestion) = observability::init(&config);
//! // `_otel_guard` must stay alive until the end of `main`.
//! ```

pub mod ingestion;
pub mod metrics;
pub mod retention;
pub mod storage;
pub mod tracing;
pub mod voice;

use opentelemetry_sdk::metrics::SdkMeterProvider;
use tokio::sync::mpsc;
pub use tracing::OtelGuard;

use crate::config::ObservabilityConfig;

/// Receivers for native telemetry ingestion channels.
///
/// Pass these to [`ingestion::spawn_ingestion_workers`] after the database pool
/// is created (the tracing subscriber is initialised before the pool).
pub struct IngestionChannels {
    pub log_rx: mpsc::Receiver<ingestion::CapturedLogEvent>,
    pub span_rx: mpsc::Receiver<ingestion::CapturedSpan>,
    pub metric_rx: mpsc::Receiver<ingestion::CapturedMetricSample>,
}

/// Initialise all observability subsystems.
///
/// This is a convenience wrapper that calls [`tracing::init`] and
/// [`metrics::init`] in the correct order (tracing first, so that log output
/// from metric initialisation is captured).
///
/// # Returns
/// * `OtelGuard` — drop-on-exit guard; keep it alive for the lifetime of `main` so providers flush
///   and shut down gracefully.
/// * `Option<SdkMeterProvider>` — always `Some` (native exporter always active).
/// * `IngestionChannels` — receivers for the native telemetry pipeline. Pass to
///   [`ingestion::spawn_ingestion_workers`] after pool creation.
pub fn init(
    config: &ObservabilityConfig,
) -> (OtelGuard, Option<SdkMeterProvider>, IngestionChannels) {
    let (log_tx, log_rx) = ingestion::log_channel();
    let (span_tx, span_rx) = ingestion::span_channel();
    let (metric_tx, metric_rx) = ingestion::metric_channel();

    let guard = tracing::init(config, log_tx, span_tx);
    let meter_provider = metrics::init(config, metric_tx);
    // Meter provider is always Some (native exporter is always active),
    // so metrics are always registered.
    metrics::register_metrics();
    metrics::register_process_memory_metric();
    (
        guard,
        meter_provider,
        IngestionChannels {
            log_rx,
            span_rx,
            metric_rx,
        },
    )
}
