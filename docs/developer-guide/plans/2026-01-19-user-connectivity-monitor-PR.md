# User Connectivity Monitor - PR Summary

## Overview

Real-time voice connection quality monitoring with historical analytics. Users can see their connection quality during voice calls and review past session data.

**Design Doc:** `docs/plans/2026-01-19-user-connectivity-monitor-design.md`
**Implementation Plan:** `docs/plans/2026-01-19-user-connectivity-monitor-implementation.md`

---

## What's New

### For Users
- **Quality indicators** in VoiceIsland and participant list (colored dots: green/yellow/orange/red)
- **Hover tooltips** showing latency, packet loss, and jitter values
- **Toast notifications** when connection quality degrades (warning at 3% loss, critical at 7%)
- **Connection History page** at `/settings/connection` with 30-day analytics

### For the Platform
- **TimescaleDB hypertable** for efficient time-series storage
- **Automatic aggregation** (minute/hour/day rollups via continuous aggregates)
- **Data retention** (7-day raw, 30-day minute, 1-year hourly aggregates)
- **Row-Level Security** ensuring users only see their own data

---

## Architecture

```
Client                              Server                          Database
───────                             ──────                          ────────
WebRTC.getStats()
    │ every 3s
    ▼
ConnectionMetrics ──WebSocket──►    VoiceStats handler
                                        │
                                        ├──► Broadcast to room participants
                                        │
                                        └──► store_metrics() ──────►  connection_metrics
                                                                      (TimescaleDB hypertable)

On disconnect:                      finalize_session() ────────►  connection_sessions
                                                                  (aggregated summary)

REST API:
    GET /api/me/connection/summary   ◄──── 30-day aggregate stats
    GET /api/me/connection/sessions  ◄──── Paginated session list
    GET /api/me/connection/sessions/:id ◄─ Session detail with metrics
```

---

## Files Changed (30 files, +2,160 lines)

### Server

| File | Purpose |
|------|---------|
| `migrations/20260119100000_connection_metrics.sql` | TimescaleDB schema, RLS policies, retention |
| `src/voice/stats.rs` | VoiceStats struct with validation |
| `src/voice/metrics.rs` | `store_metrics()` and `finalize_session()` |
| `src/voice/ws_handler.rs` | WebSocket handler for VoiceStats events |
| `src/voice/peer.rs` | Added session_id, connected_at to Peer |
| `src/ws/mod.rs` | VoiceStats/VoiceUserStats event types |
| `src/connectivity/` | REST API handlers (summary, sessions, detail) |

### Client

| File | Purpose |
|------|---------|
| `lib/webrtc/types.ts` | ConnectionMetrics, QualityLevel types |
| `lib/webrtc/browser.ts` | `getConnectionMetrics()` with delta packet loss |
| `stores/voice.ts` | Metrics loop, notification logic, participant metrics |
| `stores/settings.ts` | Connection display preferences |
| `components/ui/Toast.tsx` | Toast notification system |
| `components/voice/QualityIndicator.tsx` | Circle/number quality display |
| `components/voice/QualityTooltip.tsx` | Detailed metrics hover card |
| `components/layout/VoiceIsland.tsx` | Integrated quality indicator |
| `components/voice/VoiceParticipants.tsx` | Per-participant indicators |
| `pages/settings/ConnectionHistory.tsx` | History page with summary |
| `components/settings/ConnectionChart.tsx` | Daily quality bar chart |
| `components/settings/SessionList.tsx` | Paginated session list |

---

## Key Implementation Details

### Quality Thresholds

| Level | Latency | Packet Loss | Jitter |
|-------|---------|-------------|--------|
| Green | <100ms | <1% | <30ms |
| Yellow | 100-200ms | 1-3% | 30-50ms |
| Orange | 200-350ms | 3-5% | 50-80ms |
| Red | >350ms | >5% | >80ms |

### Notification Logic
- **Warning** (yellow toast): Packet loss ≥3% for 3+ seconds
- **Critical** (red toast): Packet loss ≥7% for 3+ seconds
- **Recovery**: Toast dismissed after 10 seconds of good quality

### Delta Packet Loss Calculation
Client tracks cumulative `packetsLost` and `packetsReceived` between samples to calculate actual packet loss percentage, not cumulative totals.

### Tab Visibility Handling
Metrics collection pauses when browser tab is hidden to save resources, resumes when visible.

---

## Security Considerations

- **RLS enforced**: Users can only query their own metrics via `current_setting('app.current_user_id')`
- **Input validation**: VoiceStats validates latency (0-10000ms), packet_loss (0-100%), jitter (0-5000ms), quality (0-3)
- **Auth required**: All connectivity endpoints use `AuthUser` extractor

---

## Testing

### Automated
- `voice/stats.rs`: 5 unit tests for validation edge cases

### Manual Testing Checklist
- [ ] Join voice channel, verify quality indicator appears in VoiceIsland
- [ ] Hover over indicator, verify tooltip shows latency/loss/jitter
- [ ] Check other participants show quality dots
- [ ] Simulate packet loss, verify warning toast appears at 3%
- [ ] Simulate high packet loss, verify critical toast at 7%
- [ ] Let quality recover, verify toast dismisses after 10s
- [ ] Leave and rejoin, verify metrics restart properly
- [ ] Visit `/settings/connection`, verify summary stats load
- [ ] Verify daily chart shows quality colors
- [ ] Verify session list shows past sessions
- [ ] Click "Load more" to test pagination

---

## Known Limitations (Future Work)

1. **No integration tests** for REST API endpoints
2. **Session ID is client-generated** - not validated server-side (acceptable for telemetry)
3. **Pagination replaces list** instead of appending (minor UX issue)
4. **No retry for session finalization** on DB failure

---

## Screenshots

*To be added during PR review*

---

## Reviewers Checklist

- [ ] TimescaleDB migration runs without errors
- [ ] RLS policies correctly isolate user data
- [ ] WebSocket events properly typed and handled
- [ ] Quality thresholds consistent across client
- [ ] Toast notifications appear and dismiss correctly
- [ ] Connection History page loads and displays data
- [ ] No console errors during normal operation
