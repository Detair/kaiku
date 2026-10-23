<!-- Parent: ../AGENTS.md -->

# lib

## Purpose

Shared utility libraries, type definitions, and platform abstraction layer. Provides type-safe wrappers for Tauri commands with browser fallback support.

## Key Files

- `types.ts` - Shared TypeScript types mirroring Rust types (User, Channel, Message, Guild, etc.)
- `tauri.ts` - Type-safe Tauri command wrappers with HTTP fallback for browser mode
- `utils.ts` - UI utility functions (timestamp formatting, message grouping, text truncation)

## Subdirectories

- `webrtc/` - WebRTC abstraction layer (browser/Tauri) - see webrtc/AGENTS.md

## For AI Agents

### Platform Abstraction

The `tauri.ts` file detects runtime environment and provides unified API:

- **REST (both modes)**: `httpRequest` in `tauri/common.ts`; in Tauri mode the base URL and access token come from the Rust session (`getAuthContext()`). Never add a Rust command that only proxies a REST call.
- **Tauri mode**: native Rust commands via `@tauri-apps/api/core` `invoke()` only for session/auth, WebSocket, voice/media, tray, local settings, crypto, presence scanning, sound and clipboard. Name the command with a string literal; `bun run lint:tauri-commands` fails on any name missing from `generate_handler!`.

### Type Safety

All types in `types.ts` match server-side Rust types for consistency:

- Use snake_case for API contracts (server convention)
- TypeScript interfaces mirror Rust structs exactly
- Includes WebSocket event types (ClientEvent/ServerEvent)

### Authentication Flow

Browser mode includes automatic token refresh (60s before expiry):

```typescript
// Token stored in localStorage
// Auto-refresh scheduled via setTimeout
// Falls back to login if refresh fails
```

### Common Patterns

```typescript
// Check if running in Tauri (contract C6) — always call it, never inline a check
import { isTauri } from "@/lib/tauri/common";
if (isTauri()) { /* native path */ }

// Call platform-agnostic API
import { login, getGuildChannels } from "@/lib/tauri";
const user = await login(serverUrl, username, password);
const channels = await getGuildChannels(guildId);
```

### File Uploads

Use `uploadMessageWithFile()` for combined message + attachment:

- Handles FormData creation
- Adds Authorization header
- Works in both browser and Tauri modes

### Utility Functions

- `formatTimestamp()` - Smart time display (today = time, older = date)
- `formatRelativeTime()` - "2 minutes ago" style
- `formatElapsedTime()` - MM:SS timer for voice connections
- `shouldGroupWithPrevious()` - Message grouping logic (same author, <5min apart)
- `getInitials()` - Avatar fallback text
- `truncate()` - Text overflow handling

### WebSocket Management

Browser mode manages WebSocket lifecycle:

- Exposed via `getBrowserWebSocket()` for event handling
- Auto-reconnect on connection loss
- Event routing to stores (messages, presence, voice)
