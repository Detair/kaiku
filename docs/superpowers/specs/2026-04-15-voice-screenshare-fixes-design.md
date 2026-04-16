# Voice & Screen Share Phase 1 — Critical Fixes

**Date:** 2026-04-15
**Status:** Draft
**Goal:** Address 8 ship-blocking issues in voice and screen share across server, web, Tauri desktop, and Android — Phase 1 of a two-phase plan.

## Context

Four parallel reviews (web client, Tauri desktop, Android mobile, cross-platform security) surfaced 28+ findings in the voice and screen share implementation. Eight are Critical: protocol violations, silent-upload risks, security gaps, missing media decode paths. This spec addresses all 8 as five independent PRs that can be developed in parallel.

The remaining 14 Important items (state cleanup, error UX harmonization, cross-platform drift) are deferred to Phase 2. The 5+ Minor polish items will be folded into whichever Phase 1 or Phase 2 PR touches the same file.

## Approach

Five independent PRs, parallelizable across worktrees. No cross-PR dependencies: no client change requires a server change (server already supports dual-PC signaling events and already expects VP8 PT=96), no web change affects Tauri or Android.

| PR | Branch | Fixes | Scope |
|----|--------|-------|-------|
| A | `fix/voice-server-security` | 1, 2, 3 | Medium (~200 lines Rust) |
| B | `fix/web-voice-ice-buffering` | 4 | Small (~30 lines TS) |
| C | `fix/tauri-voice-rtp-protocol` | 5, 6 | Small (~20 lines Rust) |
| D | `feat/tauri-vp8-decode` | 7 | Large (~600-900 lines Rust + libvpx dep + frontend canvas renderer) |
| E | `feat/android-publisher-pc` | 8 | Large (~200-300 lines Kotlin + signaling refactor) |

Testing gates stay consistent with prior audit-followup work:
- Server/Rust: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings && cargo test -p vc-server`
- Tauri: `cd client/src-tauri && cargo clippy -- -D warnings && cargo test`
- Web: `cd client && bun run test:run && bun run build`
- Android: `cd mobile/android && ./gradlew test`

---

## PR A — `fix/voice-server-security`

### Fix 1 — Self-mute enforcement with `server_muted` prep

**Problem:** When a user toggles `VoiceMute`, the server stores the flag on the peer but continues forwarding their RTP packets. Remote peers can still hear the "muted" user.

**Files:** `server/src/voice/peer.rs`, `server/src/voice/track.rs`, `server/src/voice/ws_handler.rs`

**Design:** The existing `Peer` struct at `server/src/voice/peer.rs:53` has `muted: RwLock<bool>`. Keep this primitive (async `RwLock`, matching the existing surrounding code). Rename it `self_muted`, rename the accessor/setter methods, and add a parallel `server_muted`:

```rust
pub struct Peer {
    // ... existing fields
    // Field visibility unchanged — both fields stay private, all access via methods.
    self_muted: RwLock<bool>,      // renamed from `muted`
    #[allow(dead_code)]            // set only by future moderation event
    server_muted: RwLock<bool>,
}

impl Peer {
    pub async fn is_self_muted(&self) -> bool { *self.self_muted.read().await }
    pub async fn set_self_muted(&self, muted: bool) { *self.self_muted.write().await = muted; }

    /// Returns true if either the user muted themselves or a moderator muted them.
    pub async fn is_effectively_muted(&self) -> bool {
        *self.self_muted.read().await || *self.server_muted.read().await
    }
}
```

Rename sites (verified via grep):
- `server/src/voice/peer.rs:193` — `pub async fn set_muted` → `pub async fn set_self_muted`
- `server/src/voice/peer.rs:199` — `pub async fn is_muted` → `pub async fn is_self_muted`
- `server/src/voice/ws_handler.rs:614` — `peer.set_muted(muted).await` → `peer.set_self_muted(muted).await`
- `server/src/voice/sfu.rs:214` — `peer.is_muted().await` → `peer.is_self_muted().await`

The `VoiceUserMuted` broadcast continues to reflect `self_muted` only (remote clients don't distinguish self-mute from server-mute in Phase 1 — that can come with the future moderation PR).

The `#[allow(dead_code)]` on `server_muted` suppresses the expected lint until a future `VoiceServerMute` handler writes to it.

**Forwarder integration:** `spawn_rtp_forwarder` at `track.rs:450` currently takes `source_user_id, source_type, layer, track, router`. Add `peer: Arc<Peer>` as a parameter so the mute check can happen at packet-read time without a router lookup on the hot path:

```rust
pub fn spawn_rtp_forwarder(
    source_user_id: Uuid,
    source_type: TrackSource,
    layer: Layer,
    track: Arc<TrackRemote>,
    router: Arc<TrackRouter>,
    peer: Arc<Peer>,  // NEW
) {
    tokio::spawn(async move {
        loop {
            match track.read(&mut buf).await {
                Ok((packet, _attributes)) => {
                    // Check mute before forwarding. Audio-only: mute does not
                    // apply to screen share or webcam video tracks, so skip
                    // check when source_type != TrackSource::Microphone.
                    if source_type == TrackSource::Microphone
                        && peer.is_effectively_muted().await
                    {
                        continue;
                    }
                    router.forward_rtp(source_user_id, source_type, layer, &packet).await;
                }
                Err(_) => break,
            }
        }
    });
}
```

Caller sites in `sfu.rs` (where `spawn_rtp_forwarder` is invoked on `on_track`) pass the `Arc<Peer>` from the incoming track's owner. `server_muted` has no setter until a future moderation PR adds `VoiceServerMute`.

**Async read on hot path performance note:** `self.self_muted.read().await` is a contention-free uncontended read (the only writer is the mute-event handler, which runs at human cadence). Tokio `RwLock::read()` uses an atomic fast-path when uncontended — performance is within the noise for 48kHz Opus at 50 packets/sec. No need to migrate to `AtomicBool` unless profiling shows otherwise; keeping `RwLock<bool>` preserves API consistency with the rest of the peer state.

**Test:** Add `server/tests/integration/voice_mute_enforcement.rs` (matches existing voice test convention, e.g., `server/tests/integration/voice_sfu.rs`). Joins two peers, the first sends N audio packets pre-mute (assert all N forwarded), issues `VoiceMute`, then sends M more packets (assert count does not increase). Avoids wall-clock flakiness by asserting on packet counts rather than time windows.

### Fix 2 — Rate limit voice signaling events

