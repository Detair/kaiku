# Screen Share RTP Video Fix

**Date:** 2026-03-22
**Status:** Next step
**Depends on:** PR #485 (dual PeerConnection) — signaling works, RTP forwarding works, but video renders black
**Branch:** `feature/dual-peerconnection`

## Problem

Screen share video track reaches the subscriber browser but renders as a black picture. The SFU's `forward_rtp` copies raw RTP packets between publisher and subscriber sessions. Audio (Opus) works because both sessions typically negotiate the same payload type (PT 111). Video codecs (VP8/VP9/H264) may negotiate different PTs between the two sessions — the browser can't decode because the PT in the RTP header doesn't match any codec in the subscriber's SDP.

## Evidence (2026-03-22 VPS test)

- Publisher: browser captures screen, `addTrack` on publisher PC, offer/answer succeeds
- Server: `on_track` fires, RTP forwarder active with 3000+ packets, 1 subscriber
- Subscriber: browser receives track via `ontrack`, `ScreenShareViewer` creates `MediaStream` and sets `srcObject` on `<video>` element
- Result: black picture despite RTP packets flowing

## Approaches (investigate in order)

### Approach 1: Force single video codec (simplest, get it working)

Configure the webrtc-rs API to register only ONE video codec (e.g., VP8) on both publisher and subscriber PCs. This ensures the same PT is negotiated on both sides, making raw RTP forwarding work without rewriting.

**Where:** `sfu.rs` `SfuServer::new()` (~line 294) — the codec registration section. Currently registers VP9, VP8, and H.264. Change to register only VP8 (most widely supported, simplest).

```rust
// Only VP8 for now — ensures consistent PT across publisher/subscriber sessions
m.register_codec(
    RTCRtpCodecParameters {
        capability: RTCRtpCodecCapability {
            mime_type: MIME_TYPE_VP8.to_owned(),
            clock_rate: 90000,
            channels: 0,
            sdp_fmtp_line: String::new(),
            rtcp_feedback: vec![
                RTCPRtcpFeedback { typ: "goog-remb".to_owned(), parameter: String::new() },
                RTCPRtcpFeedback { typ: "ccm".to_owned(), parameter: "fir".to_owned() },
                RTCPRtcpFeedback { typ: "nack".to_owned(), parameter: String::new() },
                RTCPRtcpFeedback { typ: "nack".to_owned(), parameter: "pli".to_owned() },
            ],
        },
        payload_type: 96,
        ..Default::default()
    },
    RTPCodecType::Video,
)?;
```

**Risk:** Low — VP8 is universally supported. Reduces quality options but gets video working.

### Approach 2: Rewrite RTP payload type in `forward_rtp`

Store the publisher's negotiated PT and the subscriber's negotiated PT per codec. In `forward_rtp`, rewrite the RTP header's payload_type field before writing to the subscriber's local track.

**Where:** `track.rs` `forward_rtp()` and `Subscription` struct — add `payload_type_map: HashMap<u8, u8>` mapping publisher PT → subscriber PT.

**Risk:** Medium — need to extract negotiated PTs from both SDP sessions. More complex but supports multi-codec.

### Approach 3: Use `TrackLocalStaticSample` instead of `TrackLocalStaticRTP`

Re-packetize media through the sample-level API rather than forwarding raw RTP. This handles PT mapping automatically but adds latency (depacketize → repacketize).

**Risk:** Higher latency, more CPU. Not recommended for real-time SFU.

## Recommended Path

1. **Start with Approach 1** (single VP8 codec) — get screen share working end-to-end
2. **Then Approach 2** (PT rewriting) — add multi-codec support later

## Verification

After fix, re-run on VPS:
1. Two users join voice channel
2. User A starts screen share
3. User B sees actual screen content (not black)
4. Check server logs: no "Auto-switched simulcast layer" for non-simulcast sources
