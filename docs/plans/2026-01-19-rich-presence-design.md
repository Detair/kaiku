# Design: Social Rich Presence

**Date:** 2026-01-19
**Status:** Design Draft
**Phase:** 4 (Advanced Features)

## Overview

Display "Rich Presence" information (e.g., "Playing Minecraft", "Coding in VS Code") next to users in the Member List, Friends List, and User Popups.

## Detection Strategy

### 1. Native Desktop (Tauri)
We need a robust way to detect running processes cross-platform.

**Libraries:**
- `sysinfo` (Rust crate): Reliable cross-platform process listing.

**Logic:**
1.  **Poll Process List:** Every 15-30 seconds.
2.  **Match Strategy:**
    - Maintain a lightweight `games.json` mapping (Process Name -> Display Name + Icon ID).
    - Example: `javaw.exe` -> "Minecraft", `Code.exe` -> "Visual Studio Code".
    - Allow fuzzy matching or regex for dynamic process names.
3.  **Privacy:**
    - Only send the *matched* Activity ID to the server, not the full process list.
    - User setting to toggle "Share Game Activity" globally or per-game.

### 2. Discord RPC Bridge (Future/Optional)
- Many games already emit rich data to local Discord IPC pipe.
- We could implement a mock Discord IPC server to intercept these payloads (e.g., "In Match: 3/5 rounds").
- *Decision:* Out of scope for v1. Stick to process name detection first.

## Data Model

### Database (`activities` table - optional, or just config)
We might not need a DB table if the mapping is static code/json. Let's use a static mapping for now to keep it simple.

### User Presence Update
Extend the existing WebSocket `presence` payload.

```json
{
  "type": "presence_update",
  "user_id": "uuid",
  "status": "online", // existing
  "activity": {
    "type": "game", // or "listening", "coding"
    "name": "Minecraft",
    "started_at": "2024-01-19T10:00:00Z",
    "details": "Creative Mode" // Future (RPC only)
  }
}
```

## UI Implementation

### Member List Item
```
[Avatar]  User Name
          🟢 Playing Minecraft
```
- If playing: Show game icon (small) or just text "Playing X".
- Color: Use a distinct color (e.g., purple/brand) for the "Playing" text to separate it from custom status.

### User Popover
```
┌──────────────────────────────────────┐
│ [Banner]                             │
│ [Avatar] User Name                   │
│          🟢 Online                   │
├──────────────────────────────────────┤
│ PLAYING A GAME                       │
│ [Icon]  Minecraft                    │
│         Started 2 hours ago          │
└──────────────────────────────────────┘
```

## Backend Changes

- Update `server/src/ws/mod.rs` to handle `activity` field in presence updates.
- Broadcast activity changes to relevant subscribers (guild members, friends).
- Sanitize activity names (max length, no profanity) if we allow custom inputs later.

## Privacy Controls

- **Settings -> Privacy:**
  - Toggle: "Display current activity as a status message"
  - List of detected games: Toggle visibility per game.

## Roadmap

1.  **Backend:** Update `Presence` struct and WebSocket handlers.
2.  **Tauri:** Implement `sysinfo` polling and `games.json` matcher.
3.  **Client:** Update `stores/presence.ts` and UI components.
