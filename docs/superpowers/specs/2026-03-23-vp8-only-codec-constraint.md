# VP8-Only Codec Constraint for Screen Share Video

**Date:** 2026-03-23
**Status:** Approved
**Branch:** `feature/dual-peerconnection`
**Depends on:** Dual PeerConnection architecture (implemented)

## Problem

The SFU forwards raw RTP packets between publisher and subscriber PeerConnections without rewriting the payload type (PT) header. When the publisher session negotiates a different PT for a video codec than the subscriber session, the receiver browser cannot decode the packets — resulting in black video. Audio works because Opus consistently negotiates PT 111 on both sides.

## Solution

Constrain all PeerConnections (server, Tauri client) to register only VP8 (PT 96) as the video codec. With a single video codec in the MediaEngine, both publisher and subscriber sessions always negotiate the same PT, eliminating the mismatch.

### Why VP8

- Universal browser support — VP8 is the WebRTC baseline codec
- No fmtp parameters — VP9 requires `profile-id=0` which adds SDP negotiation variables
- `vpx-encode` crate defaults to VP8 — VP9 requires a cargo feature flag
- Simpler RTP payload descriptor than VP9

### Why Not PT Rewriting

PT rewriting (mapping publisher PT to subscriber PT in `forward_rtp`) is the correct long-term fix for multi-codec support. VP8-only is a stepping stone that unblocks screen share now. PT rewriting can be added later when we want VP9/H.264 support.

## Changes

### 1. Server codec registration (`server/src/voice/sfu.rs`)

Remove VP9 (PT 98) and H.264 (PT 102) from `MediaEngine::register_codec` calls. Keep only:
- Opus audio (PT 111) — unchanged
- VP8 video (PT 96) with existing RTCP feedback (goog-remb, ccm fir, nack, nack pli)

### 2. Tauri client codec registration (`client/src-tauri/src/webrtc/mod.rs`)

Mirror the server change: remove VP9 and H.264 registrations, keep only VP8 (PT 96).

Change the video track creation for both screen share and webcam:
- `mime_type`: `"video/VP9"` → `"video/VP8"`
- `sdp_fmtp_line`: `"profile-id=0"` → `""` (VP8 has no fmtp)

### 3. Tauri encoder (`client/src-tauri/src/video/encoder.rs`)

- Rename `Vp9Encoder` → `Vp8Encoder`
- Change `VideoCodecId::VP9` → `VideoCodecId::VP8`
- Update `codec_mime()` → `"video/VP8"`
- Update `payload_type()` → `96`

### 4. Tauri RTP payloader (`client/src-tauri/src/video/rtp.rs`)

Replace VP9 payload descriptor with VP8 payload descriptor per RFC 7741 section 4.2.

**VP8 1-byte payload descriptor (MSB-first, network byte order):**
```
 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+
|X|R|N|S|R| PID |
+-+-+-+-+-+-+-+-+
```
- X (bit 7): Extended bits present — 0
- R (bit 6): Reserved — 0
- N (bit 5): Non-reference frame — 0
- S (bit 4): Start of VP8 partition — 1 for first packet of frame
- R (bit 3): Reserved — 0
- PID (bits 2-0): Partition index — 0

For the Tauri native path, this means:
- First packet of frame: `0x10` (S=1)
- Continuation packets: `0x00`

**RTP marker bit:** The marker bit must be set on the last RTP packet of each frame. `TrackLocalStaticRTP::write()` does not set it automatically. Switch to `write_rtp()` with an explicit `rtp::packet::Packet` where `header.marker = true` on the final fragment. This is required for VP8 decoders to detect frame boundaries.

### 5. Import updates

- `client/src-tauri/src/commands/screen_share.rs`: `Vp9Encoder` → `Vp8Encoder`
- `client/src-tauri/src/commands/webcam.rs`: `Vp9Encoder` → `Vp8Encoder`

### 6. Remove VP9 feature flag (`client/src-tauri/Cargo.toml`)

Change `vpx-encode = { version = "0.3", features = ["vp9"] }` to `vpx-encode = "0.3"`. VP8 is the default codec — the `vp9` feature is no longer needed and avoids linking dead VP9 code.

### 7. Module-level doc updates

- `client/src-tauri/src/video/mod.rs`: module doc string VP9 → VP8
- `client/src-tauri/src/video/rtp.rs`: module doc string and struct/fn docs VP9 → VP8

## What Does Not Change

- **`forward_rtp` in `track.rs`** — raw RTP forwarding logic stays the same; PT match is guaranteed by single-codec constraint
- **Browser `startScreenShare`** — browser `addTrack` triggers a browser-created offer that may list VP9 first (Chrome's default preference). The server's VP8-only MediaEngine filters out unregistered codecs during SDP negotiation and answers with only VP8. The browser accepts this and encodes as VP8. On the subscriber side, the server creates the offer with VP8-only, so the browser subscriber also uses VP8.
- **Signaling protocol** — no WebSocket message changes
- **Simulcast RID encodings** — unchanged (server skips layer filtering for non-simulcast sources)

## Testing

1. **Compile check:** `SQLX_OFFLINE=true cargo clippy -- -D warnings` for server and client
2. **Unit tests:** `cargo test` — encoder and RTP payloader tests must pass
3. **Manual verification on VPS:**
   - Browser A shares screen → Browser B sees video (not black)
   - Tauri client shares screen → browser sees video
   - Audio still works alongside screen share
   - Check browser `chrome://webrtc-internals` to confirm VP8 codec in use

## Future Work

- Add PT rewriting in `forward_rtp` to support multiple video codecs
- Re-enable VP9 and H.264 once PT rewriting is in place
- Hardware-accelerated encoding (H.264 via platform APIs)
