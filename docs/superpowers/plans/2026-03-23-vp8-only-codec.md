# VP8-Only Codec Constraint — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix black screen share video by constraining all PeerConnections to VP8 only, eliminating RTP payload type mismatch between publisher and subscriber sessions.

**Architecture:** Remove VP9 and H.264 codec registrations from server and Tauri client MediaEngines, keeping only VP8 (PT 96). Switch the Tauri native encoder from VP9 to VP8 and rewrite the RTP payloader to use VP8 payload descriptors with explicit marker bits.

**Tech Stack:** Rust (webrtc-rs, vpx-encode), TypeScript (browser WebRTC — no changes needed)

**Spec:** `docs/superpowers/specs/2026-03-23-vp8-only-codec-constraint.md`

**Worktree:** `/home/detair/GIT/detair/kaiku/.claude/worktrees/dual-pc` (branch `feature/dual-peerconnection`)

---

### Task 1: Server — Remove VP9 and H.264 codec registrations

**Files:**
- Modify: `server/src/voice/sfu.rs:316-421`

- [ ] **Step 1: Remove VP9 and H.264 registrations**

In `server/src/voice/sfu.rs`, delete lines 316–421 (the VP9, VP8, and H.264 registration blocks) and replace with only the VP8 registration. The existing VP8 block (lines 351–384) has the correct structure — keep that, delete VP9 (316–349) and H.264 (386–421). Update the comment on the remaining block from "fallback" to "only video codec":

```rust
        // Register VP8 video codec (only video codec — ensures consistent PT across sessions)
        media_engine
            .register_codec(
                RTCRtpCodecParameters {
                    capability: RTCRtpCodecCapability {
                        mime_type: "video/VP8".to_string(),
                        clock_rate: 90000,
                        channels: 0,
                        sdp_fmtp_line: String::new(),
                        rtcp_feedback: vec![
                            RTCPFeedback {
                                typ: "goog-remb".to_string(),
                                parameter: String::new(),
                            },
                            RTCPFeedback {
                                typ: "ccm".to_string(),
                                parameter: "fir".to_string(),
                            },
                            RTCPFeedback {
                                typ: "nack".to_string(),
                                parameter: String::new(),
                            },
                            RTCPFeedback {
                                typ: "nack".to_string(),
                                parameter: "pli".to_string(),
                            },
                        ],
                    },
                    payload_type: 96,
                    ..Default::default()
                },
                RTPCodecType::Video,
            )
            .map_err(|e| VoiceError::WebRtc(e.to_string()))?;
```

- [ ] **Step 2: Verify server compiles**

Run: `cd /home/detair/GIT/detair/kaiku/.claude/worktrees/dual-pc && SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
Expected: compiles clean

- [ ] **Step 3: Commit**

```bash
git add server/src/voice/sfu.rs
git commit -m "fix(voice): constrain server to VP8-only video codec"
```

---

### Task 2: Tauri client — Remove VP9 and H.264 codec registrations + update track codecs

**Files:**
- Modify: `client/src-tauri/src/webrtc/mod.rs:157-230` (codec registrations)
- Modify: `client/src-tauri/src/webrtc/mod.rs:370-406` (video track creation)

- [ ] **Step 1: Replace codec registrations**

In `client/src-tauri/src/webrtc/mod.rs`, replace lines 157–230 (VP9 + VP8 + H.264 registrations) with VP8 only. The `video_rtcp_feedback` vec stays the same but is now used only once (no need for `.clone()`):

```rust
        // Register VP8 video codec (only video codec — matches server PT 96)
        let video_rtcp_feedback = vec![
            RTCPFeedback {
                typ: "goog-remb".to_string(),
                parameter: String::new(),
            },
            RTCPFeedback {
                typ: "ccm".to_string(),
                parameter: "fir".to_string(),
            },
            RTCPFeedback {
                typ: "nack".to_string(),
                parameter: String::new(),
            },
            RTCPFeedback {
                typ: "nack".to_string(),
                parameter: "pli".to_string(),
            },
        ];

        media_engine
            .register_codec(
                RTCRtpCodecParameters {
                    capability: RTCRtpCodecCapability {
                        mime_type: "video/VP8".to_string(),
                        clock_rate: 90000,
                        channels: 0,
                        sdp_fmtp_line: String::new(),
                        rtcp_feedback: video_rtcp_feedback,
                    },
                    payload_type: 96,
                    ..Default::default()
                },
                RTPCodecType::Video,
            )
            .map_err(|e| WebRtcError::ApiError(e.to_string()))?;
