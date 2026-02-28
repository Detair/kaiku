-- Command Center: Native telemetry storage tables
-- Design reference: docs/plans/2026-02-27-phase-7-admin-command-center-design.md §11

-- =============================================================================
-- telemetry_metric_samples
-- Pre-aggregated metric samples aligned with the observability contract.
-- Histogram strategy: store pre-computed p50/p95/p99, not raw buckets.
-- =============================================================================

CREATE TABLE telemetry_metric_samples (
    ts          TIMESTAMPTZ      NOT NULL,
    metric_name TEXT             NOT NULL,
    scope       TEXT             NOT NULL DEFAULT 'cluster',
    labels      JSONB            NOT NULL DEFAULT '{}'::jsonb,
    value_count BIGINT           NULL,
    value_sum   DOUBLE PRECISION NULL,
    value_p50   DOUBLE PRECISION NULL,
    value_p95   DOUBLE PRECISION NULL,
    value_p99   DOUBLE PRECISION NULL
);

-- Try to convert to TimescaleDB hypertable (1-hour chunks).
-- Graceful fallback: if TimescaleDB is not available, the table works as
-- a standard PostgreSQL table with scheduled DELETE for retention.
DO $$
BEGIN
    PERFORM create_hypertable(
        'telemetry_metric_samples', 'ts',
        chunk_time_interval => INTERVAL '1 hour',
        if_not_exists => TRUE
    );
EXCEPTION
    WHEN undefined_function THEN
        RAISE NOTICE 'TimescaleDB not available — telemetry_metric_samples will use plain PostgreSQL with scheduled DELETE retention';
END;
$$;

CREATE INDEX idx_tms_metric_ts ON telemetry_metric_samples (metric_name, ts DESC);
CREATE INDEX idx_tms_scope_ts  ON telemetry_metric_samples (scope, ts DESC);

-- =============================================================================
-- telemetry_log_events
-- Curated log events (WARN/ERROR only). INFO stays in external Loki.
-- =============================================================================

CREATE TABLE telemetry_log_events (
    id       UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    ts       TIMESTAMPTZ NOT NULL,
    level    TEXT        NOT NULL CHECK (level IN ('ERROR', 'WARN')),
    service  TEXT        NOT NULL,
    domain   TEXT        NOT NULL,
    event    TEXT        NOT NULL,
    message  TEXT        NOT NULL,
    trace_id TEXT        NULL,
    span_id  TEXT        NULL,
    attrs    JSONB       NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX idx_tle_ts        ON telemetry_log_events (ts DESC, id DESC);
CREATE INDEX idx_tle_level_ts  ON telemetry_log_events (level, ts DESC);
CREATE INDEX idx_tle_domain_ts ON telemetry_log_events (domain, ts DESC);
CREATE INDEX idx_tle_trace_id  ON telemetry_log_events (trace_id) WHERE trace_id IS NOT NULL;

-- =============================================================================
-- telemetry_trace_index
-- Trace metadata only — no span payloads or attributes.
-- =============================================================================

CREATE TABLE telemetry_trace_index (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    trace_id    TEXT        NOT NULL,
    span_name   TEXT        NOT NULL,
    domain      TEXT        NOT NULL,
    route       TEXT        NULL,
    status_code TEXT        NULL,
    duration_ms INTEGER     NOT NULL,
    ts          TIMESTAMPTZ NOT NULL,
    service     TEXT        NOT NULL
);

CREATE INDEX idx_tti_ts        ON telemetry_trace_index (ts DESC, id DESC);
CREATE INDEX idx_tti_status_ts ON telemetry_trace_index (status_code, ts DESC);
CREATE INDEX idx_tti_domain_ts ON telemetry_trace_index (domain, ts DESC);
CREATE INDEX idx_tti_duration  ON telemetry_trace_index (duration_ms DESC, ts DESC);
CREATE INDEX idx_tti_trace_id  ON telemetry_trace_index (trace_id);