**Problem:** A malicious or buggy client can flood the server with `VoiceIceCandidate`, `VoicePublisherOffer`, `VoiceScreenShareStart/Stop`, `VoiceMute/Unmute` events, causing CPU exhaustion or signaling-table growth.

**Files:** `server/src/voice/rate_limit.rs`, `server/src/voice/ws_handler.rs`

**Design:** Add a new `TokenBucketLimiter` module alongside the existing `VoiceStatsLimiter`. The existing limiter is a fixed-interval gate for `VoiceStats` only and has different semantics; leave it in place.

Structure:

```rust
pub struct TokenBucket {
    capacity: u32,
    tokens: AtomicU32,
    refill_per_sec: u32,
    last_refill: Mutex<Instant>,
}

impl TokenBucket {
    /// Lazy refill: on every try_acquire, compute how many tokens should have
    /// been added since `last_refill` based on `refill_per_sec`, cap at `capacity`.
    /// Mutex-free fast path when uncontended; single short lock to update timestamp.
    pub fn try_acquire(&self) -> bool { /* ... */ }
}

pub struct VoiceRateLimiter {
    /// Per-PC buckets. Key is (peer_id, pc_type) for ICE, (peer_id, event_class) otherwise.
    buckets: DashMap<BucketKey, TokenBucket>,
}
```

**Bucket scope:** For `VoiceIceCandidate`, key by `(peer_id, pc_type)` so publisher and subscriber have independent buckets (halving would hurt restrictive-NAT cases that need both PCs to trickle aggressively). For other events, key by `(peer_id, event_class)`.

**Updated limits** (revised from reviewer feedback to accommodate restrictive-NAT burst patterns):

| Event | Bucket scope | Burst | Refill rate |
|-------|--------------|-------|-------------|
| `VoiceIceCandidate` | per-peer, per-pc_type | 200 | 40/sec |
| `VoicePublisherOffer` / `VoiceSubscriberAnswer` | per-peer | 5 | 1/sec (accommodates reconnect + renegotiation) |
| `VoiceScreenShareStart` / `VoiceScreenShareStop` | per-peer | 5 | 1 per 5 sec |
| `VoiceMute` / `VoiceUnmute` / `VoiceWebcamStart` / `VoiceWebcamStop` | per-peer | 10 | 2/sec |
| `VoiceSetLayerPreference` | per-peer | 20 | 5/sec |
| `VoiceStats` | handled by existing `VoiceStatsLimiter` | — | — |

Events exceeding the limit are silently dropped with `debug!` logging. The existing `VoiceError::RateLimited` variant has a specific message ("too many voice join requests") that doesn't fit general rate limiting. Generalize the error message or refactor to carry scope:

```rust
// server/src/voice/error.rs (current):
#[error("Rate limited: too many voice join requests")]
RateLimited,

// After (option 1 — simpler, generic message):
#[error("Rate limited")]
RateLimited,

// After (option 2 — carries scope, future-proof):
#[error("Rate limited: {0}")]
RateLimited(&'static str),  // e.g., VoiceError::RateLimited("ice_candidate")
```

Option 2 is preferable for observability (callers and users see which event class triggered). Migrate the one existing `RateLimited` call site in `VoiceStatsLimiter` to pass `"voice_stats"`.

Sustained violation policy unchanged: only surfaced to the client after > 10 consecutive drops of the same event class within 10 seconds, avoiding UI error flashes on legitimate bursts.

**Observability:** Expose a Prometheus counter `voice_rate_limit_drops_total{event_class,pc_type}` so baseline burst distributions can be measured in production before tuning thresholds further.

**Test:** Send 201 `VoiceIceCandidate` events on the publisher PC, assert 200 pass and 1 is dropped. Send 400 total (200/PC) and assert both 200-caps are independent. Send 10 `VoicePublisherOffer` in a loop at refill cadence, assert no drops (the reconnect case). Send 10 back-to-back, assert only 5 pass.

### Fix 3 — Screen share limiter counter leak on duplicate stream_id

**Problem:** The actual `ScreenShareLimiter` at `server/src/voice/screen_share.rs:163` is Redis-backed and keyed by `channel_id` (not `stream_id`) — it's a per-channel counter, not a per-stream-id reservation. The duplicate-stream-id check is already at `ws_handler.rs:770`. The real leak is ordering: `limiter.start(channel_id, max_shares)` runs at line 757, THEN the duplicate check runs at line 770. On a duplicate, the handler returns without calling `limiter.stop(channel_id)`, leaving the Redis counter incremented. After `max_shares` duplicates, the channel's counter is saturated and no one can start a legitimate screen share.

**Files:** `server/src/voice/ws_handler.rs` (swap order or add cleanup on duplicate), `server/src/voice/error.rs` (optional new variant for typed error)

**Design:** Swap the order so the duplicate check runs BEFORE `limiter.start()`:

```rust
// Before (server/src/voice/ws_handler.rs:754-774):
// let limiter = ...;
// if let Err(e) = limiter.start(params.channel_id, max_shares).await { return Err(...); }
// let stream_id = params.stream_id;
// if room.screen_shares.read().await.contains_key(&stream_id) {
//     return Err(VoiceError::Signaling(format!("Screen share stream already exists: {stream_id}")));
// }

// After — check duplicate FIRST, then reserve the slot:
let stream_id = params.stream_id;
if room.screen_shares.read().await.contains_key(&stream_id) {
    return Err(VoiceError::DuplicateStreamId);
}

let limiter = screen_share_limiter
    .ok_or_else(|| VoiceError::Signaling("Screen share limiter unavailable".to_string()))?;
if let Err(e) = limiter.start(params.channel_id, max_shares).await {
    warn!(user_id = %params.user_id, channel_id = %params.channel_id, error = ?e, "Screen share limit check failed");
    return Err(VoiceError::Signaling(match e {
        ScreenShareError::LimitReached => "Screen share limit reached".to_string(),
        ScreenShareError::InternalError => "Internal error".to_string(),
        _ => format!("{e:?}"),
    }));
}
```

Also add a typed `VoiceError::DuplicateStreamId` variant (with `#[error("Screen share stream already exists")]`) so the error is distinguishable without string matching.

**Alternative path considered and rejected:** Keep the current order and call `limiter.stop(channel_id)` on the duplicate path. Less clean (two mutations instead of one check), higher error surface if future refactors break the cleanup.

**Test:** Integration test that sends two `VoiceScreenShareStart` events with the same `stream_id`. Assert:
1. The second returns `Err(VoiceError::DuplicateStreamId)`.
2. The Redis counter's value (via a test-only accessor or fresh `limiter.start`) shows `max_shares - 1` slots still free (the first slot is the only one used).

