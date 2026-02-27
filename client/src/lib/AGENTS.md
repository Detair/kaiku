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

- **Tauri mode**: Invokes native Rust commands via `@tauri-apps/api/core`
- **Browser mode**: Falls back to HTTP REST API with token management

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
// Check if running in Tauri
const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

// Call platform-agnostic API
import { login, getChannels } from "@/lib/tauri";
const user = await login(serverUrl, username, password);
const channels = await getChannels();
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
