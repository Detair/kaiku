//! Generates `tests/fixtures/vp8_sample.rtp` using the existing vpx-encode path.
//! Produces a short (~1 second) VP8 stream, RTP-packetized, written to disk.
//! Run once with `cargo run --example capture_vp8_fixture` and commit the .rtp file.
//! Not invoked by CI.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

// Depend only on `vpx-encode` and `webrtc::rtp` (already in deps).
// Do not import anything from `vc_client` — keeping this standalone makes
// it buildable independent of the main crate's screen-capture dependency
// chain (which may fail on some dev machines due to libspa header mismatch).

use vpx_encode::{Config, Encoder, VideoCodecId};
use webrtc::rtp::header::Header as RtpHeader;
use webrtc::rtp::packet::Packet as RtpPacket;

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;
const FPS: u32 = 30;
const DURATION_S: u32 = 1;
const VP8_PAYLOAD_TYPE: u8 = 96;
const MAX_RTP_PAYLOAD_SIZE: usize = 1200;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Synthesize a simple moving pattern in I420 (YUV) format.
    //    Luma plane: a vertical gradient that shifts by frame index.
    //    Chroma planes: constant (grey).
    let yuv_frames = synth_yuv_frames(WIDTH, HEIGHT, FPS * DURATION_S);

    // 2. Encode with vpx-encode.
    let mut cfg = Config::new(WIDTH, HEIGHT, FPS.try_into()?, 1, VideoCodecId::VP8)?;
    cfg.set_bitrate(400_000);
    let mut encoder = Encoder::new(cfg)?;

    let mut rtp_packets: Vec<RtpPacket> = Vec::new();
    let mut seq: u16 = 0;
    let ssrc: u32 = 0xCAFE_F00D;
    let clock_hz: u64 = 90_000;
    let samples_per_frame = (clock_hz / FPS as u64) as u32;
    let mut timestamp: u32 = 0;

    for (idx, frame) in yuv_frames.iter().enumerate() {
        let pts = idx as i64;
        let encoded_frames = encoder.encode(pts, frame)?;
        for vp8_frame in encoded_frames {
            // Packetize: split into MAX_RTP_PAYLOAD_SIZE - 1 chunks (leaving room for 1-byte VP8 descriptor)
            let data = &vp8_frame.data;
            let max_payload = MAX_RTP_PAYLOAD_SIZE - 1;
            let fragments: Vec<&[u8]> = data.chunks(max_payload).collect();
            let total = fragments.len();

            for (i, chunk) in fragments.iter().enumerate() {
                let is_first = i == 0;
                let is_last = i == total - 1;

                let mut payload = Vec::with_capacity(1 + chunk.len());
                // VP8 descriptor byte: S bit for first fragment only.
                payload.push(if is_first { 0x10 } else { 0x00 });
                payload.extend_from_slice(chunk);

                let rtp = RtpPacket {
                    header: RtpHeader {
                        version: 2,
                        payload_type: VP8_PAYLOAD_TYPE,
                        marker: is_last,
                        sequence_number: seq,
                        timestamp,
                        ssrc,
                        ..Default::default()
                    },
                    payload: payload.into(),
                };
                rtp_packets.push(rtp);
                seq = seq.wrapping_add(1);
            }
        }
        timestamp = timestamp.wrapping_add(samples_per_frame);
    }

    // Flush encoder for any final buffered frames — same packetization.
    for vp8_frame in encoder.finish()? {
        let data = &vp8_frame.data;
        let max_payload = MAX_RTP_PAYLOAD_SIZE - 1;
        let fragments: Vec<&[u8]> = data.chunks(max_payload).collect();
        let total = fragments.len();
        for (i, chunk) in fragments.iter().enumerate() {
            let is_first = i == 0;
            let is_last = i == total - 1;
            let mut payload = Vec::with_capacity(1 + chunk.len());
            payload.push(if is_first { 0x10 } else { 0x00 });
            payload.extend_from_slice(chunk);
            let rtp = RtpPacket {
                header: RtpHeader {
                    version: 2,
                    payload_type: VP8_PAYLOAD_TYPE,
                    marker: is_last,
                    sequence_number: seq,
                    timestamp,
                    ssrc,
                    ..Default::default()
                },
                payload: payload.into(),
            };
            rtp_packets.push(rtp);
            seq = seq.wrapping_add(1);
        }
    }

    // 3. Serialize RTP packets to disk as length-prefixed binary:
    //    <u32 be length> <packet bytes>... repeated.
    //    Keeps the format simple and self-describing for test code.
    let output_dir = Path::new("tests/fixtures");
    fs::create_dir_all(output_dir)?;
    let out_path = output_dir.join("vp8_sample.rtp");
    let mut out = File::create(&out_path)?;

    use webrtc::util::Marshal;
    for packet in &rtp_packets {
        let bytes = packet.marshal()?;
        let len = bytes.len() as u32;
        out.write_all(&len.to_be_bytes())?;
        out.write_all(&bytes)?;
    }

    println!(
        "Wrote {} RTP packets to {}",
        rtp_packets.len(),
        out_path.display()
    );
    Ok(())
}

fn synth_yuv_frames(w: u32, h: u32, n: u32) -> Vec<Vec<u8>> {
    let y_size = (w * h) as usize;
    let uv_size = (w * h / 4) as usize;
    let frame_size = y_size + 2 * uv_size;

    let mut frames = Vec::with_capacity(n as usize);
    for i in 0..n {
        let mut buf = vec![0u8; frame_size];
        // Y plane: vertical gradient with a per-frame offset for motion.
        for row in 0..h {
            let row_start = (row * w) as usize;
            let v = ((row + i) % 256) as u8;
            for col in 0..w as usize {
                buf[row_start + col] = v;
            }
        }
        // U plane: constant 128 (neutral chroma).
        for b in &mut buf[y_size..y_size + uv_size] {
            *b = 128;
        }
        // V plane: constant 128.
        for b in &mut buf[y_size + uv_size..] {
            *b = 128;
        }
        frames.push(buf);
    }
    frames
}
