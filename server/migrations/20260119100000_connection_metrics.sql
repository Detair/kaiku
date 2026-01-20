-- User Connectivity Monitor: Schema
-- Stores connection quality metrics for voice channels
-- Uses TimescaleDB when available, falls back to regular tables otherwise

-- Check if TimescaleDB is available and create tables accordingly
DO $$
DECLARE
    timescale_available BOOLEAN := FALSE;
BEGIN
    -- Check if timescaledb extension is available
    SELECT EXISTS(
        SELECT 1 FROM pg_available_extensions WHERE name = 'timescaledb'
    ) INTO timescale_available;

    IF timescale_available THEN
        -- Enable TimescaleDB extension
        CREATE EXTENSION IF NOT EXISTS timescaledb;
    END IF;
END $$;

-- Raw metrics table
-- Stores per-second connection quality samples
CREATE TABLE IF NOT EXISTS connection_metrics (
    time        TIMESTAMPTZ NOT NULL,
    user_id     UUID NOT NULL,
    session_id  UUID NOT NULL,
    channel_id  UUID NOT NULL,
    guild_id    UUID,                    -- NULL for DM calls
    latency_ms  SMALLINT NOT NULL,       -- Round-trip latency in milliseconds
    packet_loss REAL NOT NULL,           -- Packet loss ratio (0.0 - 1.0)
    jitter_ms   SMALLINT NOT NULL,       -- Jitter in milliseconds
    quality     SMALLINT NOT NULL        -- Quality score (0-3: 0=poor, 1=fair, 2=good, 3=excellent)
);

-- Convert to hypertable if TimescaleDB is available
DO $$
BEGIN
    IF EXISTS(SELECT 1 FROM pg_extension WHERE extname = 'timescaledb') THEN
        PERFORM create_hypertable('connection_metrics', 'time', if_not_exists => TRUE);
    END IF;
END $$;

-- Indexes for common queries
CREATE INDEX IF NOT EXISTS idx_metrics_user_time ON connection_metrics (user_id, time DESC);
CREATE INDEX IF NOT EXISTS idx_metrics_session ON connection_metrics (session_id);

-- Row-Level Security: Users can only read their own metrics
ALTER TABLE connection_metrics ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS user_own_metrics ON connection_metrics;
CREATE POLICY user_own_metrics ON connection_metrics
    FOR SELECT
    USING (user_id = current_setting('app.current_user_id', true)::UUID);

-- Session summary table
-- Aggregated statistics for completed voice sessions
CREATE TABLE IF NOT EXISTS connection_sessions (
    id            UUID PRIMARY KEY,
    user_id       UUID NOT NULL,
    channel_id    UUID NOT NULL,
    guild_id      UUID,                  -- NULL for DM calls
    started_at    TIMESTAMPTZ NOT NULL,
    ended_at      TIMESTAMPTZ NOT NULL,
    avg_latency   SMALLINT,              -- Average latency over session
    avg_loss      REAL,                  -- Average packet loss over session
    avg_jitter    SMALLINT,              -- Average jitter over session
    worst_quality SMALLINT               -- Worst quality score observed
);

CREATE INDEX IF NOT EXISTS idx_sessions_user_time ON connection_sessions (user_id, started_at DESC);

-- Row-Level Security: Users can only read their own sessions
ALTER TABLE connection_sessions ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS user_own_sessions ON connection_sessions;
CREATE POLICY user_own_sessions ON connection_sessions
    FOR SELECT
    USING (user_id = current_setting('app.current_user_id', true)::UUID);

