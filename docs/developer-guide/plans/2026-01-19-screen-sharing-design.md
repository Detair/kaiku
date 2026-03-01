# Screen Sharing Design

**Date:** 2026-01-19
**Status:** Approved
**Phase:** 4 (Advanced Features)

## Overview

Full-featured screen sharing for gaming, collaboration, and presentations. Designed for webcam addition in future phases.

### Goals

- Screen sharing with adaptive quality
- Premium 1080p60 tier (permission-gated)
- No server transcoding — SFU forwards only
- Works in guild channels and DM voice calls

### Non-Goals (v1)

- Webcam video (architecture supports it, implementation later)
- Simulcast (multiple quality layers from encoder)
- Per-application audio capture
- Popout windows (use Portal overlays instead)

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                         Client                               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
│  │ Screen       │  │ Hardware     │  │ Video Track      │  │
│  │ Capture API  │──│ Encoder      │──│ (to SFU)         │  │
│  │ (display/    │  │ Detection    │  │                  │  │
│  │  window)     │  │ AV1/H264/VP9 │  │ + optional audio │  │
│  └──────────────┘  └──────────────┘  └──────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                      SFU (Server)                            │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ TrackRouter (extended)                                │  │
│  │  - Audio tracks (existing)                            │  │
│  │  - Video tracks (new): screen share, future webcam    │  │
│  │  - Track metadata: kind, source_type, codec           │  │
│  └──────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                      Viewers                                 │
│  - Receive video track + optional audio track               │
│  - Independent volume controls                              │
│  - View modes: Spotlight / PiP / Theater                    │
└─────────────────────────────────────────────────────────────┘
```

### Key Constraints

- Voice latency target (<50ms) remains priority — video is best-effort
- No codec transcoding; room negotiates common codec
- Premium quality (1080p60) gated by user feature flag

---

## Codec Strategy

### Priority Chains

| Platform | Priority | Rationale |
|----------|----------|-----------|
| **Web** | AV1 → VP9 → VP8 | Royalty-free, broad browser support |
| **Desktop (Tauri)** | AV1 → H.264 | Best compression, then best hw acceleration |

### Hardware Encoder Support Matrix

| Vendor | GPU Series | AV1 Encode | H.264 Encode | Fallback |
|--------|------------|------------|--------------|----------|
| **NVIDIA** | RTX 40+ | ✅ | ✅ | H.264 |
| **NVIDIA** | RTX 20/30 | ❌ | ✅ | H.264 |
| **AMD** | RX 9070+ (Navi 48) | ✅ B-Frame | ✅ | H.264 |
| **AMD** | RX 7000 | ✅ | ✅ | H.264 |
| **AMD** | RX 6000 | ❌ | ✅ | H.264 |
| **AMD** | RX 9000 budget (Navi 44) | ❌ | ❌ | Software VP9/VP8 |
| **Intel** | Arc A-series | ✅ | ✅ | H.264 |
| **Intel** | QuickSync (6th gen+) | ❌ | ✅ | H.264 |
| **Apple** | M3+ | ✅ | ✅ | H.264 |
| **Apple** | M1/M2 | ❌ | ✅ | H.264 |

### Room Codec Negotiation

1. Sharer announces available codecs + quality tiers
2. Server checks all current participants' capabilities
3. Server picks highest common codec
4. If new participant joins with incompatible codec:
   - They see placeholder: "Screen share uses unsupported codec"
   - No stream interruption for existing viewers

### Quality Tiers

| Tier | Resolution | FPS | Bitrate | Requirement |
|------|------------|-----|---------|-------------|
| **Low** | 480p | 15 | 0.5-1 Mbps | Auto fallback |
| **Medium** | 720p | 30 | 1.5-3 Mbps | Default |
| **High** | 1080p | 30 | 3-5 Mbps | Default for fast connections |
| **Premium** | 1080p | 60 | 4-8 Mbps | Requires `PREMIUM_VIDEO` user flag |

---

## Capture Sources & Audio

### Platform Support

| Platform | Full Screen | Application Window | Per-App Audio |
|----------|-------------|-------------------|---------------|
| **Web** | ✅ `getDisplayMedia()` | ✅ (browser-dependent) | ❌ System audio only |
| **Windows (Tauri)** | ✅ DXGI Desktop Duplication | ✅ Window handle capture | 🔮 Future (WASAPI) |
| **macOS (Tauri)** | ✅ ScreenCaptureKit | ✅ Window capture | ❌ Requires virtual driver |
| **Linux (Tauri)** | ✅ PipeWire/X11 | ✅ PipeWire window | 🔮 Future (PipeWire) |

### Audio Handling

| Scenario | Mic | Screen Audio | Viewer Experience |
|----------|-----|--------------|-------------------|
| Screen only (no audio) | ✅ Active | — | Hears sharer's voice |
| Screen + system audio | ✅ Active | ✅ Captured | Separate volume sliders |
| Sharer mutes mic | ❌ Muted | ✅ Captured | Hears screen audio only |

**Key point:** Sharer's mic is never auto-muted. They control their own mic.

### Track Structure

Each user can have up to 3 tracks:

```
User A in voice channel:
├── Audio Track (mic)           — always present when unmuted
├── Video Track (screen share)  — optional, when sharing
└── Audio Track (screen audio)  — optional, when sharing with audio
```

---

## SFU Changes

### Extended Track Metadata

```rust
#[derive(Clone, Debug)]
pub enum TrackKind {
    Audio,
    Video,
}

