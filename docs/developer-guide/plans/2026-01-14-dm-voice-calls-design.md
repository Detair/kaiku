# DM Voice Calls - Design Document

**Date:** 2026-01-14 (Updated: 2026-01-19)
**Status:** Approved
**Priority:** Latency > Simplicity/Stability > Privacy

## Overview

Add voice calling to DM and group DM conversations. DM channels double as voice channels - no separate "call" entity. Reuses existing SFU infrastructure.

### User Flow

1. User A clicks "Call" button in DM header
2. A joins immediately (SFU creates room using DM channel ID)
3. B receives notification with Join/Decline options
4. B can decline without joining (A sees "B declined")
5. Call ends when: A hangs up, timeout expires (90s), or last person leaves (group DMs)

## Architecture Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| P2P vs SFU | Always SFU | Group calls need SFU anyway; simpler single code path |
| Call state storage | Redis Streams | Event-sourced, multi-node safe, ephemeral |
| Voice room ID | DM channel ID | Reuses existing SFU, no new infrastructure |
| Timeout | 90s ring, 15s reconnect | Balance between UX and resource cleanup |

## Backend Design

### Call State (Redis Streams)

```rust
// Stream key: call_events:{channel_id}
// Events appended, state derived from replay

pub enum CallEventType {
    Started { initiator: UserId },
    Joined { user: UserId },
    Left { user: UserId },
    Declined { user: UserId },
    Ended { reason: EndReason },
}

pub enum CallState {
    Ringing { started_by: UserId, started_at: DateTime, declined_by: HashSet<UserId> },
    Active { participants: HashSet<UserId> },
    Ended { reason: EndReason, duration_secs: Option<u32> },
}

pub enum EndReason {
    Cancelled,      // Initiator hung up
    AllDeclined,    // Everyone declined
    NoAnswer,       // Timeout (90s)
    LastLeft,       // Last participant left
}
```

### API Endpoints

| Endpoint | Purpose | Rate Limit |
|----------|---------|------------|
| `POST /api/dm/:id/call/start` | Start call | 5/min per user |
| `POST /api/dm/:id/call/join` | Join active call | - |
| `POST /api/dm/:id/call/decline` | Decline without joining | - |
| `POST /api/dm/:id/call/leave` | Leave call | - |
| `GET /api/dm/:id/call` | Get current state | - |

### WebSocket Events

```rust
pub enum CallEvent {
    CallStarted { channel_id, initiator, capabilities },
    CallEnded { channel_id, reason, duration_secs },
    ParticipantJoined { channel_id, user_id },
    ParticipantLeft { channel_id, user_id },
    CallDeclined { channel_id, user_id },
    RingingReminder { channel_id },
}
```

### Security Mitigations

