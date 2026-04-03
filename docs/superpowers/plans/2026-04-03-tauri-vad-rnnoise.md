# Tauri VAD + Noise Suppression Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement RNNoise-based voice activity detection and noise suppression in the Tauri desktop client's audio capture pipeline.

**Architecture:** Integrate the `nnnoiseless` crate into the CPAL capture callback. Switch local capture from stereo to mono. Process 480-sample (10ms) frames through RNNoise to get VAD probability + denoised audio. Gate Opus encoding based on VAD state. Emit speaking events to the frontend.

**Tech Stack:** Rust, nnnoiseless 0.5, CPAL, Opus, Tauri events, TypeScript

**Spec:** `docs/superpowers/specs/2026-04-03-tauri-vad-rnnoise-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `client/src-tauri/Cargo.toml` | Modify | Add `nnnoiseless` dependency |
| `client/src-tauri/src/audio/mod.rs` | Modify | Add `CAPTURE_CHANNELS` constant, `VadState` struct |
| `client/src-tauri/src/audio/handle.rs` | Modify | Mono capture, RNNoise integration, VAD gating, speaking events |
| `client/src-tauri/src/commands/voice.rs` | Modify | Add `set_vad_config` Tauri command |
| `client/src/lib/webrtc/tauri.ts` | Modify | Wire `setVadConfig`, listen for `voice:speaking` |

---

### Task 1: Add nnnoiseless dependency

**Files:**
- Modify: `client/src-tauri/Cargo.toml`

- [ ] **Step 1: Add nnnoiseless to Cargo.toml**

The dependency may already exist commented-out. Uncomment or add under `[dependencies]`:
```toml
# Audio denoising + VAD
nnnoiseless = { version = "0.5", default-features = false }
```

- [ ] **Step 2: Verify it compiles**

Run: `cd client/src-tauri && cargo check`
Expected: PASS (dependency resolves)

- [ ] **Step 3: Verify license**

Run: `cargo deny check licenses`
Expected: PASS (BSD-3-Clause is on allowed list)

- [ ] **Step 4: Commit**

```bash
git add client/src-tauri/Cargo.toml client/src-tauri/Cargo.lock
git commit -m "chore(client): add nnnoiseless dependency for VAD + noise suppression"
```

---

### Task 2: Add CAPTURE_CHANNELS constant and VadState

**Files:**
- Modify: `client/src-tauri/src/audio/mod.rs`

- [ ] **Step 1: Add CAPTURE_CHANNELS and VadState**

Add after the existing constants (line 18):

```rust
/// Capture-only channel count (mono for RNNoise compatibility).
/// Playback/mixer stays stereo (`CHANNELS`) since remote peers may send stereo.
pub const CAPTURE_CHANNELS: u16 = 1;

/// RNNoise processes 480-sample frames (10ms at 48kHz mono)
pub const RNNOISE_FRAME_SIZE: usize = 480;
```

Add before the `AudioError` enum:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Shared VAD + noise suppression state, accessible from CPAL callback thread
#[derive(Clone)]
pub struct VadConfig {
    /// Whether VAD gating is enabled
    pub enabled: Arc<AtomicBool>,
    /// VAD threshold (0–100, mapped from 0.0–1.0 × 100 for atomic storage)
    pub threshold: Arc<std::sync::atomic::AtomicU8>,
    /// Whether noise suppression (denoised output) is active
    pub denoise_enabled: Arc<AtomicBool>,
}

impl VadConfig {
    pub fn new() -> Self {
        Self {
            enabled: Arc::new(AtomicBool::new(false)),
            threshold: Arc::new(std::sync::atomic::AtomicU8::new(50)), // 0.5 default
            denoise_enabled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn set_threshold(&self, threshold: f32) {
        let t = (threshold.clamp(0.0, 1.0) * 100.0) as u8;
        self.threshold.store(t, Ordering::Relaxed);
    }

    pub fn get_threshold(&self) -> f32 {
        self.threshold.load(Ordering::Relaxed) as f32 / 100.0
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn set_denoise(&self, enabled: bool) {
        self.denoise_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn is_denoise_enabled(&self) -> bool {
        self.denoise_enabled.load(Ordering::Relaxed)
    }
}
```

