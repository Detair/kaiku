# User Connectivity Monitor Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement real-time connection quality indicators during voice calls with historical tracking.

**Architecture:** Client extracts WebRTC stats every 3s via `getStats()`, sends to server via WebSocket, server broadcasts to room participants and stores in TimescaleDB. UI shows quality indicators in VoiceIsland and participant list, with a dedicated history page.

**Tech Stack:** WebRTC getStats(), TimescaleDB, Solid.js, Axum WebSocket, sqlx

**Design Document:** `docs/plans/2026-01-19-user-connectivity-monitor-design.md`

---

## Batch 1: Database Schema & Server Types

### Task 1: TimescaleDB Migration

**Files:**
- Create: `server/migrations/20260119100000_connection_metrics.sql`

**Step 1: Create migration file**

```sql
-- Enable TimescaleDB extension
CREATE EXTENSION IF NOT EXISTS timescaledb;

-- Raw metrics table (hypertable)
CREATE TABLE connection_metrics (
    time        TIMESTAMPTZ NOT NULL,
    user_id     UUID NOT NULL,
    session_id  UUID NOT NULL,
    channel_id  UUID NOT NULL,
    guild_id    UUID,
    latency_ms  SMALLINT NOT NULL,
    packet_loss REAL NOT NULL,
    jitter_ms   SMALLINT NOT NULL,
    quality     SMALLINT NOT NULL
);

SELECT create_hypertable('connection_metrics', 'time');

-- Indexes for common queries
CREATE INDEX idx_metrics_user_time ON connection_metrics (user_id, time DESC);
CREATE INDEX idx_metrics_session ON connection_metrics (session_id);

-- Row-Level Security
ALTER TABLE connection_metrics ENABLE ROW LEVEL SECURITY;

CREATE POLICY user_own_metrics ON connection_metrics
    FOR SELECT
    USING (user_id = current_setting('app.current_user_id', true)::UUID);

-- Session summary table
CREATE TABLE connection_sessions (
    id           UUID PRIMARY KEY,
    user_id      UUID NOT NULL,
    channel_id   UUID NOT NULL,
    guild_id     UUID,
    started_at   TIMESTAMPTZ NOT NULL,
    ended_at     TIMESTAMPTZ NOT NULL,
    avg_latency  SMALLINT,
    avg_loss     REAL,
    avg_jitter   SMALLINT,
    worst_quality SMALLINT
);

CREATE INDEX idx_sessions_user_time ON connection_sessions (user_id, started_at DESC);

ALTER TABLE connection_sessions ENABLE ROW LEVEL SECURITY;

CREATE POLICY user_own_sessions ON connection_sessions
    FOR SELECT
    USING (user_id = current_setting('app.current_user_id', true)::UUID);

-- Continuous aggregates
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
WITH NO DATA;

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
WITH NO DATA;

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
WITH NO DATA;

-- Retention policies
SELECT add_retention_policy('connection_metrics', INTERVAL '7 days');
SELECT add_compression_policy('connection_metrics', INTERVAL '1 day');

-- Continuous aggregate refresh policies
SELECT add_continuous_aggregate_policy('metrics_by_minute',
    start_offset => INTERVAL '10 minutes',
    end_offset => INTERVAL '1 minute',
    schedule_interval => INTERVAL '1 minute');

SELECT add_continuous_aggregate_policy('metrics_by_hour',
    start_offset => INTERVAL '2 hours',
    end_offset => INTERVAL '1 hour',
    schedule_interval => INTERVAL '1 hour');

SELECT add_continuous_aggregate_policy('metrics_by_day',
    start_offset => INTERVAL '2 days',
    end_offset => INTERVAL '1 day',
    schedule_interval => INTERVAL '1 day');
```

**Step 2: Run migration**

Run: `cd server && sqlx migrate run`

Expected: Migration applies successfully

**Step 3: Commit**

```bash
git add server/migrations/20260119100000_connection_metrics.sql
git commit -m "feat(db): add TimescaleDB schema for connection metrics"
```

---

### Task 2: VoiceStats Types and Validation

**Files:**
- Create: `server/src/voice/stats.rs`
- Modify: `server/src/voice/mod.rs`

**Step 1: Create stats module with types and validation**

Create `server/src/voice/stats.rs`:

```rust
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Connection metrics reported by clients
#[derive(Debug, Clone, Deserialize)]
pub struct VoiceStats {
    pub session_id: Uuid,
    pub latency: i16,
    pub packet_loss: f32,
    pub jitter: i16,
    pub quality: u8,
    pub timestamp: i64,
}

impl VoiceStats {
    /// Validate stats are within acceptable ranges
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.latency < 0 || self.latency > 10000 {
            return Err("latency out of range (0-10000ms)");
        }
        if self.packet_loss < 0.0 || self.packet_loss > 100.0 {
            return Err("packet_loss out of range (0-100%)");
        }
        if self.jitter < 0 || self.jitter > 5000 {
            return Err("jitter out of range (0-5000ms)");
        }
        if self.quality > 3 {
            return Err("quality must be 0-3");
        }
        Ok(())
    }
}

/// Stats broadcast to other participants in the room
#[derive(Debug, Clone, Serialize)]
pub struct UserStats {
    pub user_id: Uuid,
    pub latency: i16,
    pub packet_loss: f32,
    pub jitter: i16,
    pub quality: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_stats() {
        let stats = VoiceStats {
            session_id: Uuid::new_v4(),
            latency: 100,
            packet_loss: 1.5,
            jitter: 30,
            quality: 3,
            timestamp: 1234567890,
        };
        assert!(stats.validate().is_ok());
    }

    #[test]
    fn test_latency_out_of_range() {
        let stats = VoiceStats {
            session_id: Uuid::new_v4(),
            latency: -1,
            packet_loss: 0.0,
            jitter: 0,
            quality: 3,
            timestamp: 0,
        };
        assert_eq!(stats.validate(), Err("latency out of range (0-10000ms)"));

        let stats2 = VoiceStats {
            session_id: Uuid::new_v4(),
            latency: 10001,
            packet_loss: 0.0,
            jitter: 0,
            quality: 3,
            timestamp: 0,
        };
        assert_eq!(stats2.validate(), Err("latency out of range (0-10000ms)"));
    }

    #[test]
    fn test_packet_loss_out_of_range() {
        let stats = VoiceStats {
            session_id: Uuid::new_v4(),
            latency: 100,
            packet_loss: -0.1,
            jitter: 30,
            quality: 3,
            timestamp: 0,
        };
        assert_eq!(stats.validate(), Err("packet_loss out of range (0-100%)"));

        let stats2 = VoiceStats {
            session_id: Uuid::new_v4(),
            latency: 100,
            packet_loss: 100.1,
            jitter: 30,
            quality: 3,
            timestamp: 0,
        };
        assert_eq!(stats2.validate(), Err("packet_loss out of range (0-100%)"));
    }

    #[test]
    fn test_jitter_out_of_range() {
        let stats = VoiceStats {
            session_id: Uuid::new_v4(),
            latency: 100,
            packet_loss: 1.0,
            jitter: -1,
            quality: 3,
            timestamp: 0,
        };
        assert_eq!(stats.validate(), Err("jitter out of range (0-5000ms)"));
    }

    #[test]
    fn test_quality_out_of_range() {
        let stats = VoiceStats {
            session_id: Uuid::new_v4(),
            latency: 100,
            packet_loss: 1.0,
            jitter: 30,
            quality: 4,
            timestamp: 0,
        };
        assert_eq!(stats.validate(), Err("quality must be 0-3"));
    }
}
```

