//! OpenTelemetry tracer provider and tracing-subscriber initialization.
//!
//! Sets up a layered `tracing_subscriber` registry that bridges spans and logs
//! to an OTLP collector, with a JSON stdout fallback layer for all log levels.

use std::collections::HashSet;

use opentelemetry::KeyValue;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::WithExportConfig as _;
use opentelemetry_sdk::{
    logs::SdkLoggerProvider,
    trace::{BatchSpanProcessor, Sampler, SdkTracerProvider},
    Resource,
};
use tracing_subscriber::{layer::Context, registry::LookupSpan, Layer};
use tracing_subscriber::{
    layer::SubscriberExt as _, util::SubscriberInitExt as _, EnvFilter, Registry,
};

use crate::config::ObservabilityConfig;

/// RAII guard that shuts down the `OTel` providers when dropped.
///
/// Bind the returned guard to a variable that lives until end of `main`; if it
/// is dropped early the tracer and logger providers will flush and shut down
/// before the HTTP server finishes serving requests.
pub struct OtelGuard {
    inner: Option<OtelGuardInner>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RedactionLayer;

#[derive(Default)]
struct ForbiddenFieldVisitor {
    forbidden_keys: HashSet<String>,
}

impl tracing::field::Visit for ForbiddenFieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, _value: &dyn std::fmt::Debug) {
        if is_forbidden_attribute_key(field.name()) {
            self.forbidden_keys.insert(field.name().to_owned());
        }
    }
}

impl<S> Layer<S> for RedactionLayer
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: Context<'_, S>,
    ) {
        let mut visitor = ForbiddenFieldVisitor::default();
        attrs.record(&mut visitor);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(visitor.forbidden_keys);
        }
    }

    fn on_record(
        &self,
        id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        ctx: Context<'_, S>,
    ) {
        let mut visitor = ForbiddenFieldVisitor::default();
        values.record(&mut visitor);
        if let Some(span) = ctx.span(id) {
            let mut extensions = span.extensions_mut();
            if let Some(existing) = extensions.get_mut::<HashSet<String>>() {
                existing.extend(visitor.forbidden_keys);
            } else {
                extensions.insert(visitor.forbidden_keys);
            }
        }
    }
}

fn is_forbidden_attribute_key(key: &str) -> bool {
    const FORBIDDEN_PATTERNS: [&str; 10] = [
        "password",
        "token",
        "key",
        "secret",
        "credential",
        "authorization",
        "content",
        "body",
        "email",
        "ip",
    ];

    let lowered = key.to_ascii_lowercase();
    FORBIDDEN_PATTERNS
        .iter()
        .any(|pattern| lowered.contains(pattern))
}

struct OtelGuardInner {
    tracer_provider: SdkTracerProvider,
    logger_provider: SdkLoggerProvider,
}

impl Drop for OtelGuard {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            if let Err(e) = inner.tracer_provider.shutdown() {
                tracing::warn!(error = %e, "OTel tracer provider shutdown error");
            }
            if let Err(e) = inner.logger_provider.shutdown() {
                tracing::warn!(error = %e, "OTel logger provider shutdown error");
            }
        }
    }
}

/// Build a shared [`Resource`] describing this service instance.
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

/// Initialise the `OTel` tracer/logger providers and the `tracing` subscriber.
///
/// If `config.enabled` is `false` a lightweight JSON subscriber is installed
/// (stdout only, no OTLP export) and a no-op [`OtelGuard`] is returned.
///
/// The returned [`OtelGuard`] **must** remain bound to a variable for the
/// lifetime of the application; dropping it triggers graceful shutdown of both
/// providers.
pub fn init(config: &ObservabilityConfig) -> OtelGuard {
    if !config.enabled {
        // Observability disabled — install a minimal JSON subscriber and return
        // a no-op guard so the rest of the startup code is identical.
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(config.log_level.clone()));

        Registry::default()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().json())
            .init();

        return OtelGuard { inner: None };
    }

    let resource = build_resource(config);

    // ── Tracer provider ──────────────────────────────────────────────────────

    let sampler = Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(
        config.trace_sample_ratio,
    )));

    let span_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&config.otlp_endpoint)
        .build()
        .expect("Failed to build OTLP span exporter");

    let batch_processor = BatchSpanProcessor::builder(span_exporter).build();

    let tracer_provider = SdkTracerProvider::builder()
        .with_resource(resource.clone())
        .with_sampler(sampler)
        .with_span_processor(batch_processor)
        .build();

    // ── Logger provider (log bridge) ─────────────────────────────────────────

    let log_exporter = opentelemetry_otlp::LogExporter::builder()
        .with_tonic()
        .with_endpoint(&config.otlp_endpoint)
        .build()
        .expect("Failed to build OTLP log exporter");

    let logger_provider = SdkLoggerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(log_exporter)
        .build();

    // ── tracing-subscriber registry ──────────────────────────────────────────

    // Suppress noisy internal crates that produce many spans/logs.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!("{},hyper=off,tonic=off,h2=off", config.log_level))
    });

    let otel_trace_layer = tracing_opentelemetry::layer().with_tracer(
        opentelemetry::trace::TracerProvider::tracer(&tracer_provider, "vc-server"),
    );
    let otel_log_layer = OpenTelemetryTracingBridge::new(&logger_provider);

    Registry::default()
        .with(filter)
        .with(RedactionLayer)
        .with(otel_trace_layer)
        .with(otel_log_layer)
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    OtelGuard {
        inner: Some(OtelGuardInner {
            tracer_provider,
            logger_provider,
        }),
    }
}

#[cfg(test)]
mod tests {
    fn assert_contains(haystack: &str, needle: &str) {
        assert!(
            haystack.contains(needle),
            "missing expected instrumentation contract fragment: {needle}"
        );
    }

    #[test]
    fn redaction_key_match_works() {
        assert!(super::is_forbidden_attribute_key("http.request.body"));
        assert!(super::is_forbidden_attribute_key("user_email"));
        assert!(super::is_forbidden_attribute_key("authorization"));
        assert!(!super::is_forbidden_attribute_key("channel_id"));
    }

    #[test]
    fn instrument_skip_list_contract_is_present() {
        let auth = include_str!("../auth/handlers.rs");
        assert_contains(auth, "#[tracing::instrument(skip(state, body)");

        let chat = include_str!("../chat/messages.rs");
        assert_contains(chat, "#[tracing::instrument(skip(state, body)");

        let ws = include_str!("../ws/mod.rs");
        assert_contains(
            ws,
            "#[tracing::instrument(skip(state, tx, subscribed_channels, admin_subscribed, activity_state, text),",
        );

        let voice = include_str!("../voice/call_handlers.rs");
        assert_contains(
            voice,
            "#[tracing::instrument(skip(state), fields(user_id = %auth.id",
        );
    }
}