- DM membership verified on every endpoint
- Block list honored (blocked users can't call you)
- Rate limiting: per-user, per-channel, per-target
- No `seen_by` exposed (prevents presence oracle attack)
- Auto-expire calls after 90s timeout

### Performance Optimizations

- Friend/membership cached in WebSocket connection state (avoid DB in hot path)
- Redis pipeline for START (XADD + EXPIRE + PUBLISH)
- Direct WebSocket send when caller/callee on same server
- Server-authoritative timeouts with heartbeat confirmation

## Frontend Design

### UI Components

**Call Button:** Phone icon in DM header, disabled when blocked/in-call/offline

**Active Call Banner** (top of chat):
```
┌─────────────────────────────────────────────────┐
│ 🔊 Call with Alice • 02:34      [Join] [Decline]│
└─────────────────────────────────────────────────┘
```
- Green background when active, pulsing animation when ringing
- Audio ring notification with distinctive sound
- Shows duration once connected

**Inline Chat Message** (for history):
```
┌─────────────────────────────────────────────────┐
│ 📞 Bob started a call                    2:34 PM│
│    Duration: 5 minutes • Alice, Bob             │
└─────────────────────────────────────────────────┘
```

**DM Sidebar Indicator:**
- Green phone icon on DMs with active calls
- Pulse animation for incoming calls awaiting response

### State Machine

```typescript
type CallState =
  | { status: 'idle' }
  | { status: 'outgoing_ringing'; startedAt: number }
  | { status: 'incoming_ringing'; caller: UserId }
  | { status: 'connected'; participants: UserId[]; startedAt: number }
  | { status: 'reconnecting'; countdown: number }
  | { status: 'ended'; reason: EndReason; duration?: number };
```

### Notification Settings (Callee Controls)

| Setting | Behavior |
|---------|----------|
| Normal | Ring for 90s with audio, then missed call notification |
| Quiet | Single notification, no sound |
| Do Not Disturb | No notification, caller sees "unavailable" |

## Voice Connection Flow

```
1. User clicks "Call" in DM
   ├── POST /api/dm/:id/call/start
   ├── Redis Stream: XADD call_events:{dm_id} Started
   └── WebSocket: broadcast CallStarted

2. User joins SFU (same as channel voice)
   ├── WebSocket: VoiceJoin { channel_id: dm_channel_id }
   ├── SFU creates room keyed by dm_channel_id
   ├── ICE/SDP exchange
   └── Audio connected

3. Other user joins
   ├── Clicks "Join" on banner
   ├── Same VoiceJoin flow
   └── SFU routes audio between peers

4. Call ends
   ├── Last user hangs up OR timeout
   ├── Redis Stream: XADD Ended
   ├── WebSocket: broadcast CallEnded
   └── SFU room cleaned up
```

## Error Handling

### Network/Connection

| Scenario | Behavior |
|----------|----------|
| Caller loses connection | Progressive feedback at 5s, 10s; ends at 15s |
| Callee loses connection during call | "Reconnecting..." for 10s, auto-mute mic |
| SFU unreachable | Client shows error, call state in Redis for retry |

### User State Edge Cases

| Scenario | Behavior |
|----------|----------|
| User already in another call | Button disabled, tooltip: "Already in a call" |
| User blocks caller mid-ring | Call ends immediately, generic "Call ended" message |
| Both users click "Call" simultaneously | Show "You both called! Connecting..." |
| User has DND enabled | Caller sees "User is unavailable" |

### Group DM Specifics

| Scenario | Behavior |
|----------|----------|
| 3rd person joins mid-call | Allowed |
| Call initiator leaves | Call continues if others remain |
| Member removed from group mid-call | Immediately ejected |

## Testing Requirements

### Race Conditions to Test

- Simultaneous call initiation (first wins, second joins)
- Simultaneous hang-up (cleanup runs exactly once)
- Block during reconnect window
- Rapid call/end/call cycle
- Multi-device answer (first wins, other shows "answered elsewhere")

### Network Failure Simulation

- Use Toxiproxy for Redis/network partition testing
- Tokio test utilities with time control for timeout boundaries
- Property-based testing for concurrent operations

## Persona Review Summary

### Elrond (Architecture)
- ✅ Use Redis Streams for multi-node coordination
- ✅ Explicit state machine for call lifecycle
- ✅ Service boundary via CallSignaling trait

### Faramir (Security)
- ✅ DM membership verified on every endpoint
- ✅ Rate limiting prevents DoS
- ✅ No `seen_by` prevents presence oracle
- ✅ Block list honored

### Gandalf (Performance)
- ✅ Cache friendships in WebSocket connection state
- ✅ Redis pipeline for call start
- ✅ Direct WebSocket when same server

### Legolas (QA)
- ✅ Test race conditions with property-based testing
- ✅ Server-authoritative timeouts

### Éowyn (Developer)
- ✅ 15s disconnect with progressive feedback
- ✅ Frontend state machine
- ✅ Auto-mute during reconnect

### Pippin (User)
- ✅ Callee controls notification settings (not caller)
- ✅ Audio ring notification
- ✅ DND integration

## Resolved Questions

1. **Video calls:** ✅ Design for capability negotiation now. Include `capabilities` in API responses so video can be enabled later without breaking changes.
2. **Call recording:** Deferred. Not in initial scope.
3. **Screen sharing:** ✅ Include in capabilities for future enablement.

## Capability Flags

Include capability negotiation in call state to future-proof for video/screen share:

```rust
/// Call capabilities (for future video/screen share support)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CallCapabilities {
    pub audio: bool,       // Always true for now
    pub video: bool,       // Future: video calls
    pub screenshare: bool, // Future: screen sharing
}

impl CallCapabilities {
    /// Default capabilities for audio-only calls
    pub fn audio_only() -> Self {
        Self {
            audio: true,
            video: false,
            screenshare: false,
        }
    }
}
```

API responses include capabilities:
```json
{
  "channel_id": "...",
  "status": "active",
  "capabilities": { "audio": true, "video": false, "screenshare": false },
  "participants": [...]
}
```

WebSocket CallStarted event includes capabilities:
```rust
CallStarted {
    channel_id: Uuid,
    initiator: Uuid,
    initiator_name: String,
    capabilities: CallCapabilities,
}
```

## Implementation Estimate

| Component | Effort |
|-----------|--------|
| Backend: Redis Streams call state | Medium |
| Backend: API endpoints | Low |
| Backend: WebSocket events | Low |
| Frontend: Call UI components | Medium |
| Frontend: State machine | Medium |
| Frontend: Notification settings | Low |
| Testing: Race conditions | Medium |
| Integration: SFU reuse | Low |

---

*Design reviewed by: Elrond, Faramir, Gandalf, Legolas, Éowyn, Pippin*