**Step 2: Add module to voice/mod.rs**

Add to `server/src/voice/mod.rs`:

```rust
mod stats;
pub use stats::{VoiceStats, UserStats};
```

**Step 3: Run tests**

Run: `cd server && cargo test voice::stats`

Expected: All 5 tests pass

**Step 4: Commit**

```bash
git add server/src/voice/stats.rs server/src/voice/mod.rs
git commit -m "feat(voice): add VoiceStats types with validation"
```

---

### Task 3: WebSocket Event Types

**Files:**
- Modify: `server/src/ws/mod.rs`

**Step 1: Add VoiceStats to ClientEvent**

In `server/src/ws/mod.rs`, add to `ClientEvent` enum:

```rust
VoiceStats {
    channel_id: Uuid,
    session_id: Uuid,
    latency: i16,
    packet_loss: f32,
    jitter: i16,
    quality: u8,
    timestamp: i64,
},
```

**Step 2: Add VoiceUserStats to ServerEvent**

In `server/src/ws/mod.rs`, add to `ServerEvent` enum:

```rust
VoiceUserStats {
    channel_id: Uuid,
    user_id: Uuid,
    latency: i16,
    packet_loss: f32,
    jitter: i16,
    quality: u8,
},
```

**Step 3: Verify compilation**

Run: `cd server && cargo check`

Expected: Compiles without errors

**Step 4: Commit**

```bash
git add server/src/ws/mod.rs
git commit -m "feat(ws): add VoiceStats and VoiceUserStats events"
```

---

### Task 4: Peer Struct Additions

**Files:**
- Modify: `server/src/voice/peer.rs`

**Step 1: Add session tracking fields to Peer**

Add fields to `Peer` struct in `server/src/voice/peer.rs`:

```rust
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub struct Peer {
    // ... existing fields ...
    pub session_id: Uuid,
    pub connected_at: DateTime<Utc>,
}
```

**Step 2: Initialize in Peer::new()**

Update `Peer::new()` to initialize new fields:

```rust
impl Peer {
    pub fn new(/* existing params */) -> Self {
        Self {
            // ... existing fields ...
            session_id: Uuid::now_v7(),
            connected_at: Utc::now(),
        }
    }
}
```

**Step 3: Verify compilation**

Run: `cd server && cargo check`

Expected: Compiles without errors

**Step 4: Commit**

```bash
git add server/src/voice/peer.rs
git commit -m "feat(voice): add session tracking to Peer struct"
```

---

## Batch 2: Server Handlers

### Task 5: Voice Stats WebSocket Handler

**Files:**
- Modify: `server/src/voice/ws_handler.rs`
- Create: `server/src/voice/metrics.rs`

**Step 1: Create metrics storage module**

Create `server/src/voice/metrics.rs`:

```rust
use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::voice::stats::VoiceStats;

/// Store connection metrics in TimescaleDB (fire-and-forget)
pub async fn store_metrics(
    pool: PgPool,
    stats: VoiceStats,
    user_id: Uuid,
    channel_id: Uuid,
    guild_id: Option<Uuid>,
) {
    let result = sqlx::query(
        r#"
        INSERT INTO connection_metrics
        (time, user_id, session_id, channel_id, guild_id, latency_ms, packet_loss, jitter_ms, quality)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(Utc::now())
    .bind(user_id)
    .bind(stats.session_id)
    .bind(channel_id)
    .bind(guild_id)
    .bind(stats.latency)
    .bind(stats.packet_loss)
    .bind(stats.jitter)
    .bind(stats.quality as i16)
    .execute(&pool)
    .await;

    if let Err(e) = result {
        tracing::warn!(
            user_id = %user_id,
            session_id = %stats.session_id,
            "Failed to store connection metrics: {}",
            e
        );
    }
}

/// Get guild_id from channel_id
pub async fn get_guild_id(pool: &PgPool, channel_id: Uuid) -> Option<Uuid> {
    sqlx::query_scalar("SELECT guild_id FROM channels WHERE id = $1")
        .bind(channel_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
}
```

**Step 2: Add handler for VoiceStats in ws_handler.rs**

Add to `server/src/voice/ws_handler.rs`:

```rust
use crate::voice::metrics::{get_guild_id, store_metrics};
use crate::voice::stats::VoiceStats;
use crate::ws::ServerEvent;

pub async fn handle_voice_stats(
    sfu: &SfuServer,
    pool: &PgPool,
    user_id: Uuid,
    channel_id: Uuid,
    stats: VoiceStats,
) -> Result<(), VoiceError> {
    // Validate stats
    if let Err(reason) = stats.validate() {
        tracing::warn!(user_id = %user_id, "Invalid voice stats: {}", reason);
        return Ok(());
    }

    // Broadcast to room participants
    let broadcast = ServerEvent::VoiceUserStats {
        channel_id,
        user_id,
        latency: stats.latency,
        packet_loss: stats.packet_loss,
        jitter: stats.jitter,
        quality: stats.quality,
    };

    if let Some(room) = sfu.get_room(channel_id).await {
        room.broadcast_except(user_id, &broadcast).await;
    }

    // Store in database (fire-and-forget)
    let guild_id = get_guild_id(pool, channel_id).await;
    let pool_clone = pool.clone();
    tokio::spawn(async move {
        store_metrics(pool_clone, stats, user_id, channel_id, guild_id).await;
    });

    Ok(())
}
```

**Step 3: Wire up in main event handler**

In the main `handle_voice_event` function, add case for `VoiceStats`:

```rust
ClientEvent::VoiceStats {
    channel_id,
    session_id,
    latency,
    packet_loss,
    jitter,
    quality,
    timestamp,
} => {
    let stats = VoiceStats {
        session_id,
        latency,
        packet_loss,
        jitter,
        quality,
        timestamp,
    };
    handle_voice_stats(sfu, pool, user_id, channel_id, stats).await
}
```

**Step 4: Add module to voice/mod.rs**

```rust
mod metrics;
```

**Step 5: Verify compilation**

Run: `cd server && cargo check`

Expected: Compiles without errors

**Step 6: Commit**

```bash
git add server/src/voice/metrics.rs server/src/voice/ws_handler.rs server/src/voice/mod.rs
git commit -m "feat(voice): add voice stats handler with broadcast and storage"
```

---

### Task 6: Session Finalization on Disconnect

**Files:**
- Modify: `server/src/voice/metrics.rs`
- Modify: `server/src/voice/ws_handler.rs`

**Step 1: Add finalize_session function**

Add to `server/src/voice/metrics.rs`:

