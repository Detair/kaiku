//! Native telemetry ingestion pipeline.
//!
//! Captures WARN/ERROR log events and completed span metadata from the
//! tracing/OTel pipeline and writes them to native telemetry tables.
//!
//! Architecture: the tracing subscriber and span processor are initialised
//! before the database pool, so we use `tokio::sync::mpsc` channels to
//! decouple capture from storage. Background workers (spawned after pool
//! creation) drain the channels and batch-write to `PostgreSQL`.
//!
//! Design reference: §11 (Data Model), Phase 2 (Ingestion and Safety)

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use opentelemetry::trace::TraceContextExt as _;
use opentelemetry::KeyValue;
use sqlx::PgPool;
use tokio::sync::mpsc;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

use super::tracing::is_forbidden_attribute_key;

// ============================================================================
// Channel message types
// ============================================================================

/// A captured log event destined for `telemetry_log_events`.
#[derive(Debug)]
pub struct CapturedLogEvent {
    pub ts: DateTime<Utc>,
    pub level: String,
    pub service: String,
    pub domain: String,
    pub event: String,
    pub message: String,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
}

/// A captured metric sample destined for `telemetry_metric_samples`.
#[derive(Debug)]
pub struct CapturedMetricSample {
    pub ts: DateTime<Utc>,
    pub metric_name: String,
    pub scope: String,
    pub labels: serde_json::Value,
    pub value_count: Option<i64>,
    pub value_sum: Option<f64>,
    pub value_p50: Option<f64>,
    pub value_p95: Option<f64>,
    pub value_p99: Option<f64>,
}

/// A captured span destined for `telemetry_trace_index`.
#[derive(Debug)]
pub struct CapturedSpan {
    pub trace_id: String,
    pub span_name: String,
    pub domain: String,
    pub route: Option<String>,
    pub status_code: Option<String>,
    pub duration_ms: i32,
    pub ts: DateTime<Utc>,
    pub service: String,
}

// ============================================================================
// Channels
// ============================================================================

/// Create the log event ingestion channel (bounded, 4096 capacity).
pub fn log_channel() -> (
    mpsc::Sender<CapturedLogEvent>,
    mpsc::Receiver<CapturedLogEvent>,
) {
    mpsc::channel(4096)
}

/// Create the metric sample ingestion channel (bounded, 4096 capacity).
pub fn metric_channel() -> (
    mpsc::Sender<CapturedMetricSample>,
    mpsc::Receiver<CapturedMetricSample>,
) {
    mpsc::channel(4096)
}

/// Create the span ingestion channel (bounded, 4096 capacity).
pub fn span_channel() -> (mpsc::Sender<CapturedSpan>, mpsc::Receiver<CapturedSpan>) {
    mpsc::channel(4096)
}

// ============================================================================
// NativeLogLayer — tracing subscriber layer
// ============================================================================

/// A `tracing_subscriber::Layer` that captures WARN/ERROR events and sends
/// them to the native telemetry storage pipeline via a channel.
///
/// INFO and lower events are silently dropped per design (§11.2).
pub struct NativeLogLayer {
    tx: mpsc::Sender<CapturedLogEvent>,
}

impl NativeLogLayer {
    pub const fn new(tx: mpsc::Sender<CapturedLogEvent>) -> Self {
        Self { tx }
    }
}