- [ ] **Step 2: Add test for VadConfig**

Add to the existing `#[cfg(test)] mod tests` block:

```rust
#[test]
fn test_vad_config_defaults() {
    let config = VadConfig::new();
    assert!(!config.is_enabled());
    assert!((config.get_threshold() - 0.5).abs() < 0.01);
    assert!(!config.is_denoise_enabled());
}

#[test]
fn test_vad_config_setters() {
    let config = VadConfig::new();
    config.set_enabled(true);
    config.set_threshold(0.7);
    config.set_denoise(true);
    assert!(config.is_enabled());
    assert!((config.get_threshold() - 0.7).abs() < 0.02); // u8 quantization
    assert!(config.is_denoise_enabled());
}
```

- [ ] **Step 3: Run tests**

Run: `cd client/src-tauri && cargo test -p vc-client -- audio`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add client/src-tauri/src/audio/mod.rs
git commit -m "feat(client): add VadConfig and CAPTURE_CHANNELS constant"
```

---

### Task 3: Add VadConfig to AudioHandle

**Files:**
- Modify: `client/src-tauri/src/audio/handle.rs`

- [ ] **Step 1: Add VadConfig and AppHandle fields to AudioHandle**

Update the import line to include new types:
```rust
use super::{AudioDevice, AudioDeviceList, AudioError, CAPTURE_CHANNELS, CHANNELS, FRAME_SIZE, RNNOISE_FRAME_SIZE, SAMPLE_RATE, VadConfig};
```

Add fields to `AudioHandle`:

```rust
/// VAD + noise suppression configuration
vad_config: VadConfig,

/// Tauri app handle for emitting events from capture thread
app_handle: Option<tauri::AppHandle>,
```

Initialize in `AudioHandle::new()`:

```rust
vad_config: VadConfig::new(),
app_handle: None,
```

- [ ] **Step 2: Add VAD setter methods**

Add methods to `AudioHandle`:

```rust
/// Set VAD configuration
pub fn set_vad_config(&self, enabled: bool, threshold: f32) {
    self.vad_config.set_enabled(enabled);
    self.vad_config.set_threshold(threshold);
    debug!("VAD config: enabled={}, threshold={:.2}", enabled, threshold);
}

/// Set noise suppression
pub fn set_noise_suppression(&self, enabled: bool) {
    self.vad_config.set_denoise(enabled);
    debug!("Noise suppression: {}", enabled);
}

/// Get VAD config (for passing to capture task)
pub fn vad_config(&self) -> VadConfig {
    self.vad_config.clone()
}

/// Set Tauri app handle (call during voice initialization before start_capture)
pub fn set_app_handle(&mut self, handle: tauri::AppHandle) {
    self.app_handle = Some(handle);
}
```

- [ ] **Step 3: Verify compile**

Run: `cd client/src-tauri && cargo check -p vc-client`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add client/src-tauri/src/audio/handle.rs
git commit -m "feat(client): wire VadConfig into AudioHandle"
```

---

### Task 4: Switch capture to mono + integrate RNNoise

**Files:**
- Modify: `client/src-tauri/src/audio/handle.rs`

This is the core task. Modify `run_capture_task` to use mono capture, process through RNNoise, and gate output.

- [ ] **Step 1: Update run_capture_task signature**

Add `vad_config: VadConfig` and `app_handle: tauri::AppHandle` parameters to `run_capture_task`. Update the call site in `start_capture` to pass them.

- [ ] **Step 2: Switch to mono capture**

In `run_capture_task`, change:
```rust
let config = StreamConfig {
    channels: CAPTURE_CHANNELS,  // was: CHANNELS
    sample_rate: SAMPLE_RATE,
    buffer_size: BufferSize::Default,
};

let encoder = match Encoder::new(SAMPLE_RATE, OpusChannels::Mono, opus::Application::Voip) {
    // was: OpusChannels::Stereo
```