```rust
use chrono::DateTime;

/// Finalize session with aggregated metrics on disconnect
pub async fn finalize_session(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
    channel_id: Uuid,
    guild_id: Option<Uuid>,
    started_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    // Check if any metrics exist for this session
    let has_metrics: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM connection_metrics WHERE session_id = $1)",
    )
    .bind(session_id)
    .fetch_one(pool)
    .await?;

    if !has_metrics {
        // Insert session with NULL aggregates (very short call)
        sqlx::query(
            r#"
            INSERT INTO connection_sessions
            (id, user_id, channel_id, guild_id, started_at, ended_at,
             avg_latency, avg_loss, avg_jitter, worst_quality)
            VALUES ($1, $2, $3, $4, $5, NOW(), NULL, NULL, NULL, NULL)
            "#,
        )
        .bind(session_id)
        .bind(user_id)
        .bind(channel_id)
        .bind(guild_id)
        .bind(started_at)
        .execute(pool)
        .await?;
    } else {
        // Insert session with aggregated metrics
        sqlx::query(
            r#"
            INSERT INTO connection_sessions
            (id, user_id, channel_id, guild_id, started_at, ended_at,
             avg_latency, avg_loss, avg_jitter, worst_quality)
            SELECT
                $1, $2, $3, $4, $5, NOW(),
                AVG(latency_ms)::SMALLINT,
                AVG(packet_loss)::REAL,
                AVG(jitter_ms)::SMALLINT,
                MIN(quality)::SMALLINT
            FROM connection_metrics
            WHERE session_id = $1
            "#,
        )
        .bind(session_id)
        .bind(user_id)
        .bind(channel_id)
        .bind(guild_id)
        .bind(started_at)
        .execute(pool)
        .await?;
    }

    Ok(())
}
```

**Step 2: Call finalize_session in VoiceLeave handler**

In `server/src/voice/ws_handler.rs`, update the VoiceLeave handler to finalize session:

```rust
// In handle_voice_leave or where peer is removed:
if let Some(peer) = room.remove_peer(user_id).await {
    let guild_id = get_guild_id(pool, channel_id).await;

    // Finalize session in background
    let pool_clone = pool.clone();
    let session_id = peer.session_id;
    let connected_at = peer.connected_at;

    tokio::spawn(async move {
        if let Err(e) = finalize_session(
            &pool_clone,
            user_id,
            session_id,
            channel_id,
            guild_id,
            connected_at,
        ).await {
            tracing::warn!(
                user_id = %user_id,
                session_id = %session_id,
                "Failed to finalize session: {}",
                e
            );
        }
    });
}
```

**Step 3: Verify compilation**

Run: `cd server && cargo check`

Expected: Compiles without errors

**Step 4: Commit**

```bash
git add server/src/voice/metrics.rs server/src/voice/ws_handler.rs
git commit -m "feat(voice): finalize session with aggregates on disconnect"
```

---

### Task 7: Connection History API Endpoints

**Files:**
- Create: `server/src/connectivity/mod.rs`
- Create: `server/src/connectivity/handlers.rs`
- Modify: `server/src/api/mod.rs`
- Modify: `server/src/lib.rs`

**Step 1: Create connectivity module**

Create `server/src/connectivity/mod.rs`:

```rust
mod handlers;

use axum::{routing::get, Router};

use crate::api::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/summary", get(handlers::get_summary))
        .route("/sessions", get(handlers::get_sessions))
        .route("/sessions/:session_id", get(handlers::get_session_detail))
}
```

**Step 2: Create handlers**

Create `server/src/connectivity/handlers.rs`:

```rust
use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::AuthUser;

#[derive(Debug, Serialize)]
pub struct ConnectionSummary {
    pub period_days: i32,
    pub avg_latency: Option<i16>,
    pub avg_packet_loss: Option<f32>,
    pub avg_jitter: Option<i16>,
    pub total_sessions: i64,
    pub total_duration_secs: i64,
    pub daily_stats: Vec<DailyStat>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct DailyStat {
    pub date: NaiveDate,
    pub avg_latency: Option<i16>,
    pub avg_loss: Option<f32>,
    pub avg_jitter: Option<i16>,
    pub session_count: i64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SessionSummary {
    pub id: Uuid,
    pub channel_name: String,
    pub guild_name: Option<String>,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub avg_latency: Option<i16>,
    pub avg_loss: Option<f32>,
    pub avg_jitter: Option<i16>,
    pub worst_quality: Option<i16>,
}

#[derive(Debug, Serialize)]
pub struct SessionDetail {
    pub summary: SessionSummary,
    pub metrics: Vec<MetricPoint>,
    pub downsampled: bool,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct MetricPoint {
    pub time: DateTime<Utc>,
    pub latency_ms: i16,
    pub packet_loss: f32,
    pub jitter_ms: i16,
    pub quality: i16,
}

#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    20
}

pub async fn get_summary(
    auth: AuthUser,
    State(pool): State<PgPool>,
) -> Result<Json<ConnectionSummary>, axum::http::StatusCode> {
    // Set RLS context
    sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
        .bind(auth.user_id.to_string())
        .execute(&pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    // Get aggregate stats
    let stats: Option<(Option<i16>, Option<f32>, Option<i16>, i64, i64)> = sqlx::query_as(
        r#"
        SELECT
            AVG(avg_latency)::SMALLINT,
            AVG(avg_loss)::REAL,
            AVG(avg_jitter)::SMALLINT,
            COUNT(*),
            COALESCE(SUM(EXTRACT(EPOCH FROM (ended_at - started_at)))::BIGINT, 0)
        FROM connection_sessions
        WHERE user_id = $1
          AND started_at > NOW() - INTERVAL '30 days'
        "#,
    )
    .bind(auth.user_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let (avg_latency, avg_packet_loss, avg_jitter, total_sessions, total_duration_secs) =
        stats.unwrap_or((None, None, None, 0, 0));

    // Get daily stats
    let daily_stats: Vec<DailyStat> = sqlx::query_as(
        r#"
        SELECT
            DATE(started_at) AS date,
            AVG(avg_latency)::SMALLINT AS avg_latency,
            AVG(avg_loss)::REAL AS avg_loss,
            AVG(avg_jitter)::SMALLINT AS avg_jitter,
            COUNT(*) AS session_count
        FROM connection_sessions
        WHERE user_id = $1
          AND started_at > NOW() - INTERVAL '30 days'
        GROUP BY DATE(started_at)
        ORDER BY date
        "#,
    )
    .bind(auth.user_id)
    .fetch_all(&pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(ConnectionSummary {
        period_days: 30,
        avg_latency,
        avg_packet_loss,
        avg_jitter,
        total_sessions,
        total_duration_secs,
        daily_stats,
    }))
}

pub async fn get_sessions(
    auth: AuthUser,
    State(pool): State<PgPool>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<Vec<SessionSummary>>, axum::http::StatusCode> {
    // Set RLS context
    sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
        .bind(auth.user_id.to_string())
        .execute(&pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let sessions: Vec<SessionSummary> = sqlx::query_as(
        r#"
        SELECT
            s.id,
            COALESCE(c.name, 'Deleted Channel') AS channel_name,
            g.name AS guild_name,
            s.started_at,
            s.ended_at,
            s.avg_latency,
            s.avg_loss,
            s.avg_jitter,
            s.worst_quality
        FROM connection_sessions s
        LEFT JOIN channels c ON c.id = s.channel_id
        LEFT JOIN guilds g ON g.id = s.guild_id
        WHERE s.user_id = $1
        ORDER BY s.started_at DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(auth.user_id)
    .bind(params.limit.min(100))
    .bind(params.offset)
    .fetch_all(&pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(sessions))
}

pub async fn get_session_detail(
    auth: AuthUser,
    State(pool): State<PgPool>,
    Path(session_id): Path<Uuid>,
) -> Result<Json<SessionDetail>, axum::http::StatusCode> {
    const MAX_POINTS: i64 = 200;

    // Set RLS context
    sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
        .bind(auth.user_id.to_string())
        .execute(&pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    // Get session summary
    let summary: SessionSummary = sqlx::query_as(
        r#"
        SELECT
            s.id,
            COALESCE(c.name, 'Deleted Channel') AS channel_name,
            g.name AS guild_name,
            s.started_at,
            s.ended_at,
            s.avg_latency,
            s.avg_loss,
            s.avg_jitter,
            s.worst_quality
        FROM connection_sessions s
        LEFT JOIN channels c ON c.id = s.channel_id
        LEFT JOIN guilds g ON g.id = s.guild_id
        WHERE s.id = $1 AND s.user_id = $2
        "#,
    )
    .bind(session_id)
    .bind(auth.user_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(axum::http::StatusCode::NOT_FOUND)?;

    // Count total points
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM connection_metrics WHERE session_id = $1",
    )
    .bind(session_id)
    .fetch_one(&pool)
    .await
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let (metrics, downsampled) = if count <= MAX_POINTS {
        let metrics: Vec<MetricPoint> = sqlx::query_as(
            r#"
            SELECT time, latency_ms, packet_loss, jitter_ms, quality
            FROM connection_metrics
            WHERE session_id = $1
            ORDER BY time
            "#,
        )
        .bind(session_id)
        .fetch_all(&pool)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
        (metrics, false)
    } else {
        // Downsample using time_bucket
        let bucket_secs = (count / MAX_POINTS * 3) + 1;
        let query = format!(
            r#"
            SELECT
                time_bucket('{} seconds', time) AS time,
                AVG(latency_ms)::SMALLINT AS latency_ms,
                AVG(packet_loss)::REAL AS packet_loss,
                AVG(jitter_ms)::SMALLINT AS jitter_ms,
                MIN(quality)::SMALLINT AS quality
            FROM connection_metrics
            WHERE session_id = $1
            GROUP BY 1
            ORDER BY 1
            "#,
            bucket_secs
        );
        let metrics: Vec<MetricPoint> = sqlx::query_as(&query)
            .bind(session_id)
            .fetch_all(&pool)
            .await
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
        (metrics, true)
    };

    Ok(Json(SessionDetail {
        summary,
        metrics,
        downsampled,
    }))
}
```