### Commits

- `fix(voice): drop RTP for self-muted peers; add server_muted prep flag`
- `fix(voice): rate limit voice signaling events per peer`
- `fix(voice): reject duplicate screen share stream_id instead of leaking slot`

---

## PR B — `fix/web-voice-ice-buffering`

### Fix 4 — Queue ICE candidates until `setRemoteDescription` completes

**Problem:** `client/src/lib/webrtc/browser.ts:513` calls `pc.addIceCandidate()` immediately on receiving `VoiceIceCandidate`. The WebRTC spec requires remote description first. If the server sends candidates before or concurrently with the SDP answer (common on fast SFUs), `addIceCandidate` throws `InvalidStateError` and the candidate is permanently dropped. Under restrictive NATs (the exact scenario TURN serves), this causes silent connection failure.

**Files:** `client/src/lib/webrtc/browser.ts`

**Design:** Per-PC candidate queue covering both publisher and subscriber:

```typescript
interface PcState {
  pc: RTCPeerConnection;
  remoteDescriptionSet: boolean;
  pendingCandidates: RTCIceCandidateInit[];
}

private publisherState: PcState | null = null;
private subscriberState: PcState | null = null;
```

Replace the current direct-add path with buffered handling:

```typescript
private async handleIceCandidate(isPublisher: boolean, candidate: RTCIceCandidateInit) {
  const state = isPublisher ? this.publisherState : this.subscriberState;
  if (!state) return;

  if (!state.remoteDescriptionSet) {
    if (state.pendingCandidates.length >= MAX_PENDING_CANDIDATES) {
      console.warn("ICE candidate buffer full, dropping");
      return;
    }
    state.pendingCandidates.push(candidate);
    return;
  }

  try {
    await state.pc.addIceCandidate(candidate);
  } catch (err) {
    console.warn("Failed to add ICE candidate:", err);
    this.eventHandlers.onError?.(`ice_failed: ${err instanceof Error ? err.message : String(err)}`);
  }
}
```

In `handlePublisherAnswer` and `handleSubscriberOffer`, after `setRemoteDescription` succeeds, set the flag and drain:

```typescript
await pc.setRemoteDescription(desc);
state.remoteDescriptionSet = true;

const candidates = state.pendingCandidates.splice(0);
for (const candidate of candidates) {
  try {
    await pc.addIceCandidate(candidate);
  } catch (err) {
    console.warn("Drained ICE candidate failed:", err);
  }
}
```

Reset `remoteDescriptionSet = false` and `pendingCandidates = []`:
- On each new PC creation (join)
- On explicit leave
- **Before every `setRemoteDescription` call** (ICE restart / renegotiation). Without this, candidates arriving between a new offer and its remote-description application bypass buffering.

On renegotiation, any candidates that were queued against the previous session are dropped — they're invalid for the new ICE generation (the credentials in `ufrag`/`pwd` change) and would be rejected by the PC. If the client needs the same candidates for the new generation, the remote will re-send them via the standard trickle path.

Cap at 100 candidates — matches the Android `MAX_PENDING_CANDIDATES` — with a warning log on overflow.

**Test:** Unit tests covering both PCs using a fake `RTCPeerConnection`:
- Send 5 candidates before `setRemoteDescription` on publisher PC, trigger it, assert all 5 are applied.
- Repeat for subscriber PC.
- Send 105 candidates with no remote description set, assert only 100 are queued and a warning logs.
- Trigger a second `setRemoteDescription` (renegotiation), send candidates in between, assert the new candidates re-buffer correctly.

### Commit

- `fix(client): buffer ICE candidates until remote description is set (voice critical)`

---

## PR C — `fix/tauri-voice-rtp-protocol`

### Fix 5 — Per-session RTP sequence number and timestamp

**Problem:** `client/src-tauri/src/commands/voice.rs:720-721` declares static `AtomicU16`/`AtomicU32` for sequence number and timestamp. These persist for the process lifetime and are never reset between voice sessions. SSRC rotates (derived from `SystemTime::now()` at task spawn), so receivers see `{new SSRC, continued seq}` — RFC 3550 violation. Receivers interpret packets as continuation of the old session; packet-loss concealment fires for the entire "gap" duration, producing audio corruption or silence at the start of every reconnect.

**Files:** `client/src-tauri/src/commands/voice.rs`, `client/src-tauri/Cargo.toml` (if `rand` is not a direct dep)

**Design:** Replace statics with per-call locals initialized at the start of `send_audio_to_track`:

```rust
async fn send_audio_to_track(/* ... */) {
    // Start sequence number and timestamp at random per RFC 3550 §5.1.
    // SSRC is already per-session (derived from SystemTime at spawn), so
    // combining with a random seq start ensures receivers see a fresh stream.
    let mut seq: u16 = rand::random();
    let mut timestamp: u32 = rand::random();

    while let Some(frame) = audio_rx.recv().await {
        // encode to Opus
        let packet = rtp::packet::Packet {
            header: rtp::header::Header {
                version: 2,
                payload_type: OPUS_PAYLOAD_TYPE,
                sequence_number: seq,
                timestamp,
                ssrc,
                marker: false,
                ..Default::default()
            },
            payload: opus_bytes.into(),
        };

        // write packet

        seq = seq.wrapping_add(1);
        timestamp = timestamp.wrapping_add(SAMPLES_PER_FRAME as u32);
    }
}
// Use the existing SAMPLES_PER_FRAME constant at client/src-tauri/src/commands/voice.rs:724
```

The function is single-task (one sender per session), so atomics add no value — plain mutable locals are correct. Remove the `use std::sync::atomic::{AtomicU16, AtomicU32, Ordering};` imports if this is their only consumer.

`rand::random::<u16>()` and `rand::random::<u32>()` work without seeding. `rand` is a transitive dep via `webrtc`; if not already a direct dep in `src-tauri/Cargo.toml`, add `rand = "0.8"`.

### Fix 6 — VP8 RTP packetizer sets correct payload type

**Problem:** `client/src-tauri/src/video/rtp.rs:80-88` constructs the RTP header with `Header { ..Default::default() }` inside `VideoRtpSender::send_packet`. `Default::default()` leaves `payload_type: 0` and `ssrc: 0`. The SDP advertises VP8 as PT 96. With PT=0 on the wire, the SFU either rejects the packets or misroutes them through the audio codec path. Screen share and webcam video transmit but never arrive.

