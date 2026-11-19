# Audio Module

**Parent:** [Tauri Source](../AGENTS.md)

**Purpose:** Real-time audio capture, playback, encoding, and decoding for voice chat. Bridges OS audio APIs (via cpal) with Opus codec for transmission over WebRTC. **PERFORMANCE CRITICAL** — directly impacts voice latency.

## Architecture

```
Microphone → cpal Input Stream → f32 samples → Opus Encoder → RTP packets → WebRTC
                                                                                ↓
Speaker ← cpal Output Stream ← f32 samples ← Opus Decoder ← RTP packets ← WebRTC
```

## Key Files

### `mod.rs`
Module root. Defines:
- **Constants**: `SAMPLE_RATE` (48kHz), `CHANNELS` (2), `FRAME_SIZE_MS` (20ms), `FRAME_SIZE` (960 samples/channel)
- **`AudioError`**: Structured errors with `thiserror`
- **`AudioDevice`** / **`AudioDeviceList`**: Device enumeration for UI
- **Tests**: Basic smoke tests for handle creation, device enumeration, mute/deafen state

### `handle.rs`
Core implementation. Defines:
- **`AudioHandle`**: Thread-safe audio controller (Send + Sync)
- **`run_capture_task()`**: Blocking task that owns cpal input Stream
- **`run_playback_task()`**: Blocking task that owns cpal output Stream
- **`run_mic_test_task()`**: Blocking task for microphone level monitoring

## Key Patterns

### Thread-Safe Stream Ownership
**Problem:** `cpal::Stream` is neither `Send` nor `Sync`, but Tauri state must be shared across threads.

**Solution:** Move Streams into `tokio::task::spawn_blocking` tasks. Control via `mpsc::channel`:

```rust
pub struct AudioHandle {
    host: Arc<Host>,                           // Thread-safe
    muted: Arc<AtomicBool>,                    // Thread-safe state
    capture_control: Option<mpsc::Sender<CaptureControl>>, // Control channel
    // ...
}
```

Audio tasks run in blocking threads, state is shared via `Arc<AtomicBool>`.

### Capture Pipeline
1. **cpal callback** (real-time thread) receives `&[f32]` samples
2. Accumulate in buffer until `FRAME_SIZE * CHANNELS` samples ready
3. Convert f32 → i16 for Opus: `(sample * 32767.0) as i16`
4. **Opus encode** frame (20ms = 960 samples/channel; `audio::voice_encoder()`: mono VoIP, in-band FEC, 10 % expected loss)
5. Send encoded packet via `mpsc::Sender<Vec<u8>>`

**Critical:** Never block in audio callback. Use `try_send()`, not `send().await`.

### Playback Pipeline (calls: `voice/audio_mixer.rs`, `voice/playout.rs`, `voice/jitter_buffer.rs`)
1. Per remote track, `commands/voice.rs::forward_remote_audio_track` stamps each RTP packet's arrival and `try_send`s it (bounded, 64) to the mixer task
2. The mixer owns one `TrackPlayout` per track **instance** (`add_track` returns a unique instance id; `remove_track(instance)` never touches a replacement with the same track id): a `JitterBuffer` (pure, clock-injected: seq ordering, duplicates/late dropped, packets whose seq and ts disagree discarded until a second one confirms a restart, 40–160 ms adaptive target, pause detection incl. senders that do not advance ts, drift drop/insert, resets) plus one Opus decoder (PLC, or FEC from the next packet)
3. Timeline in decoded audio: packets of 10–120 ms all give one full 20 ms frame per tick; shorter or undecodable packets are rejected and counted; a failed decoder call is replaced by concealment or silence of the same length
4. Every 5 ms the mixer takes at most 16 packets per track and produces frames only while `OutputLevel` (atomic samples queued for the device) is below one device block + 20 ms: **the device clock drives playout**; each track gives exactly one frame per output frame, nothing is dropped in the mixer
5. A pusher thread appends mixed frames to the pre-allocated `VecDeque<f32>`; its safety cap (`MAX_QUEUE_SAMPLES`, 460 ms) sits above the most the mixer queues, so it never discards in steady state
6. **cpal callback** drains it and counts consumed samples into `OutputLevel` (also while deafened, then it plays zeros) and notes its block size (`fetch_max`)

