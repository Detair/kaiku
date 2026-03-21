<!-- Parent: ../AGENTS.md -->

# lib/webrtc

## Purpose

WebRTC abstraction layer for voice calls. Provides unified interface with separate implementations for browser (Web Audio API) and Tauri (native Rust audio).

## Key Files

- `index.ts` - Factory pattern: creates appropriate adapter (Browser/Tauri) based on runtime
- `types.ts` - Shared VoiceAdapter interface and Result types
- `browser.ts` - Browser implementation using RTCPeerConnection and Web Audio API
- `tauri.ts` - Tauri implementation (delegates to Rust backend via IPC)

## For AI Agents

### Factory Pattern

Use `createVoiceAdapter()` to get platform-specific implementation:

```typescript
import { createVoiceAdapter } from "@/lib/webrtc";
const adapter = await createVoiceAdapter();

// VoiceAdapter interface (both platforms) — dual PeerConnection model:
await adapter.join(channelId);
await adapter.leave();
await adapter.setMute(true);
await adapter.handlePublisherAnswer(channelId, sdp);
await adapter.handleSubscriberOffer(channelId, sdp);
await adapter.handleIceCandidate(channelId, candidate, pcType);
```

### Browser Implementation

`browser.ts` handles WebRTC signaling with a dual PeerConnection model:

- Creates two RTCPeerConnections (publisher + subscriber) with STUN/TURN servers
- Publisher PC: captures microphone, screen share, and webcam tracks (client sends offer)
- Subscriber PC: receives remote audio/video tracks (server sends offer)
- Handles ICE candidate exchange with `pc_type` routing
- Audio output via HTMLAudioElement
- Manual cleanup of streams and connections

### Tauri Implementation

`tauri.ts` is thin wrapper around Rust backend:

- Audio capture/playback via cpal (native)
- Opus encoding/decoding
- WebRTC handled by webrtc-rs
- Lower latency, better CPU efficiency

### Result Type

All operations return `Result<T, E>` for explicit error handling:

```typescript
const result = await adapter.handleSubscriberOffer(channelId, sdp);
if (result.ok) {
  const answer = result.value; // SDP answer string
} else {
  console.error(result.error); // Error message
}
```

### Singleton Management

- Single adapter instance per app lifecycle
- `getVoiceAdapter()` returns existing instance (no re-creation)
- `resetVoiceAdapter()` for cleanup/testing

### Critical Timing

ICE candidates must be processed immediately for NAT traversal:

- No batching or delayed processing
- WebSocket handler processes ICE events synchronously
- Browser mode measures processing time (<10ms target)

### State Management

Adapter does NOT manage UI state:

- Use `@/stores/voice` for connection status
- Use `@/stores/call` for call state (ringing, participants)
- Adapter focuses only on WebRTC mechanics