#[derive(Clone, Debug)]
pub enum TrackSource {
    Microphone,
    ScreenVideo,
    ScreenAudio,
    Webcam,  // Future
}

#[derive(Clone, Debug)]
pub struct TrackInfo {
    pub track_id: String,
    pub user_id: Uuid,
    pub kind: TrackKind,
    pub source: TrackSource,
    pub codec: String,
    pub label: Option<String>,
}
```

### Extended Peer Structure

```rust
pub struct Peer {
    pub user_id: Uuid,
    pub peer_connection: Arc<RTCPeerConnection>,

    // Multiple incoming tracks
    pub incoming_tracks: HashMap<TrackSource, TrackRemote>,

    // Outgoing tracks keyed by (source_user, source_type)
    pub outgoing_tracks: HashMap<(Uuid, TrackSource), TrackLocalStaticRTP>,

    pub muted: bool,
    pub signal_tx: mpsc::Sender<SignalMessage>,
}
```

### TrackRouter Extension

```rust
impl TrackRouter {
    // Existing
    pub fn subscribe_audio(&mut self, source_id: Uuid, subscriber: Subscription);

    // New
    pub fn subscribe_video(
        &mut self,
        source_id: Uuid,
        source_type: TrackSource,
        subscriber: Subscription
    );

    pub async fn forward_video_rtp(
        &self,
        source_id: Uuid,
        source_type: TrackSource,
        packet: &rtp::packet::Packet
    );
}
```

---

## Permissions & Limits

### Guild Permission (1 new bit)

```rust
bitflags! {
    pub struct GuildPermissions: u64 {
        // Existing bits 0-21...

        // New (bit 22)
        const SCREEN_SHARE = 1 << 22;
    }
}
```

### User-Level Feature Flags (new)

```sql
ALTER TABLE users ADD COLUMN feature_flags BIGINT NOT NULL DEFAULT 0;
```

```rust
bitflags! {
    pub struct UserFeatures: u64 {
        const PREMIUM_VIDEO = 1 << 0;  // 1080p60 access
    }
}
```

### Channel Settings

```rust
pub struct ChannelVoiceSettings {
    pub max_screen_shares: u32,  // Default 1
}
```

### DM Defaults

- Screen share always allowed for participants
- Default limit: 2 (both can share)
- Premium quality: requires `PREMIUM_VIDEO` user flag

### Limit Enforcement (Redis)

```rust
async fn try_start_screen_share(
    redis: &Redis,
    channel_id: Uuid,
    max_shares: u32,
) -> Result<(), ScreenShareError> {
    let key = format!("screenshare:limit:{}", channel_id);
    let count: u32 = redis.incr(&key).await?;

    if count > max_shares {
        redis.decr(&key).await?;
        return Err(ScreenShareError::LimitReached {
            current: count - 1,
            max: max_shares
        });
    }

    redis.expire(&key, 3600).await?;
    Ok(())
}
```

### Enforcement Flow

```
User clicks "Share Screen"
         │
         ▼