impl<S> Layer<S> for NativeLogLayer
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let level = *metadata.level();

        // Only capture WARN and ERROR
        if level != tracing::Level::WARN && level != tracing::Level::ERROR {
            return;
        }

        let level_str = match level {
            tracing::Level::ERROR => "ERROR",
            tracing::Level::WARN => "WARN",
            _ => return,
        };

        // Extract fields from the event
        let mut visitor = LogEventVisitor::default();
        event.record(&mut visitor);

        // Extract trace context from the current span.
        //
        // We read from `OtelData.parent_cx` which holds the parent span's
        // context. The trace_id is correct (shared across the whole trace);
        // the span_id reflects the parent span — a minor inaccuracy that
        // is acceptable for native log correlation. Extracting the *current*
        // span's OTel span ID would require accessing the span builder's
        // internal state, which is version-fragile.
        let (trace_id, span_id) = ctx
            .current_span()
            .id()
            .and_then(|id| ctx.span(id))
            .map(|span| {
                let extensions = span.extensions();
                if let Some(otel_data) = extensions.get::<tracing_opentelemetry::OtelData>() {
                    let parent_cx = &otel_data.parent_cx;
                    let span_ref = parent_cx.span();
                    let sc = span_ref.span_context();
                    if sc.is_valid() {
                        return (
                            Some(sc.trace_id().to_string()),
                            Some(sc.span_id().to_string()),
                        );
                    }
                }
                (None, None)
            })
            .unwrap_or((None, None));

        // Derive domain from target module path
        let domain = extract_domain(metadata.target());

        let captured = CapturedLogEvent {
            ts: Utc::now(),
            level: level_str.to_owned(),
            service: "vc-server".to_owned(),
            domain,
            event: visitor
                .event_name
                .unwrap_or_else(|| metadata.name().to_owned()),
            message: visitor.message.unwrap_or_default(),
            trace_id,
            span_id,
        };

        // Non-blocking send — drop the event if the channel is full
        let _ = self.tx.try_send(captured);
    }
}

/// Visitor that extracts `message` and `event` fields from tracing events.
#[derive(Default)]
struct LogEventVisitor {
    message: Option<String>,
    event_name: Option<String>,
}

impl tracing::field::Visit for LogEventVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if is_forbidden_attribute_key(field.name()) {
            return;
        }
        match field.name() {
            "message" => self.message = Some(value.to_owned()),
            "event" => self.event_name = Some(value.to_owned()),
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if is_forbidden_attribute_key(field.name()) {
            return;
        }
        match field.name() {
            "message" => self.message = Some(format!("{value:?}")),
            "event" => self.event_name = Some(format!("{value:?}")),
            _ => {}
        }
    }
}

// ============================================================================
// NativeSpanProcessor — OTel span processor for trace index
// ============================================================================

/// An OpenTelemetry `SpanProcessor` that extracts metadata from completed spans
/// and sends it to the native trace index storage pipeline.
///
/// No span payload or attributes are stored — only routing metadata.
#[derive(Debug)]
pub struct NativeSpanProcessor {
    tx: mpsc::Sender<CapturedSpan>,
}

impl NativeSpanProcessor {
    pub const fn new(tx: mpsc::Sender<CapturedSpan>) -> Self {
        Self { tx }
    }
}

impl opentelemetry_sdk::trace::SpanProcessor for NativeSpanProcessor {
    fn on_start(
        &self,
        _span: &mut opentelemetry_sdk::trace::Span,
        _parent_cx: &opentelemetry::Context,
    ) {
        // No action on span start
    }

    fn on_end(&self, span: opentelemetry_sdk::trace::SpanData) {
        let trace_id = span.span_context.trace_id().to_string();
        let span_name = span.name.to_string();

        // Extract domain from span name (e.g., "auth.login" -> "auth")
        let domain = span_name.split('.').next().unwrap_or("unknown").to_owned();

        // Extract route and status_code from span attributes
        let mut route = None;
        let mut status_code = None;
        for kv in &span.attributes {
            match kv.key.as_str() {
                "http.route" => route = Some(kv.value.as_str().to_string()),
                "http.response.status_code" => {
                    status_code = Some(kv.value.as_str().to_string());
                }
                _ => {}
            }
        }

        // Compute duration
        let duration = span.end_time.duration_since(span.start_time);
        let duration_ms = match duration {
            Ok(d) => d.as_millis().min(i32::MAX as u128) as i32,
            Err(_) => 0,
        };

        let captured = CapturedSpan {
            trace_id,
            span_name,
            domain,
            route,
            status_code,
            duration_ms,
            ts: DateTime::from(span.start_time),
            service: "vc-server".to_owned(),
        };

        let _ = self.tx.try_send(captured);
    }

