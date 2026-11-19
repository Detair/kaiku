//! A simulated voice participant with real WebRTC media, for tests that need
//! tracks to flow through the SFU (`on_track`, forwarding, renegotiation).
//!
//! Each participant has its own WebSocket and the two peer connections of the
//! dual-PC model: a publisher (the participant offers, it sends a microphone
//! track and optional video tracks with synthetic RTP) and a subscriber (the
//! server offers, the participant answers and counts the packets it receives
//! per track id, `"{source_user}:{source}"`).
//!
//! A pump task owns the WebSocket: it answers subscriber offers, applies the
//! publisher answer, applies trickled server ICE candidates (queued until the
//! remote description is set) and forwards every server event to the test.
//!
//! The SFU drops loopback candidates, so these tests need a host with a
//! non-loopback interface (every CI runner and workstation has one). They
//! contact no host outside this machine.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::{MediaEngine, MIME_TYPE_OPUS, MIME_TYPE_VP8};
use webrtc::api::APIBuilder;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp::header::Header;
use webrtc::rtp::packet::Packet;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::{TrackLocal, TrackLocalWriter};

use super::{next_ws_event, ws_connect, TestApp, TestServer, WsClient};

/// A test app for media tests: screen share limiter on, and the SFU's
/// configured STUN server points at this host (no lookups elsewhere).
pub async fn media_app(pool: sqlx::PgPool) -> TestApp {
    install_crypto_provider();
    let mut config = vc_server::config::Config::default_for_test();
    config.stun_server = "stun:127.0.0.1:3478".to_owned();
    TestApp::with_config_and_screen_share_limiter(pool, config).await
}

/// Packets received per remote track id.
pub type Received = Arc<Mutex<HashMap<String, u64>>>;

/// Options for a simulated participant.
#[derive(Clone, Copy, Debug, Default)]
pub struct SimOptions {
    /// Wait this long before answering each subscriber offer (widens the
    /// window in which the server could send a second offer).
    pub answer_delay: Duration,
}