-- TimescaleDB-specific features (only if extension is available)
DO $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM pg_extension WHERE extname = 'timescaledb') THEN
        RAISE NOTICE 'TimescaleDB not available - skipping continuous aggregates and policies';
        RETURN;
    END IF;

    -- Per-minute aggregates (for real-time monitoring)
    IF NOT EXISTS(SELECT 1 FROM pg_class WHERE relname = 'metrics_by_minute') THEN
        EXECUTE $sql$
            CREATE MATERIALIZED VIEW metrics_by_minute
            WITH (timescaledb.continuous) AS
            SELECT
                time_bucket('1 minute', time) AS bucket,
                user_id,
                AVG(latency_ms)::SMALLINT AS avg_latency,
                MAX(latency_ms) AS max_latency,
                AVG(packet_loss)::REAL AS avg_loss,
                MAX(packet_loss) AS max_loss,
                AVG(jitter_ms)::SMALLINT AS avg_jitter
            FROM connection_metrics
            GROUP BY bucket, user_id
            WITH NO DATA
        $sql$;
    END IF;

    -- Per-hour aggregates (for session history)
    IF NOT EXISTS(SELECT 1 FROM pg_class WHERE relname = 'metrics_by_hour') THEN
        EXECUTE $sql$
            CREATE MATERIALIZED VIEW metrics_by_hour
            WITH (timescaledb.continuous) AS
            SELECT
                time_bucket('1 hour', time) AS bucket,
                user_id,
                AVG(latency_ms)::SMALLINT AS avg_latency,
                AVG(packet_loss)::REAL AS avg_loss,
                AVG(jitter_ms)::SMALLINT AS avg_jitter,
                COUNT(*) AS sample_count
            FROM connection_metrics
            GROUP BY bucket, user_id
            WITH NO DATA
        $sql$;
    END IF;

    -- Per-day aggregates (for trends)
    IF NOT EXISTS(SELECT 1 FROM pg_class WHERE relname = 'metrics_by_day') THEN
        EXECUTE $sql$
            CREATE MATERIALIZED VIEW metrics_by_day
            WITH (timescaledb.continuous) AS
            SELECT
                time_bucket('1 day', time) AS bucket,
                user_id,
                AVG(latency_ms)::SMALLINT AS avg_latency,
                AVG(packet_loss)::REAL AS avg_loss,
                AVG(jitter_ms)::SMALLINT AS avg_jitter,
                COUNT(*) AS sample_count
            FROM connection_metrics
            GROUP BY bucket, user_id
            WITH NO DATA
        $sql$;
    END IF;

    -- Retention and Compression Policies
    -- Raw data: 7 days retention, compressed after 1 day
    BEGIN
        PERFORM add_retention_policy('connection_metrics', INTERVAL '7 days', if_not_exists => TRUE);
    EXCEPTION WHEN OTHERS THEN
        RAISE NOTICE 'Could not add retention policy: %', SQLERRM;
    END;

    BEGIN
        PERFORM add_compression_policy('connection_metrics', INTERVAL '1 day', if_not_exists => TRUE);
    EXCEPTION WHEN OTHERS THEN
        RAISE NOTICE 'Could not add compression policy: %', SQLERRM;
    END;

    -- Continuous Aggregate Refresh Policies
    BEGIN
        PERFORM add_continuous_aggregate_policy('metrics_by_minute',
            start_offset => INTERVAL '10 minutes',
            end_offset => INTERVAL '1 minute',
            schedule_interval => INTERVAL '1 minute',
            if_not_exists => TRUE);
    EXCEPTION WHEN OTHERS THEN
        RAISE NOTICE 'Could not add minute aggregate policy: %', SQLERRM;
    END;

    BEGIN
        PERFORM add_continuous_aggregate_policy('metrics_by_hour',
            start_offset => INTERVAL '2 hours',
            end_offset => INTERVAL '1 hour',
            schedule_interval => INTERVAL '1 hour',
            if_not_exists => TRUE);
    EXCEPTION WHEN OTHERS THEN
        RAISE NOTICE 'Could not add hour aggregate policy: %', SQLERRM;
    END;

    BEGIN
        PERFORM add_continuous_aggregate_policy('metrics_by_day',
            start_offset => INTERVAL '2 days',
            end_offset => INTERVAL '1 day',
            schedule_interval => INTERVAL '1 day',
            if_not_exists => TRUE);
    EXCEPTION WHEN OTHERS THEN
        RAISE NOTICE 'Could not add day aggregate policy: %', SQLERRM;
    END;
END $$;