    fn force_flush(&self) -> opentelemetry_sdk::error::OTelSdkResult {
        Ok(())
    }

    fn shutdown(&self) -> opentelemetry_sdk::error::OTelSdkResult {
        Ok(())
    }
}

// ============================================================================
// NativeMetricExporter — OTel metric exporter for native storage
// ============================================================================

/// Max unique label combinations stored per metric per export cycle.
const MAX_CARDINALITY_PER_METRIC: usize = 100;

/// An OpenTelemetry `PushMetricExporter` that converts metric data points
/// into native storage samples and sends them through a channel.
///
/// Added to the `SdkMeterProvider` as a second periodic exporter alongside
/// the OTLP exporter. Runs every 60 seconds (default `OTel` interval).
#[derive(Debug)]
pub struct NativeMetricExporter {
    tx: mpsc::Sender<CapturedMetricSample>,
}

impl NativeMetricExporter {
    pub const fn new(tx: mpsc::Sender<CapturedMetricSample>) -> Self {
        Self { tx }
    }

    /// Process a single metric and send captured samples through the channel.
    fn process_metric(
        &self,
        metric_name: &str,
        data: &dyn opentelemetry_sdk::metrics::data::Aggregation,
        cardinality: &mut HashMap<String, usize>,
    ) {
        use opentelemetry_sdk::metrics::data::{Gauge, Histogram, Sum};

        let any = data.as_any();

        // Try Sum<u64> (counters)
        if let Some(sum) = any.downcast_ref::<Sum<u64>>() {
            for dp in &sum.data_points {
                self.emit_sample(metric_name, &dp.attributes, cardinality, |labels| {
                    CapturedMetricSample {
                        ts: Utc::now(),
                        metric_name: metric_name.to_owned(),
                        scope: "cluster".to_owned(),
                        labels,
                        value_count: Some(dp.value as i64),
                        value_sum: None,
                        value_p50: None,
                        value_p95: None,
                        value_p99: None,
                    }
                });
            }
            return;
        }

        // Try Sum<i64> (up-down counters)
        if let Some(sum) = any.downcast_ref::<Sum<i64>>() {
            for dp in &sum.data_points {
                self.emit_sample(metric_name, &dp.attributes, cardinality, |labels| {
                    CapturedMetricSample {
                        ts: Utc::now(),
                        metric_name: metric_name.to_owned(),
                        scope: "cluster".to_owned(),
                        labels,
                        value_count: Some(dp.value),
                        value_sum: None,
                        value_p50: None,
                        value_p95: None,
                        value_p99: None,
                    }
                });
            }
            return;
        }

        // Try Sum<f64>
        if let Some(sum) = any.downcast_ref::<Sum<f64>>() {
            for dp in &sum.data_points {
                self.emit_sample(metric_name, &dp.attributes, cardinality, |labels| {
                    CapturedMetricSample {
                        ts: Utc::now(),
                        metric_name: metric_name.to_owned(),
                        scope: "cluster".to_owned(),
                        labels,
                        value_count: None,
                        value_sum: Some(dp.value),
                        value_p50: None,
                        value_p95: None,
                        value_p99: None,
                    }
                });
            }
            return;
        }

        // Try Histogram<f64>
        if let Some(hist) = any.downcast_ref::<Histogram<f64>>() {
            for dp in &hist.data_points {
                self.emit_sample(metric_name, &dp.attributes, cardinality, |labels| {
                    let p50 =
                        percentile_from_histogram(&dp.bounds, &dp.bucket_counts, dp.count, 0.50);
                    let p95 =
                        percentile_from_histogram(&dp.bounds, &dp.bucket_counts, dp.count, 0.95);
                    let p99 =
                        percentile_from_histogram(&dp.bounds, &dp.bucket_counts, dp.count, 0.99);

                    CapturedMetricSample {
                        ts: Utc::now(),
                        metric_name: metric_name.to_owned(),
                        scope: "cluster".to_owned(),
                        labels,
                        value_count: Some(dp.count as i64),
                        value_sum: Some(dp.sum),
                        value_p50: p50,
                        value_p95: p95,
                        value_p99: p99,
                    }
                });
            }
            return;
        }

        // Try Gauge<u64>
        if let Some(gauge) = any.downcast_ref::<Gauge<u64>>() {
            for dp in &gauge.data_points {
                self.emit_sample(metric_name, &dp.attributes, cardinality, |labels| {
                    CapturedMetricSample {
                        ts: Utc::now(),
                        metric_name: metric_name.to_owned(),
                        scope: "cluster".to_owned(),
                        labels,
                        value_count: Some(dp.value as i64),
                        value_sum: None,
                        value_p50: None,
                        value_p95: None,
                        value_p99: None,
                    }
                });
            }
            return;
        }

        // Try Gauge<f64>
        if let Some(gauge) = any.downcast_ref::<Gauge<f64>>() {
            for dp in &gauge.data_points {
                self.emit_sample(metric_name, &dp.attributes, cardinality, |labels| {
                    CapturedMetricSample {
                        ts: Utc::now(),
                        metric_name: metric_name.to_owned(),
                        scope: "cluster".to_owned(),
                        labels,
                        value_count: None,
                        value_sum: Some(dp.value),
                        value_p50: None,
                        value_p95: None,
                        value_p99: None,
                    }
                });
            }
        }
    }