**Files:** `client/src-tauri/src/video/rtp.rs`, `client/src-tauri/src/webrtc/mod.rs`

**Design:** Preserve the existing `VideoRtpSender::send_packet(&self, packet: &EncodedPacket)` API and its internal fragmentation loop. The fix is two small additions inside `send_packet`:

1. Store a stable SSRC as a `VideoRtpSender` field, initialized in `new()`.
2. Set `payload_type: VP8_PAYLOAD_TYPE` and `ssrc: self.ssrc` explicitly in the header construction (currently line 80-87 uses `..Default::default()` which leaves both at 0).

```rust
pub const VP8_PAYLOAD_TYPE: u8 = 96;

pub struct VideoRtpSender {
    track: Arc<TrackLocalStaticRTP>,
    seq: AtomicU16,
    ssrc: u32,  // NEW — stable per sender instance
}

impl VideoRtpSender {
    pub fn new(track: Arc<TrackLocalStaticRTP>) -> Self {
        Self {
            track,
            seq: AtomicU16::new(rand::random()),  // start at random per RFC 3550 §5.1
            ssrc: rand::random(),
        }
    }

    pub async fn send_packet(&self, packet: &EncodedPacket) -> Result<(), VideoError> {
        // ... existing fragmentation loop unchanged, except header now sets
        // payload_type and ssrc explicitly:
        let rtp_packet = RtpPacket {
            header: Header {
                version: 2,
                payload_type: VP8_PAYLOAD_TYPE,  // was: 0 (defaulted)
                marker: is_last,
                sequence_number: seq,
                timestamp,
                ssrc: self.ssrc,                 // was: 0 (defaulted)
                ..Default::default()
            },
            payload: payload.into(),
        };
        // ... rest of loop unchanged
    }
}
```

The existing `AtomicU16` for `seq` stays — it's per-instance (not process-static), so it's correctly reset on each new `VideoRtpSender`. Only the initial value changes from `0` to a random start (RFC 3550 §5.1).

**API preservation note:** Keeping `&self` and `&EncodedPacket` means no call-site changes at `client/src-tauri/src/commands/screen_share.rs:196` or `client/src-tauri/src/commands/webcam.rs:151`. The fragmentation loop, marker bit handling, and `build_vp8_payload_descriptor` logic all remain intact.

Export `VP8_PAYLOAD_TYPE` from `video/rtp.rs` and reference it from the media engine registration in `webrtc/mod.rs` (currently hardcoded 96) so the Tauri-side sites can't drift. Cross-stack coordination with the Rust server's media engine (which also hardcodes 96) is out of scope for this PR — a future cleanup can hoist the constant into `shared/vc-common` if desired.

### Commits

- `fix(voice): reset RTP seq/timestamp per session to comply with RFC 3550`
- `fix(voice): set VP8 payload_type=96 on outbound video RTP`

### Testing

Manual verification (unit tests too RTP-layer internal for practical coverage):
1. Join voice on desktop, speak, have another client listen → audio is clear at session start (Fix 5 regression check).
2. Start screen share on desktop → another client receives the video frames (Fix 6 regression check; currently silently broken).

---

## PR D — `feat/tauri-vp8-decode`

### Fix 7 — Native VP8 decode with IPC-to-webview frame rendering

**Problem:** `client/src-tauri/src/voice/video_decoder.rs` is a stub that emits lifecycle events but never decodes frames. The Rust-side subscriber PeerConnection receives VP8 RTP packets for remote screen shares but has no decoder, so the frontend never sees frames. Desktop users see a screen-share tile with a permanent spinner.

**Files:**
- `client/src-tauri/Cargo.toml` — add `env-libvpx-sys = "4"` (matches the version already pinned by transitive dep `vpx-encode 0.3.0`, verified in `Cargo.lock` as `env-libvpx-sys 4.0.13`; pinning a higher major would create a dual-version conflict at link time)
- `client/src-tauri/src/voice/video_decoder.rs` — implement VP8 decode pipeline
- `client/src-tauri/src/voice/rtp_depacketizer.rs` (new) — VP8 RTP depacketizer
- `client/src-tauri/src/voice/frame_buffer.rs` (new) — frame pool and IPC binary encoder
- `client/src/lib/voice/nativeVideoRenderer.ts` (new) — frontend canvas renderer
- `client/src/components/voice/NativeScreenShareTile.tsx` (new or modified) — swap `<video>` for `<canvas>` when Tauri
- `client/src-tauri/src/commands/voice.rs` — expose decode lifecycle commands

### Decoder architecture

```
VP8 RTP packets (from subscriber PC)
    → depacketize into VP8 frames
    → libvpx decode → YUV I420
    → emit to webview via Tauri event
        → frontend receives frame event
        → upload to WebGL2 YUV textures
        → YUV→RGB fragment shader renders to canvas
```

### Dependency choice

**`env-libvpx-sys = "4"`** — the low-level Rust binding already used transitively by `vpx-encode = "0.3"` (already in `Cargo.toml`; current lockfile version is `env-libvpx-sys 4.0.13`). Using the same major version avoids a dual-linkage conflict when two sys crates both statically link libvpx. Reuses the project's existing CI workarounds:
- Windows: vcpkg renames `libvpx → vpx.lib`; handled in `.github/workflows/tauri-build.yml:91`.
- Documented in `docs/developer-guide/development/ci.md:49`.

Alternatives considered and rejected:
- `vpx = "0.3"`: abandoned (last update 2015); will not compile against current libvpx.
- `media-codec-vpx = "0.8"`: higher-level wrapper but introduces additional transitive deps for a decoder we can drive directly.
- Pure-Rust VP8: no maintained implementation exists.
- `dav1d`: AV1-only.

License: BSD-3-Clause (compatible with CLAUDE.md dual MIT/Apache-2.0 per the allow-list).

### IPC format — YUV I420 as binary ArrayBuffer events

Decoded YUV I420 frames are sent from Rust to the webview using Tauri 2.x's binary event payload API. **Do not base64-encode** — the reviewer's analysis flagged base64's 33% overhead plus UTF-16 string conversion on the webview side as a practical CPU bottleneck, particularly for the JSON event serialization path.

At 1920×1080: Y = 2,073,600 bytes; U = V = 518,400 bytes; total = 3,110,400 bytes (~2.97 MiB/frame). At 30 fps: ~93 MB/sec sustained raw binary IPC per stream. Benchmark before accepting the architecture — WebView2 (Windows), WebKit (Linux), WKWebView (macOS) have different practical IPC ceilings, and 6 concurrent streams at 1080p30 approaches ~560 MB/sec which may exceed platform limits.

