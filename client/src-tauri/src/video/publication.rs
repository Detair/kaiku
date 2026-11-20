//! One published video (a screen share or the webcam).
//!
//! Owns everything of the publication: the capture thread's frames, the
//! encoder thread, the RTP task and the RTCP reader of its own sender. All
//! of them end with the publication (`shutdown`) or when the capture ends by
//! itself; then `on_capture_end` tells the frontend so it runs the normal
//! stop (server told, UI reset).
//!
//! Real-time rules: the capture thread only `try_send`s into a channel of
//! two frames (drops when the encoder is behind); the encoder thread waits
//! at most 16 ms per frame and never touches IPC; packets go to the async
//! RTP task through a bounded channel.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, watch};
use tracing::{debug, error, info, warn};
use webrtc::rtcp::packet::Packet as RtcpPacket;
use webrtc::rtcp::payload_feedbacks::full_intra_request::FullIntraRequest;
use webrtc::rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication;
use webrtc::rtp_transceiver::rtp_sender::RTCRtpSender;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;

use super::encoder::{VideoEncoder, Vp8Encoder};
use super::rtp::VideoRtpSender;
use super::{EncodedPacket, QualityParams};
use crate::capture::I420Frame;

/// Fewest time between two forced key frames.
pub const KEYFRAME_MIN_INTERVAL: Duration = Duration::from_secs(1);

/// Key frame requests from RTCP (PLI/FIR), rate-limited.
#[derive(Debug)]
pub struct KeyframeGate {
    requested: AtomicBool,
    last_forced: Mutex<Option<Instant>>,
    min_interval: Duration,
}

impl KeyframeGate {
    pub const fn new(min_interval: Duration) -> Self {
        Self {
            requested: AtomicBool::new(false),
            last_forced: Mutex::new(None),
            min_interval,
        }
    }

    /// A viewer asked for a key frame.
    pub fn request(&self) {
        self.requested.store(true, Ordering::Release);
    }

    /// Whether the encoder should produce a key frame now. A request inside
    /// the minimum interval stays pending until the interval has passed.
    pub fn take_due(&self, now: Instant) -> bool {
        if !self.requested.load(Ordering::Acquire) {
            return false;
        }
        let Ok(mut last) = self.last_forced.lock() else {
            return false;
        };
        if last.is_some_and(|at| now.duration_since(at) < self.min_interval) {
            return false;
        }
        self.requested.store(false, Ordering::Release);
        *last = Some(now);
        true
    }
}

/// Whether an RTCP packet asks the sender for a key frame.
pub fn is_keyframe_request(packet: &(dyn RtcpPacket + Send + Sync)) -> bool {
    let any = packet.as_any();
    any.downcast_ref::<PictureLossIndication>().is_some()
        || any.downcast_ref::<FullIntraRequest>().is_some()
}

/// Something that encodes frames and can be asked for a key frame (a seam
/// for tests; the app uses [`Vp8Encoder`]).
pub trait PublicationEncoder {
    fn encode(&mut self, frame: &I420Frame) -> Result<Vec<EncodedPacket>, String>;
    fn force_keyframe(&mut self) -> Result<(), String>;
}

impl PublicationEncoder for Vp8Encoder {
    fn encode(&mut self, frame: &I420Frame) -> Result<Vec<EncodedPacket>, String> {
        VideoEncoder::encode(self, frame).map_err(|e| e.to_string())
    }

    fn force_keyframe(&mut self) -> Result<(), String> {
        Self::force_keyframe(self).map_err(|e| e.to_string())
    }
}

/// How the encoder loop ended.
#[derive(Debug, PartialEq, Eq)]
pub enum EncodeLoopEnd {
    /// `shutdown` was asked for.
    Shutdown,
    /// The capture stopped delivering (window closed, permission revoked,
    /// portal session ended, camera gone).
    CaptureEnded,
    /// The packet channel closed (RTP task gone).
    SinkClosed,
}

