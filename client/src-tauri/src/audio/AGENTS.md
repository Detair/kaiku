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
1. Per remote track, `commands/voice.rs::forward_remote_audio_track` stamps each RTP packet's arrival and `try_send`s it (bounded, 16; drops counted as `queue_full`) to the mixer task; it also wakes when its queue closes (instance retired or mixer ended)
2. The mixer owns one `TrackPlayout` per track **instance**: the instance id is taken when the track is announced (`AudioMixer::new_instance`), a registration only uses the mixer of the session that announced it, an older instance never retires a newer one, and only the current instance reports the track as gone (`release_with`, under the transition). Each holds a `JitterBuffer` (pure, clock-injected; at most 25 packets) and one Opus decoder (PLC, or FEC from the next packet when libopus would really use it)
3. Jitter buffer rules: seq ordering; duplicates/late dropped; a packet whose seq and ts disagree (incl. < 10 ms of ts per seq, an unexplained forward ts jump) is discarded until a second one confirms a restart (same source with running seq numbers: resumed like a pause); late packets of a replaced source refused for 10 s; 40–160 ms adaptive target; pause detection incl. senders that do not advance ts; audio played while the buffer ran dry counts against a loss's gap, the stretch right before the next packet is left for its FEC; drift drop/insert (a few too many packets while playing drop the oldest)
4. Timeline in decoded audio: packets of 10–120 ms all give one full 20 ms frame per tick; shorter or undecodable packets are rejected, packets older than 500 ms when taken are expired (both counted); a failed decoder call is replaced by concealment or silence of the same length
5. `Pacer`: every 5 ms the mixer renders **one frame per 20 ms of elapsed time**, so jitter buffers are ticked evenly whatever the device block. At each callback's low point the queue is held at one frame plus the spread between the device's smallest and largest block of the last 5 s (`OutputLevel::request_range`); a frame short → one extra tick (device clock drift); far short (start, larger blocks, new device) → silence, not ticks; above target → skip one frame per second (every 200 ms when far above). Bounded by the queue's capacity. No callback for 1 s → absent: rendering stops 3 frames ahead and queued audio, held packets and decoded audio are dropped (again on resume)
6. A pusher thread writes mixed frames into a preallocated lock-free `audio::ring::SampleRing` (capacity `MAX_QUEUE_SAMPLES` = 2 × 400 ms + 3 frames)
7. **cpal callback** (`fill_output`): a replaced stream (old generation during a device switch) plays silence and takes nothing; stale audio (flush requested) is dropped; a new stream's first callback drops what lies beyond its block plus a frame; then it reads without ever waiting, counts consumed samples and the callback into `OutputLevel` (also while deafened, then it plays zeros), and notes its block size for its generation

**Output latency:** the queue swings between its low point (one frame, plus the block-size spread on devices whose blocks vary) and that plus one block, plus up to one 5 ms mixer wake (tokio timer; Windows timer granularity unverified). `AudioMixer::shutdown()` ends the mixer, the PCM thread and the readers with the session.

Counters per track reach `voice_connection_stats` (`playout[]`) and a debug log every 5 s. The contract of the whole receive path is checked by the property test `voice/playout_properties.rs`.

**Real-time note:** the callbacks use atomics and the lock-free ring only: no allocation, no lock, no logging.

### Capture → RTP
While muted, the device's samples are replaced by silence at the conversion, before any stage that keeps state (resampler, framer): nothing captured while muted can reach a frame encoded after unmuting. Capture numbers every 20 ms frame it delivers, gated and muted ones included, in `EncodedFrame { capture, frame, captured_at, talkspurt }`; `voice::rtp_clock::RtpClock` advances the RTP timestamp by exactly the frames between two packets, bridges only a restarted capture stream by the time between the two capture moments, and sets the marker bit on the first packet, after a restart and after gated or muted frames (not after a frame that could not be queued).

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
