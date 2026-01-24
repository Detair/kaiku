# Phase 5: Architectural Synergies - Implementation Plan

## Overview
Phase 5 focuses on leveraging our specific technology stack (Rust, Tauri, Postgres) to deliver features that give Canis a competitive edge in performance and capability.

## Master Checklist

### 1. Postgres Native Search
**Goal:** Instant message history search without external services.
- [ ] **DB:** Migration `20260125000000_add_message_search.sql` (tsvector + GIN index).
- [ ] **Backend:** `search_messages` query using `websearch_to_tsquery`.
- [ ] **API:** `GET /guilds/{id}/search` endpoint.
- [ ] **Client:** `SearchSidebar` component and `SearchService`.

### 2. WASM Server Plugins
**Goal:** Safe, high-performance bot logic running inside the server.
- [ ] **DB:** Migration `20260125000001_add_plugins.sql`.
- [ ] **Backend:** Integrate `wasmtime` crate.
- [ ] **Backend:** Implement `PluginEngine` and Host Functions (SDK).
- [ ] **Backend:** Hook `on_message` event to plugin triggers.
- [ ] **Client:** Admin UI for uploading `.wasm` files.

### 3. Tauri Game Overlay & System Tray
**Goal:** Background operation and in-game visibility.
- [ ] **Tauri:** Enable System Tray in config and `main.rs`.
- [ ] **Tauri:** Add transparent `overlay` window to config.
- [ ] **Rust:** Implement Windows API hook for Click-Through (`WS_EX_TRANSPARENT`).
- [ ] **Client:** Create `overlay.html` and UI.
- [ ] **Sync:** Ensure overlay receives voice state updates.

### 4. Diff-based State Sync
**Goal:** 90% bandwidth reduction for state updates.
- [ ] **Protocol:** Define `PatchEvent` JSON structure.
- [ ] **Backend:** Refactor `UserUpdate` broadcast to use generic Patch.
- [ ] **Client:** Update `UserStore` and `GuildStore` to handle Patch events.

### 5. Hardware Noise Suppression (WASM)
**Goal:** Crystal clear audio processing on the client.
- [ ] **Assets:** Add `rnnoise.wasm` to public assets.
- [ ] **Client:** Implement `AudioWorkletProcessor` to run WASM audio DSP.
- [ ] **UI:** Add "AI Noise Suppression" toggle to Audio Settings.

## Execution Order
1.  **Search (Postgres)** - Lowest risk, high value.
2.  **State Sync** - High impact on network performance, good to have early.
3.  **Noise Suppression** - Independent frontend task.
4.  **Plugins (WASM)** - High complexity, requires careful security review.
5.  **Overlay** - Highest platform risk (Windows API intricacies), save for last.

## Next Steps for Agent
Start with **Item 1: Postgres Native Search**.
1.  Read `docs/plans/2026-01-24-phase5-postgres-search.md`.
2.  Create the database migration.
3.  Implement the backend query and API.
