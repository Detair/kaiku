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

/// The sender makes a key frame at least this often, so a viewer who
/// subscribes late sees a picture within this time even when its key frame
/// request does not reach the sender.
pub const KEYFRAME_PERIOD: Duration = Duration::from_secs(3);

/// Why a capture ended by itself (set by the capture thread before it
/// drops its frame sender; `None` = the source simply ended). Capture
/// threads hand it over with [`run_capture`].
pub type EndReason = Arc<Mutex<Option<&'static str>>>;

/// Run a capture `body` that gets the frame sender and returns why it
/// ended. The reason is stored before the last frame sender is dropped:
/// the encoder is woken by the closed channel and must find it there,
/// whatever the body still tears down after it let its sender go.
pub fn run_capture<F>(frames: std_mpsc::SyncSender<I420Frame>, reason: &EndReason, body: F)
where
    F: FnOnce(std_mpsc::SyncSender<I420Frame>) -> Option<&'static str>,
{
    let keep_open = frames.clone();
    let why = body(frames);
    if let (Some(why), Ok(mut slot)) = (why, reason.lock()) {
        *slot = Some(why);
    }
    drop(keep_open);
}

/// Hand a captured frame to the encoder. `false`: the encoder is gone and
/// the capture should end (a full channel just drops the frame).
pub fn offer_frame(frames: &std_mpsc::SyncSender<I420Frame>, frame: I420Frame) -> bool {
    match frames.try_send(frame) {
        Ok(()) => true,
        Err(std_mpsc::TrySendError::Full(_)) => {
            debug!("Frame dropped (encoder behind)");
            true
        }
        Err(std_mpsc::TrySendError::Disconnected(_)) => false,
    }
}

/// Reason codes sent to the frontend with `voice:local_capture_ended`.
pub mod end_reason {
    /// The source ended (window closed, sharing stopped, camera gone).
    pub const ENDED: &str = "ended";
    /// The capture delivered no usable picture.
    pub const NO_FRAMES: &str = "no_frames";
    /// The video encoder could not start or could not hand on its packets.
    pub const ENCODER_FAILED: &str = "encoder_failed";
    /// The connection could not be renegotiated for the new track.
    pub const NOT_SENT: &str = "not_sent";
}

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

/// What the frontend is told when the encoder loop ended (`None`: a
/// shutdown, nothing to report). Every other end ends the publication.
pub fn end_report(end: &EncodeLoopEnd, reason: &EndReason) -> Option<&'static str> {
    match end {
        EncodeLoopEnd::Shutdown => None,
        EncodeLoopEnd::CaptureEnded => Some(
            reason
                .lock()
                .ok()
                .and_then(|r| *r)
                .unwrap_or(end_reason::ENDED),
        ),
        // Packets can no longer be sent: the video cannot go out.
        EncodeLoopEnd::SinkClosed => Some(end_reason::ENCODER_FAILED),
    }
}

/// The encoder thread's loop: frames in, packets out, key frames on request.
pub fn run_encode_loop<E: PublicationEncoder>(
    encoder: &mut E,
    frames: &std_mpsc::Receiver<I420Frame>,
    packets: &mpsc::Sender<Vec<EncodedPacket>>,
    keyframes: &KeyframeGate,
    shutdown: &watch::Receiver<bool>,
    keyframe_period: Duration,
) -> EncodeLoopEnd {
    let mut last_keyframe = Instant::now();
    // The last captured frame: a still screen delivers no new frames (the
    // capture is damage-driven), but a late viewer still needs a key frame.
    let mut last_frame: Option<I420Frame> = None;
    loop {
        if *shutdown.borrow() {
            return EncodeLoopEnd::Shutdown;
        }
        let (frame, fresh) = match frames.recv_timeout(Duration::from_millis(16)) {
            Ok(frame) => (Some(frame), true),
            Err(std_mpsc::RecvTimeoutError::Timeout) => (None, false),
            Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                return if *shutdown.borrow() {
                    EncodeLoopEnd::Shutdown
                } else {
                    EncodeLoopEnd::CaptureEnded
                };
            }
        };
        let now = Instant::now();
        if now.duration_since(last_keyframe) >= keyframe_period {
            keyframes.request();
        }
        // No new frame: only a due key frame re-encodes the last one.
        let force = if fresh {
            keyframes.take_due(now)
        } else if last_frame.is_some() && keyframes.take_due(now) {
            true
        } else {
            continue;
        };
        if force {
            if let Err(e) = encoder.force_keyframe() {
                warn!("Key frame could not be forced: {e}");
            }
        }
        if let Some(frame) = frame {
            last_frame = Some(frame);
        }
        let Some(current) = last_frame.as_ref() else {
            continue;
        };
        match encoder.encode(current) {
            Ok(out) if out.is_empty() => {}
            Ok(out) => {
                if out.iter().any(|p| p.is_keyframe) {
                    last_keyframe = now;
                }
                if packets.blocking_send(out).is_err() {
                    return EncodeLoopEnd::SinkClosed;
                }
            }
            Err(e) => warn!("Encode error: {e}"),
        }
    }
}

