# Voice & Screen Share Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship 8 Critical voice + screen share fixes as 5 independent PRs across server, web, Tauri desktop, and Android.

**Architecture:** Five parallel branches, each in its own worktree. No cross-PR dependencies. All testing gates mirror existing audit-followup patterns. Server changes do not require client changes and vice versa.

**Tech Stack:** Rust (server + Tauri native), TypeScript/Solid.js (web + Tauri frontend), Kotlin (Android)

**Spec:** `docs/superpowers/specs/2026-04-15-voice-screenshare-fixes-design.md`

**Parallelization safe:** Each PR touches disjoint file sets across 5 worktrees. Tasks within a PR are serial; tasks across PRs can run in parallel.

---

## Worktree Setup (run once per PR before starting tasks)

```bash
cd /home/detair/GIT/detair/kaiku
git worktree add .claude/worktrees/voice-server-security fix/voice-server-security
git worktree add .claude/worktrees/web-voice-ice-buffering fix/web-voice-ice-buffering
git worktree add .claude/worktrees/tauri-voice-rtp-protocol fix/tauri-voice-rtp-protocol
git worktree add .claude/worktrees/tauri-vp8-decode feat/tauri-vp8-decode
git worktree add .claude/worktrees/android-publisher-pc feat/android-publisher-pc
```

For worktrees with frontend changes (PR B and PR D), also run `bun install` inside the worktree's `client/` directory after creation, since `node_modules` is not shared across worktrees.

---

## Pre-Execution Verified Facts

The following has been verified against the current worktree state. The implementer can rely on these without re-checking:

- **`server/src/voice/peer.rs:53`**: `muted: RwLock<bool>` is private, with methods `set_muted` (line 193) and `is_muted` (line 199).
- **Existing `is_muted` caller**: only `server/src/voice/sfu.rs:214` reads the field. `server/src/voice/ws_handler.rs:614` calls the setter.
- **`server/src/voice/track.rs:450`**: `spawn_rtp_forwarder(source_user_id, source_type, layer, track, router)` — no `Peer` parameter.
- **`spawn_rtp_forwarder` call site**: exactly one at `server/src/voice/sfu.rs:593` (inside the `on_track` handler).
- **`server/src/voice/screen_share.rs`**: `ScreenShareLimiter` is Redis-backed, keyed by `channel_id`. `start(channel_id, max_shares)` and `stop(channel_id)` exist.
- **Duplicate stream_id check**: already at `server/src/voice/ws_handler.rs:770`. Executed AFTER `limiter.start()` at line 757 — this is the ordering bug.
- **`server/src/voice/error.rs`**: `VoiceError` enum has `RateLimited` variant at line 55 with message "Rate limited: too many voice join requests".
- **`client/src-tauri/src/commands/voice.rs:720-724`**: `SEQUENCE_NUMBER` (AtomicU16), `TIMESTAMP` (AtomicU32), `SAMPLES_PER_FRAME` (const).
- **`client/src-tauri/src/video/rtp.rs:40-100`**: `VideoRtpSender` struct has `track: Arc<TrackLocalStaticRTP>` and `seq: AtomicU16`. `send_packet(&self, packet: &EncodedPacket)` fragments internally at `MAX_PAYLOAD_SIZE` boundaries, sets `marker: is_last` per fragment.
- **`client/src-tauri/Cargo.toml`**: `vpx-encode = "0.3"` already present. `Cargo.lock` shows `env-libvpx-sys 4.0.13` pinned transitively.
- **`client/src/lib/webrtc/browser.ts:513`**: current `handleIceCandidate` calls `pc.addIceCandidate()` directly without buffering.
- **`mobile/android/app/src/main/java/io/wolftown/kaiku/data/ws/ServerEvent.kt:100`**: `VoiceIceCandidate(val channelId: String, val candidate: String)` — no `pcType` field.
- **`stream-webrtc-android:1.3.0`**: supports `addTransceiver` and `RtpTransceiver.RtpTransceiverInit`.

---

## File Map

| Worktree | Files Modified/Created |
|----------|------------------------|
| `.claude/worktrees/voice-server-security` | `server/src/voice/peer.rs`, `server/src/voice/track.rs`, `server/src/voice/sfu.rs`, `server/src/voice/ws_handler.rs`, `server/src/voice/rate_limit.rs`, `server/src/voice/error.rs`, `server/tests/integration/voice_mute_enforcement.rs` (new), `server/tests/integration/voice_rate_limit.rs` (new), `CHANGELOG.md` |
| `.claude/worktrees/web-voice-ice-buffering` | `client/src/lib/webrtc/browser.ts`, `client/src/lib/webrtc/browser.test.ts` (new), `CHANGELOG.md` |
| `.claude/worktrees/tauri-voice-rtp-protocol` | `client/src-tauri/src/commands/voice.rs`, `client/src-tauri/src/video/rtp.rs`, `client/src-tauri/src/webrtc/mod.rs`, `CHANGELOG.md` |
| `.claude/worktrees/tauri-vp8-decode` | `client/src-tauri/Cargo.toml`, `client/src-tauri/src/voice/mod.rs`, `client/src-tauri/src/voice/video_decoder.rs`, `client/src-tauri/src/voice/rtp_depacketizer.rs` (new), `client/src-tauri/src/voice/frame_buffer.rs` (new), `client/src-tauri/src/commands/voice.rs`, `client/src-tauri/tests/vp8_depacketize.rs` (new), `client/src-tauri/tests/vp8_decode.rs` (new), `client/src/lib/voice/nativeVideoRenderer.ts` (new), `client/src/components/voice/NativeScreenShareTile.tsx` (new or modified), `docs/developer-guide/development/ci.md`, `CHANGELOG.md` |
| `.claude/worktrees/android-publisher-pc` | `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt`, `mobile/android/app/src/main/java/io/wolftown/kaiku/data/repository/VoiceRepository.kt`, `mobile/android/app/src/main/java/io/wolftown/kaiku/data/ws/ClientEvent.kt`, `mobile/android/app/src/main/java/io/wolftown/kaiku/data/ws/ServerEvent.kt`, `mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt`, `CHANGELOG.md` |

---

## PR A — `fix/voice-server-security`

Work from: `/home/detair/GIT/detair/kaiku/.claude/worktrees/voice-server-security`

### Task A1: Rename `muted` → `self_muted`, add `server_muted` field

**Files:**
- Modify: `server/src/voice/peer.rs:53` (field rename), `peer.rs:193` (set_muted), `peer.rs:199` (is_muted)
- Modify: `server/src/voice/sfu.rs:214` (is_muted call site)
- Modify: `server/src/voice/ws_handler.rs:614` (set_muted call site)

- [ ] **Step 1: Rename fields and methods in `peer.rs`**

Change the `Peer` struct at line 53:

```rust
pub struct Peer {
    // ... existing fields unchanged ...

    /// Whether the user muted themselves.
    self_muted: RwLock<bool>,  // was: `muted`

    /// Whether a moderator muted the user. Set only by future moderation events.
    #[allow(dead_code)]
    server_muted: RwLock<bool>,

    // ... rest of fields ...
}
```

Rename methods:
- Line 193: `pub async fn set_muted(&self, muted: bool)` → `pub async fn set_self_muted(&self, muted: bool)`
- Line 199: `pub async fn is_muted(&self)` → `pub async fn is_self_muted(&self)`

Add the aggregate accessor:

```rust
/// Returns true if either the user self-muted or a moderator muted them.
pub async fn is_effectively_muted(&self) -> bool {
    *self.self_muted.read().await || *self.server_muted.read().await
}
```

Update `Peer::new()` to initialize both fields:
```rust
self_muted: RwLock::new(false),
server_muted: RwLock::new(false),
```

- [ ] **Step 2: Update call sites**

- `server/src/voice/sfu.rs:214`: `peer.is_muted().await` → `peer.is_self_muted().await`
- `server/src/voice/ws_handler.rs:614`: `peer.set_muted(muted).await` → `peer.set_self_muted(muted).await`

- [ ] **Step 3: Build and verify**

