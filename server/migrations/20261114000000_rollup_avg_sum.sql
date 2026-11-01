-- Add the daily mean of `value_sum` to `telemetry_trend_rollups`.
--
-- f64 gauges (e.g. `kaiku_process_cpu_percent`) are stored in `value_sum`, not
-- `value_count`, so the 7d/30d admin Command Center charts for them had no data:
-- the rollup only aggregated `value_count`/`value_p95`.
--
-- Also align `error_count` with the error-rate definition used everywhere else
-- (HTTP status >= 400, not >= 500).
--
-- Same shape and unique index as 20260711000000 (plain columns, so
-- `REFRESH MATERIALIZED VIEW CONCURRENTLY` keeps working). The view is derived
-- data over at most 30 days of samples (retention), so drop + recreate is safe;
-- `CREATE MATERIALIZED VIEW` populates it immediately.

DROP MATERIALIZED VIEW IF EXISTS telemetry_trend_rollups;

CREATE MATERIALIZED VIEW telemetry_trend_rollups AS
SELECT
    date_trunc('day', ts)               AS day,
    metric_name,
    scope,
    COALESCE(labels->>'http.route', '') AS route,
    COUNT(*)                            AS sample_count,
    AVG(value_p95)                      AS avg_p95,
    MAX(value_p95)                      AS max_p95,
    AVG(value_sum)                      AS avg_sum,
    -- SUM() over a bigint column yields NUMERIC; cast back to bigint so the
    -- trend query decodes total_count/error_count into i64.
    SUM(value_count)::bigint            AS total_count,
    SUM(CASE
        WHEN labels->>'http.response.status_code' ~ '^\d+$'
             AND (labels->>'http.response.status_code')::int >= 400
        THEN value_count
        ELSE 0
    END)::bigint AS error_count
FROM telemetry_metric_samples
GROUP BY 1, 2, 3, 4;

CREATE UNIQUE INDEX idx_ttr_day_metric
    ON telemetry_trend_rollups (day, metric_name, scope, route);