/// Starts the capture: gets the frame sender and the shutdown signal,
/// returns the capture thread's handle.
pub type StartCapture = Box<
    dyn FnOnce(
            std_mpsc::SyncSender<I420Frame>,
            watch::Receiver<bool>,
            EndReason,
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
    /// `on_capture_end(reason)` runs once if the capture ends by itself
    /// (or the encoder cannot start); see [`end_reason`].
    pub fn start(
        params: QualityParams,
        track: Arc<TrackLocalStaticRTP>,
        sender: Arc<RTCRtpSender>,
        start_capture: StartCapture,
        on_capture_end: Box<dyn FnOnce(&'static str) + Send>,
    ) -> Result<Self, String> {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (frame_tx, frame_rx) = std_mpsc::sync_channel::<I420Frame>(2);
        let reason: EndReason = Arc::new(Mutex::new(None));
        let capture = start_capture(frame_tx, shutdown_tx.subscribe(), reason.clone())?;
        let keyframes = Arc::new(KeyframeGate::new(KEYFRAME_MIN_INTERVAL));
        let (pkt_tx, mut pkt_rx) = mpsc::channel::<Vec<EncodedPacket>>(4);

        let encoder = {
            let keyframes = keyframes.clone();
            let shutdown = shutdown_rx.clone();
            tokio::task::spawn_blocking(move || {
                let mut encoder = match Vp8Encoder::new(&params) {
                    Ok(e) => e,
                    Err(e) => {
                        // Without an encoder nothing is sent: end it visibly.
                        error!("Failed to create VP8 encoder: {e}");
                        drop(frame_rx);
                        if !*shutdown.borrow() {
                            on_capture_end(end_reason::ENCODER_FAILED);
                        }
                        return;
                    }
                };
                let end = run_encode_loop(
                    &mut encoder,
                    &frame_rx,
                    &pkt_tx,
                    &keyframes,
                    &shutdown,
                    KEYFRAME_PERIOD,
                );
                info!(?end, "Video encoder stopped");
                // `frame_rx` goes with this thread: the capture sees it and ends.
                if let Some(why) = end_report(&end, &reason) {
                    on_capture_end(why);
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
        let end = run_encode_loop(
            &mut enc,
            &frame_rx,
            &pkt_tx,
            &gate,
            &shutdown_rx,
            KEYFRAME_PERIOD,
        );
        assert_eq!(end, EncodeLoopEnd::CaptureEnded);
        assert_eq!(enc.encoded, 2);
        assert_eq!(enc.forced, 1);
        assert!(pkt_rx.try_recv().is_ok());
    }

    #[test]
    fn shutdown_is_told_apart_from_a_capture_that_ended() {
        let (frame_tx, frame_rx) = std_mpsc::sync_channel::<I420Frame>(1);
        let (pkt_tx, _pkt_rx) = mpsc::channel(1);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let gate = KeyframeGate::new(KEYFRAME_MIN_INTERVAL);
        let mut enc = FakeEncoder::default();
        // The capture ends by itself: the frame sender is dropped.
        drop(frame_tx);
        assert_eq!(
            run_encode_loop(
                &mut enc,
                &frame_rx,
                &pkt_tx,
                &gate,
                &shutdown_rx,
                KEYFRAME_PERIOD
            ),
            EncodeLoopEnd::CaptureEnded
        );
        // The same ending after a shutdown request is a shutdown.
        let (frame_tx, frame_rx) = std_mpsc::sync_channel::<I420Frame>(1);
        shutdown_tx.send(true).unwrap();
        drop(frame_tx);
        assert_eq!(
            run_encode_loop(
                &mut enc,
                &frame_rx,
                &pkt_tx,
                &gate,
                &shutdown_rx,
                KEYFRAME_PERIOD
            ),
            EncodeLoopEnd::Shutdown
        );
    }

    #[test]
    fn periodic_keyframe_without_any_request() {
        let (frame_tx, frame_rx) = std_mpsc::sync_channel(8);
        let (pkt_tx, _pkt_rx) = mpsc::channel(8);
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let gate = KeyframeGate::new(Duration::ZERO);
        let mut enc = FakeEncoder::default();
        for _ in 0..3 {
            frame_tx.send(frame()).unwrap();
        }
        drop(frame_tx);
        // A zero period: every frame is due (no PLI involved).
        run_encode_loop(
            &mut enc,
            &frame_rx,
            &pkt_tx,
            &gate,
            &shutdown_rx,
            Duration::ZERO,
        );
        assert_eq!(enc.forced, 3);
        // The default period makes none in a short burst.
        let (frame_tx, frame_rx) = std_mpsc::sync_channel(8);
        let mut enc = FakeEncoder::default();
        frame_tx.send(frame()).unwrap();
        drop(frame_tx);
        run_encode_loop(
            &mut enc,
            &frame_rx,
            &pkt_tx,
            &gate,
            &shutdown_rx,
            KEYFRAME_PERIOD,
        );
        assert_eq!(enc.forced, 0);
    }

    #[test]
    fn a_still_screen_gets_key_frames_from_its_last_frame() {
        let (frame_tx, frame_rx) = std_mpsc::sync_channel(4);
        let (pkt_tx, mut pkt_rx) = mpsc::channel(64);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let gate = KeyframeGate::new(Duration::from_millis(20));
        // One captured frame, then nothing (a still, damage-driven screen).
        frame_tx.send(frame()).unwrap();
        let stop = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            shutdown_tx.send(true).unwrap();
            drop(frame_tx);
        });
        let mut enc = FakeEncoder::default();
        // A late viewer's request arrives while no frame comes.
        gate.request();
        let end = run_encode_loop(
            &mut enc,
            &frame_rx,
            &pkt_tx,
            &gate,
            &shutdown_rx,
            Duration::from_millis(50),
        );
        stop.join().unwrap();
        assert_eq!(end, EncodeLoopEnd::Shutdown);
        assert!(
            enc.forced >= 2,
            "request and periodic tick re-encode the last frame ({})",
            enc.forced
        );
        assert_eq!(
            enc.encoded, enc.forced,
            "every re-encode is a forced key frame"
        );
        let mut sent = 0;
        while pkt_rx.try_recv().is_ok() {
            sent += 1;
        }
        assert_eq!(sent, enc.encoded);
    }

    #[test]
    fn nothing_is_encoded_before_the_first_frame() {
        let (frame_tx, frame_rx) = std_mpsc::sync_channel::<I420Frame>(1);
        let (pkt_tx, _pkt_rx) = mpsc::channel(4);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let gate = KeyframeGate::new(Duration::ZERO);
        gate.request();
        let stop = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(60));
            shutdown_tx.send(true).unwrap();
            drop(frame_tx);
        });
        let mut enc = FakeEncoder::default();
        run_encode_loop(
            &mut enc,
            &frame_rx,
            &pkt_tx,
            &gate,
            &shutdown_rx,
            Duration::ZERO,
        );
        stop.join().unwrap();
        assert_eq!((enc.encoded, enc.forced), (0, 0));
    }

    #[test]
    fn the_end_reason_is_there_when_the_encoder_wakes() {
        let (frame_tx, frame_rx) = std_mpsc::sync_channel::<I420Frame>(2);
        let reason: EndReason = Arc::new(Mutex::new(None));
        // The encoder side: woken by the closed channel, reads the reason.
        let encoder = {
            let reason = reason.clone();
            std::thread::spawn(move || {
                while frame_rx.recv().is_ok() {}
                end_report(&EncodeLoopEnd::CaptureEnded, &reason)
            })
        };
        // A capture that lets its sender go, then still tears down (as
        // `PipeWire` does) before it returns its reason.
        run_capture(frame_tx, &reason, |frames| {
            drop(frames);
            std::thread::sleep(Duration::from_millis(100));
            Some(end_reason::NO_FRAMES)
        });
        assert_eq!(encoder.join().unwrap(), Some(end_reason::NO_FRAMES));
    }

    #[test]
    fn a_capture_ends_when_the_encoder_is_gone() {
        let (frame_tx, frame_rx) = std_mpsc::sync_channel::<I420Frame>(1);
        assert!(offer_frame(&frame_tx, frame()));
        assert!(offer_frame(&frame_tx, frame()), "full: dropped, go on");
        drop(frame_rx);
        assert!(!offer_frame(&frame_tx, frame()), "encoder gone: end");
    }

    #[test]
    fn every_end_but_a_shutdown_is_reported() {
        let reason: EndReason = Arc::new(Mutex::new(None));
        assert_eq!(end_report(&EncodeLoopEnd::Shutdown, &reason), None);
        assert_eq!(
            end_report(&EncodeLoopEnd::CaptureEnded, &reason),
            Some(end_reason::ENDED)
        );
        assert_eq!(
            end_report(&EncodeLoopEnd::SinkClosed, &reason),
            Some(end_reason::ENCODER_FAILED),
            "nothing can be sent any more"
        );
        *reason.lock().unwrap() = Some(end_reason::NO_FRAMES);
        assert_eq!(
            end_report(&EncodeLoopEnd::CaptureEnded, &reason),
            Some(end_reason::NO_FRAMES)
        );
    }

    #[test]
    fn rtcp_flood_gives_one_keyframe_per_interval() {
        let (frame_tx, frame_rx) = std_mpsc::sync_channel(64);
        let (pkt_tx, _pkt_rx) = mpsc::channel(64);
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let gate = KeyframeGate::new(Duration::from_mins(1));
        let mut enc = FakeEncoder::default();
        for _ in 0..20 {
            gate.request(); // a PLI before every frame
            frame_tx.send(frame()).unwrap();
        }
        drop(frame_tx);
        run_encode_loop(
            &mut enc,
            &frame_rx,
            &pkt_tx,
            &gate,
            &shutdown_rx,
            KEYFRAME_PERIOD,
        );
        assert_eq!(enc.encoded, 20);
        assert_eq!(enc.forced, 1, "rate limit holds under a flood");
    }
}