/// The encoder thread's loop: frames in, packets out, key frames on request.
pub fn run_encode_loop<E: PublicationEncoder>(
    encoder: &mut E,
    frames: &std_mpsc::Receiver<I420Frame>,
    packets: &mpsc::Sender<Vec<EncodedPacket>>,
    keyframes: &KeyframeGate,
    shutdown: &watch::Receiver<bool>,
) -> EncodeLoopEnd {
    loop {
        if *shutdown.borrow() {
            return EncodeLoopEnd::Shutdown;
        }
        match frames.recv_timeout(Duration::from_millis(16)) {
            Ok(frame) => {
                if keyframes.take_due(Instant::now()) {
                    if let Err(e) = encoder.force_keyframe() {
                        warn!("Key frame could not be forced: {e}");
                    }
                }
                match encoder.encode(&frame) {
                    Ok(out) if out.is_empty() => {}
                    Ok(out) => {
                        if packets.blocking_send(out).is_err() {
                            return EncodeLoopEnd::SinkClosed;
                        }
                    }
                    Err(e) => warn!("Encode error: {e}"),
                }
            }
            Err(std_mpsc::RecvTimeoutError::Timeout) => {}
            Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                return if *shutdown.borrow() {
                    EncodeLoopEnd::Shutdown
                } else {
                    EncodeLoopEnd::CaptureEnded
                };
            }
        }
    }
}

/// Starts the capture: gets the frame sender and the shutdown signal,
/// returns the capture thread's handle.
pub type StartCapture = Box<
    dyn FnOnce(
            std_mpsc::SyncSender<I420Frame>,
            watch::Receiver<bool>,
        ) -> Result<tokio::task::JoinHandle<()>, String>
        + Send,
>;

/// A running publication.
pub struct VideoPublication {
    pub sender: Arc<RTCRtpSender>,
    shutdown_tx: watch::Sender<bool>,
    handles: Vec<tokio::task::JoinHandle<()>>,
}