**Step 3: Add module to lib.rs**

Add to `server/src/lib.rs`:

```rust
pub mod connectivity;
```

**Step 4: Wire up routes in api/mod.rs**

In `server/src/api/mod.rs`, add:

```rust
use crate::connectivity;

// In router setup:
.nest("/api/me/connection", connectivity::router())
```

**Step 5: Verify compilation**

Run: `cd server && cargo check`

Expected: Compiles without errors

**Step 6: Commit**

```bash
git add server/src/connectivity/ server/src/lib.rs server/src/api/mod.rs
git commit -m "feat(api): add connection history API endpoints"
```

---

## Batch 3: Client Metrics Collection

### Task 8: ConnectionMetrics Interface

**Files:**
- Modify: `client/src/lib/webrtc/types.ts`

**Step 1: Add ConnectionMetrics and related types**

Add to `client/src/lib/webrtc/types.ts`:

```typescript
export type QualityLevel = 'green' | 'yellow' | 'orange' | 'red';

export interface ConnectionMetrics {
  latency: number;      // RTT in ms
  packetLoss: number;   // 0-100 percentage
  jitter: number;       // ms
  quality: QualityLevel;
  timestamp: number;
}

export interface ParticipantMetrics {
  userId: string;
  latency: number;
  packetLoss: number;
  jitter: number;
  quality: QualityLevel;
}
```

**Step 2: Add to VoiceAdapter interface**

Add method to `VoiceAdapter` interface:

```typescript
export interface VoiceAdapter {
  // ... existing methods ...

  /** Get current connection metrics from WebRTC stats */
  getConnectionMetrics(): Promise<ConnectionMetrics | null>;
}
```

**Step 3: Commit**

```bash
git add client/src/lib/webrtc/types.ts
git commit -m "feat(client): add ConnectionMetrics interface"
```

---

### Task 9: Metrics Extraction in Browser Adapter

**Files:**
- Modify: `client/src/lib/webrtc/browser.ts`

**Step 1: Add private state for delta calculation**

Add to `BrowserVoiceAdapter` class:

```typescript
private prevStats: { lost: number; received: number; timestamp: number } | null = null;
```

**Step 2: Add quality calculation method**

```typescript
private calculateQuality(latency: number, loss: number, jitter: number): QualityLevel {
  if (latency > 350 || loss > 5 || jitter > 80) return 'red';
  if (latency > 200 || loss > 3 || jitter > 50) return 'orange';
  if (latency > 100 || loss > 1 || jitter > 30) return 'yellow';
  return 'green';
}
```

**Step 3: Implement getConnectionMetrics**

```typescript
async getConnectionMetrics(): Promise<ConnectionMetrics | null> {
  if (!this.peerConnection) return null;

  try {
    const stats = await this.peerConnection.getStats();
    let latency = 0;
    let jitter = 0;
    let totalLost = 0;
    let totalReceived = 0;

    stats.forEach((report) => {
      if (report.type === 'candidate-pair' && report.state === 'succeeded') {
        latency = (report.currentRoundTripTime ?? 0) * 1000;
      }
      if (report.type === 'inbound-rtp' && report.kind === 'audio') {
        totalLost += report.packetsLost ?? 0;
        totalReceived += report.packetsReceived ?? 0;
        jitter = Math.max(jitter, (report.jitter ?? 0) * 1000);
      }
    });

    // Calculate delta packet loss since last sample
    let packetLoss = 0;
    const now = Date.now();

    if (this.prevStats) {
      const deltaLost = totalLost - this.prevStats.lost;
      const deltaReceived = totalReceived - this.prevStats.received;
      const deltaTotal = deltaLost + deltaReceived;

      if (deltaTotal > 0) {
        packetLoss = (deltaLost / deltaTotal) * 100;
      }
    }

    this.prevStats = { lost: totalLost, received: totalReceived, timestamp: now };

    return {
      latency: Math.round(latency),
      packetLoss: Math.round(packetLoss * 100) / 100,
      jitter: Math.round(jitter),
      quality: this.calculateQuality(latency, packetLoss, jitter),
      timestamp: now,
    };
  } catch (err) {
    console.warn('Failed to extract metrics:', err);
    return null;
  }
}
```

**Step 4: Reset prevStats on leave**

In `leave()` method, add:

```typescript
this.prevStats = null;
```

**Step 5: Verify compilation**

Run: `cd client && bun run check`

Expected: No type errors

**Step 6: Commit**

```bash
git add client/src/lib/webrtc/browser.ts
git commit -m "feat(client): implement WebRTC metrics extraction"
```

---

### Task 10: Voice Store Metrics Loop

**Files:**
- Modify: `client/src/stores/voice.ts`
- Modify: `client/src/lib/types.ts`

**Step 1: Add metrics state to VoiceStoreState**

In `client/src/stores/voice.ts`, extend state:

```typescript
interface VoiceStoreState {
  // ... existing fields ...
  sessionId: string | null;
  connectedAt: number | null;
  localMetrics: ConnectionMetrics | 'unknown' | null;
  participantMetrics: Map<string, ParticipantMetrics>;
}
```

**Step 2: Add metrics loop management**

Add to voice store:

```typescript
let metricsInterval: number | null = null;

function startMetricsLoop() {
  if (metricsInterval) return;

  metricsInterval = window.setInterval(async () => {
    const adapter = getVoiceAdapter();
    if (!adapter) return;

    try {
      const metrics = await adapter.getConnectionMetrics();
      if (metrics) {
        setVoiceState('localMetrics', metrics);

        // Send to server
        const sessionId = voiceState.sessionId;
        if (sessionId && voiceState.channelId) {
          wsSend({
            type: 'VoiceStats',
            channel_id: voiceState.channelId,
            session_id: sessionId,
            latency: metrics.latency,
            packet_loss: metrics.packetLoss,
            jitter: metrics.jitter,
            quality: qualityToNumber(metrics.quality),
            timestamp: metrics.timestamp,
          });
        }
      } else {
        setVoiceState('localMetrics', 'unknown');
      }
    } catch (err) {
      console.warn('Failed to collect metrics:', err);
    }
  }, 3000);
}

function stopMetricsLoop() {
  if (metricsInterval) {
    clearInterval(metricsInterval);
    metricsInterval = null;
  }
}

function qualityToNumber(quality: QualityLevel): number {
  switch (quality) {
    case 'green': return 3;
    case 'yellow': return 2;
    case 'orange': return 1;
    case 'red': return 0;
  }
}
```

**Step 3: Start/stop metrics loop on connect/disconnect**

In `joinVoice()`:

```typescript
setVoiceState('sessionId', crypto.randomUUID());
setVoiceState('connectedAt', Date.now());
startMetricsLoop();
```

In `leaveVoice()`:

```typescript
stopMetricsLoop();
setVoiceState('sessionId', null);
setVoiceState('connectedAt', null);
setVoiceState('localMetrics', null);
setVoiceState('participantMetrics', new Map());
```

**Step 4: Handle VoiceUserStats event**

Add WebSocket event handler:

```typescript
// In WebSocket message handler
case 'VoiceUserStats': {
  const { user_id, latency, packet_loss, jitter, quality } = data;
  const newMetrics = new Map(voiceState.participantMetrics);
  newMetrics.set(user_id, {
    userId: user_id,
    latency,
    packetLoss: packet_loss,
    jitter,
    quality: numberToQuality(quality),
  });
  setVoiceState('participantMetrics', newMetrics);
  break;
}

function numberToQuality(n: number): QualityLevel {
  switch (n) {
    case 3: return 'green';
    case 2: return 'yellow';
    case 1: return 'orange';
    default: return 'red';
  }
}
```

**Step 5: Export getter functions**

```typescript
export function getLocalMetrics(): ConnectionMetrics | 'unknown' | null {
  return voiceState.localMetrics;
}

export function getParticipantMetrics(userId: string): ParticipantMetrics | undefined {
  return voiceState.participantMetrics.get(userId);
}
```

**Step 6: Verify compilation**

Run: `cd client && bun run check`

Expected: No type errors

**Step 7: Commit**

```bash
git add client/src/stores/voice.ts client/src/lib/types.ts
git commit -m "feat(client): add metrics loop and participant metrics tracking"
```

---

### Task 11: Notification Logic

**Files:**
- Modify: `client/src/stores/voice.ts`

**Step 1: Add notification state**

```typescript
let currentIncidentStart: number | null = null;
let lastGoodQualityTime: number = 0;
const INCIDENT_RECOVERY_THRESHOLD = 10_000; // 10s
```

**Step 2: Add notification check function**

```typescript
import { showToast, dismissToast } from '../components/ui/Toast';

function checkPacketLossThresholds(metrics: ConnectionMetrics) {
  const now = Date.now();
  const isBadQuality = metrics.packetLoss >= 3;

  if (isBadQuality) {
    if (!currentIncidentStart) {
      currentIncidentStart = now;

      if (metrics.packetLoss >= 7) {
        showToast({
          type: 'error',
          title: 'Connection severely degraded',
          message: `${metrics.packetLoss.toFixed(1)}% packet loss`,
          duration: 0,
          id: 'connection-critical',
        });
      } else {
        showToast({
          type: 'warning',
          title: 'Your connection is unstable',
          message: `${metrics.packetLoss.toFixed(1)}% packet loss`,
          duration: 5000,
          id: 'connection-warning',
        });
      }
    } else if (metrics.packetLoss >= 7) {
      dismissToast('connection-warning');
      showToast({
        type: 'error',
        title: 'Connection severely degraded',
        message: `${metrics.packetLoss.toFixed(1)}% packet loss`,
        duration: 0,
        id: 'connection-critical',
      });
    }
  } else {
    if (currentIncidentStart && now - lastGoodQualityTime > INCIDENT_RECOVERY_THRESHOLD) {
      currentIncidentStart = null;
      dismissToast('connection-critical');
      dismissToast('connection-warning');
    }
    lastGoodQualityTime = now;
  }
}
```

**Step 3: Call in metrics loop**

Update metrics loop to call `checkPacketLossThresholds(metrics)` after setting state.

**Step 4: Reset on disconnect**

In `leaveVoice()`:

```typescript
currentIncidentStart = null;
lastGoodQualityTime = 0;
dismissToast('connection-critical');
dismissToast('connection-warning');
```

**Step 5: Commit**

```bash
git add client/src/stores/voice.ts
git commit -m "feat(client): add packet loss notification system"
```

---

### Task 12: Tab Visibility Handling

**Files:**
- Modify: `client/src/stores/voice.ts`

**Step 1: Add visibility change handler**

```typescript
if (typeof document !== 'undefined') {
  document.addEventListener('visibilitychange', () => {
    if (document.hidden) {
      stopMetricsLoop();
    } else if (voiceState.state === 'connected' && !metricsInterval) {
      startMetricsLoop();
    }
  });
}
```

**Step 2: Commit**

```bash
git add client/src/stores/voice.ts
git commit -m "feat(client): pause metrics collection when tab hidden"
```

---

## Batch 4: Client UI Components

### Task 13: QualityIndicator Component

**Files:**
- Create: `client/src/components/voice/QualityIndicator.tsx`

**Step 1: Create component**

```typescript
import { Component, Show } from 'solid-js';
import type { ConnectionMetrics, QualityLevel } from '../../lib/webrtc/types';

interface QualityIndicatorProps {
  metrics: ConnectionMetrics | 'unknown' | null;
  mode: 'circle' | 'number';
  class?: string;
}

const qualityColors: Record<QualityLevel, string> = {
  green: 'bg-green-500',
  yellow: 'bg-yellow-500',
  orange: 'bg-orange-500',
  red: 'bg-red-500',
};

const qualityTextColors: Record<QualityLevel, string> = {
  green: 'text-green-500',
  yellow: 'text-yellow-500',
  orange: 'text-orange-500',
  red: 'text-red-500',
};

export const QualityIndicator: Component<QualityIndicatorProps> = (props) => {
  const isLoading = () => props.metrics === null || props.metrics === 'unknown';
  const metrics = () => (typeof props.metrics === 'object' ? props.metrics : null);

  return (
    <div class={`inline-flex items-center ${props.class ?? ''}`}>
      <Show
        when={!isLoading()}
        fallback={
          <div class="w-2 h-2 rounded-full bg-gray-500 animate-pulse" />
        }
      >
        <Show
          when={props.mode === 'circle'}
          fallback={
            <span class={`text-xs font-mono ${qualityTextColors[metrics()!.quality]}`}>
              {metrics()!.latency}ms
            </span>
          }
        >
          <div
            class={`w-2 h-2 rounded-full ${qualityColors[metrics()!.quality]}`}
          />
        </Show>
      </Show>
    </div>
  );
};

export default QualityIndicator;
```