┌─────────────────────────────────┐
│ Is this a DM?                   │──── Yes ───▶ Skip permission check
└─────────────────────────────────┘
         │ No (Guild)
         ▼
┌─────────────────────────────────┐
│ Check SCREEN_SHARE permission   │──── No ───▶ Error: "No permission"
└─────────────────────────────────┘
         │ Yes
         ▼
┌─────────────────────────────────┐
│ Redis: INCR limit counter       │──── Over ──▶ Error: "Limit reached"
└─────────────────────────────────┘
         │ OK
         ▼
┌─────────────────────────────────┐
│ User requesting 1080p60?        │──── Yes ───▶ Check user.feature_flags
└─────────────────────────────────┘
         │ No / Has premium
         ▼
    Approved: Start capture
```

---

## Client Architecture

### Platform-Specific Capture

```typescript
interface VoiceAdapter {
    // New methods
    startScreenShare(options?: ScreenShareOptions): Promise<ScreenShareResult>;
    stopScreenShare(): Promise<void>;

    // New events
    onScreenShareStarted: (info: ScreenShareInfo) => void;
    onScreenShareStopped: (userId: string) => void;
    onScreenShareTrack: (userId: string, track: MediaStreamTrack) => void;
}

// Browser: Uses native picker
class BrowserVoiceAdapter implements VoiceAdapter {
    async startScreenShare(options?: ScreenShareOptions): Promise<ScreenShareResult> {
        const stream = await navigator.mediaDevices.getDisplayMedia({
            video: { cursor: 'always' },
            audio: options?.withAudio ?? false,
        });
        return { approved: true, stream };
    }
}

// Tauri: Custom picker with thumbnails
class TauriVoiceAdapter implements VoiceAdapter {
    async startScreenShare(options: ScreenShareOptions): Promise<ScreenShareResult> {
        return await invoke('start_screen_share', { options });
    }
}
```

### UI Components

```
Components
├── ScreenShareViewer.tsx      — Portal overlay for viewing
├── ScreenShareControls.tsx    — Integrated into VoiceIsland
└── TauriScreenPicker.tsx      — Tauri-only source picker

View Modes (Portal overlays)
├── Spotlight: Full overlay with small participant strip
├── PiP: Draggable floating panel
└── Theater: Wide overlay with chat visible on side
```

### Quality Settings (Pre-capture)

```
┌─────────────────────────────────┐
│ Screen Share Quality            │
│ ○ Auto (recommended)            │
│ ○ 720p 30fps                    │
│ ● 1080p 30fps                   │
│ ○ 1080p 60fps (Premium)         │
│                [Start Sharing]  │
└─────────────────────────────────┘
```

### Extended VoiceParticipant

```typescript
interface VoiceParticipant {
    user_id: string;
    username?: string;
    display_name?: string;
    muted: boolean;
    speaking: boolean;
    screen_sharing: boolean;  // New
}
```

---

## API & WebSocket Events

### REST Endpoints

```
POST /api/channels/:id/screenshare/check
  → Pre-capture permission + limit check
  Request:  { requested_quality?: Quality }
  Response: {
    allowed: boolean,
    granted_quality: Quality,
    error?: "no_permission" | "limit_reached" | "not_in_channel"
  }

POST /api/channels/:id/screenshare/start
  → Reserve slot + notify room
  Request:  { quality: Quality, has_audio: boolean, source_label: string }
  Response: { ok: boolean, error?: string }