```

- [ ] **Step 2: Update video track creation — screen share and webcam**

Change both `TrackLocalStaticRTP::new()` calls for video tracks. Screen share track (~line 370):

```rust
        let video_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: "video/VP8".to_string(),
                clock_rate: 90000,
                channels: 0,
                sdp_fmtp_line: String::new(),
                rtcp_feedback: vec![],
            },
            "screen-video".to_string(),
            "screen-share-stream".to_string(),
        ));
```

Webcam track (~line 396):

```rust
        let webcam_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: "video/VP8".to_string(),
                clock_rate: 90000,
                channels: 0,
                sdp_fmtp_line: String::new(),
                rtcp_feedback: vec![],
            },
            "webcam-video".to_string(),
            "webcam-stream".to_string(),
        ));
```

- [ ] **Step 3: Verify Tauri client compiles**

Run: `cd /home/detair/GIT/detair/kaiku/.claude/worktrees/dual-pc && SQLX_OFFLINE=true cargo clippy -p vc-client -- -D warnings`
Expected: compiles clean (encoder changes come next, so there may be warnings about VP9 — that's OK for now)

- [ ] **Step 4: Commit**

```bash
git add client/src-tauri/src/webrtc/mod.rs
git commit -m "fix(voice): constrain Tauri client to VP8-only video codec"
```

---

### Task 3: Tauri encoder — Switch from VP9 to VP8

**Files:**
- Modify: `client/src-tauri/src/video/encoder.rs` (full file)
- Modify: `client/src-tauri/src/commands/screen_share.rs:16,152` (import + usage)
- Modify: `client/src-tauri/src/commands/webcam.rs:10,107` (import + usage)
- Modify: `client/src-tauri/Cargo.toml:65` (remove vp9 feature)

- [ ] **Step 1: Rename Vp9Encoder to Vp8Encoder and switch codec**

In `client/src-tauri/src/video/encoder.rs`, make these changes:

1. Module doc: `VP9 software encoding` → `VP8 software encoding`
2. Struct doc: `VP9 encoder using libvpx` → `VP8 encoder using libvpx`
3. Rename `Vp9Encoder` → `Vp8Encoder`
4. In `new()`: `VideoCodecId::VP9` → `VideoCodecId::VP8`
5. In `new()`: error message `"VP9 encoder: {e}"` → `"VP8 encoder: {e}"`
6. In `new()`: tracing message `"VP9 encoder initialized"` → `"VP8 encoder initialized"`
7. In `encode()`: error message `"VP9 encode: {e}"` → `"VP8 encode: {e}"`
8. `codec_mime()` → `"video/VP8"`
9. `payload_type()` → `96` with comment `// Matches server sfu.rs VP8 payload type`

- [ ] **Step 2: Update imports in screen_share.rs and webcam.rs**

In `client/src-tauri/src/commands/screen_share.rs`:
- Line 16: `Vp9Encoder` → `Vp8Encoder`
- Line 152: `Vp9Encoder::new` → `Vp8Encoder::new`
- Line 155: `"Failed to create VP9 encoder: {e}"` → `"Failed to create VP8 encoder: {e}"`

In `client/src-tauri/src/commands/webcam.rs`:
- Line 10: `Vp9Encoder` → `Vp8Encoder`
- Line 107: `Vp9Encoder::new` → `Vp8Encoder::new`
- Line 110: error message `"VP9"` → `"VP8"` (check exact text)

- [ ] **Step 3: Remove vp9 feature from Cargo.toml**

In `client/src-tauri/Cargo.toml` line 65, change:
```toml
vpx-encode = { version = "0.3", features = ["vp9"] }
```
to:
```toml
vpx-encode = "0.3"
```

- [ ] **Step 4: Verify compilation**