**Step 2: Commit**

```bash
git add client/src/components/voice/QualityIndicator.tsx
git commit -m "feat(ui): add QualityIndicator component"
```

---

### Task 14: QualityTooltip Component

**Files:**
- Create: `client/src/components/voice/QualityTooltip.tsx`

**Step 1: Create component**

```typescript
import { Component, Show } from 'solid-js';
import type { ConnectionMetrics, QualityLevel } from '../../lib/webrtc/types';

interface QualityTooltipProps {
  metrics: ConnectionMetrics;
}

const qualityLabels: Record<QualityLevel, string> = {
  green: 'Excellent',
  yellow: 'Good',
  orange: 'Fair',
  red: 'Poor',
};

const thresholds = {
  latency: { yellow: 100, orange: 200, red: 350 },
  packetLoss: { yellow: 1, orange: 3, red: 5 },
  jitter: { yellow: 30, orange: 50, red: 80 },
};

function getMetricStatus(value: number, metric: keyof typeof thresholds): 'ok' | 'warning' | 'critical' {
  const t = thresholds[metric];
  if (value >= t.red) return 'critical';
  if (value >= t.orange) return 'warning';
  return 'ok';
}

export const QualityTooltip: Component<QualityTooltipProps> = (props) => {
  const latencyStatus = () => getMetricStatus(props.metrics.latency, 'latency');
  const lossStatus = () => getMetricStatus(props.metrics.packetLoss, 'packetLoss');
  const jitterStatus = () => getMetricStatus(props.metrics.jitter, 'jitter');

  const statusIcon = (status: 'ok' | 'warning' | 'critical') => {
    switch (status) {
      case 'ok': return '✓';
      case 'warning': return '⚠';
      case 'critical': return '✗';
    }
  };

  const statusColor = (status: 'ok' | 'warning' | 'critical') => {
    switch (status) {
      case 'ok': return 'text-green-400';
      case 'warning': return 'text-yellow-400';
      case 'critical': return 'text-red-400';
    }
  };

  return (
    <div class="bg-surface-layer2 rounded-lg p-3 shadow-lg min-w-48">
      <div class="text-sm font-medium text-text-primary mb-2">
        Connection Quality
      </div>
      <div class="border-t border-surface-layer1 my-2" />

      <div class="space-y-1.5 text-xs">
        <div class="flex justify-between items-center">
          <span class="text-text-secondary">Latency:</span>
          <span class="flex items-center gap-1">
            <span class={latencyStatus() !== 'ok' ? 'font-medium text-text-primary' : 'text-text-secondary'}>
              {props.metrics.latency}ms
            </span>
            <span class={statusColor(latencyStatus())}>{statusIcon(latencyStatus())}</span>
          </span>
        </div>

        <div class="flex justify-between items-center">
          <span class="text-text-secondary">Packet Loss:</span>
          <span class="flex items-center gap-1">
            <span class={lossStatus() !== 'ok' ? 'font-medium text-text-primary' : 'text-text-secondary'}>
              {props.metrics.packetLoss.toFixed(1)}%
            </span>
            <span class={statusColor(lossStatus())}>{statusIcon(lossStatus())}</span>
          </span>
        </div>

        <div class="flex justify-between items-center">
          <span class="text-text-secondary">Jitter:</span>
          <span class="flex items-center gap-1">
            <span class={jitterStatus() !== 'ok' ? 'font-medium text-text-primary' : 'text-text-secondary'}>
              {props.metrics.jitter}ms
            </span>
            <span class={statusColor(jitterStatus())}>{statusIcon(jitterStatus())}</span>
          </span>
        </div>
      </div>

      <div class="border-t border-surface-layer1 my-2" />

      <div class="text-xs text-text-secondary">
        Quality: <span class="text-text-primary">{qualityLabels[props.metrics.quality]}</span>
      </div>
    </div>
  );
};

export default QualityTooltip;
```

**Step 2: Commit**

```bash
git add client/src/components/voice/QualityTooltip.tsx
git commit -m "feat(ui): add QualityTooltip component"
```

---

### Task 15: VoiceIsland Integration

**Files:**
- Modify: `client/src/components/layout/VoiceIsland.tsx`

**Step 1: Import components and state**

```typescript
import { QualityIndicator } from '../voice/QualityIndicator';
import { QualityTooltip } from '../voice/QualityTooltip';
import { getLocalMetrics } from '../../stores/voice';
import { getConnectionDisplayMode } from '../../stores/settings';
```

**Step 2: Add tooltip state**

```typescript
const [showTooltip, setShowTooltip] = createSignal(false);
```

**Step 3: Add quality indicator to UI**

Add next to the timer display:

```tsx
<div
  class="relative"
  onMouseEnter={() => setShowTooltip(true)}
  onMouseLeave={() => setShowTooltip(false)}
>
  <QualityIndicator
    metrics={getLocalMetrics()}
    mode={getConnectionDisplayMode()}
  />
  <Show when={showTooltip() && typeof getLocalMetrics() === 'object'}>
    <div class="absolute bottom-full left-1/2 -translate-x-1/2 mb-2 z-50">
      <QualityTooltip metrics={getLocalMetrics() as ConnectionMetrics} />
    </div>
  </Show>
</div>
```

**Step 4: Commit**

```bash
git add client/src/components/layout/VoiceIsland.tsx
git commit -m "feat(ui): add quality indicator to VoiceIsland"
```

---

### Task 16: VoiceParticipants Integration

**Files:**
- Modify: `client/src/components/voice/VoiceParticipants.tsx`

**Step 1: Import components and state**

```typescript
import { QualityIndicator } from './QualityIndicator';
import { QualityTooltip } from './QualityTooltip';
import { getParticipantMetrics, getLocalMetrics } from '../../stores/voice';
import { getConnectionDisplayMode } from '../../stores/settings';
```

**Step 2: Add quality indicator to each participant row**

For each participant, add indicator before the mute icon:

```tsx
{(participant) => {
  const [showTooltip, setShowTooltip] = createSignal(false);
  const metrics = () => participant.user_id === currentUserId
    ? getLocalMetrics()
    : getParticipantMetrics(participant.user_id);

  return (
    <div class="flex items-center gap-2 px-2 py-1">
      <span class="flex-1 truncate">{participant.display_name || participant.username}</span>

      <div
        class="relative"
        onMouseEnter={() => setShowTooltip(true)}
        onMouseLeave={() => setShowTooltip(false)}
      >
        <QualityIndicator
          metrics={metrics() ?? null}
          mode={getConnectionDisplayMode()}
        />
        <Show when={showTooltip() && typeof metrics() === 'object'}>
          <div class="absolute bottom-full right-0 mb-2 z-50">
            <QualityTooltip metrics={metrics() as ConnectionMetrics} />
          </div>
        </Show>
      </div>

      {/* existing mute/speaking indicators */}
    </div>
  );
}}
```

