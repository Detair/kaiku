# User Connectivity Monitor - Design Document

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Provide real-time connection quality visibility during voice calls, with historical tracking for users.

**Architecture:** Client extracts WebRTC stats every 3 seconds, reports to server, server broadcasts to room and stores in TimescaleDB. Users see quality indicators in VoiceIsland and participant list, with a dedicated history page for trend analysis.

**Tech Stack:** WebRTC `getStats()`, TimescaleDB (PostgreSQL extension), Solid.js signals, WebSocket events

---

## Overview

**User Connectivity Monitor v1** provides:
- Real-time quality indicators (latency, packet loss, jitter)
- Per-participant quality visibility in voice channels
- Configurable display (colored circle or latency number)
- Packet loss notifications (warning at 3%, critical at 7%)
- Historical connection tracking with 30-day trends
- Per-session drill-down for troubleshooting

---

## Data Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                         CLIENT                                   │
│  RTCPeerConnection.getStats() → ConnectionMetrics → WebSocket   │
│         ↓ every 3s                                              │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                         SERVER                                   │
│  Receive voice_stats → Broadcast to room → Store in TimescaleDB │
│                              │                                   │
│              ┌───────────────┼───────────────┐                  │
│              ▼               ▼               ▼                  │
│         Raw samples    Minute aggs     Hourly/Daily aggs        │
│          (7 days)      (30 days)         (1yr/forever)          │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                        CONSUMERS                                 │
│  • Real-time UI (VoiceIsland, Participants)                     │
│  • User history page (/settings/connection)                     │
│  • Admin dashboard (future: built-in + Grafana)                 │
└─────────────────────────────────────────────────────────────────┘
```

---

## Metrics Collected

Each sample contains:
- **Latency (RTT)**: Round-trip time to SFU in milliseconds
- **Packet Loss**: Percentage of lost RTP packets (delta between samples)
- **Jitter**: Variation in packet arrival time in milliseconds
- **Quality**: Computed color (0=red, 1=orange, 2=yellow, 3=green)
- **Timestamp**: When the measurement was taken
- **Session ID**: Groups metrics by voice call
- **User/Channel/Guild context**: Who, where

---

## Quality Thresholds

| Color | Latency | Packet Loss | Jitter | Experience |
|-------|---------|-------------|--------|------------|
| **Green** | <100ms | <1% | <30ms | Excellent |
| **Yellow** | 100-200ms | 1-3% | 30-50ms | Good, minor delay |
| **Orange** | 200-350ms | 3-5% | 50-80ms | Noticeable issues |
| **Red** | >350ms | >5% | >80ms | Poor quality |

Quality is determined by the **worst metric** - if any single metric exceeds a threshold, that color applies.

---

## UI Components

### VoiceIsland (Your Quality)

```
┌─────────────────────────────────────────────┐
│  🎤 General Voice  │  03:24  ●  │  ⚙️  ✕   │
│                            ↑                │
│                     Quality indicator       │
└─────────────────────────────────────────────┘
```

- **Circle mode**: Colored dot (green/yellow/orange/red)
- **Number mode**: Latency value with color ("127ms" in green)
- **Toggle**: User preference stored in localStorage
- **Initial state**: Gray dot until first metrics sample

### Hover Tooltip

```
┌────────────────────────────────┐
│  Connection Quality            │
│  ─────────────────────         │
│  Latency:     127ms   ✓        │
│  Packet Loss: 0.3%    ✓        │
│  Jitter:      58ms    ⚠        │  ← highlighted as "worst" metric
│                                │
│  Quality: Good                 │
└────────────────────────────────┘
```

### Participant List

```
┌─────────────────────────────────┐
│  👤 Alice          ●   🎤      │  ← green dot
│  👤 Bob            ●   🔇      │  ← yellow dot
│  👤 You            ●   🎤      │  ← your indicator
└─────────────────────────────────┘
```

Same hover tooltip behavior for each participant.

### Notification Toasts

- **Warning (3% loss)**: "Your connection is unstable" - yellow, auto-dismiss 5s
- **Critical (7% loss)**: "Connection severely degraded" - red, persists until quality improves
- **Cooldown**: New incident only triggered after 10s of good quality recovery
- **Escalation**: Warning can escalate to critical within same incident

---

## Connection History Page

Route: `/settings/connection`

```
┌─────────────────────────────────────────────────────────────────────┐
│  ← Settings          Connection History                             │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Overall Quality (Last 30 days)                                     │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  ████████████████████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ │   │
│  │  Jan 1        Jan 7        Jan 14        Jan 19            │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  Average:  87ms latency  •  0.4% loss  •  24ms jitter              │
│                                                                     │
│  ─────────────────────────────────────────────────────────────────  │
│                                                                     │
│  Recent Sessions                                                    │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  ● Today, 14:32 - 15:47 (1h 15m)    General Voice           │   │
│  │    Avg: 92ms • 0.2% loss • 18ms jitter           ● Green    │   │
│  ├─────────────────────────────────────────────────────────────┤   │
│  │  ● Yesterday, 20:15 - 21:30 (1h 15m)  Gaming Channel        │   │
│  │    Avg: 156ms • 1.8% loss • 42ms jitter          ● Yellow   │   │
│  ├─────────────────────────────────────────────────────────────┤   │
│  │  ● Jan 17, 19:00 - 19:12 (12m)        DM with Alice         │   │
│  │    Avg: 340ms • 4.2% loss • 71ms jitter          ● Orange   │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  [Load more...]                                                     │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

