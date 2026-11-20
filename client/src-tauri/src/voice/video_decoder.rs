//! VP8 Video Decoder
//!
//! Reads RTP packets from subscriber video tracks, depacketizes VP8 frames,
//! decodes them into YUV (I420) via libvpx and hands each frame to every
//! open view of the stream ([`FrameSinks`]). The views are looked up for
//! every packet: a view opened late, re-opened after a layout change, or
//! closed (`unsubscribe_video_frames`) is seen at once. While nobody
//! watches, packets are read and dropped without decoding (so the SFU does
//! not see a stalled receiver). When the first view appears the decoder
//! starts fresh and a key frame is requested on the subscriber connection.
//! The server forwards the request to the sender (at most once per second
//! per track), so the picture appears within about a second; with an older
//! server it appears with the sender's periodic key frame (at least every
//! 3 s, also of a still screen: `video::publication::KEYFRAME_PERIOD`).
//!
//! The native VP8 decoder + RTP depacketizer live in the `kaiku-vp8-decoder`
//! sub-crate (under `client/src-tauri/vp8-decoder/`) so the unsafe libvpx FFI
//! is isolated behind a locally relaxed `unsafe_code = "allow"` lint.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

// Re-export the sub-crate types so existing `crate::voice::video_decoder::X`
// call sites in commands/ and tests/ continue to compile unchanged.
pub use kaiku_vp8_decoder::{DecodedFrame, Vp8VideoDecoder};
use tauri::Emitter;
use tracing::{debug, info, warn};
use webrtc::rtp::packet::Packet as RtpPacket;
use webrtc::track::track_remote::TrackRemote;

use crate::voice::frame_buffer::FrameSinks;

/// Maximum concurrent video decode streams.
///
/// Caps CPU/memory usage — excess tracks still receive metadata events
/// so the UI can show indicators, but packets are not read.
pub const MAX_VIDEO_STREAMS: usize = 2;

/// Emitted when a remote video track starts/stops (for UI indicators).
#[derive(Clone, serde::Serialize)]
pub struct VideoTrackEvent {
    pub user_id: String,
    pub stream_id: String,
    pub source_type: String,
}

/// A packet-to-frames decoder (a seam for tests; the app uses VP8).
pub trait PacketDecoder {
    fn process(&mut self, packet: &RtpPacket) -> Result<Vec<DecodedFrame>, String>;
}

impl PacketDecoder for Vp8VideoDecoder {
    fn process(&mut self, packet: &RtpPacket) -> Result<Vec<DecodedFrame>, String> {
        self.process_packet(packet).map_err(|e| e.to_string())
    }
}

/// Routes the packets of one track to the views of its stream.
pub struct DecodeRouter<D, F, K>
where
    F: FnMut() -> Option<D>,
    K: FnMut(),
{
    stream_id: String,
    sinks: FrameSinks,
    new_decoder: F,
    request_keyframe: K,
    decoder: Option<D>,
    watched: bool,
}

impl<D, F, K> DecodeRouter<D, F, K>
where
    D: PacketDecoder,
    F: FnMut() -> Option<D>,
    K: FnMut(),
{
    pub const fn new(
        stream_id: String,
        sinks: FrameSinks,
        new_decoder: F,
        request_keyframe: K,
    ) -> Self {
        Self {
            stream_id,
            sinks,
            new_decoder,
            request_keyframe,
            decoder: None,
            watched: false,
        }
    }

    /// Handle one packet; returns how many frames reached views.
    pub fn on_packet(&mut self, packet: &RtpPacket) -> usize {
        let views = self.sinks.sinks_for(&self.stream_id);
        if views.is_empty() {
            // Nobody watches: drop the decoder state, decode nothing.
            self.watched = false;
            self.decoder = None;
            return 0;
        }
        if !self.watched {
            self.watched = true;
            // A fresh decoder: old reference frames are gone anyway.
            self.decoder = (self.new_decoder)();
            (self.request_keyframe)();
        }
        let Some(decoder) = self.decoder.as_mut() else {
            return 0;
        };
        match decoder.process(packet) {
            Ok(frames) => {
                let mut delivered = 0;
                for frame in frames {
                    // One shared frame for every view: no copies of the planes.
                    let frame = Arc::new(frame);
                    for view in &views {
                        view.push(frame.clone());
                    }
                    delivered += 1;
                }
                delivered
            }
            Err(e) => {
                // Usually delta frames before the first key frame.
                debug!(stream_id = %self.stream_id, error = %e, "VP8 decode error; continuing");
                0
            }
        }
    }
}

