# Kaiku Roadmap

**Current Phase:** Phase 6 (Competitive Differentiators & Mastery) - In Progress
**Last Updated:** 2026-03-25

## Recent Deliveries (2026-03-25)

- [Chat] Discord-style unread message tracking — forward-only read cursor, "New Messages" divider, scroll-to-bottom ack (#488)
- [Voice] VAD noise gate — audio gating with 300ms hold-open, PTT priority (#489)
- [Client] Desktop session persistence via OS keyring — no more re-login on restart (#490)
- [Voice] Dual-PeerConnection for desktop — subscriber PC, Opus mixer, VP8 decoder stub (#491, #492)

## Active Initiatives

- [Beta] Closed Beta Readiness (47 items from 2026-03-19 deep review)
  - Checklist: [Beta checklist](../developer-guide/plans/2026-03-19-beta-checklist.md)
- [Client] Desktop Client Parity — see detailed TODO below
- [Infra] SaaS Observability & Telemetry
  - Plan: [Phase 7 observability task plan](../developer-guide/plans/2026-02-27-phase-7-observability-telemetry-task-plan.md)

## Desktop Client TODO (Tauri Parity with Web)

### Completed (2026-03-25)
- [x] Session persistence — keyring-based restore on startup (#490)
- [x] Dual-PeerConnection — subscriber PC for remote audio/video (#491)
- [x] Audio mixer — multi-user Opus decode + PCM mixing + CPAL playback (#491)
- [x] Mixer pacing — 20ms interval timer, playback buffer cap (#492)
- [x] Subscriber signaling — offer/answer/ICE fully wired (#491)

### In Progress
- [ ] **VP8 video decoding** — stub reads RTP packets, emits track lifecycle events, but does not decode frames. Needs: libvpx integration, VP8 depacketization, JPEG encode, frame emission via Tauri events. `client/src-tauri/src/voice/video_decoder.rs` has documented TODO pipeline.
- [ ] **VAD gating in Tauri** — browser adapter has full VAD (300ms hold, PTT priority). Tauri adapter's `setVadConfig` is a no-op. Rust audio capture pipeline needs VAD monitoring + track gating.

### Not Started
- [ ] **Connection metrics** — `getConnectionMetrics()` always returns null on desktop. Needs Tauri command to fetch `RTCStatsReport` from webrtc-rs.
- [ ] **Speaking indicator (local user)** — `onSpeakingChange` never emitted from Tauri adapter. Rust capture pipeline should emit `voice:speaking` event based on audio level.
- [ ] **Output device selection** — `setOutputDevice()` Tauri command exists but implementation unclear/untested.
- [ ] **Noise suppression** — Tauri accepts setting but backend logs "not implemented."
- [ ] **System tray** — minimize to tray, tray icon with unread badge.
- [ ] **Auto-update** — Tauri updater plugin integration.
- [ ] **WebSocket reconnect channel refresh** — reconnect doesn't re-fetch channel list metadata (stale unread dots until guild switch).