/// One simulated voice participant.
pub struct SimParticipant {
    pub user_id: Uuid,
    pub channel_id: Uuid,
    pub publisher: Arc<RTCPeerConnection>,
    pub subscriber: Arc<RTCPeerConnection>,
    /// Packets received on the subscriber PC, per track id.
    pub received: Received,
    /// Every server event (JSON), in order.
    pub events: mpsc::UnboundedReceiver<serde_json::Value>,
    out: mpsc::UnboundedSender<serde_json::Value>,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for SimParticipant {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

/// DTLS needs a process-wide rustls provider (the server binary installs
/// the same one at start).
pub fn install_crypto_provider() {
    let _ = rustls::crypto::CryptoProvider::install_default(
        rustls::crypto::ring::default_provider(),
    );
}

fn api() -> webrtc::api::API {
    install_crypto_provider();
    let mut media = MediaEngine::default();
    media.register_default_codecs().expect("codecs");
    let registry =
        register_default_interceptors(Registry::new(), &mut media).expect("interceptors");
    APIBuilder::new()
        .with_media_engine(media)
        .with_interceptor_registry(registry)
        .build()
}

/// Apply a trickled ICE candidate, or queue it until the remote description
/// is set.
async fn add_or_queue(
    pc: &RTCPeerConnection,
    queue: &mut Vec<RTCIceCandidateInit>,
    candidate: RTCIceCandidateInit,
) {
    if pc.remote_description().await.is_some() {
        let _ = pc.add_ice_candidate(candidate).await;
    } else {
        queue.push(candidate);
    }
}

async fn flush(pc: &RTCPeerConnection, queue: &mut Vec<RTCIceCandidateInit>) {
    for c in queue.drain(..) {
        let _ = pc.add_ice_candidate(c).await;
    }
}

/// Wait (bounded) until ICE gathering of `pc` is complete, after the local
/// description was set with `gather` taken before.
async fn wait_gathered(mut gather: tokio::sync::mpsc::Receiver<()>) {
    let _ = tokio::time::timeout(Duration::from_secs(5), gather.recv()).await;
}

impl SimParticipant {
    /// Connect a WebSocket for `token`, wait for `ready` and start the pump.
    /// Does not join; call [`Self::join`].
    pub async fn connect(
        server: &TestServer,
        token: &str,
        user_id: Uuid,
        channel_id: Uuid,
        options: SimOptions,
    ) -> Self {
        let mut ws = ws_connect(server, token).await;
        next_ws_event(&mut ws, "ready", Duration::from_secs(15)).await;

        let api = api();
        let publisher = Arc::new(
            api.new_peer_connection(RTCConfiguration::default())
                .await
                .expect("publisher pc"),
        );
        let subscriber = Arc::new(
            api.new_peer_connection(RTCConfiguration::default())
                .await
                .expect("subscriber pc"),
        );

        let received: Received = Arc::new(Mutex::new(HashMap::new()));
        let counts = received.clone();
        subscriber.on_track(Box::new(move |track, _, _| {
            let counts = counts.clone();
            Box::pin(async move {
                let id = track.id();
                counts.lock().unwrap().entry(id.clone()).or_insert(0);
                tokio::spawn(async move {
                    while track.read_rtp().await.is_ok() {
                        *counts.lock().unwrap().entry(id.clone()).or_insert(0) += 1;
                    }
                });
            })
        }));

        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let pump = tokio::spawn(pump(
            ws,
            out_rx,
            out_tx.clone(),
            events_tx,
            publisher.clone(),
            subscriber.clone(),
            channel_id,
            options,
        ));

        Self {
            user_id,
            channel_id,
            publisher,
            subscriber,
            received,
            events: events_rx,
            out: out_tx,
            tasks: vec![pump],
        }
    }

    /// Send a raw client event.
    pub fn send(&self, value: serde_json::Value) {
        let _ = self.out.send(value);
    }

    /// Wait for the next event of `event_type` (others are skipped).
    pub async fn next_event(&mut self, event_type: &str, timeout: Duration) -> serde_json::Value {
        self.try_next_event(event_type, timeout)
            .await
            .unwrap_or_else(|| panic!("timed out after {timeout:?} waiting for {event_type:?}"))
    }

    /// Like [`Self::next_event`] but `None` on timeout.
    pub async fn try_next_event(
        &mut self,
        event_type: &str,
        timeout: Duration,
    ) -> Option<serde_json::Value> {
        let wait = async {
            loop {
                let ev = self.events.recv().await?;
                if ev["type"] == event_type {
                    return Some(ev);
                }
            }
        };
        tokio::time::timeout(timeout, wait).await.ok().flatten()
    }

    /// Wait for the next event matching `pred`.
    pub async fn try_next_matching(
        &mut self,
        timeout: Duration,
        mut pred: impl FnMut(&serde_json::Value) -> bool,
    ) -> Option<serde_json::Value> {
        let wait = async {
            loop {
                let ev = self.events.recv().await?;
                if pred(&ev) {
                    return Some(ev);
                }
            }
        };
        tokio::time::timeout(timeout, wait).await.ok().flatten()
    }

    /// Send `voice_join` (does not wait for anything).
    pub fn send_join(&self) {
        self.send(serde_json::json!({ "type": "voice_join", "channel_id": self.channel_id }));
    }

    /// Join and wait for `voice_room_state`.
    pub async fn join(&mut self) -> serde_json::Value {
        self.send_join();
        self.next_event("voice_room_state", Duration::from_secs(15))
            .await
    }

    /// Add a local track to the publisher PC and keep writing synthetic RTP
    /// to it every 20 ms.
    pub async fn add_track(&mut self, video: bool, id: &str) -> Arc<TrackLocalStaticRTP> {
        let (mime, clock_rate, channels, pt) = if video {
            (MIME_TYPE_VP8, 90_000, 0, 96)
        } else {
            (MIME_TYPE_OPUS, 48_000, 2, 111)
        };
        let track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: mime.to_owned(),
                clock_rate,
                channels,
                ..Default::default()
            },
            id.to_owned(),
            format!("stream-{id}"),
        ));
        let sender = self
            .publisher
            .add_track(track.clone() as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .expect("add track");
        // Drain RTCP so the interceptors keep running.
        self.tasks.push(tokio::spawn(async move {
            while sender.read_rtcp().await.is_ok() {}
        }));
        let writer = track.clone();
        self.tasks.push(tokio::spawn(async move {
            let mut seq: u16 = 0;
            let mut ts: u32 = 0;
            let mut tick = tokio::time::interval(Duration::from_millis(20));
            loop {
                tick.tick().await;
                seq = seq.wrapping_add(1);
                ts = ts.wrapping_add(if video { 3000 } else { 960 });
                let payload: &'static [u8] = if video {
                    // A VP8 payload descriptor + a few bytes; the SFU forwards
                    // opaque payloads.
                    &[0x10, 0x00, 0x9d, 0x01, 0x2a, 0, 0, 0, 0]
                } else {
                    &[0xf8, 0xff, 0xfe]
                };
                let pkt = Packet {
                    header: Header {
                        version: 2,
                        payload_type: pt,
                        sequence_number: seq,
                        timestamp: ts,
                        marker: video,
                        ..Default::default()
                    },
                    payload: bytes::Bytes::from_static(payload),
                };
                let _ = writer.write_rtp(&pkt).await;
            }
        }));
        track
    }

    /// Create a publisher offer (after gathering) and send it.
    pub async fn publish(&self) {
        let offer = self.publisher.create_offer(None).await.expect("offer");
        let gather = self.publisher.gathering_complete_promise().await;
        self.publisher
            .set_local_description(offer)
            .await
            .expect("set local offer");
        wait_gathered(gather).await;
        let sdp = self
            .publisher
            .local_description()
            .await
            .expect("local description")
            .sdp;
        self.send(serde_json::json!({
            "type": "voice_publisher_offer",
            "channel_id": self.channel_id,
            "sdp": sdp,
        }));
    }

    /// Join, add a microphone track and publish it.
    pub async fn join_and_publish_mic(&mut self) -> serde_json::Value {
        let state = self.join().await;
        self.add_track(false, "mic").await;
        self.publish().await;
        state
    }

    /// Packets received so far on track `track_id`.
    pub fn packets(&self, track_id: &str) -> u64 {
        self.received
            .lock()
            .unwrap()
            .get(track_id)
            .copied()
            .unwrap_or(0)
    }

    /// Track ids ever seen on the subscriber PC.
    pub fn seen_tracks(&self) -> Vec<String> {
        self.received.lock().unwrap().keys().cloned().collect()
    }

    /// Wait until more than `min` packets arrived on `track_id`.
    pub async fn wait_for_packets(&self, track_id: &str, min: u64, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        while tokio::time::Instant::now() < deadline {
            if self.packets(track_id) > min {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    /// Close both peer connections and the WebSocket pump.
    pub async fn close(self) {
        let _ = self.publisher.close().await;
        let _ = self.subscriber.close().await;
    }
}

/// Track id the SFU gives the microphone of `user`.
pub fn mic_track(user: Uuid) -> String {
    format!("{user}:microphone")
}

#[allow(clippy::too_many_arguments)]
async fn pump(
    mut ws: WsClient,
    mut out: mpsc::UnboundedReceiver<serde_json::Value>,
    out_tx: mpsc::UnboundedSender<serde_json::Value>,
    events: mpsc::UnboundedSender<serde_json::Value>,
    publisher: Arc<RTCPeerConnection>,
    subscriber: Arc<RTCPeerConnection>,
    channel_id: Uuid,
    options: SimOptions,
) {
    let mut pub_queue: Vec<RTCIceCandidateInit> = Vec::new();
    let mut sub_queue: Vec<RTCIceCandidateInit> = Vec::new();
    loop {
        tokio::select! {
            msg = out.recv() => {
                let Some(msg) = msg else { return };
                if ws.send(Message::Text(msg.to_string().into())).await.is_err() {
                    return;
                }
            }
            frame = ws.next() => {
                let text = match frame {
                    Some(Ok(Message::Text(t))) => t,
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                    Some(Ok(_)) => continue,
                };
                let Ok(ev) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
                match ev["type"].as_str().unwrap_or_default() {
                    "voice_publisher_answer" => {
                        if let Ok(answer) =
                            RTCSessionDescription::answer(ev["sdp"].as_str().unwrap_or_default().to_owned())
                        {
                            let _ = publisher.set_remote_description(answer).await;
                            flush(&publisher, &mut pub_queue).await;
                        }
                    }
                    "voice_subscriber_offer" => {
                        if !options.answer_delay.is_zero() {
                            tokio::time::sleep(options.answer_delay).await;
                        }
                        let Ok(offer) =
                            RTCSessionDescription::offer(ev["sdp"].as_str().unwrap_or_default().to_owned())
                        else {
                            continue;
                        };
                        if subscriber.set_remote_description(offer).await.is_err() {
                            let _ = events.send(serde_json::json!({ "type": "sim_offer_rejected" }));
                            continue;
                        }
                        flush(&subscriber, &mut sub_queue).await;
                        let Ok(answer) = subscriber.create_answer(None).await else { continue };
                        let gather = subscriber.gathering_complete_promise().await;
                        if subscriber.set_local_description(answer).await.is_err() {
                            continue;
                        }
                        wait_gathered(gather).await;
                        if let Some(local) = subscriber.local_description().await {
                            let _ = out_tx.send(serde_json::json!({
                                "type": "voice_subscriber_answer",
                                "channel_id": channel_id,
                                "sdp": local.sdp,
                            }));
                        }
                    }
                    "voice_ice_candidate" => {
                        let Ok(candidate) = serde_json::from_str::<RTCIceCandidateInit>(
                            ev["candidate"].as_str().unwrap_or_default(),
                        ) else {
                            continue;
                        };
                        if ev["pc_type"] == "subscriber" {
                            add_or_queue(&subscriber, &mut sub_queue, candidate).await;
                        } else {
                            add_or_queue(&publisher, &mut pub_queue, candidate).await;
                        }
                    }
                    _ => {}
                }
                let _ = events.send(ev);
            }
        }
    }
}