Update `frame_samples`:
```rust
let frame_samples = FRAME_SIZE * CAPTURE_CHANNELS as usize; // 960 × 1 = 960
```

- [ ] **Step 3: Create RNNoise DenoiseState**

All RNNoise processing and Opus encoding happen inside the CPAL callback closure, so these variables are moved into the closure — no Mutex needed (the callback owns them exclusively). Create them before `build_input_stream` so they're captured by move:

```rust
let mut denoiser = nnnoiseless::DenoiseState::new();
let mut rnnoise_input = [0.0f32; RNNOISE_FRAME_SIZE];
let mut rnnoise_output = [0.0f32; RNNOISE_FRAME_SIZE];
let mut rnnoise_pos: usize = 0;
let mut denoised_accumulator: Vec<f32> = Vec::with_capacity(FRAME_SIZE);
let mut original_accumulator: Vec<f32> = Vec::with_capacity(FRAME_SIZE);
let mut first_frame = true;

// VAD gate state
let mut gate_open = false;
let mut indicator_speaking = false; // separate from gate for VAD-disabled mode
let mut last_speech_time = std::time::Instant::now();
const VAD_HOLD_MS: u128 = 300;
```

Remove the existing `sample_buffer` Mutex — all buffering now happens in the closure-owned accumulators.

- [ ] **Step 4: Rewrite the CPAL callback**

Replace the inner callback logic. Remove the existing `sample_buffer` Mutex pattern — everything now lives in the closure. For each incoming f32 sample:

1. If `muted`: return early (existing behavior; also covers PTT since PTT toggles mute)
2. Store original sample, accumulate into `rnnoise_input[rnnoise_pos]` scaled to i16 range (`* 32767.0`)
3. Increment `rnnoise_pos`; when it reaches `RNNOISE_FRAME_SIZE` (480):
   a. Call `denoiser.process_frame(&mut rnnoise_output, &rnnoise_input)` → returns VAD probability
   b. If `first_frame`: discard output, set `first_frame = false`, reset `rnnoise_pos`, continue
   c. Scale denoised output back (`/ 32768.0`), push 480 samples to `denoised_accumulator`
   d. Push original 480 samples to `original_accumulator`
   e. Update speaking indicator: if `vad_prob > threshold` → `indicator_speaking = true`, reset timer. Else if `elapsed > VAD_HOLD_MS` → `indicator_speaking = false`. Emit `voice:speaking` on transitions via `app_handle.emit("voice:speaking", indicator_speaking)`. Note: `AppHandle::emit()` is synchronous, safe to call from the CPAL thread.
   f. Update gate: if VAD enabled → `gate_open = indicator_speaking`. If VAD disabled → `gate_open = true` (always open, but speaking events still fire for the UI indicator).
   g. Reset `rnnoise_pos = 0`
4. When `denoised_accumulator.len() >= FRAME_SIZE` (960 samples):
   a. If `gate_open`: choose denoised vs original based on `denoise_enabled`, convert to i16 (`(s * 32767.0).clamp(...) as i16`), Opus encode, send via `output_tx`
   b. Drain both accumulators

- [ ] **Step 5: Update start_capture to pass new params**

Modify `start_capture` to pass `self.vad_config.clone()` and `self.app_handle.clone()` to the capture task. The `app_handle` field was added in Task 3; ensure `set_app_handle()` is called during voice initialization (in `ensure_voice()` or `handle_voice_publisher_answer` in `commands/voice.rs`) before `start_capture`.

- [ ] **Step 6: Verify compile**