If benchmark shows IPC is the bottleneck, the fallback is to downscale server-side via `VoiceSetLayerPreference` (request the Low simulcast layer) or to render at reduced framerate on the desktop (drop every other frame).

WebGL2 fragment shader converts I420 → RGB in a single fragment pass using **`gl.R8` / `gl.RED` textures** (WebGL2 native formats, better optimized than the legacy `LUMINANCE`). Modern GPUs handle this at 60fps trivially.

### Rust decode pipeline

New file `client/src-tauri/src/voice/rtp_depacketizer.rs`:

```rust
pub struct Vp8Depacketizer {
    frame_buffer: Vec<u8>,
    last_seq: Option<u16>,
    expecting_keyframe: bool,
}

impl Vp8Depacketizer {
    pub fn new() -> Self { /* ... */ }

    /// Feed an RTP packet. Returns a complete VP8 frame if one is assembled.
    /// Follows RFC 7741:
    /// - Parse VP8 payload descriptor (first byte, optional extension)
    /// - Accumulate fragments until marker bit set
    /// - Reset on seq gap (frame corrupted — relies on existing interval PLI
    ///   sender for recovery; no new PLI-request plumbing added)
    /// - Extract keyframe bit from VP8 frame header
    pub fn depacketize(&mut self, packet: &rtp::packet::Packet) -> Option<Vec<u8>> { /* ... */ }
}
```

Modified `video_decoder.rs`:

```rust
// NOTE ON THE CODE SAMPLE BELOW:
// env-libvpx-sys is a raw FFI crate exposing C symbols like
// `vpx_codec_dec_init_ver`, `vpx_codec_decode`, `vpx_codec_get_frame`.
// It does NOT ship a Rust `Decoder` type. The `vpx::Decoder` names below
// are placeholder prose for "a thin safe shim we implement in this file
// that wraps unsafe env_libvpx_sys FFI calls". Expect ~150-200 lines of
// unsafe wrapper code (init, decode, get_frame iteration, error mapping,
// destroy-on-drop). Reference: see `vpx-encode 0.3.0` source for the
// parallel Encoder wrapper pattern (that crate is already in the tree
// and demonstrates the exact shape of the shim).

pub struct Vp8VideoDecoder {
    ctx: vpx_codec_ctx_t,          // raw FFI struct, zeroed then initialized
    depacketizer: Vp8Depacketizer,
    stream_id: String,
    frame_sink: mpsc::Sender<DecodedFrame>,
}

pub struct DecodedFrame {
    pub stream_id: String,
    pub width: u32,
    pub height: u32,
    pub y_plane: Vec<u8>,
    pub u_plane: Vec<u8>,
    pub v_plane: Vec<u8>,
    pub y_stride: u32,
    pub uv_stride: u32,
    pub pts_ms: u64,
}

impl Vp8VideoDecoder {
    pub fn new(stream_id: String, frame_sink: mpsc::Sender<DecodedFrame>) -> Result<Self> {
        let mut ctx = unsafe { std::mem::zeroed() };
        let iface = unsafe { vpx_codec_vp8_dx() };
        let res = unsafe {
            vpx_codec_dec_init_ver(
                &mut ctx,
                iface,
                std::ptr::null(),
                0,
                VPX_DECODER_ABI_VERSION as i32,
            )
        };
        if res != VPX_CODEC_OK {
            return Err(VideoError::DecoderInit(res));
        }
        Ok(Self { ctx, depacketizer: Vp8Depacketizer::new(), stream_id, frame_sink })
    }

    pub async fn process_packet(&mut self, packet: &rtp::packet::Packet) -> Result<()> {
        if let Some(frame_bytes) = self.depacketizer.depacketize(packet) {
            let res = unsafe {
                vpx_codec_decode(
                    &mut self.ctx,
                    frame_bytes.as_ptr(),
                    frame_bytes.len() as u32,
                    std::ptr::null_mut(),
                    0,
                )
            };
            if res != VPX_CODEC_OK {
                return Err(VideoError::Decode(res));
            }

            // Iterate decoded frames via vpx_codec_get_frame
            let mut iter: vpx_codec_iter_t = std::ptr::null();
            loop {
                let img = unsafe { vpx_codec_get_frame(&mut self.ctx, &mut iter) };
                if img.is_null() { break; }
                let yuv_frame = unsafe { yuv_image_to_decoded_frame(img, &self.stream_id) };
                let _ = self.frame_sink.send(yuv_frame).await;
            }
        }
        Ok(())
    }
}

impl Drop for Vp8VideoDecoder {
    fn drop(&mut self) {
        unsafe { vpx_codec_destroy(&mut self.ctx); }
    }
}
```

The `yuv_image_to_decoded_frame` helper copies the Y/U/V planes out of the borrowed `vpx_image_t` into owned `Vec<u8>` buffers (the `vpx_image_t` becomes invalid after the next `vpx_codec_get_frame` call, so copying is mandatory). The decoder is not `Send + Sync` due to the raw FFI context; drive it from a single tokio task per stream.

A single background task consumes `frame_sink` and emits Tauri events (`voice:video_frame`) to the frontend. The `mpsc::Sender<DecodedFrame>` uses `mpsc::channel(3)` — bounded capacity of 3 frames. If the emitter task falls behind, newer frames displace older ones via explicit drop-oldest policy (`try_send` returning `Full` → dequeue one and retry). This costs at most 100ms of latency at 30fps steady state and bounds memory to ~9 MB of buffered I420 per stream.

### Lifecycle

`Vp8VideoDecoder` is created when the subscriber PC receives a new VP8 track (existing hook in `voice/subscriber.rs`), destroyed when the track ends. The stub's current lifecycle events are preserved so the frontend still knows when a decode-capable stream starts/stops — it just now produces actual frames.

### Frontend canvas renderer

New file `client/src/lib/voice/nativeVideoRenderer.ts`:

```typescript
/**
 * Renders YUV I420 frames from Tauri native decode to a canvas using
 * WebGL2 with a YUV→RGB fragment shader.
 */
export class NativeYuvRenderer {
  private gl: WebGL2RenderingContext;
  private program: WebGLProgram;
  private yTexture: WebGLTexture;
  private uTexture: WebGLTexture;
  private vTexture: WebGLTexture;

  constructor(canvas: HTMLCanvasElement) {
    this.gl = canvas.getContext("webgl2")!;
    // compile YUV→RGB fragment shader, set up textures + VBOs
  }

  renderFrame(frame: DecodedFrame) {
    // Upload Y, U, V planes to respective R8 single-channel textures
    // Draw fullscreen quad with YUV→RGB shader
  }

  dispose() {
    // Clean up GL resources
  }
}
```

