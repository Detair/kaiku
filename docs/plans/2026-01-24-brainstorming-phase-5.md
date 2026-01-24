# Brainstorming: Phase 5 & Beyond

This document captures potential features and improvements for the next stages of VoiceChat (Canis) development.

## 1. AI & Automation (Agents)
*   **AI Channel Summarizer:** An agent that can provide summaries of missed conversations.
*   **Voice Transcription:** Real-time speech-to-text for voice channels, allowing for searchable transcripts.
*   **Translation Agent:** Real-time message translation for multi-lingual communities.
*   **Auto-Moderation:** AI-driven detection of toxicity, spam, and NSFW content.

## 2. Developer Ecosystem (The "Bot" Phase)
*   **Slash Commands:** Standardized API for user interaction with bots.
*   **Gateway API:** Robust WebSocket events for external applications.
*   **Webhooks:** Outgoing and incoming webhooks for integrations (GitHub, GitLab, etc.).
*   **Official SDKs:** Rust, TypeScript, and Python libraries to build Canis bots.

## 3. Communication Enhancements
*   **Threaded Conversations:** Discord/Slack style threads to keep side-discussions organized.
*   **Native Polls:** Integrated UI for creating and voting on polls.
*   **Events Calendar:** A guild-specific calendar for scheduling raids, meetings, or game nights.
*   **Rich Text Editor:** Improving the markdown experience with a toolbar and better previews.

## 4. Advanced Voice & Video
*   **Screen Sharing (Phase 2+):** Supporting multiple simultaneous screen shares and webcams.
*   **AI Noise Suppression:** Integration with local AI models (like RNNoise or similar) for crystal clear audio.
*   **Spatial Audio:** Positional audio in voice channels for a more immersive experience.
*   **Video Recording:** Option to record voice/video calls (with consent indicators).

## 5. UI/UX & Customization
*   **Custom Emoji/Sticker Packs:** Guild-level custom assets.
*   **CSS Themes:** Support for advanced users to apply custom CSS (potentially shared via community "skin" marketplace).
*   **Animated Avatars & Banners:** Support for GIF/APNG/WebP animations.
*   **Custom Sound Packs:** Expanding the current sound pack system to allow user-uploaded notification sounds.

## 6. Infrastructure & SaaS Features
*   **Federation (Matrix-like):** Allowing users from different Canis instances to communicate.
*   **Multi-tenancy Improvements:** Better isolation and resource management for SaaS providers.
*   **Mobile App (Tauri v2):** Bridging the gap to Android and iOS.
*   **Analytics Dashboard:** Privacy-respecting stats for guild owners.

## 7. Gaming Integrations
*   **Steam/Epic Games Presence:** Better integration with game launchers to show detailed activity.
*   **In-Game Overlay:** A lightweight overlay to see who's talking and respond to messages without Alt-Tab.
*   **Tournament System:** Native support for brackets and score tracking.

## 8. Architectural Synergies (Recommended)
Features that leverage our specific technical stack (Rust, Tauri, Postgres) to gain a competitive advantage.

### 8.1. Postgres Native Full-Text Search
*   **Concept:** Instead of adding a heavy external dependency like Elasticsearch or Meilisearch, we utilize PostgreSQL's built-in `tsvector` and `tsquery` capabilities to provide instant message history search.
*   **Implementation Path:**
    *   **Backend:** Add a generated column `content_search` (tsvector) to the `messages` table, automatically updated via triggers or generated on insert. Create a GIN index on this column.
    *   **API:** specific endpoint `/api/v1/guilds/{id}/search?q=query` that executes `websearch_to_tsquery`.
    *   **Client:** A "Search" sidebar component that highlights results and allows jumping to the specific message context (using our existing virtual scroll).
*   **Strategic Value:**
    *   **Performance:** Sub-millisecond search queries for millions of messages without extra RAM usage.
    *   **Simplicity:** Keeps the deployment footprint small (single container), crucial for self-hosted users.

### 8.2. WASM-based Server Plugins (Sandboxed Bots)
*   **Concept:** Allow users to upload compiled WebAssembly (WASM) modules that run *inside* the server process to handle events, essentially "Server-side Bots" that are safe and fast.
*   **Implementation Path:**
    *   **Runtime:** Integrate `wasmtime` or `wasmer` into the Rust backend.
    *   **API:** Define a `canis-plugin-sdk` (Rust crate) that exposes safe host functions (e.g., `send_message`, `kick_user`, `get_role`).
    *   **Execution:** When an event occurs (e.g., `MessageCreate`), the server serializes the event and invokes the registered WASM module's handler.
*   **Strategic Value:**
    *   **Performance:** Plugins run at near-native speed with zero network latency (no HTTP round-trips like standard Webhook bots).
    *   **Security:** WASM is sandboxed by default; a bad plugin cannot crash the server or access the file system unless explicitly allowed.
    *   **Differentiation:** A major selling point against Discord (which only allows HTTP bots).

### 8.3. Tauri Game Overlay & System Tray
*   **Concept:** A lightweight UI layer that draws *over* full-screen games, allowing users to see who is talking, toggle mute, or read notifications without Alt-Tabbing.
*   **Implementation Path:**
    *   **System Tray:** Use Tauri's native system tray API for background operation, "Push-to-Talk" globally, and quick settings.
    *   **Overlay:** This is complex. On Windows, it requires hooking DirectX/OpenGL or creating a transparent, click-through top-most window. On Linux, it requires Wayland/X11 compositing tricks.
    *   **Approach:** Start with a "Widget" mode (floating always-on-top window) which is natively supported by Tauri, then investigate platform-specific injection.
*   **Strategic Value:**
    *   **User Retention:** Gamers *need* to see who is talking. This is the #1 feature gap compared to TeamSpeak/Discord for hardcore gamers.
    *   **Efficiency:** Rust/Tauri footprint is minimal, ensuring the overlay doesn't lower game FPS.

### 8.4. Diff-based State Sync (Bandwidth Optimization)
*   **Concept:** Drastically reduce data usage by sending only the *changes* (deltas) to client state, rather than full object snapshots.
*   **Implementation Path:**
    *   **Protocol:** Implement a Delta-State system. When a user changes their nickname, instead of broadcasting the full `User` object (1KB+), send `op: update, id: 123, field: nickname, val: "NewName"` (<50 bytes).
    *   **Tooling:** Use `serde-diff` or manual struct diffing in Rust.
    *   **Client:** The frontend store (SolidJS/React) applies these patches to its local state.
*   **Strategic Value:**
    *   **Mobile Experience:** Critical for users on unstable 4G/5G connections.
    *   **Scale:** Reduces WebSocket bandwidth usage by 90% during "thundering herd" events (e.g., thousands of users coming online at once).

### 8.5. Hardware-Accelerated Noise Suppression
*   **Concept:** Integrate high-quality, AI-driven noise suppression directly into the audio pipeline, running locally on the client.
*   **Implementation Path:**
    *   **Library:** Bind to `rnnoise` (C library) or a Rust port (`nnnoiseless`) directly in the `src-tauri` layer.
    *   **Pipeline:** Intercept raw PCM audio from the microphone -> Process through Noise Suppression Model -> Encode to Opus -> Send to WebRTC.
    *   **Optimization:** Use SIMD instructions (AVX2/NEON) which Rust supports well.
*   **Strategic Value:**
    *   **Quality:** Elevates voice quality to professional standards (filtering out keyboard clacking, fans, breathing).
    *   **Privacy:** Processing happens 100% on-device; no audio is ever sent to a cloud API for cleaning.