Run: `cd client/src-tauri && cargo check -p vc-client`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add client/src-tauri/src/audio/handle.rs
git commit -m "feat(client): RNNoise VAD + denoising in mono capture pipeline"
```

---

### Task 5: Add set_vad_config Tauri command

**Files:**
- Modify: `client/src-tauri/src/commands/voice.rs`

- [ ] **Step 1: Add set_vad_config command**

Follow the pattern of `set_mute` (line 509). Add:

```rust
#[tauri::command]
pub async fn set_vad_config(
    enabled: bool,
    threshold: f32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    debug!("Setting VAD config: enabled={}, threshold={:.2}", enabled, threshold);

    let voice = state.voice.read().await;
    let voice_state = voice.as_ref().ok_or("Voice not initialized")?;

    voice_state.audio.set_vad_config(enabled, threshold);

    Ok(())
}
```

- [ ] **Step 2: Add set_noise_suppression command**

```rust
#[tauri::command]
pub async fn set_noise_suppression(enabled: bool, state: State<'_, AppState>) -> Result<(), String> {
    debug!("Setting noise suppression: {}", enabled);

    let voice = state.voice.read().await;
    let voice_state = voice.as_ref().ok_or("Voice not initialized")?;

    voice_state.audio.set_noise_suppression(enabled);

    Ok(())
}
```

- [ ] **Step 3: Register commands in main.rs**

Find the `invoke_handler` in `main.rs` and add `set_vad_config` and `set_noise_suppression` to the list.

- [ ] **Step 4: Verify compile**

Run: `cd client/src-tauri && cargo check -p vc-client`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add client/src-tauri/src/commands/voice.rs client/src-tauri/src/main.rs
git commit -m "feat(client): add set_vad_config and set_noise_suppression Tauri commands"
```

---

### Task 6: Wire TypeScript adapter

**Files:**
- Modify: `client/src/lib/webrtc/tauri.ts`

- [ ] **Step 1: Wire setVadConfig to Tauri command**

Replace the no-op `setVadConfig` (line 137-139):

```typescript
setVadConfig(enabled: boolean, threshold: number): void {
  invoke("set_vad_config", { enabled, threshold }).catch((err) =>
    console.error("[TauriVoiceAdapter] Failed to set VAD config:", err),
  );
}
```

- [ ] **Step 2: Clean up setNoiseSuppression**

The existing code already invokes `set_noise_suppression` but has a fallback catch that logs "not implemented." Remove the fallback now that the backend exists (around line 125-134):

```typescript
async setNoiseSuppression(
  enabled: boolean,
): Promise<VoiceResult<void>> {
  try {
    await invoke("set_noise_suppression", { enabled });
    this.noiseSuppression = enabled;
    return { ok: true, value: undefined };
  } catch (err) {
    console.error("[TauriVoiceAdapter] Failed to set noise suppression:", err);
    return { ok: false, error: { type: "unknown", message: String(err) } };
  }
}
```

- [ ] **Step 3: Add voice:speaking event listener**

In the `setupEventListeners` method (around line 528), add after the existing listeners:

```typescript
this.unlisteners.push(
  await listen<boolean>("voice:speaking", (event) => {
    this.events.onSpeakingChange?.(event.payload);
  }),
);
```

- [ ] **Step 4: Run client tests**

Run: `cd client && bun run test:run`
Expected: 577+ tests pass

- [ ] **Step 5: Commit**

```bash
git add client/src/lib/webrtc/tauri.ts
git commit -m "feat(client): wire VAD config and speaking events in Tauri adapter"
```

---

### Task 7: Update CHANGELOG and final verification

**Files:**
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Add CHANGELOG entries**

Under `[Unreleased] > ### Added`:
```markdown
- Voice activity detection (VAD) on desktop client — microphone is gated using RNNoise ML model with configurable threshold and 300ms hold-open
- Noise suppression on desktop client — RNNoise denoises audio in real-time when enabled in voice settings
- Local speaking indicator on desktop client — green ring animates when you're speaking
```

Under `[Unreleased] > ### Changed`:
```markdown
- Desktop voice capture switched from stereo to mono for bandwidth efficiency and RNNoise compatibility
```

- [ ] **Step 2: Run full verification**

```bash
SQLX_OFFLINE=true cargo clippy -p vc-client -- -D warnings
cd client && bun run tsc --noEmit && bun run test:run
```
Expected: All pass

- [ ] **Step 3: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs: add CHANGELOG entries for desktop VAD + noise suppression"
```
