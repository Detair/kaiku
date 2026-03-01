# Screen Sharing Phase 2: Server Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement the server-side signaling, track routing, and permission enforcement for screen sharing.

**Architecture:** Extend `TrackRouter` to handle video tracks, implement Redis-based limits, and add REST/WebSocket endpoints for signaling.

**Tech Stack:** Rust (server), Axum, Redis, WebRTC

**Design Doc:** `docs/plans/2026-01-19-screen-sharing-design.md`

**Working Directory:** `/home/detair/GIT/canis/.worktrees/screen-sharing`

**Build Command:** `SQLX_OFFLINE=true cargo build --workspace`

---

## Task 1: Extend TrackRouter for Video Tracks

**Files:**
- Modify: `server/src/voice/track.rs`
- Modify: `server/src/voice/peer.rs`

**Step 1: Update TrackRouter struct**
Update `TrackRouter` to manage subscriptions by `(user_id, TrackSource)`.
Currently it likely maps `user_id -> broadcast_track` (assuming audio only).
We need to support multiple tracks per user.

**Step 2: Add subscribe_video method**
Implement `subscribe_video` to handle video-specific subscription logic (e.g., requesting keyframes).

**Step 3: Update RTP forwarding**
Ensure `forward_video_rtp` correctly routes packets to subscribers of that specific source.

---

## Task 2: Implement Redis Limit Enforcement

**Files:**
- Modify: `server/src/voice/screen_share.rs` (add logic here or new file)
- Modify: `server/src/db/redis.rs` (if generic helper needed)

**Step 1: Implement try_start_screen_share**
Add a function that:
1. Generates a key `screenshare:limit:{channel_id}`.
2. Uses `INCR` to increment the counter.
3. Checks against `max_shares` (from channel config).
4. If limit exceeded, `DECR` and return error.
5. Sets/Updates expiration (e.g., 1 hour).

**Step 2: Implement stop_screen_share**
Add function to `DECR` the counter when a user stops sharing or disconnects.

---

## Task 3: Implement REST Endpoints

**Files:**
- Create: `server/src/api/channels/screenshare.rs`
- Modify: `server/src/api/channels/mod.rs`
- Modify: `server/src/api/routes.rs`

**Step 1: POST /api/channels/:id/screenshare/check**
- Check `SCREEN_SHARE` permission.
- Check channel limit (Redis peek, don't increment yet).
- Check `PREMIUM_VIDEO` flag if requesting 1080p60.
- Return `ScreenShareCheckResponse`.

**Step 2: POST /api/channels/:id/screenshare/start**
- Perform the actual Redis INCR check.
- Notify the SFU/Room state that user is sharing.
- Return success/failure.

**Step 3: POST /api/channels/:id/screenshare/stop**
- Decrement Redis counter.
- Notify SFU/Room state.

---

## Task 4: WebSocket Events & Room State

**Files:**
- Modify: `server/src/ws/events.rs`
- Modify: `server/src/voice/sfu.rs`

**Step 1: Define new ServerEvents**
- `ScreenShareStarted`
- `ScreenShareStopped`
- `ScreenShareQualityChanged`

**Step 2: Update VoiceRoomState**
Include `screen_shares: Vec<ScreenShareInfo>` in the initial room state sent on join.

**Step 3: Broadcast events**
Update `Room` methods to broadcast these events when REST endpoints are called.

---

## Task 5: Handle Late Joiners (PLI)

**Files:**
- Modify: `server/src/voice/track.rs`

**Step 1: Request PLI on subscription**
When a new peer subscribes to an existing video track, immediately send a Picture Loss Indication (PLI) to the publisher.
This forces the encoder to send a keyframe, so the new viewer doesn't see a black screen.

---

## Summary Checklist

- [x] `TrackRouter` supports multiple tracks per user
- [x] Redis limit enforcement implemented
- [x] REST API endpoints created and registered
- [x] WebSocket events defined and broadcast
- [x] PLI requested for new subscribers
- [x] Unit tests added for screen_share.rs and track.rs (PR #38)

## Implementation Status

**Completed:** 2026-01-23

All Phase 2 tasks have been implemented:
- `TrackRouter` in `server/src/voice/track.rs` uses `(Uuid, TrackSource)` keying
- Redis limits in `server/src/voice/screen_share.rs` with `try_start_screen_share` and `stop_screen_share`
- REST endpoints in `server/src/chat/screenshare.rs` (check, start, stop)
- WebSocket events in `server/src/ws/events.rs` (ScreenShareStarted, ScreenShareStopped, ScreenShareQualityChanged)
- PLI in `server/src/voice/ws_handler.rs` via `request_keyframe`

**Test Coverage (PR #38):**
- 20 tests for screen_share.rs (types, validation, serialization)
- 9 tests for track.rs (construction, empty router, concurrent access)