**Step 3: Commit**

```bash
git add client/src/components/voice/VoiceParticipants.tsx
git commit -m "feat(ui): add quality indicators to participant list"
```

---

## Batch 5: Connection History Page

### Task 17: Settings Store for Connection Preferences

**Files:**
- Modify: `client/src/stores/settings.ts` (or create if needed)

**Step 1: Add connection settings**

```typescript
interface ConnectionSettings {
  displayMode: 'circle' | 'number';
  showNotifications: boolean;
}

const STORAGE_KEY = 'connection-settings';

const defaultSettings: ConnectionSettings = {
  displayMode: 'circle',
  showNotifications: true,
};

function loadConnectionSettings(): ConnectionSettings {
  if (typeof localStorage === 'undefined') return defaultSettings;
  const stored = localStorage.getItem(STORAGE_KEY);
  return stored ? { ...defaultSettings, ...JSON.parse(stored) } : defaultSettings;
}

const [connectionSettings, setConnectionSettings] = createSignal(loadConnectionSettings());

export function getConnectionDisplayMode(): 'circle' | 'number' {
  return connectionSettings().displayMode;
}

export function setConnectionDisplayMode(mode: 'circle' | 'number') {
  const updated = { ...connectionSettings(), displayMode: mode };
  setConnectionSettings(updated);
  localStorage.setItem(STORAGE_KEY, JSON.stringify(updated));
}

export function getShowNotifications(): boolean {
  return connectionSettings().showNotifications;
}

export function setShowNotifications(show: boolean) {
  const updated = { ...connectionSettings(), showNotifications: show };
  setConnectionSettings(updated);
  localStorage.setItem(STORAGE_KEY, JSON.stringify(updated));
}
```

**Step 2: Commit**

```bash
git add client/src/stores/settings.ts
git commit -m "feat(client): add connection settings store"
```

---

### Task 18: ConnectionHistory Page

**Files:**
- Create: `client/src/pages/settings/ConnectionHistory.tsx`
- Modify: `client/src/App.tsx` (add route)

**Step 1: Create page component**

```typescript
import { Component, createResource, Show, For } from 'solid-js';
import { A } from '@solidjs/router';
import { ArrowLeft } from 'lucide-solid';
import { fetchApi } from '../../lib/tauri';
import { ConnectionChart } from '../../components/settings/ConnectionChart';
import { SessionList } from '../../components/settings/SessionList';

interface ConnectionSummary {
  period_days: number;
  avg_latency: number | null;
  avg_packet_loss: number | null;
  avg_jitter: number | null;
  total_sessions: number;
  total_duration_secs: number;
  daily_stats: DailyStat[];
}

interface DailyStat {
  date: string;
  avg_latency: number | null;
  avg_loss: number | null;
  avg_jitter: number | null;
  session_count: number;
}

async function fetchSummary(): Promise<ConnectionSummary> {
  return fetchApi('/api/me/connection/summary');
}

export const ConnectionHistory: Component = () => {
  const [summary] = createResource(fetchSummary);

  const formatDuration = (secs: number) => {
    const hours = Math.floor(secs / 3600);
    const mins = Math.floor((secs % 3600) / 60);
    if (hours > 0) return `${hours}h ${mins}m`;
    return `${mins}m`;
  };

  return (
    <div class="min-h-screen bg-surface-base text-text-primary p-6">
      <div class="max-w-4xl mx-auto">
        <div class="flex items-center gap-4 mb-6">
          <A href="/settings" class="p-2 hover:bg-surface-layer1 rounded-lg">
            <ArrowLeft class="w-5 h-5" />
          </A>
          <h1 class="text-xl font-semibold">Connection History</h1>
        </div>

        <Show
          when={!summary.loading}
          fallback={<div class="text-text-secondary">Loading...</div>}
        >
          <Show
            when={summary()?.total_sessions > 0}
            fallback={
              <div class="text-center py-16">
                <div class="text-4xl mb-4">📊</div>
                <div class="text-lg font-medium mb-2">No voice sessions yet</div>
                <div class="text-text-secondary">
                  Join a voice channel to start tracking your connection quality over time.
                </div>
              </div>
            }
          >
            <div class="space-y-6">
              {/* Summary stats */}
              <div class="bg-surface-layer1 rounded-lg p-4">
                <h2 class="text-sm font-medium text-text-secondary mb-3">
                  Last {summary()!.period_days} Days
                </h2>
                <div class="grid grid-cols-4 gap-4 text-center">
                  <div>
                    <div class="text-2xl font-semibold">
                      {summary()!.avg_latency ?? '-'}ms
                    </div>
                    <div class="text-xs text-text-secondary">Avg Latency</div>
                  </div>
                  <div>
                    <div class="text-2xl font-semibold">
                      {summary()!.avg_packet_loss?.toFixed(1) ?? '-'}%
                    </div>
                    <div class="text-xs text-text-secondary">Avg Loss</div>
                  </div>
                  <div>
                    <div class="text-2xl font-semibold">
                      {summary()!.avg_jitter ?? '-'}ms
                    </div>
                    <div class="text-xs text-text-secondary">Avg Jitter</div>
                  </div>
                  <div>
                    <div class="text-2xl font-semibold">
                      {formatDuration(summary()!.total_duration_secs)}
                    </div>
                    <div class="text-xs text-text-secondary">Total Time</div>
                  </div>
                </div>
              </div>

              {/* Chart */}
              <div class="bg-surface-layer1 rounded-lg p-4">
                <h2 class="text-sm font-medium text-text-secondary mb-3">
                  Quality Over Time
                </h2>
                <ConnectionChart data={summary()!.daily_stats} />
              </div>

              {/* Sessions */}
              <div class="bg-surface-layer1 rounded-lg p-4">
                <h2 class="text-sm font-medium text-text-secondary mb-3">
                  Recent Sessions
                </h2>
                <SessionList />
              </div>
            </div>
          </Show>
        </Show>
      </div>
    </div>
  );
};

export default ConnectionHistory;
```

**Step 2: Add route to App.tsx**

```typescript
import { ConnectionHistory } from './pages/settings/ConnectionHistory';

// In routes:
<Route path="/settings/connection" component={ConnectionHistory} />
```

**Step 3: Commit**

```bash
git add client/src/pages/settings/ConnectionHistory.tsx client/src/App.tsx
git commit -m "feat(ui): add ConnectionHistory page"
```

---

### Task 19: ConnectionChart Component

**Files:**
- Create: `client/src/components/settings/ConnectionChart.tsx`

**Step 1: Create simple bar chart component**

