//! VP8 Video Decoder (Stub)
//!
//! Reads RTP packets from subscriber video tracks and emits lifecycle events
//! to the frontend. Actual VP8 frame decoding is not yet implemented — this
//! stub allows end-to-end pipeline testing while audio (the critical path)
//! is fully functional.
//!
//! Future implementation steps:
//! 1. VP8 RTP depacketization via `webrtc::rtp::codecs::vp8::Vp8Packet`
//! 2. VP8 frame decode to YUV via libvpx FFI (`vpx_codec_decode`)
//! 3. YUV → RGB conversion
//! 4. JPEG encode via `image` crate
//! 5. Base64 encode + emit as `VideoFrame` event
//! 6. Throttle to ~15 fps

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tauri::Emitter;
use tracing::{debug, info};
use webrtc::track::track_remote::TrackRemote;

/// Maximum concurrent video decode streams.
///
/// Caps CPU/memory usage — excess tracks still receive metadata events
/// so the UI can show indicators, but packets are not read.
pub const MAX_VIDEO_STREAMS: usize = 2;

/// Video frame payload for frontend (not yet emitted by stub).
#[derive(Clone, serde::Serialize)]
pub struct VideoFrame {
    pub user_id: String,
    pub stream_id: String,
    pub source_type: String,
    pub width: u32,
    pub height: u32,
    /// Base64-encoded JPEG data.
    pub data: String,
}

/// Video track metadata event sent to frontend on track start/stop.
#[derive(Clone, serde::Serialize)]
pub struct VideoTrackEvent {
    pub user_id: String,
    pub stream_id: String,
    pub source_type: String,
}

/// Spawn a background task that reads RTP packets from a video track.
///
/// Currently a **stub**: reads and counts packets, emits track lifecycle
/// events, but does not decode VP8 frames or produce `VideoFrame` events.
pub fn spawn_video_decode_task(
    track: Arc<TrackRemote>,
    user_id: String,
    source_type: String,
    stream_id: String,
    app: tauri::AppHandle,
    active_count: Arc<AtomicUsize>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!(
            user_id = %user_id,
            source_type = %source_type,
            stream_id = %stream_id,
            "Video decode task started"
        );

        // Emit track-started event so frontend can show the indicator
        let start_event = if source_type.starts_with("screen_video") {
            "voice:screen_share_track"
        } else {
            "voice:webcam_track"
        };
        let _ = app.emit(
            start_event,
            VideoTrackEvent {
                user_id: user_id.clone(),
                stream_id: stream_id.clone(),
                source_type: source_type.clone(),
            },
        );

        // Read RTP packets from the track (drains the buffer even though we
        // don't decode yet — important so the SFU doesn't think we stalled).
        let mut buf = vec![0u8; 65_535];
        let mut packet_count: u64 = 0;

        loop {
            match track.read(&mut buf).await {
                Ok((rtp_packet, _attributes)) => {
                    let payload = &rtp_packet.payload[..];
                    if payload.is_empty() {
                        continue;
                    }

                    packet_count += 1;

                    // Log progress every ~10 seconds at 30 fps (300 packets)
                    if packet_count % 300 == 0 {
                        debug!(
                            user_id = %user_id,
                            packet_count,
                            payload_len = payload.len(),
                            "Video track receiving packets"
                        );
                    }

                    // TODO: VP8 decode pipeline
                    // 1. Depacketize RTP → complete VP8 frames
                    //    (webrtc::rtp::codecs::vp8::Vp8Packet)
                    // 2. Decode VP8 → YUV (libvpx via vpx-encode FFI)
                    // 3. Convert YUV → RGB
                    // 4. JPEG encode (image crate)
                    // 5. Base64 encode
                    // 6. Emit VideoFrame event
                    // 7. Throttle to ~15 fps
                }
                Err(e) => {
                    info!(
                        user_id = %user_id,
                        error = %e,
                        packet_count,
                        "Video track ended"
                    );
                    break;
                }
            }
        }

        // Emit track-removed event so frontend hides the indicator
        let remove_event = if source_type.starts_with("screen_video") {
            "voice:screen_share_track_removed"
        } else {
            "voice:webcam_track_removed"
        };
        let _ = app.emit(
            remove_event,
            VideoTrackEvent {
                user_id: user_id.clone(),
                stream_id: stream_id.clone(),
                source_type: source_type.clone(),
            },
        );

        active_count.fetch_sub(1, Ordering::Relaxed);
        debug!(packet_count, "Video decode task ended");
    })
}
