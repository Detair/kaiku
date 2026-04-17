//! VP8 Video Decoder
//!
//! Reads RTP packets from subscriber video tracks, depacketizes VP8 frames,
//! decodes them into YUV (I420) via libvpx, and pushes the result through a
//! `FrameBuffer` (drop-oldest watch channel) to the webview. The frontend
//! must call `subscribe_video_frames(stream_id, channel)` to register a
//! sink — until it does, the decode task drains incoming packets without
//! decoding (so the SFU does not see a stalled receiver).
//!
//! The native VP8 decoder + RTP depacketizer live in the `kaiku-vp8-decoder`
//! sub-crate (under `client/src-tauri/vp8-decoder/`) so the unsafe libvpx FFI
//! is isolated behind a locally relaxed `unsafe_code = "allow"` lint. This
//! module only orchestrates the decoder lifecycle on a tokio task and forwards
//! decoded frames into the Tauri-owned `FrameBuffer`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

// Re-export the sub-crate types so existing `crate::voice::video_decoder::X`
// call sites in commands/ and tests/ continue to compile unchanged.
pub use kaiku_vp8_decoder::{DecodedFrame, VideoDecodeError, Vp8VideoDecoder};
use tauri::Emitter;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
use webrtc::track::track_remote::TrackRemote;

use crate::voice::frame_buffer::FrameBuffer;

/// Maximum concurrent video decode streams.
///
/// Caps CPU/memory usage — excess tracks still receive metadata events
/// so the UI can show indicators, but packets are not read.
pub const MAX_VIDEO_STREAMS: usize = 2;

/// Video frame payload for frontend (not yet emitted by stub).
#[derive(Clone, serde::Serialize)]
pub struct VideoFrame {
    pub stream_id: String,
    pub width: u32,
    pub height: u32,
    /// I420 Y plane followed by U and V planes, concatenated.
    pub yuv: Vec<u8>,
    pub timestamp_us: u64,
}

/// Emitted when a remote video track starts/stops (for UI indicators).
#[derive(Clone, serde::Serialize)]
pub struct VideoTrackEvent {
    pub user_id: String,
    pub stream_id: String,
    pub source_type: String,
}

/// Deadline for the webview to register a FrameBuffer sink before the decoder
/// gives up and falls back to drain-only mode (keeps the SFU from backing up).
const SUBSCRIPTION_LOOKUP_DEADLINE: Duration = Duration::from_secs(5);

/// Spawn a tokio task that reads RTP packets from the given video track and
/// decodes VP8 frames into the caller's `FrameBuffer` sink (once registered).
///
/// The task terminates when the track ends. The sink slot for `stream_id` is
/// removed from `video_frame_sinks` on exit so a subsequent track reusing the
/// same stream ID starts fresh.
pub fn spawn_video_decode_task(
    track: Arc<TrackRemote>,
    user_id: String,
    source_type: String,
    stream_id: String,
    app: tauri::AppHandle,
    active_count: Arc<AtomicUsize>,
    video_frame_sinks: Arc<Mutex<HashMap<String, FrameBuffer>>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!(
            user_id = %user_id,
            source_type = %source_type,
            stream_id = %stream_id,
            "Video decode task started"
        );

        // Emit track-started event so frontend can show the indicator and
        // (for screen shares) call subscribe_video_frames in response.
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

        let mut buf = vec![0u8; 65_535];
        let mut packet_count: u64 = 0;
        // The decoder is paired with its FrameBuffer sink once the frontend
        // registers one; before that, packets are drained without decoding.
        let mut decoder_with_sink: Option<(Vp8VideoDecoder, FrameBuffer)> = None;
        // We start the deadline window when the first packet arrives, not at
        // task start — otherwise tracks that take a moment to deliver any
        // packets at all could miss the window.
        let mut subscription_deadline: Option<Instant> = None;
        let mut decoder_init_failed = false;

        loop {
            match track.read(&mut buf).await {
                Ok((rtp_packet, _attributes)) => {
                    let payload = &rtp_packet.payload[..];
                    if payload.is_empty() {
                        continue;
                    }

                    packet_count += 1;

                    if packet_count % 300 == 0 {
                        debug!(
                            user_id = %user_id,
                            packet_count,
                            payload_len = payload.len(),
                            decoder_active = decoder_with_sink.is_some(),
                            "Video track receiving packets"
                        );
                    }

                    // Lazy decoder activation: once a frontend subscription
                    // appears in the shared map, claim the FrameBuffer and
                    // build a decoder. Stop polling after the deadline or
                    // after a previous init failure to avoid hammering the
                    // mutex on every packet for the rest of the track's life.
                    if decoder_with_sink.is_none() && !decoder_init_failed {
                        let deadline = *subscription_deadline
                            .get_or_insert_with(|| Instant::now() + SUBSCRIPTION_LOOKUP_DEADLINE);
                        if Instant::now() < deadline {
                            let mut sinks = video_frame_sinks.lock().await;
                            if let Some(fb) = sinks.remove(&stream_id) {
                                drop(sinks);
                                match Vp8VideoDecoder::new(stream_id.clone()) {
                                    Ok(d) => {
                                        info!(
                                            stream_id = %stream_id,
                                            user_id = %user_id,
                                            "VP8 decoder activated for stream"
                                        );
                                        decoder_with_sink = Some((d, fb));
                                    }
                                    Err(e) => {
                                        warn!(
                                            error = %e,
                                            stream_id = %stream_id,
                                            "VP8 decoder init failed; falling back to drain-only"
                                        );
                                        decoder_init_failed = true;
                                    }
                                }
                            }
                        } else {
                            // Past deadline without a subscriber — log once
                            // and stop checking by treating as init-failed.
                            info!(
                                stream_id = %stream_id,
                                user_id = %user_id,
                                "No frame subscription within deadline; draining track"
                            );
                            decoder_init_failed = true;
                        }
                    }

                    if let Some((d, fb)) = decoder_with_sink.as_mut() {
                        match d.process_packet(&rtp_packet) {
                            Ok(frames) => {
                                // FrameBuffer is a watch channel: push never
                                // blocks and silently drops the previous
                                // unread frame (drop-oldest semantics).
                                for frame in frames {
                                    fb.push(frame);
                                }
                            }
                            Err(e) => {
                                // Decode errors are typically recoverable
                                // (e.g., reference frame loss). Log and
                                // continue — libvpx resyncs on the next
                                // keyframe.
                                warn!(
                                    error = %e,
                                    stream_id = %stream_id,
                                    packet_count,
                                    "VP8 decode error; continuing"
                                );
                            }
                        }
                    }
                    // No decoder: packet already consumed from the buffer,
                    // which is the SFU-friendly drain behavior we want.
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

        // Drop the decoder before emitting removal so vpx_codec_destroy runs
        // first (Drop on Vp8VideoDecoder), keeping teardown ordering clean.
        drop(decoder_with_sink);

        // If we had registered a sink but never picked it up, remove the
        // stale entry so the next stream with the same ID doesn't reuse it.
        video_frame_sinks.lock().await.remove(&stream_id);

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
