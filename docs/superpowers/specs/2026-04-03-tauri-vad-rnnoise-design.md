# Tauri VAD + Noise Suppression via RNNoise

**Date:** 2026-04-03
**Status:** Approved
**Scope:** Desktop client (Tauri) — voice activity detection and noise suppression

## Problem

The Tauri desktop client's VAD toggle is a no-op. `setVadConfig` in the TypeScript adapter discards its arguments. Desktop users see a working VAD toggle in settings but it does nothing — audio transmits continuously regardless. The speaking indicator never animates. Noise suppression is also unimplemented on the backend.

## Solution

Integrate the `nnnoiseless` crate (pure Rust RNNoise port, BSD-3-Clause) into the CPAL audio capture pipeline. RNNoise provides both VAD probability and denoised audio in a single pass, closing two gaps at once.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| VAD approach | RNNoise (ML-based) | Better than RMS: handles non-speech noise (fans, typing). Also provides denoising for free. |
| RNNoise binding | `nnnoiseless` (pure Rust, BSD-3-Clause) | No native dependency, no FFI. Cross-platform build stays simple. Use `default-features = false` to avoid CLI/DAP deps. |
| Channel count | Mono (was stereo) | Voice chat has no use for stereo mic input. Halves processing, aligns with RNNoise's mono requirement. Discord and most voice apps use mono. |
| Speaking indicator | Rust → Tauri event | CPAL callback is the source of truth for gate state. Emits `voice:speaking` event on transitions. |

## Pipeline Architecture

```
Microphone (CPAL, mono 48kHz f32)
    │
    ▼
Accumulate 480 samples (10ms)
    │
    ▼
Scale f32 [-1.0, 1.0] → i16 range [-32768.0, 32767.0]
(nnnoiseless expects i16-range f32 values)
    │
    ▼
RNNoise process_frame()
    ├─ Output: denoised PCM (480 samples, i16-range f32)
    ├─ Output: VAD probability (0.0–1.0)
    └─ Note: discard first frame output (fade-in artifact)
    │
    ▼
Scale back → f32 [-1.0, 1.0] (for denoised path)
    │
    ▼
Accumulate 2 frames → 960 samples (20ms)
    │
    ▼
VAD Gate Decision
    ├─ probability > threshold → gate OPEN
    ├─ 300ms hold-open after last speech
    ├─ PTT active → bypass VAD (gate OPEN)
    └─ Muted → skip everything
    │
    ▼
Opus Encode (mono, 48kHz, 960 samples)
    │
    ▼
Send to WebRTC track (or discard if gate closed)
```

## VAD Gate Logic

The gate is a simple state machine:

- **CLOSED → OPEN**: VAD probability exceeds user threshold on any 10ms frame.
- **OPEN → HOLD**: VAD probability drops below threshold. Start 300ms timer.
- **HOLD → OPEN**: VAD probability exceeds threshold before timer expires.
- **HOLD → CLOSED**: Timer expires without speech detected.

State transitions emit `voice:speaking` Tauri events. The frontend Tauri adapter listens and calls `onSpeakingChange(speaking)` to drive the UI speaking ring.

**When VAD is disabled:** RNNoise still runs for noise suppression. The gate stays permanently open. `voice:speaking` events are still emitted based on VAD probability (for the speaking indicator UI), but the gate does not close.

Priority chain: **Mute > PTT > VAD**. If muted, no audio captured. If PTT active, VAD bypassed. Otherwise VAD controls the gate.

## Noise Suppression

RNNoise always runs in the pipeline. The `setNoiseSuppression(enabled)` toggle controls which samples go to Opus:
- **Enabled**: Denoised samples from RNNoise output (scaled back to f32 range).
- **Disabled**: Original samples (but RNNoise still runs for VAD probability).

This avoids conditional pipeline branching and gives noise suppression with zero additional cost.

## Mono Pipeline Migration

Current: stereo (2 channels), 960 samples/channel per 20ms frame.
New: mono (1 channel), 960 samples per 20ms frame.

The mono change affects multiple sites — use a separate `CAPTURE_CHANNELS = 1` constant for the local capture pipeline while keeping `CHANNELS = 2` for the playback/mixer path (remote peers may send stereo from browsers):

- **CPAL input config**: use `CAPTURE_CHANNELS` (1) for mic capture
- **Opus encoder**: stereo → mono (VoIP profile unchanged)
- **Remote Opus decoder** (`commands/voice.rs`): stays stereo — browser peers send stereo
- **Audio mixer** (`voice/audio_mixer.rs`): stays stereo — mixes remote tracks for playback
- **Playback buffer cap** (`handle.rs`): stays stereo-based for output path
- **RNNoise**: processes 480-sample mono frames (10ms at 48kHz)
- Two RNNoise frames = one 960-sample Opus frame

## Files to Change

### Rust (client/src-tauri/src/)

1. **`audio/mod.rs`** — Add `CAPTURE_CHANNELS = 1` constant (keep `CHANNELS = 2` for playback). Add VAD state struct (enabled, threshold, hold timer, gate state, noise suppression toggle).

2. **`audio/handle.rs`** — Core changes:
   - Switch CPAL capture config to mono (`CAPTURE_CHANNELS`)
   - Create `nnnoiseless::DenoiseState` on capture start (heap-allocated via `Box`)
   - Discard first `process_frame` output (fade-in artifact)
   - In audio callback: accumulate 480 samples, scale to i16 range, run `process_frame()`, collect VAD probability and denoised output
   - Scale denoised output back to f32 range
   - Accumulate two processed frames (960 samples) for Opus
   - Apply gate logic (threshold + hold timer) before encoding
   - Emit `voice:speaking` on gate transitions
   - Choose denoised vs original based on noise suppression flag

3. **`commands/voice.rs`** — Add `set_vad_config` Tauri command that sets enabled + threshold on the shared audio state. Add `set_noise_suppression` forwarding if not already wired.

### TypeScript (client/src/lib/webrtc/)

4. **`tauri.ts`** — Wire `setVadConfig(enabled, threshold)` to invoke `set_vad_config` Tauri command. Register listener for `voice:speaking` events, call `onSpeakingChange` callback.

### Build

5. **`client/src-tauri/Cargo.toml`** — Add `nnnoiseless = { version = "0.5", default-features = false }`.

## Constants

| Constant | Value | Source |
|----------|-------|--------|
| Sample rate | 48000 Hz | Existing |
| Capture channels | 1 (mono) | New — local capture only |
| Playback channels | 2 (stereo) | Existing — remote audio unchanged |
| RNNoise frame size | 480 samples (10ms) | RNNoise requirement |
| Opus frame size | 960 samples (20ms) | Existing |
| VAD hold time | 300ms | Matches browser |
| Default threshold | 0.5 | Matches browser default |

## Success Criteria

- Desktop VAD toggle gates audio transmission (no more no-op)
- Speaking ring animates for local user on desktop
- Noise suppression toggle produces audibly cleaner audio
- No latency increase (RNNoise processes 10ms frames within 20ms Opus cadence)
- `cargo clippy` passes, `bun run test:run` passes
- License check passes (`nnnoiseless` is BSD-3-Clause — on allowed list)

## Out of Scope

- VP8 video decoding (separate spec)
- Custom RNNoise models / training
- Per-user noise suppression profiles
- Simulcast encoding improvements