**Output latency:** the queue holds between one device block + 20 ms and one block + 40 ms (whole frames), plus up to one 5 ms mixer wake (tokio timer; Windows timer granularity unverified). The block size is the largest request of the last one to two seconds, capped at 400 ms (`MAX_REQUEST_SAMPLES`) and reset on a device switch; larger blocks get short gaps, the mixer never runs ahead.

Counters per track reach `voice_connection_stats` (`playout[]`) and a debug log every 5 s.

**Real-time note:** the callback still takes a `std::sync::Mutex` for the hand-over (pre-existing; replacing it with a lock-free ring is a follow-up); keep everything else there to atomics, no allocation, no logging.

### Capture → RTP
The capture counts frames the voice-activity gate skipped (`EncodedFrame::skipped_frames`); `voice::rtp_clock::RtpClock` advances the RTP timestamp over them and sets the marker bit on the first packet after a pause; uncounted gaps (mute, restarted microphone) use the wall clock unless packets are queued behind.

### Mute/Deafen State
- **Muted**: Capture stream runs but doesn't send encoded packets
- **Deafened**: Playback stream runs but fills output with silence
- State is `Arc<AtomicBool>` checked in real-time audio callbacks

### Device Selection
- Default: Use OS default device (`host.default_input_device()`)
- Custom: Store device name, search by name on next start
- **Gotcha:** Device names may change between runs (OS renames devices)

## Performance Targets

| Metric | Target | Why |
|--------|--------|-----|
| Audio callback latency | <10ms | Avoid glitches/dropouts |
| Opus encode/decode | <2ms/frame | 20ms frame size → ~10% CPU budget |
| Buffer size | ~40-100ms | Balance latency vs stability |
| Memory (per stream) | <10MB | Opus state + buffers |

## Common Issues

### Permission Denied
- **macOS/Linux**: Microphone access requires OS permission
- **Error**: `AudioError::PermissionDenied`
- **Fix**: Check system settings, request permission in Tauri config

### Device In Use
- **Error**: `AudioError::DeviceInUse`
- **Cause**: Another app has exclusive access (rare on modern OS)
- **Fix**: Stop other audio apps, or use different device

### Stream Build Fails
- **Error**: `AudioError::StreamError`
- **Cause**: Unsupported sample rate/format
- **Fix**: cpal should handle format conversion, but may fail on exotic hardware

### Choppy Audio
- **Symptoms**: Clicks, pops, gaps
- **Causes**:
  1. Buffer underrun (playback faster than network delivers)
  2. CPU overload in audio callback
  3. Blocking calls in audio callback
- **Debug**: Add `tracing::trace!` for buffer fill level

## Testing

### Unit Tests
```bash
cargo test --package vc_client --lib audio
```

Tests may fail on headless CI (no audio hardware). Wrapped with `let _ = result;` where appropriate.

### Integration Testing
Not yet implemented. Future:
- Mock audio streams with sine wave generator
- Measure encode/decode latency
- Test device enumeration across platforms

### Manual Testing
Use `commands/voice.rs::start_mic_test` to verify:
1. Device enumeration
2. Capture working (mic level updates)
3. No permission errors

## Future Improvements

### High Priority
1. ~~**Adaptive Jitter Buffer**~~: done (see Playback Pipeline)
2. **Echo Cancellation**: Use speexdsp or WebRTC AEC
3. **Noise Suppression**: Use RNNoise or WebRTC NS

### Medium Priority
4. **Volume Control**: Separate from system volume
5. **VAD (Voice Activity Detection)**: Only send when speaking
6. **Opus DTX**: Discontinuous transmission for bandwidth savings

### Low Priority
7. **Multi-channel Support**: 5.1/7.1 surround (gaming use case)
8. **Stereo Width Control**: Spatial audio adjustments

## Security Notes

- **No sensitive data**: Audio samples are ephemeral, not persisted
- **Opus encode/decode**: Memory-safe (Rust bindings)
- **Stream isolation**: Each user gets own decode task (no cross-contamination)

## Related Documentation

- [Voice Commands](../commands/voice.rs) — Tauri commands that use AudioHandle
- [WebRTC Module](../webrtc/AGENTS.md) — Sends/receives audio packets
- [STANDARDS.md](../../../../STANDARDS.md) — Opus codec configuration