Modified `NativeScreenShareTile.tsx`:

```tsx
<Show when={isTauri()}>
  <canvas ref={canvasRef} class="w-full h-full object-contain" />
</Show>
<Show when={!isTauri()}>
  <video srcObject={videoTrack} autoplay muted />
</Show>
```

A Solid.js `createEffect` subscribes to Tauri events for the stream's `voice:video_frame` events and calls `renderer.renderFrame(decoded)`. Cleanup tears down the WebGL context.

### Testing

- **Rust unit:** `Vp8Depacketizer` tests using recorded RTP fixtures. Ship a fixture-capture script at `client/src-tauri/tests/capture_vp8_fixture.rs` that spawns a minimal libvpx encoder → RTP packetizer → writes to `tests/fixtures/vp8_sample.rtp`. Run once during setup; the `.rtp` file is committed. Tests assert frame assembly produces correct keyframe/delta counts and correct VP8 frame bytes.
- **Rust integration:** `Vp8VideoDecoder` test feeding the fixture file, asserts `DecodedFrame` is emitted with correct dimensions. Gate behind `#[cfg_attr(not(feature = "decode-integration"), ignore)]` so CI can opt in/out of the slower path.
- **Frontend:** No automated tests (WebGL canvas tests are flaky). Manual verification: start screen share on browser client, desktop client displays video in the canvas tile.

### Commits

- `feat(voice): add VP8 RTP depacketizer`
- `feat(voice): implement native VP8 decode via libvpx`
- `feat(voice): emit decoded YUV frames to webview via Tauri events`
- `feat(voice): add WebGL YUV renderer for native video frames`
- `feat(voice): use canvas for screen share tiles in Tauri mode`

### Risks and open questions

- **libvpx build reproducibility:** libvpx's autoconf-based build has flaked other projects' CI. Using `env-libvpx-sys = "4"` reuses the project's existing CI workarounds (vcpkg naming quirk on Windows, documented in `docs/developer-guide/development/ci.md:49`). No new build dance should be needed.
- **IPC performance at 4K:** 4K screen share at 30fps = 360 MB/sec. May push platform IPC limits. If 4K testing reveals issues, downscale server-side via `VoiceSetLayerPreference`.
- **Memory footprint:** Each active decoder holds ~10MB at 1080p. Six concurrent shares = 60MB. Within budget.
- **VP8 keyframe loss recovery:** Existing PLI interval sender handles this. No new work.

---

## PR E — `feat/android-publisher-pc`

### Fix 8 — Add publisher PeerConnection for dual-PC parity

**Problem:** Android creates a single `PeerConnection`, adds the local mic track, and responds to the server's offer with an answer. The server uses a dual-PC model (publisher PC for outbound, subscriber PC for inbound). When the subscriber PC's offer is `recvonly` from the server's perspective, the mic track added via `addTrack` is silently dropped in SDP negotiation. Android users may not be transmitting audio at all.

**Files:**
- `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt` — extend with dual-PC management
- `mobile/android/app/src/main/java/io/wolftown/kaiku/data/repository/VoiceRepository.kt` — signal flow changes
- `mobile/android/app/src/main/java/io/wolftown/kaiku/data/ws/ClientEvent.kt` — add `VoicePublisherOffer`, `VoiceSubscriberAnswer`; add `pcType` field to existing `VoiceIceCandidate`
- `mobile/android/app/src/main/java/io/wolftown/kaiku/data/ws/ServerEvent.kt` — add `VoicePublisherAnswer`, `VoiceSubscriberOffer`; **add `pcType` field to existing `VoiceIceCandidate` (currently at line 100 without it)**
- `mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt` — extended tests

### State model

Extend `WebRtcManager` to hold both PCs with explicit `publisher`/`subscriber` prefixes:

```kotlin
@Singleton
class WebRtcManager @Inject constructor(
    @ApplicationContext private val context: Context,
    private val voiceApi: VoiceApi
) {
    companion object {
        private val logger = Logger.getLogger("WebRtcManager")
        private const val LOCAL_AUDIO_TRACK_ID = "kaiku-local-audio"
        private const val MAX_PENDING_CANDIDATES = 100
    }

    private val initMutex = Mutex()
    private var factory: PeerConnectionFactory? = null

    // Publisher PC — uploads local mic to SFU
    @Volatile private var publisherPc: PeerConnection? = null
    private var publisherRemoteDescriptionSet = false
    private val publisherPendingCandidates = mutableListOf<String>()

    // Subscriber PC — receives all remote media from SFU
    @Volatile private var subscriberPc: PeerConnection? = null
    private var subscriberRemoteDescriptionSet = false
    private val subscriberPendingCandidates = mutableListOf<String>()

    private var audioSource: AudioSource? = null
    private var audioDeviceModule: AudioDeviceModule? = null
    var localAudioTrack: AudioTrack? = null
        private set

    private val _remoteAudioTracks = MutableStateFlow<Map<String, AudioTrack>>(emptyMap())
    val remoteAudioTracks: StateFlow<Map<String, AudioTrack>> = _remoteAudioTracks.asStateFlow()
    private val _remoteVideoTracks = MutableStateFlow<Map<String, VideoTrack>>(emptyMap())
    val remoteVideoTracks: StateFlow<Map<String, VideoTrack>> = _remoteVideoTracks.asStateFlow()

    var isMuted: Boolean = false
        private set

    @Volatile var onPublisherOffer: ((String) -> Unit)? = null
    @Volatile var onPublisherIceCandidate: ((String) -> Unit)? = null
    @Volatile var onSubscriberAnswer: ((String) -> Unit)? = null
    @Volatile var onSubscriberIceCandidate: ((String) -> Unit)? = null
    @Volatile var onError: ((String) -> Unit)? = null
}
```

### Publisher PC flow (new)

Android initiates on `joinChannel`:
1. Create `publisherPc` with one `sendonly` audio transceiver
2. Add `localAudioTrack` to the transceiver
3. Create offer, `setLocalDescription`
4. Invoke `onPublisherOffer(sdp)` → `VoiceRepository` sends `VoicePublisherOffer` over WS
5. Receive `VoicePublisherAnswer` from server
6. `setRemoteDescription(answer)` → set `publisherRemoteDescriptionSet = true` → drain `publisherPendingCandidates`
7. Local ICE candidates fire `onPublisherIceCandidate(json)` → WS `VoiceIceCandidate { pc_type: "publisher" }`

