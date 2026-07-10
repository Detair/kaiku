-- Fix: the `telemetry_trend_rollups` materialized view could never be refreshed
-- with `REFRESH MATERIALIZED VIEW CONCURRENTLY`, so the hourly retention job
-- failed every cycle and the view stayed empty — the admin Command Center's
-- 7d/30d trend charts had no data.
--
-- Cause: the unique index was built on an EXPRESSION (`COALESCE(route, '')`).
-- PostgreSQL only accepts a unique index on plain columns as the row-matching
-- index for a concurrent refresh; an expression index is ignored, which yields
-- `cannot refresh materialized view ... concurrently`.
--
-- Fix: compute `route` as a non-null column inside the view (so NULL routes for
-- non-HTTP metrics collapse to '') and put the unique index on plain columns.

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
    -- SUM() over a bigint column yields NUMERIC; cast back to bigint so the
    -- trend query decodes total_count/error_count into i64.
    SUM(value_count)::bigint            AS total_count,
    SUM(CASE
        WHEN labels->>'http.response.status_code' ~ '^\d+$'
             AND (labels->>'http.response.status_code')::int >= 500
        THEN value_count
        ELSE 0
    END)::bigint AS error_count
FROM telemetry_metric_samples
GROUP BY 1, 2, 3, 4;

-- Plain-column unique index (no expression) so REFRESH ... CONCURRENTLY works.
-- `route` is now non-null, so it can participate directly.
CREATE UNIQUE INDEX idx_ttr_day_metric
    ON telemetry_trend_rollups (day, metric_name, scope, route);