POST /api/channels/:id/screenshare/stop
  → Release slot + notify room
  Response: { ok: boolean }

GET /api/guilds/:id/channels/:id
  → Includes voice_settings.max_screen_shares

PATCH /api/guilds/:id/channels/:id
  → Admin updates max_screen_shares
```

### WebSocket Events (Server → Client)

```typescript
interface VoiceRoomState {
  type: 'voice_room_state';
  channel_id: string;
  participants: VoiceParticipant[];
  screen_shares: ScreenShareInfo[];
}

interface ScreenShareStarted {
  type: 'screen_share_started';
  channel_id: string;
  user_id: string;
  username: string;
  source_label: string;
  has_audio: boolean;
  quality: Quality;
}

interface ScreenShareStopped {
  type: 'screen_share_stopped';
  channel_id: string;
  user_id: string;
  reason: 'user_stopped' | 'disconnected' | 'error';
}

interface ScreenShareQualityChanged {
  type: 'screen_share_quality_changed';
  channel_id: string;
  user_id: string;
  new_quality: Quality;
  reason: 'bandwidth' | 'cpu';
}
```

### Adaptive Quality (Server-Driven)

```rust
fn on_remb_feedback(&mut self, bitrate: u32) {
    let new_quality = match bitrate {
        b if b < 500_000 => Quality::Low,
        b if b < 1_500_000 => Quality::Medium,
        b if b < 4_000_000 => Quality::High,
        _ => Quality::Premium,
    };

    if new_quality != self.current_quality {
        self.request_encoder_quality(new_quality);
        self.broadcast_quality_changed(new_quality, "bandwidth");
    }
}
```

---

## Network Handling

### Poor Connection Behavior

- Automatic quality reduction with notification toast
- Server monitors bandwidth via REMB/TWCC feedback
- Quality changes are server-driven (no viewer requests in v1)

### Late Joiner Handling

- SFU requests PLI (Picture Loss Indication) from sharer
- Viewer sees loading state until keyframe received
- Timeout after 5 seconds with error message

---

## Implementation Phases

### Phase 1: Foundation

- Extend `Peer` struct for multiple tracks
- Add `TrackSource` enum and track metadata
- Register video codecs in MediaEngine
- Hardware encoder detection (Tauri)
- Add `users.feature_flags` column
- Add `SCREEN_SHARE` permission bit (bit 22)

### Phase 2: Server

- Extend TrackRouter for video tracks
- Track-level cleanup (separate from peer disconnect)
- Implement `/channels/:id/screenshare/*` endpoints
- Redis-based limit enforcement
- DM voice settings defaults (max_screen_shares: 2)
- WebSocket events (started/stopped/quality_changed)
- Keyframe (PLI) request on late-joiner subscribe
- Channel voice settings (max_screen_shares)

### Phase 3: Client

- Extend VoiceAdapter interface with screen share methods
- Browser: `getDisplayMedia()` integration
- Tauri: Platform capture + custom source picker
- Enable screen share button in VoiceIsland
- Quality settings panel
- Pre-capture permission check flow

### Phase 4: Viewer UI

- ScreenShareViewer component (Portal overlay)
- View modes (Spotlight/PiP/Theater)
- Separate volume controls (screen audio vs voice)
- Participant list screen share indicator

### Phase 5: Polish

- Adaptive quality via REMB feedback
- Quality change notifications (toast)
- Error recovery (renegotiation failures)
- Admin UI for channel voice settings

---

## Future Enhancements (Post-v1)

- **Webcam video** — Same track infrastructure, add `TrackSource::Webcam`
- **Simulcast** — Multiple quality layers from encoder for better adaptive
- **Per-app audio** — Windows WASAPI session filtering, Linux PipeWire routing
- **Zoom/pan** — Viewer controls for code review scenarios
- **Keyboard shortcuts** — Ctrl+Shift+1/2/3 for view modes
- **Sharer-controlled quality** — Option for presentation channels