**Empty state:**
```
┌─────────────────────────────────────────────────────────────────┐
│  Connection History                                             │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│                    📊                                           │
│                                                                 │
│           No voice sessions yet                                 │
│                                                                 │
│   Join a voice channel to start tracking                        │
│   your connection quality over time.                            │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

---

## Database Schema

### TimescaleDB Setup

```sql
-- Enable extension
CREATE EXTENSION IF NOT EXISTS timescaledb;

-- Raw metrics table (hypertable)
CREATE TABLE connection_metrics (
    time        TIMESTAMPTZ NOT NULL,
    user_id     UUID NOT NULL,
    session_id  UUID NOT NULL,
    channel_id  UUID NOT NULL,
    guild_id    UUID,  -- NULL for DM calls
    latency_ms  SMALLINT NOT NULL,
    packet_loss REAL NOT NULL,     -- 0.0 to 100.0
    jitter_ms   SMALLINT NOT NULL,
    quality     SMALLINT NOT NULL  -- 0=red, 1=orange, 2=yellow, 3=green
);

SELECT create_hypertable('connection_metrics', 'time');

-- Indexes for common queries
CREATE INDEX idx_metrics_user_time ON connection_metrics (user_id, time DESC);
CREATE INDEX idx_metrics_channel_time ON connection_metrics (channel_id, time DESC);
CREATE INDEX idx_metrics_session ON connection_metrics (session_id);

-- Row-Level Security
ALTER TABLE connection_metrics ENABLE ROW LEVEL SECURITY;

CREATE POLICY user_own_metrics ON connection_metrics
    FOR SELECT
    USING (user_id = current_setting('app.current_user_id')::UUID);
```

### Session Summary Table

```sql
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
    USING (user_id = current_setting('app.current_user_id')::UUID);
```

### Continuous Aggregates

```sql
-- Minute aggregates
CREATE MATERIALIZED VIEW metrics_by_minute
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('1 minute', time) AS bucket,
    user_id,
    AVG(latency_ms)::SMALLINT AS avg_latency,
    MAX(latency_ms) AS max_latency,
    AVG(packet_loss)::REAL AS avg_loss,
    MAX(packet_loss) AS max_loss,
    AVG(jitter_ms)::SMALLINT AS avg_jitter,
    percentile_cont(0.95) WITHIN GROUP (ORDER BY latency_ms) AS p95_latency
FROM connection_metrics
GROUP BY bucket, user_id;

-- Hourly aggregates
CREATE MATERIALIZED VIEW metrics_by_hour
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('1 hour', time) AS bucket,
    user_id,
    AVG(latency_ms)::SMALLINT AS avg_latency,
    AVG(packet_loss)::REAL AS avg_loss,
    AVG(jitter_ms)::SMALLINT AS avg_jitter,
    percentile_cont(0.95) WITHIN GROUP (ORDER BY latency_ms) AS p95_latency,
    percentile_cont(0.99) WITHIN GROUP (ORDER BY latency_ms) AS p99_latency
FROM connection_metrics
GROUP BY bucket, user_id;

-- Daily aggregates
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
GROUP BY bucket, user_id;
```

### Retention Policies

```sql
-- Auto-drop raw data after 7 days
SELECT add_retention_policy('connection_metrics', INTERVAL '7 days');

-- Compress data older than 1 day (10x storage savings)
SELECT add_compression_policy('connection_metrics', INTERVAL '1 day');

-- Retention for aggregates
SELECT add_retention_policy('metrics_by_minute', INTERVAL '30 days');
SELECT add_retention_policy('metrics_by_hour', INTERVAL '1 year');
-- metrics_by_day: kept forever (minimal size)
-- connection_sessions: kept forever (one row per call)
```

---

## Client Implementation

### ConnectionMetrics Interface

```typescript
// client/src/lib/webrtc/types.ts

interface ConnectionMetrics {
  latency: number;      // RTT in ms
  packetLoss: number;   // 0-100 percentage
  jitter: number;       // ms
  quality: 'green' | 'yellow' | 'orange' | 'red';
  timestamp: number;
}

