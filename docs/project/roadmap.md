# Kaiku Roadmap

**Current Phase:** Phase 6 (Competitive Differentiators & Mastery) - Near complete
**Last Updated:** 2026-05-07

## Recent Deliveries

### 2026-04 (post-beta hardening + test infrastructure)
- [Voice/Tauri] Native VP8 decode for remote screen shares — closes the desktop screen-share viewer gap (#532)
- [Voice/Tauri] RNNoise VAD + noise suppression + speaking indicator — closes 3 desktop parity gaps in one PR (#507)
- [Voice/Server] Server security: self-mute, voice rate limiting, screen share slot leak fix (#529)
- [Voice/Tauri] Tauri RTP protocol: per-session seq/ts, VP8 payload_type (#531)
- [Voice/Web] Buffer ICE candidates until remote description is set (#530)
- [Voice/Android] Publisher PeerConnection for dual-PC parity (#533)
- [Client/Web] Responsive overhaul with mobile drawer navigation (#528)
- [Infra] sqlx::test integration migration — per-test DB isolation eliminates cross-process Postgres deadlock flake (#541–#546)
- [Infra] Codebase consistency standards across 9 phases (#512)
- [Infra] CI drift fix on main: nightly fmt routing, rustls-webpki advisory bump (#536)
- [Security] Audit follow-ups: advisory ignores, client dep patches, OSV-scanner CI job (#513, #514, #518)
- [Voice/Infra] TURN HMAC time-limited credentials + coturn HMAC shared-secret (#499, #500)
- [Android] Test infrastructure (EglBaseProvider DI seam) + 13-test triage + 3 CoroutineScope-injection fixes (#534, #537, #538, #539, #540)

### 2026-03-25
- [Chat] Discord-style unread message tracking — forward-only read cursor, "New Messages" divider, scroll-to-bottom ack (#488)
- [Voice] VAD noise gate — audio gating with 300ms hold-open, PTT priority (#489)
- [Client] Desktop session persistence via OS keyring — no more re-login on restart (#490)
- [Voice] Dual-PeerConnection for desktop — subscriber PC, Opus mixer, VP8 decoder stub (#491, #492)

## Active Initiatives

- [Beta] Closed Beta Readiness (47 items from 2026-03-19 deep review)
  - Checklist: [Beta checklist](../developer-guide/plans/2026-03-19-beta-checklist.md)
- [Client] Desktop Client Parity — see detailed TODO below
- [Client] Android Native Client — substantial progress through Phase 2; spec at `docs/superpowers/specs/2026-04-13-mobile-implementation-fixes-design.md`
- [Infra] SaaS Observability & Telemetry
  - Plan: [Phase 7 observability task plan](../developer-guide/plans/2026-02-27-phase-7-observability-telemetry-task-plan.md)

## Desktop Client TODO (Tauri Parity with Web)

### Completed
- [x] Session persistence — keyring-based restore on startup (#490)
- [x] Dual-PeerConnection — subscriber PC for remote audio/video (#491)
- [x] Audio mixer — multi-user Opus decode + PCM mixing + CPAL playback (#491)
- [x] Mixer pacing — 20ms interval timer, playback buffer cap (#492)
- [x] Subscriber signaling — offer/answer/ICE fully wired (#491)
- [x] **VP8 video decoding** — libvpx integration, depacketization, frame emission via Tauri events (#532)
- [x] **VAD gating in Tauri** — RNNoise-driven mic gating with 300ms hold-open, PTT priority (#507)
- [x] **Noise suppression** — RNNoise denoising in capture pipeline (#507)
- [x] **Speaking indicator (local user)** — `voice:speaking` events emitted from Rust capture pipeline (#507)

### Not Started
- [ ] **Connection metrics** — `getConnectionMetrics()` always returns null on desktop. Needs Tauri command to fetch `RTCStatsReport` from webrtc-rs.
- [ ] **Output device selection** — `setOutputDevice()` Tauri command exists but implementation unclear/untested.
- [ ] **System tray** — minimize to tray, tray icon with unread badge.
- [ ] **Auto-update** — Tauri updater plugin integration.
- [ ] **WebSocket reconnect channel refresh** — reconnect doesn't re-fetch channel list metadata (stale unread dots until guild switch).
