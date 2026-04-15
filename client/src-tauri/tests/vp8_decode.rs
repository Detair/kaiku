//! Integration test for Vp8VideoDecoder.
//!
//! Gated behind `decode-integration` feature flag because it requires:
//! - Live libvpx (via env-libvpx-sys)
//! - The fixture at tests/fixtures/vp8_sample.rtp (captured by examples/capture_vp8_fixture.rs)
//!
//! Run with: cargo test --features decode-integration --test vp8_decode

#[tokio::test]
#[cfg_attr(not(feature = "decode-integration"), ignore)]
async fn decodes_vp8_sample() {
    // Load tests/fixtures/vp8_sample.rtp.
    // Feed each packet through Vp8VideoDecoder::process_packet.
    // Assert at least one DecodedFrame arrives on the sink with:
    //   - width == 320, height == 240
    //   - y_plane.len() >= y_stride * height
    //   - u_plane.len() >= uv_stride * (height / 2)
    //   - v_plane.len() >= uv_stride * (height / 2)
    //
    // NOTE: This test is intentionally a compile-only skeleton. When the
    // fixture is generated (Task D3 Outcome A or a future fixture run),
    // replace this body with the real assertions. Until then, the #[ignore]
    // + decode-integration feature gate skips it.
    //
    // The `voice` module is currently `mod voice;` (private) in `lib.rs`, so
    // the real implementation will need to either route through a public
    // re-export or move into the unit-test layer alongside the depacketizer
    // tests in `src/voice/video_decoder.rs`.
    let _ = ();
}