```kotlin
suspend fun createPublisherOffer() {
    val pcFactory = factory ?: throw IllegalStateException("Factory not initialized")

    publisherRemoteDescriptionSet = false
    publisherPendingCandidates.clear()

    val iceConfig = fetchIceServers()
    val rtcConfig = PeerConnection.RTCConfiguration(iceConfig).apply {
        sdpSemantics = PeerConnection.SdpSemantics.UNIFIED_PLAN
        continualGatheringPolicy = PeerConnection.ContinualGatheringPolicy.GATHER_CONTINUALLY
    }
    publisherPc = pcFactory.createPeerConnection(rtcConfig, createPublisherObserver())
        ?: throw IllegalStateException("Failed to create publisher PC")

    audioSource = pcFactory.createAudioSource(MediaConstraints())
    localAudioTrack = pcFactory.createAudioTrack(LOCAL_AUDIO_TRACK_ID, audioSource).also {
        it.setEnabled(!isMuted)
    }

    publisherPc?.addTransceiver(
        localAudioTrack,
        RtpTransceiver.RtpTransceiverInit(RtpTransceiver.RtpTransceiverDirection.SEND_ONLY)
    )

    publisherPc?.createOffer(object : SdpObserverAdapter("createPublisherOffer", onError) {
        override fun onCreateSuccess(desc: SessionDescription) {
            publisherPc?.setLocalDescription(object : SdpObserverAdapter("setPublisherLocalDesc", onError) {
                override fun onSetSuccess() {
                    onPublisherOffer?.invoke(desc.description)
                }
            }, desc)
        }
    }, MediaConstraints())
}

suspend fun handlePublisherAnswer(sdp: String) {
    val pc = publisherPc ?: return
    val answer = SessionDescription(SessionDescription.Type.ANSWER, sdp)
    pc.setRemoteDescription(object : SdpObserverAdapter("setPublisherRemoteDesc", onError) {
        override fun onSetSuccess() {
            publisherRemoteDescriptionSet = true
            drainPublisherCandidates()
        }
    }, answer)
}
```

### Subscriber PC flow (existing, renamed)

The existing `handleOffer` logic moves to `handleSubscriberOffer`, creating the subscriber PC and responding with an answer. Semantics unchanged; callback names renamed.

### ICE candidate routing

ICE candidates are PC-specific. Extend `VoiceIceCandidate` with a `pc_type` field:

```kotlin
@Serializable
@SerialName("voice_ice_candidate")
data class VoiceIceCandidate(
    val channelId: String,
    val candidate: String,
    val pcType: String  // "publisher" or "subscriber"
) : ClientEvent()
```

Server-side already has `pc_type` in `VoiceIceCandidate` events. Android currently omits the field; adding it is a protocol-compatible extension.

Incoming candidates route by `pc_type`:

```kotlin
fun addIceCandidate(pcType: String, candidateJson: String) {
    val (pc, candidateBuffer, remoteSet) = when (pcType) {
        "publisher" -> Triple(publisherPc, publisherPendingCandidates, publisherRemoteDescriptionSet)
        "subscriber" -> Triple(subscriberPc, subscriberPendingCandidates, subscriberRemoteDescriptionSet)
        else -> {
            logger.warning("Unknown pc_type: $pcType")
            return
        }
    }
    if (!remoteSet) {
        if (candidateBuffer.size >= MAX_PENDING_CANDIDATES) {
            logger.warning("ICE candidate buffer full for $pcType, dropping")
            return
        }
        candidateBuffer.add(candidateJson)
        return
    }
    // parse and add to pc
}
```

### VoiceRepository signaling changes

Join flow:
1. `joinChannel(channelId)` → WS `VoiceJoin(channelId)`
2. `webRtcManager.initialize()` + `createPublisherOffer()` (Android-initiated)
3. Send `VoicePublisherOffer(channelId, sdp)` via WS
4. Receive `VoicePublisherAnswer` → `handlePublisherAnswer(sdp)`
5. Receive `VoiceSubscriberOffer` (server-initiated, after peer admission) → `handleSubscriberOffer(sdp)` → creates subscriber PC, responds with `VoiceSubscriberAnswer(channelId, sdp)`
6. Both PCs reach ICE `Connected` → emit `Connected` state

`VoiceRepository.connectionState` transitions to `Connected` only when both `publisherPc` and `subscriberPc` report ICE connected:

```kotlin
private val publisherIceState = MutableStateFlow<PeerConnection.IceConnectionState?>(null)
private val subscriberIceState = MutableStateFlow<PeerConnection.IceConnectionState?>(null)

val voiceIceConnected: StateFlow<Boolean> = combine(publisherIceState, subscriberIceState) { p, s ->
    p == PeerConnection.IceConnectionState.CONNECTED &&
    s == PeerConnection.IceConnectionState.CONNECTED
}.stateIn(scope, SharingStarted.Eagerly, false)
```

### Cleanup

`closePeerConnection()` becomes `closePeerConnections()`, applying the null-first pattern to both:

```kotlin
fun closePeerConnections() {
    localAudioTrack?.dispose()
    localAudioTrack = null
    audioSource?.dispose()
    audioSource = null

    _remoteAudioTracks.value = emptyMap()
    _remoteVideoTracks.value = emptyMap()
    publisherRemoteDescriptionSet = false
    subscriberRemoteDescriptionSet = false
    publisherPendingCandidates.clear()
    subscriberPendingCandidates.clear()

    val pub = publisherPc
    publisherPc = null
    pub?.close()
    pub?.dispose()

    val sub = subscriberPc
    subscriberPc = null
    sub?.close()
    sub?.dispose()

    logger.info("Publisher and subscriber PeerConnections closed")
}
```

The `leaveMutex` in `VoiceRepository` continues to protect concurrent access.

### Tests

- `WebRtcManagerTest`: publisher-side tests mirroring existing subscriber tests — offer creation, local description set, answer handling, ICE candidate buffering with `pc_type=publisher`.
- Integration-style test exercising the full join flow with a mock server sending both `VoicePublisherAnswer` and `VoiceSubscriberOffer`, asserts `voiceIceConnected` transitions correctly.

### Commits

- `refactor(voice): rename PC methods for publisher/subscriber clarity`
- `feat(voice): add publisher PeerConnection for mic upload`
- `feat(voice): route ICE candidates by pc_type across publisher and subscriber`
- `feat(voice): gate Connected state on both PCs' ICE connected`
- `test(voice): add publisher PC unit tests`