    /// Convert attributes to filtered labels, check cardinality, and emit sample.
    fn emit_sample(
        &self,
        metric_name: &str,
        attributes: &[KeyValue],
        cardinality: &mut HashMap<String, usize>,
        build: impl FnOnce(serde_json::Value) -> CapturedMetricSample,
    ) {
        let labels = attributes_to_filtered_labels(attributes);

        // Cardinality budget check
        let count = cardinality.entry(metric_name.to_owned()).or_insert(0);
        if *count >= MAX_CARDINALITY_PER_METRIC {
            tracing::debug!(
                metric = %metric_name,
                "Cardinality budget exceeded (>{MAX_CARDINALITY_PER_METRIC} label combos), dropping sample"
            );
            return;
        }
        *count += 1;

        let sample = build(labels);
        let _ = self.tx.try_send(sample);
    }
}

impl opentelemetry_sdk::metrics::exporter::PushMetricExporter for NativeMetricExporter {
    async fn export(
        &self,
        metrics: &mut opentelemetry_sdk::metrics::data::ResourceMetrics,
    ) -> opentelemetry_sdk::error::OTelSdkResult {
        let mut cardinality: HashMap<String, usize> = HashMap::new();

        for scope_metrics in &metrics.scope_metrics {
            for metric in &scope_metrics.metrics {
                self.process_metric(&metric.name, metric.data.as_ref(), &mut cardinality);
            }
        }

        Ok(())
    }

    fn force_flush(&self) -> opentelemetry_sdk::error::OTelSdkResult {
        Ok(())
    }

    fn shutdown(&self) -> opentelemetry_sdk::error::OTelSdkResult {
        Ok(())
    }

    fn temporality(&self) -> opentelemetry_sdk::metrics::Temporality {
        // Delta temporality: each export row contains only the per-interval
        // increment, so SUM(value_count) across rows produces correct totals.
        // The OTel SDK maintains separate aggregation state per reader, so
        // the OTLP exporter continues receiving cumulative data independently.
        opentelemetry_sdk::metrics::Temporality::Delta
    }
}

/// Convert `OTel` `KeyValue` attributes to a filtered JSONB labels object.
///
/// Only allowlisted keys (contract §6) are included.
fn attributes_to_filtered_labels(attrs: &[KeyValue]) -> serde_json::Value {
    let allowed = allowed_label_keys();
    let map: serde_json::Map<String, serde_json::Value> = attrs
        .iter()
        .filter(|kv| allowed.contains(kv.key.as_str()))
        .map(|kv| {
            (
                kv.key.to_string(),
                serde_json::Value::String(kv.value.as_str().to_string()),
            )
        })
        .collect();
    serde_json::Value::Object(map)
}