impl VideoPublication {
    /// Start capture → encode → RTP plus the RTCP reader for `sender`.
    /// `on_capture_end` runs once if the capture ends by itself.
    pub fn start(
        params: QualityParams,
        track: Arc<TrackLocalStaticRTP>,
        sender: Arc<RTCRtpSender>,
        start_capture: StartCapture,
        on_capture_end: Box<dyn FnOnce() + Send>,
    ) -> Result<Self, String> {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (frame_tx, frame_rx) = std_mpsc::sync_channel::<I420Frame>(2);
        let capture = start_capture(frame_tx, shutdown_tx.subscribe())?;
        let keyframes = Arc::new(KeyframeGate::new(KEYFRAME_MIN_INTERVAL));
        let (pkt_tx, mut pkt_rx) = mpsc::channel::<Vec<EncodedPacket>>(4);

        let encoder = {
            let keyframes = keyframes.clone();
            let shutdown = shutdown_rx.clone();
            tokio::task::spawn_blocking(move || {
                let mut encoder = match Vp8Encoder::new(&params) {
                    Ok(e) => e,
                    Err(e) => {
                        error!("Failed to create VP8 encoder: {e}");
                        return;
                    }
                };
                let end = run_encode_loop(&mut encoder, &frame_rx, &pkt_tx, &keyframes, &shutdown);
                info!(?end, "Video encoder stopped");
                if end == EncodeLoopEnd::CaptureEnded {
                    on_capture_end();
                }
            })
        };

        let rtp = tokio::spawn(async move {
            let rtp_sender = VideoRtpSender::new(track);
            while let Some(packets) = pkt_rx.recv().await {
                for pkt in &packets {
                    if let Err(e) = rtp_sender.send_packet(pkt).await {
                        debug!("RTP send error: {e}");
                    }
                }
            }
        });

        let rtcp = {
            let sender = sender.clone();
            let mut shutdown = shutdown_rx;
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = shutdown.changed() => break,
                        read = sender.read_rtcp() => match read {
                            Ok((packets, _)) => {
                                if packets.iter().any(|p| is_keyframe_request(p.as_ref())) {
                                    keyframes.request();
                                }
                            }
                            // The sender was removed or stopped.
                            Err(_) => break,
                        },
                    }
                }
            })
        };

        Ok(Self {
            sender,
            shutdown_tx,
            handles: vec![capture, encoder, rtp, rtcp],
        })
    }

    /// Stop every task of the publication (bounded wait).
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(true);
        let wait = async {
            for handle in self.handles {
                let _ = handle.await;
            }
        };
        if tokio::time::timeout(Duration::from_secs(3), wait)
            .await
            .is_err()
        {
            warn!("Video publication did not stop within 3 s");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> I420Frame {
        I420Frame {
            y: vec![0; 4],
            u: vec![0; 1],
            v: vec![0; 1],
            width: 2,
            height: 2,
        }
    }

    #[derive(Default)]
    struct FakeEncoder {
        encoded: usize,
        forced: usize,
    }

    impl PublicationEncoder for FakeEncoder {
        fn encode(&mut self, _frame: &I420Frame) -> Result<Vec<EncodedPacket>, String> {
            self.encoded += 1;
            Ok(vec![EncodedPacket {
                data: vec![1],
                is_keyframe: false,
                pts: 0,
            }])
        }

        fn force_keyframe(&mut self) -> Result<(), String> {
            self.forced += 1;
            Ok(())
        }
    }

    #[test]
    fn pli_and_fir_are_keyframe_requests_other_rtcp_is_not() {
        let pli = PictureLossIndication {
            sender_ssrc: 1,
            media_ssrc: 2,
        };
        let fir = FullIntraRequest {
            sender_ssrc: 1,
            media_ssrc: 2,
            fir: vec![],
        };
        let rr = webrtc::rtcp::receiver_report::ReceiverReport::default();
        assert!(is_keyframe_request(&pli));
        assert!(is_keyframe_request(&fir));
        assert!(!is_keyframe_request(&rr));
    }

    #[test]
    fn keyframe_gate_is_rate_limited_and_keeps_a_pending_request() {
        let gate = KeyframeGate::new(Duration::from_secs(1));
        let t0 = Instant::now();
        assert!(!gate.take_due(t0), "nothing asked");
        gate.request();
        assert!(gate.take_due(t0));
        assert!(!gate.take_due(t0), "taken once");
        // A burst of PLIs inside the interval: one more key frame, later.
        gate.request();
        gate.request();
        assert!(!gate.take_due(t0 + Duration::from_millis(500)));
        assert!(gate.take_due(t0 + Duration::from_secs(1)));
        assert!(!gate.take_due(t0 + Duration::from_millis(1500)));
    }

    #[test]
    fn encode_loop_forces_a_keyframe_when_rtcp_asked() {
        let (frame_tx, frame_rx) = std_mpsc::sync_channel(4);
        let (pkt_tx, mut pkt_rx) = mpsc::channel(8);
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let gate = KeyframeGate::new(Duration::ZERO);
        let mut enc = FakeEncoder::default();
        frame_tx.send(frame()).unwrap();
        gate.request();
        frame_tx.send(frame()).unwrap();
        drop(frame_tx);
        let end = run_encode_loop(&mut enc, &frame_rx, &pkt_tx, &gate, &shutdown_rx);
        assert_eq!(end, EncodeLoopEnd::CaptureEnded);
        assert_eq!(enc.encoded, 2);
        assert_eq!(enc.forced, 1);
        assert!(pkt_rx.try_recv().is_ok());
    }

    #[test]
    fn capture_that_ends_by_itself_is_told_apart_from_shutdown() {
        let (frame_tx, frame_rx) = std_mpsc::sync_channel::<I420Frame>(1);
        let (pkt_tx, _pkt_rx) = mpsc::channel(1);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let gate = KeyframeGate::new(KEYFRAME_MIN_INTERVAL);
        shutdown_tx.send(true).unwrap();
        drop(frame_tx);
        let mut enc = FakeEncoder::default();
        assert_eq!(
            run_encode_loop(&mut enc, &frame_rx, &pkt_tx, &gate, &shutdown_rx),
            EncodeLoopEnd::Shutdown
        );
    }
}