Run: `cd /home/detair/GIT/detair/kaiku/.claude/worktrees/dual-pc && SQLX_OFFLINE=true cargo clippy -p vc-client -- -D warnings`
Expected: compiles clean

- [ ] **Step 5: Commit**

```bash
git add client/src-tauri/src/video/encoder.rs client/src-tauri/src/commands/screen_share.rs client/src-tauri/src/commands/webcam.rs client/src-tauri/Cargo.toml
git commit -m "fix(voice): switch Tauri encoder from VP9 to VP8"
```

---

### Task 4: Tauri RTP payloader — VP8 payload descriptor + marker bit

This is the most critical task. The current payloader uses VP9 descriptors and `write()` (which doesn't set the RTP marker bit). VP8 decoders need the marker bit on the last packet of each frame.

**Files:**
- Modify: `client/src-tauri/src/video/rtp.rs` (full rewrite)
- Modify: `client/src-tauri/src/video/mod.rs:1-3` (module doc)

- [ ] **Step 1: Write failing tests for VP8 payload descriptor**

Replace the existing tests in `client/src-tauri/src/video/rtp.rs` with VP8-specific tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vp8_descriptor_first_packet() {
        let desc = build_vp8_payload_descriptor(true);
        assert_eq!(desc, 0x10, "S bit should be set for first packet");
    }

    #[test]
    fn vp8_descriptor_continuation_packet() {
        let desc = build_vp8_payload_descriptor(false);
        assert_eq!(desc, 0x00, "No bits set for continuation packet");
    }
}
```

- [ ] **Step 2: Rewrite the payload descriptor function**

Replace `build_vp9_payload_descriptor` with `build_vp8_payload_descriptor`:

```rust
/// VP8 RTP payload descriptor (1-byte, no extensions).
///
/// Layout (MSB-first):
///   X=0 | R=0 | N=0 | S | R=0 | PID=000
///
/// S (bit 4): Start of VP8 partition — 1 for first packet of frame.
/// All other bits are 0 (no extensions, single partition).
const fn build_vp8_payload_descriptor(is_first: bool) -> u8 {
    if is_first { 0x10 } else { 0x00 }
}
```

- [ ] **Step 3: Run tests to verify descriptor**

Run: `cd /home/detair/GIT/detair/kaiku/.claude/worktrees/dual-pc && cargo test -p vc-client -- video::rtp`
Expected: 2 tests pass

- [ ] **Step 4: Rewrite send_packet to use write_rtp with marker bit**

Replace the full `VideoRtpSender` impl. The key changes:
1. Use `write_rtp()` instead of `write()` to control the RTP header
2. Set `header.marker = true` on the last packet of each frame
3. Set `header.version = 2` (RFC 3550 requires this — `Default` gives 0)
4. Track sequence number with `AtomicU16` on the struct
5. Use `build_vp8_payload_descriptor` instead of VP9 variant

**Important:** `write_rtp` is on the `TrackLocalWriter` trait — keep the existing import `use webrtc::track::track_local::TrackLocalWriter;`. The complete import block for the file:

```rust
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;

use tracing::trace;
use webrtc::rtp::header::Header;
use webrtc::rtp::packet::Packet as RtpPacket;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocalWriter;

use super::{EncodedPacket, VideoError};
```

New struct definition (add `seq` field):

```rust
/// Sends VP8 encoded video as RTP packets to a WebRTC track.
pub struct VideoRtpSender {
    track: Arc<TrackLocalStaticRTP>,
    seq: AtomicU16,
}