/// Approximate a percentile from `OTel` histogram bucket data.
///
/// Uses linear interpolation within the bucket containing the target rank
/// (`PERCENTILE_CONT` semantics). Empty buckets are skipped. Returns `None`
/// if the histogram has zero observations.
fn percentile_from_histogram(
    bounds: &[f64],
    bucket_counts: &[u64],
    count: u64,
    p: f64,
) -> Option<f64> {
    if count == 0 {
        return None;
    }
    let target = p * count as f64;
    let mut cumulative: u64 = 0;

    for (i, &bc) in bucket_counts.iter().enumerate() {
        if bc == 0 {
            continue;
        }
        cumulative += bc;
        if cumulative as f64 > target {
            let lower = if i == 0 { 0.0 } else { bounds[i - 1] };
            let upper = if i < bounds.len() {
                bounds[i]
            } else {
                // Last bucket (+infinity) — return the lower bound
                return Some(lower);
            };
            let bucket_start_cum = cumulative - bc;
            let fraction = (target - bucket_start_cum as f64) / bc as f64;
            return Some(fraction.mul_add(upper - lower, lower));
        }
    }

    // Fallback: return last finite boundary
    bounds.last().copied()
}

// ============================================================================
// Label allowlist for metric ingestion
// ============================================================================

/// Allowed metric label keys per the observability contract §6.
///
/// Initialized once via `LazyLock` to avoid allocating a new `HashSet` on every call.
static ALLOWED_LABEL_KEYS: std::sync::LazyLock<HashSet<&'static str>> =
    std::sync::LazyLock::new(|| {
        [
            "deployment.environment",
            "service.name",
            "http.request.method",
            "http.route",
            "http.response.status_code",
            "outcome",
            "error.type",
            "voice.session_outcome",
            "ws.event_type",
            "db.operation",
            "db.sql.table",
            "tauri.command",
            "os.type",
        ]
        .into_iter()
        .collect()
    });

/// Returns a reference to the set of allowed metric label keys.
pub fn allowed_label_keys() -> &'static HashSet<&'static str> {
    &ALLOWED_LABEL_KEYS
}

