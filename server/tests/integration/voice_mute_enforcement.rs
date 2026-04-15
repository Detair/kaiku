//! Integration tests for self-mute RTP enforcement.
//!
//! Verifies the mute state model consumed by `spawn_rtp_forwarder`
//! (see `server/src/voice/track.rs`), which drops audio RTP packets
//! from peers whose `Peer::is_effectively_muted` returns `true` while
//! leaving video tracks (screen share / webcam) untouched.
//!
//! ## Scope of coverage — what is feasible from an integration test
//!
//! The `voice::peer` and `voice::track` modules are private, so the
//! `Peer` constructor, the `TrackRouter`, and `spawn_rtp_forwarder`
//! are not reachable from this crate-external test. The webrtc-rs
//! `TrackRemote` type also cannot be constructed outside the crate
//! (`TrackRemote::new` is `pub(crate)`), so driving the forwarder
//! loop directly and counting packets in/out is not possible here.
//!
//! Instead, these tests exercise the mute state machine that the
//! forwarder consumes:
//!   * `Peer::is_effectively_muted()` correctly combines the
//!     self-mute flag with the server-mute flag.
//!   * `Peer::set_self_muted()` toggles the flag observable both
//!     on the peer and through `Room::get_participant_info()`.
//!   * The classification (`TrackSource::is_video`) the forwarder
//!     uses to skip mute enforcement for video stays intact.
//!
//! A regression that accidentally removed the `if muted { continue }`
//! branch inside `spawn_rtp_forwarder` would NOT be caught here —
//! that requires either a full two-peer WebRTC harness (out of scope
//! for the current test infrastructure) or in-crate `#[cfg(test)]`
//! coverage alongside `spawn_rtp_forwarder` itself.
//!
//! Run with: `cargo test --test integration voice_mute_enforcement`

use std::sync::Arc;

use tokio::sync::mpsc;
use uuid::Uuid;
use vc_server::config::Config;
use vc_server::voice::sfu::SfuServer;
use vc_server::voice::TrackSource;
use vc_server::ws::OutboundMsg;

/// Build an `SfuServer` configured for tests. No `PostgreSQL`, Valkey, or
/// network is required — `SfuServer::new` only constructs an in-process
/// `WebRTC` API from a `Config`.
fn build_sfu() -> Arc<SfuServer> {
    let config = Arc::new(Config::default_for_test());
    Arc::new(SfuServer::new(config, None).expect("SFU creation should succeed"))
}

// ============================================================================
// Tests
// ============================================================================

/// Newly-joined peer is unmuted by default.
#[tokio::test]
async fn peer_starts_unmuted() {
    let sfu = build_sfu();
    let room = sfu.get_or_create_room(Uuid::new_v4()).await;

    let user_id = Uuid::new_v4();
    let (tx, _rx) = mpsc::channel::<OutboundMsg>(32);
    let peer = sfu
        .create_peer(
            user_id,
            "alice".into(),
            "Alice Display".into(),
            room.channel_id,
            tx,
        )
        .await
        .expect("create_peer should succeed");
    room.add_peer(peer.clone()).await.expect("add_peer");

    assert!(
        !peer.is_self_muted().await,
        "new peer should not be self-muted"
    );
    assert!(
        !peer.is_effectively_muted().await,
        "new peer should not be effectively muted"
    );

    // Participant info should reflect the same state.
    let participants = room.get_participant_info().await;
    let alice = participants
        .iter()
        .find(|p| p.user_id == user_id)
        .expect("alice should be in room");
    assert!(
        !alice.muted,
        "ParticipantInfo.muted should be false for unmuted peer"
    );
}

/// `set_self_muted(true)` flips both `is_self_muted` and
/// `is_effectively_muted`, which is the gate used by
/// `spawn_rtp_forwarder` to drop microphone RTP packets.
#[tokio::test]
async fn self_mute_flips_effective_mute() {
    let sfu = build_sfu();
    let room = sfu.get_or_create_room(Uuid::new_v4()).await;

    let user_id = Uuid::new_v4();
    let (tx, _rx) = mpsc::channel::<OutboundMsg>(32);
    let peer = sfu
        .create_peer(
            user_id,
            "bob".into(),
            "Bob Display".into(),
            room.channel_id,
            tx,
        )
        .await
        .expect("create_peer");
    room.add_peer(peer.clone()).await.expect("add_peer");

    // Mute.
    peer.set_self_muted(true).await;
    assert!(peer.is_self_muted().await, "self_muted should be true");
    assert!(
        peer.is_effectively_muted().await,
        "effective mute should follow self-mute — this is the predicate \
         the SFU forwarder uses to drop microphone RTP packets"
    );

    let participants = room.get_participant_info().await;
    assert!(
        participants
            .iter()
            .find(|p| p.user_id == user_id)
            .expect("peer present")
            .muted,
        "ParticipantInfo.muted should reflect self-mute"
    );

    // Unmute.
    peer.set_self_muted(false).await;
    assert!(
        !peer.is_self_muted().await,
        "self_muted should be cleared after unmute"
    );
    assert!(
        !peer.is_effectively_muted().await,
        "effective mute should clear after self-unmute (no server mute set)"
    );

    let participants = room.get_participant_info().await;
    assert!(
        !participants
            .iter()
            .find(|p| p.user_id == user_id)
            .expect("peer present")
            .muted,
        "ParticipantInfo.muted should reflect unmute"
    );
}