```typescript
import { Component, For } from 'solid-js';

interface DailyStat {
  date: string;
  avg_latency: number | null;
  avg_loss: number | null;
  session_count: number;
}

interface ConnectionChartProps {
  data: DailyStat[];
}

function getQualityFromStats(latency: number | null, loss: number | null): number {
  if (latency === null || loss === null) return 0;
  if (latency > 350 || loss > 5) return 1;
  if (latency > 200 || loss > 3) return 2;
  if (latency > 100 || loss > 1) return 3;
  return 4;
}

const qualityColors = ['bg-gray-600', 'bg-red-500', 'bg-orange-500', 'bg-yellow-500', 'bg-green-500'];

export const ConnectionChart: Component<ConnectionChartProps> = (props) => {
  const maxSessions = () => Math.max(...props.data.map(d => d.session_count), 1);

  return (
    <div class="h-32 flex items-end gap-1">
      <For each={props.data}>
        {(day) => {
          const quality = getQualityFromStats(day.avg_latency, day.avg_loss);
          const height = (day.session_count / maxSessions()) * 100;

          return (
            <div class="flex-1 flex flex-col items-center gap-1">
              <div
                class={`w-full rounded-t ${qualityColors[quality]}`}
                style={{ height: `${Math.max(height, 4)}%` }}
                title={`${day.date}: ${day.session_count} sessions, ${day.avg_latency ?? '-'}ms latency`}
              />
              <span class="text-[10px] text-text-secondary">
                {new Date(day.date).getDate()}
              </span>
            </div>
          );
        }}
      </For>
    </div>
  );
};

export default ConnectionChart;
```

**Step 2: Commit**

```bash
git add client/src/components/settings/ConnectionChart.tsx
git commit -m "feat(ui): add ConnectionChart component"
```

---

### Task 20: SessionList Component

**Files:**
- Create: `client/src/components/settings/SessionList.tsx`

**Step 1: Create component**

```typescript
import { Component, createResource, createSignal, Show, For } from 'solid-js';
import { fetchApi } from '../../lib/tauri';

interface SessionSummary {
  id: string;
  channel_name: string;
  guild_name: string | null;
  started_at: string;
  ended_at: string;
  avg_latency: number | null;
  avg_loss: number | null;
  avg_jitter: number | null;
  worst_quality: number | null;
}

const qualityColors = ['bg-red-500', 'bg-orange-500', 'bg-yellow-500', 'bg-green-500'];
const qualityLabels = ['Poor', 'Fair', 'Good', 'Excellent'];

async function fetchSessions(offset: number): Promise<SessionSummary[]> {
  return fetchApi(`/api/me/connection/sessions?limit=10&offset=${offset}`);
}

export const SessionList: Component = () => {
  const [offset, setOffset] = createSignal(0);
  const [sessions] = createResource(offset, fetchSessions);

  const formatTime = (iso: string) => {
    const d = new Date(iso);
    return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  };

  const formatDate = (iso: string) => {
    const d = new Date(iso);
    const today = new Date();
    const yesterday = new Date(today);
    yesterday.setDate(yesterday.getDate() - 1);

    if (d.toDateString() === today.toDateString()) return 'Today';
    if (d.toDateString() === yesterday.toDateString()) return 'Yesterday';
    return d.toLocaleDateString([], { month: 'short', day: 'numeric' });
  };

  const formatDuration = (start: string, end: string) => {
    const ms = new Date(end).getTime() - new Date(start).getTime();
    const mins = Math.floor(ms / 60000);
    const hours = Math.floor(mins / 60);
    if (hours > 0) return `${hours}h ${mins % 60}m`;
    return `${mins}m`;
  };

  return (
    <div class="space-y-2">
      <Show
        when={!sessions.loading}
        fallback={<div class="text-text-secondary text-sm">Loading...</div>}
      >
        <For each={sessions()}>
          {(session) => (
            <div class="flex items-center gap-3 p-3 bg-surface-layer2 rounded-lg">
              <div
                class={`w-2 h-2 rounded-full ${qualityColors[session.worst_quality ?? 0]}`}
              />
              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-2">
                  <span class="font-medium truncate">{session.channel_name}</span>
                  <Show when={session.guild_name}>
                    <span class="text-text-secondary text-xs">
                      in {session.guild_name}
                    </span>
                  </Show>
                </div>
                <div class="text-xs text-text-secondary">
                  {formatDate(session.started_at)}, {formatTime(session.started_at)} - {formatTime(session.ended_at)} ({formatDuration(session.started_at, session.ended_at)})
                </div>
              </div>
              <div class="text-right text-xs text-text-secondary">
                <div>{session.avg_latency ?? '-'}ms</div>
                <div>{session.avg_loss?.toFixed(1) ?? '-'}% loss</div>
              </div>
            </div>
          )}
        </For>

        <Show when={sessions()?.length === 10}>
          <button
            class="w-full py-2 text-sm text-text-secondary hover:text-text-primary"
            onClick={() => setOffset(o => o + 10)}
          >
            Load more...
          </button>
        </Show>
      </Show>
    </div>
  );
};

export default SessionList;
```

**Step 2: Commit**

```bash
git add client/src/components/settings/SessionList.tsx
git commit -m "feat(ui): add SessionList component"
```

---

## Final Steps

### Task 21: Export Components and Final Wiring

**Files:**
- Modify: `client/src/components/voice/index.ts`
- Modify: `client/src/components/settings/index.ts` (create if needed)

**Step 1: Export voice components**

Update `client/src/components/voice/index.ts`:

```typescript
export { QualityIndicator } from './QualityIndicator';
export { QualityTooltip } from './QualityTooltip';
// ... existing exports
```

**Step 2: Create settings components index**

Create `client/src/components/settings/index.ts`:

```typescript
export { ConnectionChart } from './ConnectionChart';
export { SessionList } from './SessionList';
```

**Step 3: Run full build**

Run: `cd client && bun run build`

Expected: Build succeeds without errors

**Step 4: Run server tests**

Run: `cd server && cargo test`

Expected: All tests pass

**Step 5: Final commit**

```bash
git add client/src/components/voice/index.ts client/src/components/settings/index.ts
git commit -m "feat: complete User Connectivity Monitor implementation"
```

---

## Summary

| Batch | Tasks | Focus |
|-------|-------|-------|
| 1 | 1-4 | Database schema, server types |
| 2 | 5-7 | Server handlers and API |
| 3 | 8-12 | Client metrics collection |
| 4 | 13-16 | UI components |
| 5 | 17-21 | History page and final wiring |

**Total: 21 tasks**

**Key Files Created:**
- `server/migrations/20260119100000_connection_metrics.sql`
- `server/src/voice/stats.rs`
- `server/src/voice/metrics.rs`
- `server/src/connectivity/mod.rs`
- `server/src/connectivity/handlers.rs`
- `client/src/components/voice/QualityIndicator.tsx`
- `client/src/components/voice/QualityTooltip.tsx`
- `client/src/components/settings/ConnectionChart.tsx`
- `client/src/components/settings/SessionList.tsx`
- `client/src/pages/settings/ConnectionHistory.tsx`

**Key Files Modified:**
- `server/src/voice/mod.rs`
- `server/src/voice/peer.rs`
- `server/src/voice/ws_handler.rs`
- `server/src/ws/mod.rs`
- `server/src/api/mod.rs`
- `client/src/lib/webrtc/types.ts`
- `client/src/lib/webrtc/browser.ts`
- `client/src/stores/voice.ts`
- `client/src/stores/settings.ts`
- `client/src/components/layout/VoiceIsland.tsx`
- `client/src/components/voice/VoiceParticipants.tsx`
- `client/src/App.tsx`