### Thread safety (explicit carry-over from round-2 audit)

The existing single-PC implementation already has non-`@Volatile` `remoteDescriptionSet` and non-synchronized `pendingCandidates`. This spec duplicates that pattern for the publisher PC rather than fixing it now — the race is identical in scope and cost, and the full fix is already in Phase 2's tracking list ("Android correctness: non-Volatile `remoteDescriptionSet`/`pendingCandidates` thread safety"). Intentionally deferred here to keep PR E focused on the dual-PC architecture change.

**Explicitly noted for the implementer:** If you touch this code path, resist the urge to fix the thread safety here — it belongs in Phase 2 where it can be applied consistently to both PCs in one reviewable chunk. Document the assumption via a comment:

```kotlin
// NOTE: publisherRemoteDescriptionSet and publisherPendingCandidates are
// mutated from both the WebRTC signaling thread (via Observer callbacks) and
// the IO dispatcher (via addIceCandidate from WS events). Phase 2 audit will
// add @Volatile + synchronization for both PCs consistently.
```

### Audio source creation — single site

The existing implementation creates `audioSource` + `localAudioTrack` in the single-PC `createPeerConnection()` path. Post-refactor, these are created only in `createPublisherOffer()` (the new method). The old creation site is removed. The subscriber PC no longer adds tracks (it's purely `recvonly` from the server's perspective, answered with standard SDP negotiation).

### Risks

- **Backwards compatibility:** The current single-PC flow may work today if the server sends `sendrecv` offers. The new publisher PC fully replaces the `addTrack` on the subscriber PC. Manual A/B test: before and after, verify mic arrives at the server from Android.
- **Signaling ordering:** Current flow is server-first (server offers, client answers). The new publisher flow is client-first. Server handlers for `VoicePublisherOffer` already exist; confirm they don't require new server state.
- **`addTransceiver` availability:** `stream-webrtc-android:1.3.0` supports `addTransceiver`; no version bump needed.
- **Connection state transition:** After the refactor, `VoiceRepository.connectionState` transitions to `Connected` only when BOTH PCs' ICE is `Connected`. Existing code (which gates on single-PC ICE) is removed in this PR to avoid dual-signal ambiguity. Add `TestCoroutineDispatcher`-based unit tests in `VoiceRepositoryTest` to verify state propagation through the `combine` operator.

---

## Deferred to Phase 2

14 Important findings are out of scope for Phase 1. A follow-up spec will address them; rough groupings:

- **State cleanup across clients:** web `leaveVoice` missing mute/deafen reset, `screenShareViewer.clearAll` never called, user-stopped screen share not notifying server, `track.onended` handler conflicts
- **Error UX harmonization:** ICE failures invisible on all clients, mid-stream media failures silent
- **Tauri stability:** mutex in CPAL callback, webcam busy-wait, WS token refresh on reconnect, session restore re-registering voice listeners
- **Android correctness:** non-Volatile `remoteDescriptionSet`/`pendingCandidates` thread safety, foreground service kill recovery, `detectAvailableRoutes` overriding manual selection, stream-ID substring match fragility
- **Cross-platform drift:** token refresh, error propagation, reconnect behavior implemented differently on each client

---

## CHANGELOG updates per PR

Per `CLAUDE.md`: "Jede benutzerrelevante Änderung MUSS in CHANGELOG.md unter [Unreleased] dokumentiert werden." Fixes that are user-visible:

| PR | Section | Entry |
|----|---------|-------|
| A | `### Security` | Self-muted users' audio is now actually dropped at the server rather than being forwarded to listeners |
| A | `### Fixed` | Screen share slots are no longer leaked by duplicate stream IDs |
| A | `### Security` | Voice signaling events are rate-limited per peer to prevent flooding |
| B | `### Fixed` | Browser voice connections no longer fail under restrictive NATs due to ICE candidate race (candidates now buffered until remote description is set) |
| C | `### Fixed` | Desktop voice audio is now clear at session start (fixed RTP sequence number regression on reconnect) |
| C | `### Fixed` | Desktop screen share now actually reaches other participants (fixed VP8 payload type) |
| D | `### Added` | Desktop client now displays remote screen shares via native VP8 decode |
| E | `### Fixed` | Android microphone audio now reliably reaches other participants (added dedicated publisher PeerConnection) |

PR A's rate limiting and PR E's thread safety carry-over are refactor/architecture changes not added to CHANGELOG per CLAUDE.md's refactor exclusion.

## Docs updates per PR

- **PR A:** Update `server/src/voice/AGENTS.md` to document `is_effectively_muted()` and the dual-flag model; update `server/src/voice/rate_limit.rs` module docs for the new `TokenBucketLimiter`.
- **PR D:** Add a "native video decode" section to `docs/developer-guide/development/ci.md` documenting the libvpx decode feature flag and fixture-capture script.
- **PR E:** Update `docs/developer-guide/architecture/overview.md` (if it covers voice) and/or `server/src/voice/AGENTS.md` to note that all three clients (web, Tauri, Android) now follow the dual-PC signaling model.

## Cross-cutting notes

**Rate limit + reconnect interaction (PR A × PR B/C/E):** The new rate limits must accommodate the actual reconnect cadence of all clients. Current reconnect backoff per client:
- Web: exponential, 1s → 30s cap (`client/src/stores/websocket/index.ts`)
- Tauri: exponential, 1s → 30s cap (`client/src-tauri/src/network/websocket.rs`)
- Android: exponential, 1s → 30s cap (`mobile/android/.../KaikuWebSocket.kt`)

At the worst case (aggressive reconnect loop with ICE restart every second), the client sends 1 `VoicePublisherOffer` per reconnect. The burst=5/refill=1/sec setting allows this without false positives.

**Backwards compatibility for existing web/Tauri clients (before PR B/C ships):** Server-side rate limits apply to all clients equally. If a pre-PR web client under restrictive NAT exceeds 200 ICE candidates/PC during connect, it will hit the limit and lose connectivity. Monitor the Prometheus counter `voice_rate_limit_drops_total` for two weeks after PR A ships; adjust the ICE burst ceiling if the 99th percentile drop rate exceeds 0.1% of sessions.

## Summary

Eight Critical fixes → five independent PRs. No cross-PR dependencies. All parallelizable across worktrees. Testing gates mirror existing audit-followup patterns. Risks are documented per PR. CHANGELOG and docs updates are spelled out per PR.

Phase 2 remains as a follow-up spec covering 14 Important items and related polish.