```bash
SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
cargo test -p vc-server
```
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add server/src/voice/peer.rs server/src/voice/sfu.rs server/src/voice/ws_handler.rs
git commit -m "refactor(voice): rename muted to self_muted, add server_muted prep flag"
```

### Task A2: Thread `Arc<Peer>` through `spawn_rtp_forwarder`

**Files:**
- Modify: `server/src/voice/track.rs:450` (signature + loop)
- Modify: `server/src/voice/sfu.rs:593` (call site)

- [ ] **Step 1: Add `peer: Arc<Peer>` parameter to `spawn_rtp_forwarder`**

At `server/src/voice/track.rs:450`, update the signature:

```rust
pub fn spawn_rtp_forwarder(
    source_user_id: Uuid,
    source_type: TrackSource,
    layer: Layer,
    track: Arc<TrackRemote>,
    router: Arc<TrackRouter>,
    peer: Arc<Peer>,  // NEW
) {
```

Add the mute check inside the packet loop (the forwarder's hot path, where `track.read()` returns `Ok`). Scope the check to audio-only sources:

```rust
match track.read(&mut buf).await {
    Ok((packet, _attributes)) => {
        packet_count += 1;

        // Drop audio packets from muted peers. Video tracks (screen share,
        // webcam) are not affected by mute state.
        if source_type == TrackSource::Microphone
            && peer.is_effectively_muted().await
        {
            continue;
        }

        // ... existing packet_count debug log + forward_rtp call unchanged
    }
    // ... existing error branch
}
```

Add the `Peer` import at the top of `track.rs`:

```rust
use super::peer::Peer;
```

- [ ] **Step 2: Update call site in `sfu.rs`**

At `server/src/voice/sfu.rs:593`, the `spawn_rtp_forwarder` call lives inside the `on_track` handler. The peer reference (`Arc<Peer>`) is already in scope as the owner of the publisher PC. Add it as the final argument:

```rust
spawn_rtp_forwarder(
    user_id,
    source,
    layer,
    track,
    router.clone(),
    peer.clone(),  // NEW
);
```

(The exact variable name may be `peer_arc` or `self_peer` depending on context — use whichever `Arc<Peer>` is in scope at that call site.)

- [ ] **Step 3: Build and verify**

```bash
SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
cargo test -p vc-server
```
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add server/src/voice/track.rs server/src/voice/sfu.rs
git commit -m "fix(voice): drop RTP packets from self-muted peers in forwarder"
```

### Task A3: Add mute-enforcement integration test

**Files:**
- Create: `server/tests/integration/voice_mute_enforcement.rs`

- [ ] **Step 1: Add test file**

Create `server/tests/integration/voice_mute_enforcement.rs` following the existing voice test convention (see `server/tests/integration/voice_sfu.rs`).

Test outline:

```rust
//! Integration test for self-mute RTP enforcement.

// ... existing test setup imports + helpers from voice_sfu.rs ...

#[tokio::test]
async fn self_mute_drops_audio_rtp() {
    // 1. Start SFU, spawn two peers A and B.
    // 2. A publishes an audio track; B subscribes.
    // 3. A sends N=10 RTP audio packets. Assert B's subscriber receives 10.
    // 4. A sends VoiceMute.
    // 5. A sends M=10 more RTP audio packets. Assert B's receive count still equals 10.
    // 6. A sends VoiceUnmute.
    // 7. A sends K=10 more packets. Assert B's receive count is 20 (10 pre-mute + 10 post-unmute).

    // Key assertion: deterministic packet count, no wall-clock timing.
}

#[tokio::test]
async fn video_tracks_not_affected_by_mute() {
    // 1. Peer A publishes a screen share video track + mic.
    // 2. A mutes.
    // 3. A sends video packets. Assert B receives them (video ignores mute).
}
```

- [ ] **Step 2: Run test**

```bash
cargo test -p vc-server --test voice_mute_enforcement
```
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add server/tests/integration/voice_mute_enforcement.rs
git commit -m "test(voice): add integration test for self-mute RTP enforcement"
```

### Task A4: Generalize `VoiceError::RateLimited` with scope

**Files:**
- Modify: `server/src/voice/error.rs`
- Modify: call sites of `VoiceError::RateLimited` (one in `VoiceStatsLimiter` per spec)

- [ ] **Step 1: Change `RateLimited` to carry scope**

At `server/src/voice/error.rs:55-57`, change:

```rust
#[error("Rate limited: too many voice join requests")]
RateLimited,
```

To:

```rust
#[error("Rate limited: {0}")]
RateLimited(&'static str),
```

- [ ] **Step 2: Update `IntoResponse` arm in `error.rs`**

The existing `IntoResponse` impl at line 95 has `Self::RateLimited => (...)` — change to `Self::RateLimited(_) => (...)` (otherwise the match is non-exhaustive and build fails):

```rust
Self::RateLimited(_) => (
    StatusCode::TOO_MANY_REQUESTS,
    "RATE_LIMITED",
    self.to_string(),
),
```

- [ ] **Step 3: Update all call sites**

Run:
```bash
grep -rn "VoiceError::RateLimited" server/src/ server/tests/
```

Expected three match hits:
- `server/src/voice/rate_limit.rs:70` — constructor in `VoiceStatsLimiter`: pass `"voice_stats"`.
- `server/src/voice/sfu.rs:861` — constructor in join rate limit: pass `"voice_join"`.
- `server/src/voice/ws_handler_test.rs:270` — test match arm `VoiceError::RateLimited => ...` must become `VoiceError::RateLimited(_) => ...`.

Update each accordingly. Miss the test file and the build fails on the non-exhaustive match.

- [ ] **Step 4: Build and verify**

```bash
SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
cargo test -p vc-server
```
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add server/src/voice/error.rs server/src/voice/rate_limit.rs server/src/voice/sfu.rs server/src/voice/ws_handler_test.rs
git commit -m "refactor(voice): carry scope in VoiceError::RateLimited"
```

### Task A5: Add `TokenBucketLimiter` module

**Files:**
- Modify: `server/src/voice/rate_limit.rs`

- [ ] **Step 1: Add `TokenBucket` and `VoiceRateLimiter`**

Append to `server/src/voice/rate_limit.rs`:

```rust
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;
use dashmap::DashMap;
use uuid::Uuid;

/// Event class key for per-peer rate limiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventClass {
    IceCandidatePublisher,
    IceCandidateSubscriber,
    PublisherOffer,      // includes SubscriberAnswer
    ScreenShareToggle,   // covers Start + Stop
    MuteOrWebcamToggle,  // covers Mute/Unmute/WebcamStart/WebcamStop
    SetLayerPreference,
}

#[derive(Debug)]
pub struct TokenBucket {
    capacity: u32,
    tokens: AtomicU32,
    refill_per_sec: u32,
    last_refill: std::sync::Mutex<Instant>,
}

impl TokenBucket {
    pub fn new(capacity: u32, refill_per_sec: u32) -> Self {
        Self {
            capacity,
            tokens: AtomicU32::new(capacity),
            refill_per_sec,
            last_refill: std::sync::Mutex::new(Instant::now()),
        }
    }

    /// Attempt to acquire one token. Lazy refill: compute elapsed time since
    /// `last_refill`, add `elapsed * refill_per_sec` tokens (capped at capacity).
    pub fn try_acquire(&self) -> bool {
        // Refill tokens based on elapsed time
        let now = Instant::now();
        let mut last = self.last_refill.lock().unwrap();
        let elapsed_ms = now.duration_since(*last).as_millis() as u64;
        if elapsed_ms > 0 {
            let tokens_to_add = ((elapsed_ms * self.refill_per_sec as u64) / 1000) as u32;
            if tokens_to_add > 0 {
                let current = self.tokens.load(Ordering::Relaxed);
                let new_value = (current + tokens_to_add).min(self.capacity);
                self.tokens.store(new_value, Ordering::Relaxed);
                *last = now;
            }
        }
        drop(last);

        // Try to take a token
        let mut current = self.tokens.load(Ordering::Relaxed);
        loop {
            if current == 0 {
                return false;
            }
            match self.tokens.compare_exchange_weak(
                current,
                current - 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(actual) => current = actual,
            }
        }
    }
}

/// Per-peer, per-event-class token bucket rate limiter for voice signaling events.
#[derive(Debug, Default)]
pub struct VoiceRateLimiter {
    buckets: DashMap<(Uuid, EventClass), TokenBucket>,
}

impl VoiceRateLimiter {
    pub fn new() -> Self { Self::default() }

    /// Try to acquire a token for (peer_id, event_class).
    /// Returns `true` if allowed, `false` if rate-limited.
    pub fn try_acquire(&self, peer_id: Uuid, class: EventClass) -> bool {
        let bucket = self.buckets
            .entry((peer_id, class))
            .or_insert_with(|| Self::bucket_for_class(class));
        bucket.try_acquire()
    }

    /// Remove all buckets for a peer (on disconnect).
    pub fn forget_peer(&self, peer_id: Uuid) {
        self.buckets.retain(|(pid, _), _| *pid != peer_id);
    }

    fn bucket_for_class(class: EventClass) -> TokenBucket {
        match class {
            EventClass::IceCandidatePublisher
            | EventClass::IceCandidateSubscriber => TokenBucket::new(200, 40),
            EventClass::PublisherOffer => TokenBucket::new(5, 1),
            EventClass::ScreenShareToggle => TokenBucket::new(5, 1), // refill 1 per 5s... adjust
            EventClass::MuteOrWebcamToggle => TokenBucket::new(10, 2),
            EventClass::SetLayerPreference => TokenBucket::new(20, 5),
        }
    }
}
```

Note: ScreenShareToggle's "1 per 5 seconds" is 0 in integer-per-second terms. Use a fractional refill approach: set `refill_per_sec = 1` and `capacity = 5`, effectively capping to 5 toggles with slow regeneration. If stricter is needed, add `refill_per_5_sec` but start with the simpler approach.

- [ ] **Step 2: Wire into `ws_handler.rs`**

In the voice event dispatch, check the limiter before processing each event class. Example for `VoiceIceCandidate`:

```rust
if !rate_limiter.try_acquire(peer.user_id, match pc_type {
    PcType::Publisher => EventClass::IceCandidatePublisher,
    PcType::Subscriber => EventClass::IceCandidateSubscriber,
}) {
    debug!(user_id = %peer.user_id, "rate limited: ice_candidate");
    // Track consecutive drops for sustained-violation policy (deferred to observability phase)
    return Ok(());  // drop silently
}
// ... existing processing
```

Repeat for `VoicePublisherOffer`, `VoiceSubscriberAnswer`, `VoiceScreenShareStart/Stop`, `VoiceMute/Unmute`, `VoiceWebcamStart/Stop`, `VoiceSetLayerPreference`.

- [ ] **Step 3: Add Prometheus counter (observability hook)**

If the server already uses `prometheus` or `metrics` crate, add:

```rust
use metrics::counter;

// On drop:
counter!("voice_rate_limit_drops_total", "event_class" => class_name(class)).increment(1);
```

If no metrics infra exists yet, add a `tracing::info!` log with structured fields; a follow-up PR can wire Prometheus.

- [ ] **Step 4: Build and verify**

```bash
SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
cargo test -p vc-server
```
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add server/src/voice/rate_limit.rs server/src/voice/ws_handler.rs
git commit -m "feat(voice): rate limit voice signaling events per peer"
```

### Task A6: Add rate-limit integration test

**Files:**
- Create: `server/tests/integration/voice_rate_limit.rs`

- [ ] **Step 1: Add test file**

```rust
//! Integration test for voice signaling rate limiting.

#[tokio::test(start_paused = true)]
async fn ice_candidate_rate_limit_per_pc() {
    // 1. Start SFU, spawn peer A.
    // 2. Send 201 VoiceIceCandidate with pc_type=publisher.
    // 3. Assert first 200 are processed, 201st is dropped.
    // 4. Send 200 with pc_type=subscriber in parallel — all 200 pass (independent bucket).
}

#[tokio::test(start_paused = true)]
async fn publisher_offer_rate_limit_refills_on_virtual_time() {
    // `start_paused = true` enables tokio's virtual time, so we can
    // "sleep" 5 seconds without wall-clock flake.

    // 1. Send 5 VoicePublisherOffer back-to-back — all pass (burst=5).
    // 2. Send a 6th immediately — dropped (bucket empty).
    // 3. `tokio::time::advance(Duration::from_secs(5)).await` — adds 5 tokens via lazy refill.
    // 4. Send 5 more — all pass.
}
```

`#[tokio::test(start_paused = true)]` + `tokio::time::advance(...)` replaces real wall-clock `sleep` so the test is deterministic. The `TokenBucket::try_acquire` implementation uses `Instant::now()` internally — the pause mode virtualizes that too (`tokio::time::Instant::now()` is the clock source, which pausing freezes).

**Dependency check:** `TokenBucket::try_acquire` in Task A5 uses `std::time::Instant`. For virtual time to work, switch to `tokio::time::Instant` in the implementation so paused tests advance correctly:

```rust
use tokio::time::Instant;  // not std::time::Instant

pub struct TokenBucket {
    // ...
    last_refill: std::sync::Mutex<Instant>,  // tokio::time::Instant
}
```

Update the import in `rate_limit.rs` and the test becomes deterministic.

- [ ] **Step 2: Run tests**

```bash
cargo test -p vc-server --test voice_rate_limit
```
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add server/tests/integration/voice_rate_limit.rs
git commit -m "test(voice): add integration tests for signaling rate limiter"
```

### Task A7: Fix screen share limiter leak + add `DuplicateStreamId` variant

**Files:**
- Modify: `server/src/voice/error.rs` (add `DuplicateStreamId` variant)
- Modify: `server/src/voice/ws_handler.rs:757-774` (swap order)

- [ ] **Step 1: Add `DuplicateStreamId` variant**

At `server/src/voice/error.rs`, add to the enum:

```rust
/// Screen share stream_id already exists in the room.
#[error("Screen share stream already exists")]
DuplicateStreamId,
```

Also update `IntoResponse` impl at line 70 to map `DuplicateStreamId → (StatusCode::CONFLICT, "DUPLICATE_STREAM_ID", ...)`.

- [ ] **Step 2: Swap order in `ws_handler.rs`**

At `server/src/voice/ws_handler.rs:754-774`, swap so the duplicate check runs BEFORE `limiter.start()`:

```rust
let stream_id = params.stream_id;

// Check for duplicate stream_id FIRST — before reserving the channel slot,
// otherwise a duplicate leaks the slot.
if room.screen_shares.read().await.contains_key(&stream_id) {
    return Err(VoiceError::DuplicateStreamId);
}

let max_shares: u32 = max_screen_shares.try_into().unwrap_or(6);

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

// ... rest of the handler unchanged (pending_track_sources, username, etc.)
```

- [ ] **Step 3: Build and verify**

```bash
SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
cargo test -p vc-server
```
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add server/src/voice/error.rs server/src/voice/ws_handler.rs
git commit -m "fix(voice): check screen share duplicate stream_id before reserving slot"
```

### Task A8: CHANGELOG

**File:** `CHANGELOG.md`

- [ ] **Step 1: Add entries**

In `## [Unreleased]` section:

Under `### Security`:
- Self-muted users' audio is now actually dropped at the server rather than being forwarded to listeners
- Voice signaling events are rate-limited per peer to prevent flooding

Under `### Fixed`:
- Screen share slots are no longer leaked by duplicate stream IDs

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs(voice): CHANGELOG entries for PR A voice server security"
```

---

## PR B — `fix/web-voice-ice-buffering`

Work from: `/home/detair/GIT/detair/kaiku/.claude/worktrees/web-voice-ice-buffering`

### Task B1: Add ICE candidate buffering to `browser.ts`

**Files:**
- Modify: `client/src/lib/webrtc/browser.ts`

- [ ] **Step 1: Read current state**

Read `client/src/lib/webrtc/browser.ts` to understand the current `handleIceCandidate` implementation (around line 513). Note the existing `publisherPc` / `subscriberPc` fields.

- [ ] **Step 2: Add `PcState` type and fields**

Near the top of the `BrowserVoiceAdapter` class (or equivalent), add:

```typescript
const MAX_PENDING_CANDIDATES = 100;

interface PcState {
  pc: RTCPeerConnection;
  remoteDescriptionSet: boolean;
  pendingCandidates: RTCIceCandidateInit[];
}
```

Replace `publisherPc: RTCPeerConnection | null` with `publisherState: PcState | null` (and same for subscriber). Update `new` paths that create the PCs to wrap them in `PcState`:

```typescript
this.publisherState = {
  pc: new RTCPeerConnection(config),
  remoteDescriptionSet: false,
  pendingCandidates: [],
};
```

- [ ] **Step 3: Update `handleIceCandidate` to buffer**

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

- [ ] **Step 4: Drain candidates in `handlePublisherAnswer` / `handleSubscriberOffer`**

In both handlers, before `setRemoteDescription` call: reset `state.remoteDescriptionSet = false` and `state.pendingCandidates = []`. After `setRemoteDescription` succeeds: set flag true and drain.

```typescript
// Before setRemoteDescription:
state.remoteDescriptionSet = false;
state.pendingCandidates = [];

await state.pc.setRemoteDescription(desc);
state.remoteDescriptionSet = true;

// Drain buffered candidates from this session
const candidates = state.pendingCandidates.splice(0);
for (const candidate of candidates) {
  try {
    await state.pc.addIceCandidate(candidate);
  } catch (err) {
    console.warn("Drained ICE candidate failed:", err);
  }
}
```

The pre-`setRemoteDescription` reset drops any candidates queued from a prior session (the `ufrag`/`pwd` in the new SDP invalidates them).

- [ ] **Step 5: Build and verify**

```bash
cd client && bun run build
```
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add client/src/lib/webrtc/browser.ts
git commit -m "fix(client): buffer ICE candidates until remote description is set"
```

### Task B2: Unit test for ICE buffering

**Files:**
- Create: `client/src/lib/webrtc/browser.test.ts`

- [ ] **Step 1: Add test with fake RTCPeerConnection**

Create the test file:

```typescript
import { describe, test, expect, vi } from "vitest";
// import BrowserVoiceAdapter from "./browser";  // adjust import

class FakeRtcPc {
  remoteDesc: RTCSessionDescriptionInit | null = null;
  added: RTCIceCandidateInit[] = [];

  async setRemoteDescription(desc: RTCSessionDescriptionInit) {
    this.remoteDesc = desc;
  }
  async addIceCandidate(candidate: RTCIceCandidateInit) {
    if (!this.remoteDesc) throw new Error("InvalidStateError");
    this.added.push(candidate);
  }
}

describe("BrowserVoiceAdapter ICE buffering", () => {
  test("buffers candidates before remote description is set (publisher)", async () => {
    // Send 5 candidates before setRemoteDescription
    // Trigger setRemoteDescription
    // Assert all 5 are in pc.added
  });

  test("buffers candidates before remote description is set (subscriber)", async () => {
    // Mirror the above test for subscriber PC
  });

  test("drops candidates exceeding MAX_PENDING_CANDIDATES", async () => {
    // Send 105 candidates pre-description-set
    // Trigger setRemoteDescription
    // Assert exactly 100 were applied
  });

  test("resets buffer on renegotiation", async () => {
    // Send 3 candidates, trigger setRemoteDescription (drain them)
    // Send 2 more (applied directly)
    // Trigger a second setRemoteDescription
    // Send 2 more (buffered by new session)
    // Assert the final drain applies the 2 new candidates only
  });
});
```

- [ ] **Step 2: Run tests**

```bash
cd client && bun run test:run -- browser.test.ts
```
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add client/src/lib/webrtc/browser.test.ts
git commit -m "test(client): unit tests for ICE candidate buffering"
```

### Task B3: CHANGELOG

**File:** `CHANGELOG.md`

- [ ] **Step 1: Add entry**

In `## [Unreleased]` → `### Fixed`:
- Browser voice connections no longer fail under restrictive NATs due to ICE candidate race (candidates now buffered until remote description is set)

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs(client): CHANGELOG entry for web ICE buffering"
```

---

## PR C — `fix/tauri-voice-rtp-protocol`

Work from: `/home/detair/GIT/detair/kaiku/.claude/worktrees/tauri-voice-rtp-protocol`

### Task C1: Per-session RTP seq/timestamp in audio sender

**Files:**
- Modify: `client/src-tauri/src/commands/voice.rs:720-741`

- [ ] **Step 1: Replace statics with per-session locals**

At `client/src-tauri/src/commands/voice.rs`, find `send_audio_to_track`. The current code declares `static SEQUENCE_NUMBER: AtomicU16 = AtomicU16::new(0)` and `static TIMESTAMP: AtomicU32 = AtomicU32::new(0)` at lines 720-721.

Replace those static declarations with function-local mutable variables initialized at random values:

```rust
async fn send_audio_to_track(/* existing params */) {
    // Start sequence number and timestamp at random per RFC 3550 §5.1.
    // SSRC is already per-session (derived from SystemTime at spawn), so
    // combining with a random seq start ensures receivers see a fresh stream.
    let mut seq: u16 = rand::random();
    let mut timestamp: u32 = rand::random();

    while let Some(frame) = audio_rx.recv().await {
        // ... existing Opus encode logic ...
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

        // ... existing track.write_rtp(&packet).await call ...

        seq = seq.wrapping_add(1);
        timestamp = timestamp.wrapping_add(SAMPLES_PER_FRAME as u32);
    }
}
```

Remove the `use std::sync::atomic::{AtomicU16, AtomicU32, Ordering};` import if this was its only consumer (grep the file first).

- [ ] **Step 2: Add `rand` direct dep (if needed)**

Check `client/src-tauri/Cargo.toml` for `rand` as a direct dep. If absent, add:

```toml
rand = "0.8"
```

`rand` is likely a transitive dep via `webrtc`, but direct inclusion keeps the spec clean.

- [ ] **Step 3: Build and verify**

```bash
cd client/src-tauri && cargo clippy -- -D warnings && cargo test
```
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add client/src-tauri/src/commands/voice.rs client/src-tauri/Cargo.toml
git commit -m "fix(voice): reset RTP seq/timestamp per session to comply with RFC 3550"
```

### Task C2: VP8 payload_type + SSRC in video RTP sender

**Files:**
- Modify: `client/src-tauri/src/video/rtp.rs:40-100`
- Modify: `client/src-tauri/src/webrtc/mod.rs` (reference the constant)

- [ ] **Step 1: Add `VP8_PAYLOAD_TYPE` constant and `ssrc` field**

At `client/src-tauri/src/video/rtp.rs`, add near the top of the file:

```rust
/// VP8 payload type per the server's media engine registration.
pub const VP8_PAYLOAD_TYPE: u8 = 96;
```

Modify `VideoRtpSender`:

```rust
pub struct VideoRtpSender {
    track: Arc<TrackLocalStaticRTP>,
    seq: AtomicU16,
    ssrc: u32,  // NEW — stable per sender instance
}

impl VideoRtpSender {
    pub fn new(track: Arc<TrackLocalStaticRTP>) -> Self {
        Self {
            track,
            seq: AtomicU16::new(rand::random()),  // was: AtomicU16::new(0)
            ssrc: rand::random(),
        }
    }

    // send_packet: API preserved (&self, packet: &EncodedPacket). Only change
    // the header construction to set payload_type and ssrc explicitly.
    pub async fn send_packet(&self, packet: &EncodedPacket) -> Result<(), VideoError> {
        // ... existing logic unchanged until header construction ...

        let rtp_packet = RtpPacket {
            header: Header {
                version: 2,
                payload_type: VP8_PAYLOAD_TYPE,  // was: missing (defaulted to 0)
                marker: is_last,
                sequence_number: seq,
                timestamp,
                ssrc: self.ssrc,                 // was: missing (defaulted to 0)
                ..Default::default()
            },
            payload: payload.into(),
        };

        // ... rest unchanged
    }
}
```

- [ ] **Step 2: Reference the constant from `webrtc/mod.rs`**

In `client/src-tauri/src/webrtc/mod.rs`, find the VP8 codec registration (hardcoded `96`). Replace with an import:

```rust
use crate::video::rtp::VP8_PAYLOAD_TYPE;

// In the codec registration:
RTCRtpCodecCapability {
    payload_type: VP8_PAYLOAD_TYPE as u8,
    // ...
},
```

- [ ] **Step 3: Build and verify**

```bash
cd client/src-tauri && cargo clippy -- -D warnings && cargo test
```
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add client/src-tauri/src/video/rtp.rs client/src-tauri/src/webrtc/mod.rs
git commit -m "fix(voice): set VP8 payload_type and stable SSRC on outbound video RTP"
```

### Task C3: CHANGELOG

**File:** `CHANGELOG.md`

- [ ] **Step 1: Add entries**

In `## [Unreleased]` → `### Fixed`:
- Desktop voice audio is now clear at session start (fixed RTP sequence number regression on reconnect)
- Desktop screen share now actually reaches other participants (fixed VP8 payload type)

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs(voice): CHANGELOG entries for PR C Tauri RTP protocol fixes"
```

---

## PR D — `feat/tauri-vp8-decode`

Work from: `/home/detair/GIT/detair/kaiku/.claude/worktrees/tauri-vp8-decode`

This is the largest PR. Task count is higher but each task is focused.

### Task D1: Add `env-libvpx-sys` dependency

**Files:**
- Modify: `client/src-tauri/Cargo.toml`

- [ ] **Step 1: Add direct dep**

Add alongside `vpx-encode`:

```toml
env-libvpx-sys = "4"  # match existing vpx-encode 0.3.0 transitive pin (4.0.13)
```

- [ ] **Step 2: Verify build**

```bash
cd client/src-tauri && cargo check
```
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add client/src-tauri/Cargo.toml
git commit -m "build(voice): add env-libvpx-sys dep for VP8 decode"
```

### Task D2: VP8 RTP depacketizer

**Files:**
- Create: `client/src-tauri/src/voice/rtp_depacketizer.rs`
- Create: `client/src-tauri/tests/vp8_depacketize.rs`

- [ ] **Step 1: Create depacketizer module**

Create `client/src-tauri/src/voice/rtp_depacketizer.rs`:

```rust
//! VP8 RTP depacketizer per RFC 7741.

use webrtc::rtp::packet::Packet as RtpPacket;

pub struct Vp8Depacketizer {
    frame_buffer: Vec<u8>,
    last_seq: Option<u16>,
    expecting_keyframe: bool,
}

impl Vp8Depacketizer {
    pub fn new() -> Self {
        Self {
            frame_buffer: Vec::new(),
            last_seq: None,
            expecting_keyframe: true,
        }
    }

    /// Feed an RTP packet. Returns a complete VP8 frame if one is assembled.
    /// Resets on sequence gap (frame corrupted — existing interval PLI sender
    /// will recover; no new PLI request plumbing added here).
    pub fn depacketize(&mut self, packet: &RtpPacket) -> Option<Vec<u8>> {
        // 1. Check sequence continuity. If gap detected, drop partial frame_buffer.
        // 2. Parse VP8 payload descriptor (first byte, optional extension bytes).
        // 3. Append payload (after descriptor) to frame_buffer.
        // 4. If marker bit set, return the complete frame; reset buffer.
        // 5. Otherwise return None.
        //
        // VP8 payload descriptor format (RFC 7741 §4.2):
        //   Byte 0: X|R|N|S|PID (5 bits)
        //   If X: Byte 1: I|L|T|K|RSV
        //     If I: PictureID (1 or 2 bytes)
        //     If L: TL0PICIDX (1 byte)
        //     If T or K: TID|Y|KEYIDX (1 byte)
        todo!("implement per RFC 7741 §4.2")
    }
}
```

Fill in the implementation per RFC 7741.

- [ ] **Step 2: Write tests against a fixture**

Create `client/src-tauri/tests/vp8_depacketize.rs`:

```rust
//! Tests for VP8 RTP depacketizer.

use std::fs::read;

#[test]
fn assembles_keyframe_from_rtp_fragments() {
    // Load tests/fixtures/vp8_sample.rtp (packed RTP stream).
    // Feed packets through Vp8Depacketizer.
    // Assert exactly one complete frame is returned after the marker-bit packet.
    // Assert the first frame is a keyframe (VP8 frame tag byte 0, bit 0 is 0).
}

#[test]
fn drops_partial_frame_on_sequence_gap() {
    // Feed packets 1, 2, skip 3, send 4 with marker.
    // Assert no frame returned (frame_buffer dropped on gap detection).
}
```

The fixture `client/src-tauri/tests/fixtures/vp8_sample.rtp` is captured by a separate script (see Task D3).

- [ ] **Step 3: Run tests (will fail until implementation + fixture exist)**

Skip running until Task D3 provides the fixture.

- [ ] **Step 4: Commit**

```bash
git add client/src-tauri/src/voice/rtp_depacketizer.rs client/src-tauri/tests/vp8_depacketize.rs
git commit -m "feat(voice): add VP8 RTP depacketizer"
```

### Task D3: Capture-fixture helper script

**Files:**
- Create: `client/src-tauri/examples/capture_vp8_fixture.rs` (Cargo example target)
- Create: `client/src-tauri/tests/fixtures/vp8_sample.rtp` (committed binary)

The script lives under `examples/` (not `tests/`) because `cargo run --example` is the canonical way to run a one-shot script bundled with a crate; `tests/` is reserved for actual test harnesses picked up by `cargo test`. The File Map's prior `tests/` reference is superseded by this task.

- [ ] **Step 1: Add fixture-capture helper**

Create `client/src-tauri/examples/capture_vp8_fixture.rs`:

```rust
//! Generates tests/fixtures/vp8_sample.rtp using the existing vpx-encode
//! path. Produces a short (1 second) VP8 stream, RTP-packetized, written
//! to disk. Run once, commit the .rtp file. Not invoked by CI.

fn main() {
    // 1. Create a small RGB frame (320x240, 1 second @ 30fps = 30 frames).
    // 2. Convert RGB → YUV I420.
    // 3. Encode with vpx-encode (existing VP8Encoder path).
    // 4. Wrap each encoded frame in RTP using existing VideoRtpSender logic.
    // 5. Serialize RTP packets to a single .rtp file (length-prefixed, or
    //    use the pcap format if a crate exists).
}
```

Run once:
```bash
cd client/src-tauri && cargo run --example capture_vp8_fixture
```

- [ ] **Step 2: Commit fixture + script**

```bash
git add client/src-tauri/examples/capture_vp8_fixture.rs \
  client/src-tauri/tests/fixtures/vp8_sample.rtp
git commit -m "test(voice): add VP8 RTP fixture capture helper"
```

- [ ] **Step 3: Run Task D2's tests against the fixture**

```bash
cd client/src-tauri && cargo test --test vp8_depacketize
```
Expected: PASS

### Task D4: VP8 decoder (safe wrapper around `env_libvpx_sys`)

**Files:**
- Modify: `client/src-tauri/src/voice/video_decoder.rs`
- Modify: `client/src-tauri/src/voice/mod.rs` (export new modules)
- Create: `client/src-tauri/tests/vp8_decode.rs`

- [ ] **Step 1: Implement `Vp8VideoDecoder` via `env_libvpx_sys` FFI**

Replace the stub in `client/src-tauri/src/voice/video_decoder.rs` with:

```rust
//! Native VP8 video decoder using env-libvpx-sys.
//!
//! This is a safe wrapper around the raw C bindings. The env-libvpx-sys
//! crate only provides FFI declarations; we implement init/decode/get_frame
//! as a thin safe shim here. See vpx-encode 0.3.0 for the parallel Encoder
//! pattern on the same underlying library.

use env_libvpx_sys::*;
use tokio::sync::mpsc;
use crate::voice::rtp_depacketizer::Vp8Depacketizer;
use webrtc::rtp::packet::Packet as RtpPacket;

#[derive(Debug, thiserror::Error)]
pub enum VideoDecodeError {
    #[error("VP8 decoder init failed: code {0}")]
    Init(u32),
    #[error("VP8 decode failed: code {0}")]
    Decode(u32),
    #[error("frame sink closed")]
    FrameSinkClosed,
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

pub struct Vp8VideoDecoder {
    ctx: vpx_codec_ctx_t,
    depacketizer: Vp8Depacketizer,
    stream_id: String,
    frame_sink: mpsc::Sender<DecodedFrame>,
}

// Safety: the vpx_codec_ctx_t is not Send-safe in the crate's opinion,
// but our usage is single-task-bound — we manually mark Send.
unsafe impl Send for Vp8VideoDecoder {}

impl Vp8VideoDecoder {
    pub fn new(stream_id: String, frame_sink: mpsc::Sender<DecodedFrame>) -> Result<Self, VideoDecodeError> {
        let mut ctx: vpx_codec_ctx_t = unsafe { std::mem::zeroed() };
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
            return Err(VideoDecodeError::Init(res as u32));
        }
        Ok(Self { ctx, depacketizer: Vp8Depacketizer::new(), stream_id, frame_sink })
    }

    pub async fn process_packet(&mut self, packet: &RtpPacket) -> Result<(), VideoDecodeError> {
        let Some(frame_bytes) = self.depacketizer.depacketize(packet) else {
            return Ok(());
        };

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
            return Err(VideoDecodeError::Decode(res as u32));
        }

        // Iterate decoded frames
        let mut iter: vpx_codec_iter_t = std::ptr::null();
        loop {
            let img = unsafe { vpx_codec_get_frame(&mut self.ctx, &mut iter) };
            if img.is_null() { break; }

            let yuv = unsafe { copy_image_to_decoded_frame(img, &self.stream_id) };
            if self.frame_sink.send(yuv).await.is_err() {
                return Err(VideoDecodeError::FrameSinkClosed);
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

/// Copy Y, U, V planes out of a borrowed vpx_image_t into owned buffers.
/// The vpx_image_t is invalidated by the next vpx_codec_get_frame call, so
/// copying is mandatory.
unsafe fn copy_image_to_decoded_frame(img: *const vpx_image_t, stream_id: &str) -> DecodedFrame {
    let img = &*img;
    let width = img.d_w;
    let height = img.d_h;

    // I420: Y is full-size, U/V are half-size (4:2:0 chroma subsampling).
    let y_stride = img.stride[0] as u32;
    let uv_stride = img.stride[1] as u32;

    let y_size = (y_stride * height) as usize;
    let uv_size = (uv_stride * (height / 2)) as usize;

    let y_plane = std::slice::from_raw_parts(img.planes[0], y_size).to_vec();
    let u_plane = std::slice::from_raw_parts(img.planes[1], uv_size).to_vec();
    let v_plane = std::slice::from_raw_parts(img.planes[2], uv_size).to_vec();

    DecodedFrame {
        stream_id: stream_id.to_string(),
        width,
        height,
        y_plane,
        u_plane,
        v_plane,
        y_stride,
        uv_stride,
        pts_ms: 0, // TODO: thread RTP timestamp through if needed for A/V sync
    }
}
```

- [ ] **Step 2: Export from `mod.rs`**

In `client/src-tauri/src/voice/mod.rs`, add:

```rust
pub mod rtp_depacketizer;
pub mod video_decoder;
```

- [ ] **Step 3: Add integration test**

Create `client/src-tauri/tests/vp8_decode.rs`:

```rust
//! Integration test for Vp8VideoDecoder.

#[tokio::test]
#[cfg_attr(not(feature = "decode-integration"), ignore)]
async fn decodes_vp8_sample() {
    // Load tests/fixtures/vp8_sample.rtp.
    // Feed each packet through Vp8VideoDecoder::process_packet.
    // Assert at least one DecodedFrame arrives on the sink with:
    //   - width == 320, height == 240
    //   - y_plane.len() == 320*240
    //   - u_plane.len() == 160*120
    //   - v_plane.len() == 160*120
}
```

- [ ] **Step 4: Build and verify**

```bash
cd client/src-tauri && cargo clippy -- -D warnings && cargo test
cargo test --features decode-integration --test vp8_decode
```
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add client/src-tauri/src/voice/video_decoder.rs \
  client/src-tauri/src/voice/mod.rs \
  client/src-tauri/tests/vp8_decode.rs
git commit -m "feat(voice): implement native VP8 decode via env-libvpx-sys"
```

### Task D5: Frame sink → Tauri Channel for binary streaming

**Files:**
- Create: `client/src-tauri/src/voice/frame_buffer.rs`
- Modify: `client/src-tauri/src/voice/video_decoder.rs` (update `frame_sink` type)
- Modify: `client/src-tauri/src/commands/voice.rs`

**Why `tokio::sync::watch` + Tauri `Channel<T>`:** Tauri's standard `emit` JSON-serializes payloads, which would turn each YUV plane into a JSON array of numbers (~10× the raw size and CPU cost), defeating the spec's "no base64" requirement. Tauri 2.x `Channel<T>` is the canonical API for streaming typed payloads from Rust to the frontend; it handles `Vec<u8>` efficiently via the structured-clone protocol. Combined with `watch::channel` for drop-oldest semantics, this bounds latency to one frame.

- [ ] **Step 1: Mark `DecodedFrame` serializable with efficient bytes encoding**

In `client/src-tauri/src/voice/video_decoder.rs`, derive `Serialize` on `DecodedFrame` with `serde_bytes` on the plane fields so they aren't serialized as number arrays:

```rust
use serde::Serialize;
use serde_bytes::ByteBuf;

#[derive(Serialize, Clone)]
pub struct DecodedFrame {
    pub stream_id: String,
    pub width: u32,
    pub height: u32,
    #[serde(with = "serde_bytes")]
    pub y_plane: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub u_plane: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub v_plane: Vec<u8>,
    pub y_stride: u32,
    pub uv_stride: u32,
    pub pts_ms: u64,
}
```

Add to `Cargo.toml` if not present: `serde = { version = "1", features = ["derive"] }`, `serde_bytes = "0.11"`.

- [ ] **Step 2: Create frame buffer module with watch channel + Tauri Channel**

Create `client/src-tauri/src/voice/frame_buffer.rs`:

```rust
//! Frame buffer bridging the decoder (async Rust) to the webview (Tauri Channel).
//!
//! Uses `tokio::sync::watch` internally so that the decoder can always send
//! without backpressure — the watch channel retains only the latest value,
//! giving natural drop-oldest semantics (only the last frame matters for
//! real-time video display). The emitter task awakes on `changed()` and
//! forwards the latest frame via a Tauri Channel<T> to the frontend.

use tokio::sync::watch;
use tauri::ipc::Channel;
use crate::voice::video_decoder::DecodedFrame;

pub struct FrameBuffer {
    tx: watch::Sender<Option<DecodedFrame>>,
}

impl FrameBuffer {
    pub fn new() -> (Self, watch::Receiver<Option<DecodedFrame>>) {
        let (tx, rx) = watch::channel(None);
        (Self { tx }, rx)
    }

    /// Push a decoded frame. Never blocks; replaces any previously unread frame.
    pub fn push(&self, frame: DecodedFrame) {
        // Ignore send errors: means no receiver, which is fine for our use case.
        let _ = self.tx.send(Some(frame));
    }
}

/// Spawn the emitter task that forwards frames to a Tauri Channel.
pub fn spawn_frame_emitter(
    channel: Channel<DecodedFrame>,
    mut rx: watch::Receiver<Option<DecodedFrame>>,
) {
    tokio::spawn(async move {
        // Tauri Channel::send is infallible under normal operation.
        // We loop on `rx.changed()` to react to each new frame.
        while rx.changed().await.is_ok() {
            if let Some(frame) = rx.borrow_and_update().clone() {
                if let Err(e) = channel.send(frame) {
                    tracing::warn!(error = %e, "Failed to send frame over Tauri channel");
                    break;
                }
            }
        }
    });
}
```

Add the module to `voice/mod.rs`:

```rust
pub mod frame_buffer;
```

- [ ] **Step 3: Update `Vp8VideoDecoder` to use `FrameBuffer`**

Modify `Vp8VideoDecoder` in `video_decoder.rs`:

```rust
pub struct Vp8VideoDecoder {
    ctx: vpx_codec_ctx_t,
    depacketizer: Vp8Depacketizer,
    stream_id: String,
    frame_sink: FrameBuffer,  // was: mpsc::Sender<DecodedFrame>
}

impl Vp8VideoDecoder {
    pub fn new(stream_id: String, frame_sink: FrameBuffer) -> Result<Self, VideoDecodeError> {
        // ... same init logic
    }

    pub async fn process_packet(&mut self, packet: &RtpPacket) -> Result<(), VideoDecodeError> {
        // ... depacketize + decode ...

        // Use push (never blocks, drop-oldest implicit via watch channel)
        let mut iter: vpx_codec_iter_t = std::ptr::null();
        loop {
            let img = unsafe { vpx_codec_get_frame(&mut self.ctx, &mut iter) };
            if img.is_null() { break; }

            let yuv = unsafe { copy_image_to_decoded_frame(img, &self.stream_id) };
            self.frame_sink.push(yuv);
        }
        Ok(())
    }
}
```

- [ ] **Step 4: Wire into `commands/voice.rs`**

Add a new Tauri command that the frontend invokes to subscribe to a stream's frames:

```rust
#[tauri::command]
pub async fn subscribe_video_frames(
    stream_id: String,
    on_frame: Channel<DecodedFrame>,
    voice_state: State<'_, VoiceState>,
) -> Result<(), String> {
    let (frame_buffer, frame_rx) = FrameBuffer::new();
    spawn_frame_emitter(on_frame, frame_rx);

    // Register `frame_buffer` with the voice subscriber so that when the
    // subscriber PC's on_track handler receives a VP8 track matching
    // stream_id, it creates a Vp8VideoDecoder using this frame_buffer.
    voice_state
        .register_video_frame_sink(stream_id, frame_buffer)
        .await;

    Ok(())
}
```

The `VoiceState` needs a `video_frame_sinks: HashMap<String, FrameBuffer>` map and the subscriber track handler pulls the right `FrameBuffer` by `stream_id` when creating the decoder. (Exact wiring depends on the existing voice state layout — read `voice/mod.rs` and follow the pattern used for audio subscribers.)

- [ ] **Step 5: Build and verify**

```bash
cd client/src-tauri && cargo clippy -- -D warnings && cargo test
```
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add client/src-tauri/src/voice/frame_buffer.rs \
  client/src-tauri/src/voice/video_decoder.rs \
  client/src-tauri/src/voice/mod.rs \
  client/src-tauri/src/commands/voice.rs \
  client/src-tauri/Cargo.toml
git commit -m "feat(voice): stream decoded YUV frames to webview via Tauri Channel"
```

### Task D6: WebGL YUV renderer

**Files:**
- Create: `client/src/lib/voice/nativeVideoRenderer.ts`

- [ ] **Step 1: Implement `NativeYuvRenderer`**

Create `client/src/lib/voice/nativeVideoRenderer.ts`:

```typescript
/**
 * Renders YUV I420 frames from Tauri native decode to a canvas using
 * WebGL2 with a YUV→RGB fragment shader.
 */

interface DecodedFrame {
  stream_id: string;
  width: number;
  height: number;
  y_plane: Uint8Array;
  u_plane: Uint8Array;
  v_plane: Uint8Array;
  y_stride: number;
  uv_stride: number;
  pts_ms: number;
}

const VERTEX_SHADER = `#version 300 es
in vec2 a_pos;
out vec2 v_uv;
void main() {
  v_uv = vec2(a_pos.x * 0.5 + 0.5, 1.0 - (a_pos.y * 0.5 + 0.5));
  gl_Position = vec4(a_pos, 0.0, 1.0);
}`;

const FRAGMENT_SHADER = `#version 300 es
precision highp float;
uniform highp sampler2D u_y;
uniform highp sampler2D u_u;
uniform highp sampler2D u_v;
in vec2 v_uv;
out vec4 outColor;

// BT.601 YUV→RGB conversion for I420
void main() {
  float y = texture(u_y, v_uv).r;
  float u = texture(u_u, v_uv).r - 0.5;
  float v = texture(u_v, v_uv).r - 0.5;

  float r = y + 1.402 * v;
  float g = y - 0.344 * u - 0.714 * v;
  float b = y + 1.772 * u;

  outColor = vec4(r, g, b, 1.0);
}`;

export class NativeYuvRenderer {
  private gl: WebGL2RenderingContext;
  private program: WebGLProgram;
  private yTex: WebGLTexture;
  private uTex: WebGLTexture;
  private vTex: WebGLTexture;
  private vao: WebGLVertexArrayObject;

  constructor(canvas: HTMLCanvasElement) {
    const gl = canvas.getContext("webgl2");
    if (!gl) throw new Error("WebGL2 not supported");
    this.gl = gl;

    // Compile shaders, link program
    this.program = this.compileProgram(VERTEX_SHADER, FRAGMENT_SHADER);

    // Create R8 textures for each plane
    this.yTex = this.makeTexture();
    this.uTex = this.makeTexture();
    this.vTex = this.makeTexture();

    // Fullscreen quad VAO
    this.vao = this.makeVao();
  }

  renderFrame(frame: DecodedFrame) {
    const { gl } = this;
    if (gl.canvas.width !== frame.width || gl.canvas.height !== frame.height) {
      gl.canvas.width = frame.width;
      gl.canvas.height = frame.height;
      gl.viewport(0, 0, frame.width, frame.height);
    }

    // Upload Y, U, V planes to respective R8 single-channel textures
    this.uploadPlane(this.yTex, 0, frame.y_plane, frame.y_stride, frame.height);
    this.uploadPlane(this.uTex, 1, frame.u_plane, frame.uv_stride, frame.height / 2);
    this.uploadPlane(this.vTex, 2, frame.v_plane, frame.uv_stride, frame.height / 2);

    // Draw fullscreen quad with YUV→RGB shader
    gl.useProgram(this.program);
    gl.uniform1i(gl.getUniformLocation(this.program, "u_y"), 0);
    gl.uniform1i(gl.getUniformLocation(this.program, "u_u"), 1);
    gl.uniform1i(gl.getUniformLocation(this.program, "u_v"), 2);
    gl.bindVertexArray(this.vao);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  }

  dispose() {
    const { gl } = this;
    gl.deleteTexture(this.yTex);
    gl.deleteTexture(this.uTex);
    gl.deleteTexture(this.vTex);
    gl.deleteProgram(this.program);
    gl.deleteVertexArray(this.vao);
  }

  // ... private helpers: compileProgram, makeTexture, makeVao, uploadPlane
  // (~60 lines — standard WebGL boilerplate)
}
```

- [ ] **Step 2: Build and verify**

```bash
cd client && bun run build
```
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add client/src/lib/voice/nativeVideoRenderer.ts
git commit -m "feat(voice): add WebGL YUV renderer for native video frames"
```

### Task D7: Wire renderer into screen share tile

**Files:**
- Modify or create: `client/src/components/voice/NativeScreenShareTile.tsx` (or existing `ScreenShareTile` with Tauri detection)

- [ ] **Step 1: Update the screen share tile component**

Find the existing `ScreenShareTile` component. Add Tauri-specific rendering:

```tsx
import { NativeYuvRenderer } from "@/lib/voice/nativeVideoRenderer";
import { isTauri } from "@/lib/tauri/detect";  // use existing detection helper

<Show when={isTauri()} fallback={<video srcObject={videoTrack} autoplay muted />}>
  <canvas ref={canvasRef} class="w-full h-full object-contain" />
</Show>
```

Add a `createEffect` that invokes the `subscribe_video_frames` Tauri command for this stream's ID using a Tauri `Channel`:

```tsx
import { invoke, Channel } from "@tauri-apps/api/core";

let renderer: NativeYuvRenderer | null = null;
let channel: Channel<DecodedFrame> | null = null;

createEffect(() => {
  if (!isTauri() || !canvasRef) return;
  renderer = new NativeYuvRenderer(canvasRef);

  // Tauri Channel<T> receives the typed payload; y_plane/u_plane/v_plane
  // arrive as Uint8Array (via serde_bytes on the Rust side).
  channel = new Channel<DecodedFrame>();
  channel.onmessage = (frame) => {
    if (frame.stream_id === props.streamId) {
      renderer?.renderFrame(frame);
    }
  };

  invoke("subscribe_video_frames", {
    streamId: props.streamId,
    onFrame: channel,
  }).catch((err) => console.error("Failed to subscribe to video frames:", err));

  onCleanup(() => {
    renderer?.dispose();
    renderer = null;
    channel = null;
    // The Rust-side FrameBuffer is dropped when the stream ends; the webview
    // side channel's GC is sufficient cleanup here.
  });
});
```

- [ ] **Step 2: Build and test**

```bash
cd client && bun run build && bun run test:run
```
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add client/src/components/voice/  # whichever file was modified
git commit -m "feat(voice): use canvas + WebGL for screen share tiles in Tauri mode"
```

### Task D8: Docs update for decode feature

**File:** `docs/developer-guide/development/ci.md`

- [ ] **Step 1: Add decode section**

Append a section documenting:
- `env-libvpx-sys` dep matches existing `vpx-encode 0.3` transitive version.
- Windows vcpkg `libvpx → vpx.lib` rename workaround is unchanged.
- `decode-integration` feature flag gates the integration test.
- Fixture regen: `cargo run --example capture_vp8_fixture`.

- [ ] **Step 2: Commit**

```bash
git add docs/developer-guide/development/ci.md
git commit -m "docs(voice): document native VP8 decode feature and test flag"
```

### Task D9: CHANGELOG

**File:** `CHANGELOG.md`

- [ ] **Step 1: Add entry**

In `## [Unreleased]` → `### Added`:
- Desktop client now displays remote screen shares via native VP8 decode

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs(voice): CHANGELOG entry for PR D native VP8 decode"
```

---

## PR E — `feat/android-publisher-pc`

Work from: `/home/detair/GIT/detair/kaiku/.claude/worktrees/android-publisher-pc`

### Task E1: Update `ClientEvent` and `ServerEvent` for pc_type and new variants

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/data/ws/ClientEvent.kt`
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/data/ws/ServerEvent.kt`

- [ ] **Step 1: Add pc_type to `ClientEvent.VoiceIceCandidate`**

In `ClientEvent.kt`, find the `VoiceIceCandidate` variant. Add a `pcType` field:

```kotlin
@Serializable
@SerialName("voice_ice_candidate")
data class VoiceIceCandidate(
    val channelId: String,
    val candidate: String,
    val pcType: String  // "publisher" or "subscriber"
) : ClientEvent()
```

Add new variants:

```kotlin
@Serializable
@SerialName("voice_publisher_offer")
data class VoicePublisherOffer(val channelId: String, val sdp: String) : ClientEvent()

@Serializable
@SerialName("voice_subscriber_answer")
data class VoiceSubscriberAnswer(val channelId: String, val sdp: String) : ClientEvent()
```

- [ ] **Step 2: Add pc_type to `ServerEvent.VoiceIceCandidate`**

In `ServerEvent.kt` at line 100, modify:

```kotlin
@Serializable
@SerialName("voice_ice_candidate")
data class VoiceIceCandidate(
    val channelId: String,
    val candidate: String,
    val pcType: String  // NEW
) : ServerEvent()
```

Add new variants:

```kotlin
@Serializable
@SerialName("voice_publisher_answer")
data class VoicePublisherAnswer(val channelId: String, val sdp: String) : ServerEvent()

@Serializable
@SerialName("voice_subscriber_offer")
data class VoiceSubscriberOffer(val channelId: String, val sdp: String) : ServerEvent()
```

- [ ] **Step 3: Build**

```bash
cd mobile/android && ./gradlew compileDebugKotlin
```
Expected: PASS (existing call sites may break — fix in next tasks)

- [ ] **Step 4: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/ws/ClientEvent.kt \
  mobile/android/app/src/main/java/io/wolftown/kaiku/data/ws/ServerEvent.kt
git commit -m "feat(voice): extend voice events with pcType field for dual-PC"
```

### Task E2: Extend `WebRtcManager` with publisher PC state

**File:** `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt`

- [ ] **Step 1: Add publisher PC fields**

At the top of `WebRtcManager` class body (alongside the existing `@Volatile private var peerConnection`), add:

```kotlin
// Publisher PC — uploads local mic to SFU
@Volatile private var publisherPc: PeerConnection? = null
private var publisherRemoteDescriptionSet = false
private val publisherPendingCandidates = mutableListOf<String>()

// NOTE: publisherRemoteDescriptionSet and publisherPendingCandidates are
// mutated from both the WebRTC signaling thread (via Observer callbacks) and
// the IO dispatcher (via addIceCandidate from WS events). Phase 2 audit will
// add @Volatile + synchronization for both PCs consistently.
```

Rename the existing `peerConnection` field to `subscriberPc`. Rename existing `remoteDescriptionSet` → `subscriberRemoteDescriptionSet`, `pendingCandidates` → `subscriberPendingCandidates`. Apply the same rename to all existing references inside this file.

- [ ] **Step 2: Add publisher-specific callbacks**

```kotlin
@Volatile var onPublisherOffer: ((String) -> Unit)? = null
@Volatile var onPublisherIceCandidate: ((String) -> Unit)? = null

// Rename existing onLocalDescription → onSubscriberAnswer
// Rename existing onIceCandidate → onSubscriberIceCandidate
```

Update the existing `Observer` impl in the subscriber PC creation path to invoke `onSubscriberIceCandidate` and `onSubscriberAnswer`.

- [ ] **Step 3: Build**

```bash
cd mobile/android && ./gradlew compileDebugKotlin
```
Expected: PASS (internals only; external callers will break — fix in Task E4)

- [ ] **Step 4: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt
git commit -m "refactor(voice): rename PC methods/fields for publisher/subscriber clarity"
```

### Task E3: Implement publisher PC lifecycle methods

**File:** `mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt`

- [ ] **Step 1: Add `createPublisherOffer` and `handlePublisherAnswer`**

Add to `WebRtcManager`:

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

    val pub = pcFactory.createPeerConnection(rtcConfig, createPublisherObserver())
        ?: throw IllegalStateException("Failed to create publisher PC")
    publisherPc = pub

    // Create and attach mic track
    audioSource = pcFactory.createAudioSource(MediaConstraints())
    localAudioTrack = pcFactory.createAudioTrack(LOCAL_AUDIO_TRACK_ID, audioSource).also {
        it.setEnabled(!isMuted)
    }

    pub.addTransceiver(
        localAudioTrack,
        RtpTransceiver.RtpTransceiverInit(RtpTransceiver.RtpTransceiverDirection.SEND_ONLY)
    )

    pub.createOffer(object : SdpObserverAdapter("createPublisherOffer", onError) {
        override fun onCreateSuccess(desc: SessionDescription) {
            pub.setLocalDescription(object : SdpObserverAdapter("setPublisherLocalDesc", onError) {
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

private fun createPublisherObserver(): PeerConnection.Observer =
    object : PeerConnection.Observer {
        override fun onIceCandidate(candidate: IceCandidate) {
            val json = """{"candidate":"${candidate.sdp}","sdpMLineIndex":${candidate.sdpMLineIndex},"sdpMid":"${candidate.sdpMid}"}"""
            onPublisherIceCandidate?.invoke(json)
        }
        // Implement other Observer methods; mostly empty or logger.info.
        // Critical: onIceConnectionChange feeds publisherIceState StateFlow.
        override fun onIceConnectionChange(state: PeerConnection.IceConnectionState?) {
            publisherIceState.value = state
        }
        // ... implement other required methods
    }

private fun drainPublisherCandidates() {
    val drained = publisherPendingCandidates.toList()
    publisherPendingCandidates.clear()
    drained.forEach { candidateJson ->
        addIceCandidate("publisher", candidateJson)
    }
}

private val publisherIceState = MutableStateFlow<PeerConnection.IceConnectionState?>(null)
private val subscriberIceState = MutableStateFlow<PeerConnection.IceConnectionState?>(null)

val voiceIceConnected: StateFlow<Boolean> = combine(publisherIceState, subscriberIceState) { p, s ->
    p == PeerConnection.IceConnectionState.CONNECTED &&
    s == PeerConnection.IceConnectionState.CONNECTED
}.stateIn(CoroutineScope(Dispatchers.IO), SharingStarted.Eagerly, false)
```

- [ ] **Step 2: Update `addIceCandidate` to route by `pc_type`**

Replace the existing `addIceCandidate(candidateJson: String)` with:

```kotlin
fun addIceCandidate(pcType: String, candidateJson: String) {
    val (pc, candidateBuffer, remoteSet) = when (pcType) {
        "publisher" -> Triple(
            publisherPc,
            publisherPendingCandidates,
            publisherRemoteDescriptionSet
        )
        "subscriber" -> Triple(
            subscriberPc,
            subscriberPendingCandidates,
            subscriberRemoteDescriptionSet
        )
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

    // parse and add — same logic as before
    // ...
}
```

- [ ] **Step 3: Update `closePeerConnections` (rename from `closePeerConnection`)**

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

Remove audioSource + localAudioTrack creation from the old `createPeerConnection` path (that code is now in `createPublisherOffer`).

- [ ] **Step 4: Build**

```bash
cd mobile/android && ./gradlew compileDebugKotlin
```
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/voice/WebRtcManager.kt
git commit -m "feat(voice): add publisher PeerConnection for Android mic upload"
```

### Task E4: Update `VoiceRepository` for dual-PC signaling

**File:** `mobile/android/app/src/main/java/io/wolftown/kaiku/data/repository/VoiceRepository.kt`

- [ ] **Step 1: Update join flow and signal routing**

In the `joinChannel(channelId)` flow:

1. Call `webRtcManager.initialize()` + `webRtcManager.createPublisherOffer()`.
2. On `onPublisherOffer(sdp)` callback, send `VoicePublisherOffer(channelId, sdp)` via WS.
3. Add handler for `ServerEvent.VoicePublisherAnswer` → call `webRtcManager.handlePublisherAnswer(sdp)`.
4. Add handler for `ServerEvent.VoiceSubscriberOffer` → call existing subscriber handler.
5. Change existing `onLocalDescription` → `onSubscriberAnswer` and the WS send to use `VoiceSubscriberAnswer`.
6. Route incoming `ServerEvent.VoiceIceCandidate` to `webRtcManager.addIceCandidate(pcType, candidate)`.
7. Wire publisher ICE candidates: `onPublisherIceCandidate { candidate -> wsSend(VoiceIceCandidate(channelId, candidate, pcType = "publisher")) }`.
8. Update existing subscriber ICE candidate handler to pass `pcType = "subscriber"`.

- [ ] **Step 2: Replace single-PC `ConnectionState.Connected` transition**

Instead of observing the old single-PC ICE state, observe `webRtcManager.voiceIceConnected`:

```kotlin
scope.launch {
    webRtcManager.voiceIceConnected.collect { bothConnected ->
        _connectionState.value = if (bothConnected) {
            ConnectionState.Connected
        } else {
            ConnectionState.Connecting  // or keep Connected until explicit disconnect
        }
    }
}
```

- [ ] **Step 3: Build**

```bash
cd mobile/android && ./gradlew compileDebugKotlin
```
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/repository/VoiceRepository.kt
git commit -m "feat(voice): route dual-PC signaling through VoiceRepository"
```

### Task E5: Tests for publisher PC and voiceIceConnected

**File:** `mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt`

- [ ] **Step 1: Add publisher-side tests**

Add tests mirroring existing subscriber tests:

```kotlin
@Test
fun `createPublisherOffer emits offer and sets local description`() = runTest {
    // Mock PeerConnectionFactory to return a fake PC.
    // Call createPublisherOffer.
    // Assert onPublisherOffer callback was invoked with the SDP string.
}

@Test
fun `handlePublisherAnswer drains publisher ICE candidate buffer`() = runTest {
    // Add publisher ICE candidates before handlePublisherAnswer.
    // Call handlePublisherAnswer(sdp).
    // Assert publisherPendingCandidates is cleared and candidates were applied to publisherPc.
}

@Test
fun `addIceCandidate routes by pcType`() = runTest {
    // Add ICE with pcType="publisher", assert publisher buffer grows.
    // Add ICE with pcType="subscriber", assert subscriber buffer grows.
    // Add ICE with invalid pcType, assert nothing happens + warning logged.
}

@Test
fun `voiceIceConnected emits true only when both PCs connect`() = runTest(TestCoroutineDispatcher()) {
    // Using TestCoroutineDispatcher for deterministic StateFlow behavior.
    // Set publisherIceState to CONNECTED — assert voiceIceConnected stays false.
    // Set subscriberIceState to CONNECTED — assert voiceIceConnected becomes true.
}
```

- [ ] **Step 2: Run tests**

```bash
cd mobile/android && ./gradlew test
```
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add mobile/android/app/src/test/java/io/wolftown/kaiku/data/voice/WebRtcManagerTest.kt
git commit -m "test(voice): add publisher PC and voiceIceConnected tests"
```

### Task E6: CHANGELOG

**File:** `CHANGELOG.md`

- [ ] **Step 1: Add entry**

In `## [Unreleased]` → `### Fixed`:
- Android microphone audio now reliably reaches other participants (added dedicated publisher PeerConnection)

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs(voice): CHANGELOG entry for PR E Android publisher PC"
```

---

## Final Verification (per PR)

After all tasks complete in a worktree, before pushing:

- [ ] **PR A:**
  - `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings` — clean
  - `cargo test -p vc-server` — passes including new tests
  - `git log --oneline main..HEAD` — ~8 commits
  - `git push origin fix/voice-server-security`

- [ ] **PR B:**
  - `cd client && bun run test:run && bun run build` — passes
  - `git log --oneline main..HEAD` — ~3 commits
  - `git push origin fix/web-voice-ice-buffering`

- [ ] **PR C:**
  - `cd client/src-tauri && cargo clippy -- -D warnings && cargo test` — clean
  - `git log --oneline main..HEAD` — ~3 commits
  - `git push origin fix/tauri-voice-rtp-protocol`

- [ ] **PR D:**
  - `cd client/src-tauri && cargo clippy -- -D warnings && cargo test` — clean
  - `cargo test --features decode-integration --test vp8_decode` — passes
  - `cd client && bun run test:run && bun run build` — passes
  - `git log --oneline main..HEAD` — ~9 commits
  - `git push origin feat/tauri-vp8-decode`

- [ ] **PR E:**
  - `cd mobile/android && ./gradlew test` — passes
  - `git log --oneline main..HEAD` — ~6 commits
  - `git push origin feat/android-publisher-pc`

All 5 PRs can then be opened on GitHub and merged in any order (no cross-PR dependencies). Recommended merge order for history clarity: A → B → C → D → E (smallest to largest).