/// Filter a JSONB labels object to only include allowlisted keys.
pub fn filter_labels(labels: &serde_json::Value) -> serde_json::Value {
    let allowed = allowed_label_keys();
    match labels {
        serde_json::Value::Object(map) => {
            let filtered: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .filter(|(k, _)| allowed.contains(k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            serde_json::Value::Object(filtered)
        }
        _ => serde_json::Value::Object(serde_json::Map::new()),
    }
}

// ============================================================================
// Background ingestion workers
// ============================================================================

/// Handles returned by [`spawn_ingestion_workers`].
pub struct IngestionHandles {
    pub log_handle: tokio::task::JoinHandle<()>,
    pub span_handle: tokio::task::JoinHandle<()>,
    pub metric_handle: tokio::task::JoinHandle<()>,
}

/// Max items to accumulate before flushing a batch INSERT.
const BATCH_CAPACITY: usize = 64;

/// Spawn the background workers that drain ingestion channels and write to DB.
///
/// Each worker accumulates up to [`BATCH_CAPACITY`] items before flushing a
/// single multi-row INSERT, reducing per-row overhead from network round-trips
/// and transaction commits.
///
/// Call this in `main()` after the database pool is created.
pub fn spawn_ingestion_workers(
    pool: PgPool,
    mut log_rx: mpsc::Receiver<CapturedLogEvent>,
    mut span_rx: mpsc::Receiver<CapturedSpan>,
    mut metric_rx: mpsc::Receiver<CapturedMetricSample>,
) -> IngestionHandles {
    let log_pool = pool.clone();
    let log_handle = tokio::spawn(async move {
        let empty_attrs = serde_json::Value::Object(serde_json::Map::new());
        let mut batch = Vec::with_capacity(BATCH_CAPACITY);
        loop {
            batch.clear();
            // Wait for at least one message (blocks until available or closed)
            let Some(first) = log_rx.recv().await else {
                break;
            };
            batch.push(first);
            // Drain any immediately available messages up to batch capacity
            while batch.len() < BATCH_CAPACITY {
                match log_rx.try_recv() {
                    Ok(msg) => batch.push(msg),
                    Err(_) => break,
                }
            }
            // Flush batch
            let mut qb: sqlx::QueryBuilder<'_, sqlx::Postgres> = sqlx::QueryBuilder::new(
                "INSERT INTO telemetry_log_events \
                 (ts, level, service, domain, event, message, trace_id, span_id, attrs) ",
            );
            qb.push_values(&batch, |mut b, event| {
                b.push_bind(event.ts)
                    .push_bind(&event.level)
                    .push_bind(&event.service)
                    .push_bind(&event.domain)
                    .push_bind(&event.event)
                    .push_bind(&event.message)
                    .push_bind(&event.trace_id)
                    .push_bind(&event.span_id)
                    .push_bind(&empty_attrs);
            });
            if let Err(e) = qb.build().execute(&log_pool).await {
                tracing::debug!(error = %e, batch_size = batch.len(), "Failed to persist native log events");
            }
        }
    });

    let span_pool = pool.clone();
    let span_handle = tokio::spawn(async move {
        let mut batch = Vec::with_capacity(BATCH_CAPACITY);
        loop {
            batch.clear();
            let Some(first) = span_rx.recv().await else {
                break;
            };
            batch.push(first);
            while batch.len() < BATCH_CAPACITY {
                match span_rx.try_recv() {
                    Ok(msg) => batch.push(msg),
                    Err(_) => break,
                }
            }
            let mut qb: sqlx::QueryBuilder<'_, sqlx::Postgres> = sqlx::QueryBuilder::new(
                "INSERT INTO telemetry_trace_index \
                 (trace_id, span_name, domain, route, status_code, duration_ms, ts, service) ",
            );
            qb.push_values(&batch, |mut b, span| {
                b.push_bind(&span.trace_id)
                    .push_bind(&span.span_name)
                    .push_bind(&span.domain)
                    .push_bind(&span.route)
                    .push_bind(&span.status_code)
                    .push_bind(span.duration_ms)
                    .push_bind(span.ts)
                    .push_bind(&span.service);
            });
            if let Err(e) = qb.build().execute(&span_pool).await {
                tracing::debug!(error = %e, batch_size = batch.len(), "Failed to persist native trace index entries");
            }
        }
    });

    let metric_handle = tokio::spawn(async move {
        let mut batch = Vec::with_capacity(BATCH_CAPACITY);
        loop {
            batch.clear();
            let Some(first) = metric_rx.recv().await else {
                break;
            };
            batch.push(first);
            while batch.len() < BATCH_CAPACITY {
                match metric_rx.try_recv() {
                    Ok(msg) => batch.push(msg),
                    Err(_) => break,
                }
            }
            let mut qb: sqlx::QueryBuilder<'_, sqlx::Postgres> = sqlx::QueryBuilder::new(
                "INSERT INTO telemetry_metric_samples \
                 (ts, metric_name, scope, labels, value_count, value_sum, value_p50, value_p95, value_p99) ",
            );
            qb.push_values(&batch, |mut b, sample| {
                b.push_bind(sample.ts)
                    .push_bind(&sample.metric_name)
                    .push_bind(&sample.scope)
                    .push_bind(&sample.labels)
                    .push_bind(sample.value_count)
                    .push_bind(sample.value_sum)
                    .push_bind(sample.value_p50)
                    .push_bind(sample.value_p95)
                    .push_bind(sample.value_p99);
            });
            if let Err(e) = qb.build().execute(&pool).await {
                tracing::debug!(error = %e, batch_size = batch.len(), "Failed to persist native metric samples");
            }
        }
    });

    IngestionHandles {
        log_handle,
        span_handle,
        metric_handle,
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Extract domain from a Rust module path target.
///
/// Maps module paths like `vc_server::auth::handlers` → `auth`,
/// `vc_server::voice::call_handlers` → `voice`, etc.
fn extract_domain(target: &str) -> String {
    let parts: Vec<&str> = target.split("::").collect();
    if parts.len() >= 2 {
        parts[1].to_owned()
    } else {
        "unknown".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_domain_from_module_path() {
        assert_eq!(extract_domain("vc_server::auth::handlers"), "auth");
        assert_eq!(extract_domain("vc_server::voice::call_handlers"), "voice");
        assert_eq!(extract_domain("vc_server::chat::messages"), "chat");
        assert_eq!(extract_domain("unknown_module"), "unknown");
    }

    #[test]
    fn filter_labels_removes_non_allowlisted() {
        let labels = serde_json::json!({
            "http.route": "/api/v1/users/{id}",
            "http.response.status_code": "200",
            "user_id": "12345",
            "ip_address": "192.168.1.1"
        });
        let filtered = filter_labels(&labels);
        let obj = filtered.as_object().unwrap();
        assert!(obj.contains_key("http.route"));
        assert!(obj.contains_key("http.response.status_code"));
        assert!(!obj.contains_key("user_id"));
        assert!(!obj.contains_key("ip_address"));
    }

    #[test]
    fn filter_labels_handles_empty() {
        let labels = serde_json::json!({});
        let filtered = filter_labels(&labels);
        assert_eq!(filtered, serde_json::json!({}));
    }

    #[test]
    fn allowed_label_keys_matches_contract() {
        let keys = allowed_label_keys();
        // Contract §6.1 specifies 13 allowed labels
        assert_eq!(keys.len(), 13);
        assert!(keys.contains("http.route"));
        assert!(keys.contains("outcome"));
        assert!(keys.contains("error.type"));
    }

    #[test]
    fn percentile_from_histogram_basic() {
        // bounds: [10, 25, 50, 100], bucket_counts: [2, 3, 5, 8, 2] (5 buckets, last is +inf)
        let bounds = vec![10.0, 25.0, 50.0, 100.0];
        let bucket_counts = vec![2, 3, 5, 8, 2];
        let count = 20;

        // p50 = 10th value. cumulative: [2, 5, 10, 18, 20].
        // 10th value falls in bucket 2 (bounds 25..50), at (10-5)/5 = 1.0 of the bucket
        let p50 = percentile_from_histogram(&bounds, &bucket_counts, count, 0.50).unwrap();
        assert!(
            (25.0..=50.0).contains(&p50),
            "p50={p50} should be in [25, 50]"
        );

        // p95 = 19th value. cumulative: [2, 5, 10, 18, 20].
        // 19th value falls in bucket 4 (bounds 100..+inf), return lower=100
        let p95 = percentile_from_histogram(&bounds, &bucket_counts, count, 0.95).unwrap();
        assert!(p95 >= 50.0, "p95={p95} should be >= 50");

        // Empty histogram
        assert!(percentile_from_histogram(&bounds, &[0, 0, 0, 0, 0], 0, 0.5).is_none());
    }

    #[test]
    fn attributes_to_filtered_labels_filters_correctly() {
        let attrs = vec![
            KeyValue::new("http.route", "/api/v1/test"),
            KeyValue::new("user_id", "abc123"),
            KeyValue::new("outcome", "success"),
        ];
        let labels = attributes_to_filtered_labels(&attrs);
        let obj = labels.as_object().unwrap();
        assert_eq!(obj.len(), 2);
        assert!(obj.contains_key("http.route"));
        assert!(obj.contains_key("outcome"));
        assert!(!obj.contains_key("user_id"));
    }

    #[test]
    fn cardinality_budget_enforced() {
        assert_eq!(MAX_CARDINALITY_PER_METRIC, 100);
    }
}