/// Spawn a tokio task that reads RTP packets from the given video track and
/// routes them to the stream's views.
#[allow(clippy::too_many_arguments)]
pub fn spawn_video_decode_task(
    track: Arc<TrackRemote>,
    user_id: String,
    source_type: String,
    stream_id: String,
    app: tauri::AppHandle,
    active_count: Arc<AtomicUsize>,
    sinks: FrameSinks,
    request_keyframe: Arc<dyn Fn(u32) + Send + Sync>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!(user_id = %user_id, source_type = %source_type, stream_id = %stream_id, "Video decode task started");

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

        let ssrc = track.ssrc();
        let decoder_stream = stream_id.clone();
        let mut router = DecodeRouter::new(
            stream_id.clone(),
            sinks,
            move || match Vp8VideoDecoder::new(decoder_stream.clone()) {
                Ok(d) => Some(d),
                Err(e) => {
                    warn!(error = %e, "VP8 decoder init failed");
                    None
                }
            },
            move || request_keyframe(ssrc),
        );

        let mut buf = vec![0u8; 65_535];
        let mut packet_count: u64 = 0;
        loop {
            match track.read(&mut buf).await {
                Ok((rtp_packet, _attributes)) => {
                    if rtp_packet.payload.is_empty() {
                        continue;
                    }
                    packet_count += 1;
                    router.on_packet(&rtp_packet);
                }
                Err(e) => {
                    info!(user_id = %user_id, error = %e, packet_count, "Video track ended");
                    break;
                }
            }
        }

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

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::voice::frame_buffer::FrameBuffer;

    struct OneFramePerPacket;
    impl PacketDecoder for OneFramePerPacket {
        fn process(&mut self, packet: &RtpPacket) -> Result<Vec<DecodedFrame>, String> {
            Ok(vec![DecodedFrame {
                stream_id: "s".into(),
                width: 2,
                height: 2,
                y_plane: vec![0; 4],
                u_plane: vec![0],
                v_plane: vec![0],
                y_stride: 2,
                uv_stride: 1,
                pts_ms: u64::from(packet.header.sequence_number),
            }])
        }
    }

    fn packet(seq: u16) -> RtpPacket {
        let mut p = RtpPacket::default();
        p.header.sequence_number = seq;
        p.payload = vec![1].into();
        p
    }

    fn latest(rx: &tokio::sync::watch::Receiver<Option<Arc<DecodedFrame>>>) -> Option<u64> {
        rx.borrow().as_ref().map(|f| f.pts_ms)
    }

    #[test]
    fn frames_follow_the_views_looked_up_for_every_packet() {
        let sinks = FrameSinks::default();
        let made = Rc::new(Cell::new(0));
        let keyframes = Rc::new(Cell::new(0));
        let (m, k) = (made.clone(), keyframes.clone());
        let mut router = DecodeRouter::new(
            "s1".into(),
            sinks.clone(),
            move || {
                m.set(m.get() + 1);
                Some(OneFramePerPacket)
            },
            move || k.set(k.get() + 1),
        );

        // Nobody watches yet (tile opens late): nothing decoded.
        assert_eq!(router.on_packet(&packet(1)), 0);
        assert_eq!(made.get(), 0);

        // The tile opens: decoder starts, one key frame request.
        let (fb, rx) = FrameBuffer::new();
        let (first, _) = sinks.subscribe("s1", "page", fb);
        assert_eq!(router.on_packet(&packet(2)), 1);
        assert_eq!(latest(&rx), Some(2));
        assert_eq!((made.get(), keyframes.get()), (1, 1));

        // Layout change: a second view of the same stream gets frames at once.
        let (fb2, rx2) = FrameBuffer::new();
        let (second, _) = sinks.subscribe("s1", "page", fb2);
        router.on_packet(&packet(3));
        assert_eq!(latest(&rx2), Some(3));
        assert_eq!(keyframes.get(), 1, "no new request while watched");

        // The old view closes; the new one keeps getting frames.
        assert!(sinks.unsubscribe("s1", first));
        router.on_packet(&packet(4));
        assert_eq!(latest(&rx2), Some(4));
        assert_eq!(latest(&rx), Some(3), "closed view gets nothing more");

        // All views closed: decoding stops; reopening asks for a key frame again.
        assert!(sinks.unsubscribe("s1", second));
        assert_eq!(router.on_packet(&packet(5)), 0);
        let (fb3, rx3) = FrameBuffer::new();
        sinks.subscribe("s1", "page", fb3);
        router.on_packet(&packet(6));
        assert_eq!(latest(&rx3), Some(6));
        assert_eq!((made.get(), keyframes.get()), (2, 2));
    }

    #[test]
    fn views_of_another_stream_get_nothing() {
        let sinks = FrameSinks::default();
        let (fb, rx) = FrameBuffer::new();
        sinks.subscribe("other", "page", fb);
        let mut router = DecodeRouter::new("s1".into(), sinks, || Some(OneFramePerPacket), || {});
        assert_eq!(router.on_packet(&packet(1)), 0);
        assert_eq!(latest(&rx), None);
    }
}
