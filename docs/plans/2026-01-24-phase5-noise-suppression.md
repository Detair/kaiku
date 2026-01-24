# Design: Hardware-Accelerated Noise Suppression (Phase 5)

## 1. Overview
Implement client-side noise suppression using `rnnoise` (RNN-based Noise Suppression) compiled to WebAssembly, running inside an AudioWorklet. This avoids the latency of the Tauri Bridge.

## 2. Architecture

### 2.1. The Component Stack
1.  **Mic Input:** `navigator.mediaDevices.getUserMedia` (Browser Standard).
2.  **AudioContext:** The Web Audio API graph.
3.  **Processor:** `AudioWorkletNode` loading `rnnoise.wasm`.
4.  **Destination:** `MediaStreamAudioDestinationNode` -> WebRTC PeerConnection.

### 2.2. The WASM Module
We need a pre-compiled `rnnoise.wasm` that exposes a `process_frame` function.
*   Input: `Float32Array` (480 samples / 10ms at 48kHz).
*   Output: `Float32Array` (Cleaned audio).

## 3. Implementation Details

### 3.1. Files
*   `client/public/wasm/rnnoise.wasm`: The binary.
*   `client/public/processors/noise-processor.js`: The AudioWorklet code.

### 3.2. Worklet Code (`noise-processor.js`)
```javascript
class NoiseProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.port.onmessage = this.onInit.bind(this);
  }

  async onInit(e) {
    const wasmBytes = e.data;
    // Instantiate WASM
    this.wasm = await WebAssembly.instantiate(wasmBytes, ...);
    this.ready = true;
  }

  process(inputs, outputs) {
    if (!this.ready) return true;
    // Pass input[0] channel data to WASM memory
    // Call wasm.process()
    // Copy result to output[0]
    return true;
  }
}
registerProcessor('noise-processor', NoiseProcessor);
```

### 3.3. Client Logic (`client/src/lib/webrtc/audio_processing.ts`)
*   Function `setupAudioGraph(stream: MediaStream): MediaStream`.
*   Fetches the `.wasm` file.
*   Creates the Context and Worklet.
*   Pipes the stream through.

## 4. Step-by-Step Plan
1.  **Assets:** Acquire `rnnoise.wasm` and place in `client/public`.
2.  **Client:** Write `noise-processor.js` (The bridge between AudioWorklet and WASM).
3.  **Client:** Implement `NoiseSuppressor` class in TypeScript to manage the Worklet lifecycle.
4.  **UI:** Add toggle in `AudioDeviceSettings.tsx` to enable/disable.
5.  **Integration:** Hook into `VoiceService.ts` before the stream is added to the PeerConnection.