-- Command Center: Materialized view for 30-day trend rollups
-- Design reference: docs/plans/2026-02-27-phase-7-admin-command-center-design.md §11.4
--
-- Daily rollup of metric samples used for 7d/30d trend queries.
-- Live queries serve ranges <= 24h directly from telemetry_metric_samples.
-- Refreshed hourly by the retention background job.

CREATE MATERIALIZED VIEW telemetry_trend_rollups AS
SELECT
    date_trunc('day', ts) AS day,
    metric_name,
    scope,
    labels->>'http.route' AS route,
    COUNT(*)              AS sample_count,
    AVG(value_p95)        AS avg_p95,
    MAX(value_p95)        AS max_p95,
    SUM(value_count)      AS total_count,
    SUM(CASE
        WHEN labels->>'http.response.status_code' ~ '^\d+$'
             AND (labels->>'http.response.status_code')::int >= 500
        THEN value_count
        ELSE 0
    END) AS error_count
FROM telemetry_metric_samples
GROUP BY 1, 2, 3, 4;

-- Unique index required for REFRESH MATERIALIZED VIEW CONCURRENTLY.
-- COALESCE handles NULL route values (non-HTTP metrics) which would otherwise
-- break uniqueness semantics since NULL != NULL in B-tree indexes.
CREATE UNIQUE INDEX idx_ttr_day_metric
    ON telemetry_trend_rollups (day, metric_name, scope, COALESCE(route, ''));