/// Toggling self-mute multiple times is deterministic — the effective
/// mute state always tracks the latest value, with no sleeps or timing.
#[tokio::test]
async fn self_mute_toggle_is_deterministic() {
    let sfu = build_sfu();
    let room = sfu.get_or_create_room(Uuid::new_v4()).await;

    let user_id = Uuid::new_v4();
    let (tx, _rx) = mpsc::channel::<OutboundMsg>(32);
    let peer = sfu
        .create_peer(
            user_id,
            "carol".into(),
            "Carol Display".into(),
            room.channel_id,
            tx,
        )
        .await
        .expect("create_peer");
    room.add_peer(peer.clone()).await.expect("add_peer");

    for &muted in &[true, false, true, true, false, false, true] {
        peer.set_self_muted(muted).await;
        assert_eq!(
            peer.is_self_muted().await,
            muted,
            "is_self_muted should match the last-set value ({muted})"
        );
        assert_eq!(
            peer.is_effectively_muted().await,
            muted,
            "is_effectively_muted should track self-mute when no server mute is active"
        );
    }
}

/// Multiple peers have independent mute state: muting one must not
/// affect the others. This guards the invariant that the SFU's
/// mute-drop logic is keyed on the *source* peer, not a global.
#[tokio::test]
async fn per_peer_mute_state_is_independent() {
    let sfu = build_sfu();
    let room = sfu.get_or_create_room(Uuid::new_v4()).await;

    let alice_id = Uuid::new_v4();
    let bob_id = Uuid::new_v4();

    let (tx_a, _rx_a) = mpsc::channel::<OutboundMsg>(32);
    let alice = sfu
        .create_peer(
            alice_id,
            "alice".into(),
            "Alice Display".into(),
            room.channel_id,
            tx_a,
        )
        .await
        .expect("create_peer alice");
    room.add_peer(alice.clone()).await.expect("add_peer alice");

    let (tx_b, _rx_b) = mpsc::channel::<OutboundMsg>(32);
    let bob = sfu
        .create_peer(
            bob_id,
            "bob".into(),
            "Bob Display".into(),
            room.channel_id,
            tx_b,
        )
        .await
        .expect("create_peer bob");
    room.add_peer(bob.clone()).await.expect("add_peer bob");

    alice.set_self_muted(true).await;

    assert!(alice.is_effectively_muted().await, "alice should be muted");
    assert!(
        !bob.is_effectively_muted().await,
        "bob should NOT be affected by alice's mute — the forwarder's \
         mute-check is per-peer, so bob's audio packets must still flow"
    );

    // Confirm via public room surface.
    let participants = room.get_participant_info().await;
    let alice_info = participants
        .iter()
        .find(|p| p.user_id == alice_id)
        .expect("alice present");
    let bob_info = participants
        .iter()
        .find(|p| p.user_id == bob_id)
        .expect("bob present");

    assert!(alice_info.muted, "alice.muted = true");
    assert!(!bob_info.muted, "bob.muted = false");
}

/// Documents the forwarder's *narrow* mute-drop policy: only
/// `TrackSource::Microphone` is dropped on self-mute — system audio
/// (`ScreenAudio`) and all video sources are forwarded unchanged.
///
/// The forwarder contract, from `spawn_rtp_forwarder`:
///
/// ```ignore
/// if source_type == TrackSource::Microphone && peer.is_effectively_muted().await {
///     continue;
/// }
/// ```
///
/// The per-variant `is_audio()`/`is_video()` classification is already
/// covered by in-crate unit tests in `voice::track_types` — this test
/// guards only the two properties the forwarder relies on that are
/// *not* covered there:
///
/// 1. `Microphone` and `ScreenAudio` are distinct variants, so the
///    narrow `== TrackSource::Microphone` match excludes system audio
///    from the mute-drop path.
/// 2. `ScreenAudio` is classified as audio — a status-quo pin so that
///    if the policy ever broadens from "drop muted microphone" to
///    "drop all muted audio", the forwarder must be updated in lockstep.
#[test]
fn track_source_forwarder_mute_policy_contract() {
    let stream_id = Uuid::new_v4();

    assert_ne!(
        TrackSource::Microphone,
        TrackSource::ScreenAudio(stream_id),
        "Microphone must be distinct from ScreenAudio so the forwarder's \
         `source_type == TrackSource::Microphone` check can exclude system audio"
    );

    // Status-quo pin: if this ever flips, the forwarder's narrow
    // `Microphone`-only mute-drop needs a second look.
    assert!(
        TrackSource::ScreenAudio(stream_id).is_audio(),
        "ScreenAudio is classified as audio — forwarder policy only drops \
         Microphone, not all audio; revisit spawn_rtp_forwarder if this changes"
    );
}