impl VideoRtpSender {
    /// Create a new RTP sender for the given video track.
    pub fn new(track: Arc<TrackLocalStaticRTP>) -> Self {
        Self {
            track,
            seq: AtomicU16::new(0),
        }
    }
```

Note: `new` can no longer be `const fn` because `AtomicU16::new` in a `const fn` requires the struct to also be constructed in a const context, which isn't needed here.

New `send_packet` implementation:

```rust
    /// Send an encoded packet as one or more RTP packets.
    ///
    /// Large frames are fragmented at `MAX_PAYLOAD_SIZE` boundaries.
    /// Uses `write_rtp()` to set the RTP marker bit on the last fragment,
    /// which VP8 decoders need for frame boundary detection.
    pub async fn send_packet(&self, packet: &EncodedPacket) -> Result<(), VideoError> {
        let data = &packet.data;
        let timestamp = packet.pts as u32;

        if data.is_empty() {
            return Ok(());
        }

        // Fragment into MTU-sized chunks
        let chunks: Vec<&[u8]> = data.chunks(MAX_PAYLOAD_SIZE).collect();
        let total_chunks = chunks.len();

        for (i, chunk) in chunks.iter().enumerate() {
            let is_first = i == 0;
            let is_last = i == total_chunks - 1;

            let descriptor = build_vp8_payload_descriptor(is_first);

            // Build payload: 1 byte descriptor + encoded data
            let mut payload = Vec::with_capacity(1 + chunk.len());
            payload.push(descriptor);
            payload.extend_from_slice(chunk);

            let seq = self.seq.fetch_add(1, Ordering::Relaxed);

            let rtp_packet = RtpPacket {
                header: Header {
                    version: 2,
                    marker: is_last,
                    sequence_number: seq,
                    timestamp,
                    ..Default::default()
                },
                payload: payload.into(),
            };

            self.track
                .write_rtp(&rtp_packet)
                .await
                .map_err(|e| VideoError::RtpSendFailed(e.to_string()))?;

            trace!(
                ts = timestamp,
                seq = seq,
                len = chunk.len() + 1,
                first = is_first,
                last = is_last,
                keyframe = packet.is_keyframe,
                "Sent VP8 RTP packet"
            );
        }

        Ok(())
    }
```

Note: `ssrc` and `payload_type` are left as default (0) because `TrackLocalStaticRTP::write_rtp` overwrites them from the negotiated binding before sending.

- [ ] **Step 5: Update module docs**

In `client/src-tauri/src/video/rtp.rs`, update lines 1–4:
```rust
//! VP8 RTP Payloader
//!
//! Packetizes VP8 encoded data into RTP packets per RFC 7741.
//! Sends via `TrackLocalStaticRTP` with 90kHz clock and 1200-byte MTU.
```

In `client/src-tauri/src/video/mod.rs`, update line 3:
```rust
//! VP8 software encoding and RTP packetization for screen sharing.
```

Also update `VideoRtpSender` struct doc:
```rust
/// Sends VP8 encoded video as RTP packets to a WebRTC track.
```

- [ ] **Step 6: Verify full compilation**

Run: `cd /home/detair/GIT/detair/kaiku/.claude/worktrees/dual-pc && SQLX_OFFLINE=true cargo clippy -p vc-client -- -D warnings`
Expected: compiles clean

- [ ] **Step 7: Run all client tests**

Run: `cd /home/detair/GIT/detair/kaiku/.claude/worktrees/dual-pc && cargo test -p vc-client`
Expected: all tests pass

- [ ] **Step 8: Commit**

```bash
git add client/src-tauri/src/video/rtp.rs client/src-tauri/src/video/mod.rs
git commit -m "fix(voice): VP8 RTP payloader with marker bit for frame boundaries"
```

---

### Task 5: Update CHANGELOG.md

**Files:**
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Add entry under `[Unreleased]` → `### Fixed`**

Add to the `### Fixed` section (create if absent):

```markdown
- Screen share video now renders correctly instead of showing black (switched to VP8-only codec to fix RTP payload type mismatch between publisher/subscriber sessions)
```

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs: changelog entry for VP8 screen share fix"
```

---

### Task 6: Full workspace verification

- [ ] **Step 1: Clippy on full workspace**

Run: `cd /home/detair/GIT/detair/kaiku/.claude/worktrees/dual-pc && SQLX_OFFLINE=true cargo clippy -- -D warnings`
Expected: no errors or warnings

- [ ] **Step 2: Run all Rust tests**

Run: `cd /home/detair/GIT/detair/kaiku/.claude/worktrees/dual-pc && cargo test`
Expected: all tests pass

- [ ] **Step 3: Run client frontend tests**

Run: `cd /home/detair/GIT/detair/kaiku/.claude/worktrees/dual-pc/client && bun run test:run`
Expected: all tests pass (no frontend changes, but verify nothing broke)