interface VoiceAdapter {
  // ... existing methods
  getConnectionMetrics(): Promise<ConnectionMetrics | null>;
}
```

### Metrics Extraction

```typescript
// client/src/lib/webrtc/browser.ts

private prevStats: { lost: number; received: number; timestamp: number } | null = null;

private async extractMetrics(): Promise<ConnectionMetrics | null> {
  if (!this.peerConnection) return null;

  const stats = await this.peerConnection.getStats();
  let latency = 0, jitter = 0;
  let totalLost = 0, totalReceived = 0;

  stats.forEach(report => {
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
    timestamp: now
  };
}

private calculateQuality(latency: number, loss: number, jitter: number): Quality {
  if (latency > 350 || loss > 5 || jitter > 80) return 'red';
  if (latency > 200 || loss > 3 || jitter > 50) return 'orange';
  if (latency > 100 || loss > 1 || jitter > 30) return 'yellow';
  return 'green';
}
```

### Metrics Reporting Loop

```typescript
// client/src/stores/voice.ts

private metricsInterval: number | null = null;
private sessionId: string | null = null;

onConnected(channelId: string) {
  // Only create new session if channel changed
  if (!this.sessionId || this.currentChannelId !== channelId) {
    this.sessionId = crypto.randomUUID();
    this.connectedAt = Date.now();
  }
  this.currentChannelId = channelId;

  this.metricsInterval = setInterval(() => this.reportMetrics(), 3000);
}

onDisconnected(intentional: boolean) {
  if (this.metricsInterval) clearInterval(this.metricsInterval);
  this.metricsInterval = null;

  if (intentional) {
    this.sessionId = null;
    this.prevStats = null;
    this.currentIncidentStart = null;
  }
}

private async reportMetrics() {
  try {
    const metrics = await adapter.getConnectionMetrics();
    if (metrics) {
      setLocalMetrics(metrics);
      websocket.send({
        type: 'voice_stats',
        data: { session_id: this.sessionId, ...metrics }
      });
      this.checkPacketLossThresholds(metrics);
    } else {
      setLocalMetrics('unknown');
    }
  } catch (err) {
    console.warn('Failed to collect metrics:', err);
  }
}
```

### Tab Visibility Handling

```typescript
document.addEventListener('visibilitychange', () => {
  if (document.hidden) {
    if (this.metricsInterval) {
      clearInterval(this.metricsInterval);
      this.metricsInterval = null;
    }
  } else if (this.isConnected && !this.metricsInterval) {
    this.metricsInterval = setInterval(() => this.reportMetrics(), 3000);
    this.reportMetrics();
  }
});
```

### Notification Logic

```typescript
private currentIncidentStart: number | null = null;
private lastGoodQualityTime: number = 0;
private readonly INCIDENT_RECOVERY_THRESHOLD = 10_000; // 10s

private checkPacketLossThresholds(metrics: ConnectionMetrics) {
  const now = Date.now();
  const isBadQuality = metrics.packetLoss >= 3;

  if (isBadQuality) {
    if (!this.currentIncidentStart) {
      this.currentIncidentStart = now;

      if (metrics.packetLoss >= 7) {
        showToast({ type: 'error', title: 'Connection severely degraded', duration: 0, id: 'connection-critical' });
      } else {
        showToast({ type: 'warning', title: 'Your connection is unstable', duration: 5000, id: 'connection-warning' });
      }
    } else if (metrics.packetLoss >= 7) {
      dismissToast('connection-warning');
      showToast({ type: 'error', id: 'connection-critical', title: 'Connection severely degraded', duration: 0 });
    }
  } else {
    if (this.currentIncidentStart && now - this.lastGoodQualityTime > INCIDENT_RECOVERY_THRESHOLD) {
      this.currentIncidentStart = null;
      dismissToast('connection-critical');
      dismissToast('connection-warning');
    }
    this.lastGoodQualityTime = now;
  }
}
```

---

## Server Implementation

### Peer Struct Additions

```rust
// server/src/voice/peer.rs

pub struct Peer {
    // ... existing fields
    pub session_id: Uuid,
    pub connected_at: DateTime<Utc>,
}
```

### Input Validation

```rust
#[derive(Debug, Deserialize)]
pub struct VoiceStats {
    session_id: Uuid,
    latency: i16,
    packet_loss: f32,
    jitter: i16,
    quality: u8,
    timestamp: i64,
}

impl VoiceStats {
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
```

### WebSocket Handler

```rust
pub async fn handle_voice_stats(
    user_id: Uuid,
    channel_id: Uuid,
    stats: VoiceStats,
    sfu: &Sfu,
    pool: &PgPool,
) -> Result<()> {
    if let Err(reason) = stats.validate() {
        tracing::warn!(user_id = %user_id, "Invalid voice stats: {}", reason);
        return Ok(());
    }

    // Broadcast to room
    let broadcast = VoiceEvent::UserStats {
        user_id,
        latency: stats.latency,
        packet_loss: stats.packet_loss,
        jitter: stats.jitter,
        quality: stats.quality,
    };
    sfu.broadcast_to_room(channel_id, &broadcast).await?;

    // Store (fire-and-forget)
    let guild_id = get_guild_id(pool, channel_id).await;
    tokio::spawn(store_metrics(pool.clone(), stats, user_id, channel_id, guild_id));

    Ok(())
}
```

### Session Finalization

```rust
pub async fn finalize_session(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
    channel_id: Uuid,
    guild_id: Option<Uuid>,
    started_at: DateTime<Utc>,
) -> Result<()> {
    let has_metrics: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM connection_metrics WHERE session_id = $1)"
    )
    .bind(session_id)
    .fetch_one(pool)
    .await?;

    if !has_metrics {
        sqlx::query(r#"
            INSERT INTO connection_sessions
            (id, user_id, channel_id, guild_id, started_at, ended_at,
             avg_latency, avg_loss, avg_jitter, worst_quality)
            VALUES ($1, $2, $3, $4, $5, NOW(), NULL, NULL, NULL, NULL)
        "#)
        .bind(session_id).bind(user_id).bind(channel_id).bind(guild_id).bind(started_at)
        .execute(pool).await?;
    } else {
        sqlx::query(r#"
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
        "#)
        .bind(session_id).bind(user_id).bind(channel_id).bind(guild_id).bind(started_at)
        .execute(pool).await?;
    }

    Ok(())
}
```

---

## API Endpoints

### Summary

```
GET /api/me/connection/summary
```

Returns 30-day aggregate stats and daily chart data.

### Sessions List

```
GET /api/me/connection/sessions?limit=20&offset=0
```

Returns paginated session list with channel/guild names.

### Session Detail

```
GET /api/me/connection/sessions/:session_id
```

Returns session summary + downsampled metrics (max 200 points).

---

## Data Access Control

### Real-time (during voice call)
**Intentionally shared** - Server broadcasts stats only to users in the same voice channel.

### Historical (connection history page)
**Strictly private** - Enforced via:
1. API handler filters by authenticated user_id
2. Row-Level Security as defense-in-depth

---

## Error Handling

- **Metrics extraction failure**: Log warning, show "unknown" state, voice continues working
- **WebSocket send failure**: Log, continue to other peers
- **Database insert failure**: Log warning, don't block voice
- **Invalid client data**: Log warning, silently drop

---

## User Settings

Stored in localStorage:

```typescript
interface ConnectionSettings {
  qualityDisplayMode: 'circle' | 'number';
  showNotifications: boolean;
  warningThreshold: number;   // Default 3%
  criticalThreshold: number;  // Default 7%
}
```

---

## Files to Create/Modify

### Client
- `client/src/lib/webrtc/types.ts` - Add ConnectionMetrics interface
- `client/src/lib/webrtc/browser.ts` - Add getStats() extraction
- `client/src/stores/voice.ts` - Metrics loop, notifications, session tracking
- `client/src/stores/settings.ts` - Connection display preferences
- `client/src/components/voice/QualityIndicator.tsx` - New component
- `client/src/components/voice/QualityTooltip.tsx` - New component
- `client/src/components/voice/VoiceIsland.tsx` - Add indicator
- `client/src/components/voice/VoiceParticipants.tsx` - Add per-user indicators
- `client/src/pages/settings/ConnectionHistory.tsx` - New page
- `client/src/components/settings/ConnectionChart.tsx` - New component
- `client/src/components/settings/SessionList.tsx` - New component

### Server
- `server/migrations/YYYYMMDD_connection_metrics.sql` - TimescaleDB schema
- `server/src/voice/messages.rs` - Add VoiceStats, VoiceUserStats events
- `server/src/voice/ws_handler.rs` - Handle voice_stats, broadcast
- `server/src/voice/peer.rs` - Add session_id, connected_at
- `server/src/connectivity/mod.rs` - New module
- `server/src/connectivity/handlers.rs` - History API endpoints
- `server/src/api/mod.rs` - Wire up routes

---

## Out of Scope (Future Phases)

- Network diagnostics / troubleshooting tools
- Admin analytics dashboard
- Grafana integration
- Automatic quality adjustments (bitrate, codec)
- Reconnection notifications

---

## Success Criteria

1. Users see their quality indicator in VoiceIsland while in voice
2. Users see all participants' quality in the participant list
3. Hovering shows detailed breakdown with problem metric highlighted
4. Toast appears when packet loss exceeds 3%, critical at 7%
5. Users can view their connection history at `/settings/connection`
6. Data retention works as specified (7d/30d/1y/forever)
7. Users can only access their own historical data
