//! Integration tests for voice signaling rate limiting.

use std::time::Duration;

use uuid::Uuid;
use vc_server::voice::rate_limit::{EventClass, VoiceRateLimiter};

#[tokio::test(start_paused = true)]
async fn ice_candidate_rate_limit_publisher_exhausts_at_capacity() {
    let limiter = VoiceRateLimiter::new();
    let peer = Uuid::new_v4();

    // ICE publisher bucket is capacity=200 per spec.
    for _ in 0..200 {
        assert!(limiter.try_acquire(peer, EventClass::IceCandidatePublisher));
    }
    assert!(!limiter.try_acquire(peer, EventClass::IceCandidatePublisher));
}

#[tokio::test(start_paused = true)]
async fn ice_candidate_buckets_are_independent_per_pc_type() {
    let limiter = VoiceRateLimiter::new();
    let peer = Uuid::new_v4();

    // Drain the publisher bucket
    for _ in 0..200 {
        assert!(limiter.try_acquire(peer, EventClass::IceCandidatePublisher));
    }
    // Subscriber bucket is still full — independent per (peer, pc_type)
    for _ in 0..200 {
        assert!(limiter.try_acquire(peer, EventClass::IceCandidateSubscriber));
    }
    assert!(!limiter.try_acquire(peer, EventClass::IceCandidatePublisher));
    assert!(!limiter.try_acquire(peer, EventClass::IceCandidateSubscriber));
}

#[tokio::test(start_paused = true)]
async fn publisher_offer_rate_limit_refills_on_virtual_time() {
    let limiter = VoiceRateLimiter::new();
    let peer = Uuid::new_v4();

    // Burst of 5 passes (capacity=5)
    for _ in 0..5 {
        assert!(limiter.try_acquire(peer, EventClass::PublisherOffer));
    }
    // Sixth fails
    assert!(!limiter.try_acquire(peer, EventClass::PublisherOffer));

    // Advance virtual time — refill is 1/sec, so 5 seconds = 5 tokens back (capped at capacity).
    tokio::time::advance(Duration::from_secs(5)).await;

    // 5 more pass
    for _ in 0..5 {
        assert!(limiter.try_acquire(peer, EventClass::PublisherOffer));
    }
    assert!(!limiter.try_acquire(peer, EventClass::PublisherOffer));
}

#[tokio::test(start_paused = true)]
async fn rate_limits_are_per_peer() {
    let limiter = VoiceRateLimiter::new();
    let peer_a = Uuid::new_v4();
    let peer_b = Uuid::new_v4();

    // A exhausts publisher offer bucket
    for _ in 0..5 {
        assert!(limiter.try_acquire(peer_a, EventClass::PublisherOffer));
    }
    assert!(!limiter.try_acquire(peer_a, EventClass::PublisherOffer));

    // B is unaffected
    for _ in 0..5 {
        assert!(limiter.try_acquire(peer_b, EventClass::PublisherOffer));
    }
}

#[tokio::test(start_paused = true)]
async fn forget_peer_frees_all_buckets_for_that_peer() {
    let limiter = VoiceRateLimiter::new();
    let peer_a = Uuid::new_v4();
    let peer_b = Uuid::new_v4();

    // Exhaust multiple bucket classes for peer_a and peer_b
    for _ in 0..5 {
        assert!(limiter.try_acquire(peer_a, EventClass::PublisherOffer));
    }
    for _ in 0..10 {
        assert!(limiter.try_acquire(peer_a, EventClass::MuteOrWebcamToggle));
    }
    for _ in 0..5 {
        assert!(limiter.try_acquire(peer_b, EventClass::PublisherOffer));
    }

    limiter.forget_peer(peer_a);

    // peer_a's fresh bucket is full again after forget; peer_b remains exhausted
    for _ in 0..5 {
        assert!(limiter.try_acquire(peer_a, EventClass::PublisherOffer));
    }
    assert!(!limiter.try_acquire(peer_b, EventClass::PublisherOffer));
}
