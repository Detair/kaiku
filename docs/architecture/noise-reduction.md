# Noise Reduction Specification

This document outlines the strategy for implementing background noise reduction in the VoiceChat platform (Canis), catering to both Desktop (Tauri) and Browser clients.

## Strategy: The Hybrid "Tiered" Approach

We will implement a two-tier system to balance performance, complexity, and quality.

### Tier 1: Browser Native (Default / MVP)
Utilize the built-in audio processing capabilities of the browser engine (Chromium/WebKit). This is efficient, battery-friendly, and requires zero additional dependencies.

*   **Mechanism:** `MediaStreamConstraints` in `getUserMedia`.
*   **Performance Cost:** Negligible (handled by browser internals).
*   **Quality:** Good for steady noise (fans), variable for complex noise (voices, typing).
*   **Availability:** Universal (Chrome, Firefox, Safari, Edge).

### Tier 2: Advanced AI Suppression (Phase 4 / "Gamer Mode")
Utilize `rnnoise` (Recurrent Neural Network for Audio Noise Reduction) compiled to WebAssembly (WASM). This provides consistent, high-quality suppression comparable to dedicated gaming voice apps.

*   **Mechanism:** `AudioWorklet` + WASM.
*   **Performance Cost:** Higher CPU usage (running a neural net in real-time).
*   **Quality:** Excellent. Removes keyboard clicking, shouting in background, etc.
*   **Availability:** Requires `AudioWorklet` support (modern browsers).

---

## Implementation Details

### 1. Tier 1: Native Constraints (Implementation Ready)

**Location:** `client/src/lib/webrtc/browser.ts`

Modify the `getUserMedia` call to explicitly request processing:

```typescript
const constraints: MediaStreamConstraints = {
  audio: {
    deviceId: this.inputDeviceId ? { exact: this.inputDeviceId } : undefined,
    // Enable browser processing
    echoCancellation: true,
    noiseSuppression: true,
    autoGainControl: true,
    // Optional: High-pass filter to remove rumble
    // channelCount: 1 (RNNoise requires mono anyway)
  },
};
```

**Configuration:**
Add a toggle in `AudioSettings` store: `noiseSuppression: boolean`.

### 2. Tier 2: WASM Audio Processor (Future)

**Architecture:**
1.  **WASM Module:** Compile `rnnoise` C library to `rnnoise.wasm`.
2.  **Processor:** Create `RNNoiseProcessor.js` extending `AudioWorkletProcessor`.
3.  **Pipeline:**
    `Microphone Source` -> `RNNoise Worklet Node` -> `WebRTC MediaStream Destination`

**Data Flow:**
1.  Input: 48kHz / Float32.
2.  Worklet: Buffers 480 samples (10ms).
3.  WASM: Processes frame, returns denoised frame.
4.  Output: 48kHz / Float32.

**Crate Recommendation:**
Instead of raw C bindings, use a Rust crate that compiles to WASM, like `nnnoiseless` (Rust port of RNNoise).

---

## Roadmap Integration

### Phase 2 (Current)
- [ ] Implement Tier 1 (Native Constraints).
- [ ] Add toggle in Audio Settings UI.

### Phase 4 (Advanced)
- [ ] Build/Integrate `rnnoise-wasm`.
- [ ] Implement `AudioWorklet` pipeline.
- [ ] Add "High Quality" noise suppression option.

## Desktop vs. Browser

Since Tauri 2.0 uses the OS WebView:
*   **Desktop:** Behaves exactly like the browser. We do *not* need a native Rust audio processing thread unless we switch to `cpal` + `webrtc-rs` for local capture (which bypasses the WebView entirely).
*   **Recommendation:** Stick to the Web Audio API approach for consistency. It simplifies the codebase by sharing 100% of the audio logic between Web and Desktop.
