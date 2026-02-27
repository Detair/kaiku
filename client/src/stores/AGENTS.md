<!-- Parent: ../AGENTS.md -->

# stores

## Purpose

Solid.js signal-based state management. Each store manages a domain-specific slice of application state with reactive updates.

## Key Files

- `auth.ts` - User authentication state and session management
- `websocket.ts` - WebSocket connection and event routing
- `messages.ts` - Message history per channel, E2EE encrypt/decrypt routing (Olm 1:1 + Megolm group)
- `channels.ts` - Channel list and selection
- `guilds.ts` - Guild/server list and active selection
- `voice.ts` - Voice connection state and participants
- `call.ts` - DM call state (ringing, active, ended)
- `presence.ts` - User online status tracking
- `friends.ts` - Friend list and requests
- `dms.ts` - Direct message channels
- `e2ee.ts` - E2EE state management: Olm (1:1) and Megolm (group) encrypt/decrypt operations
- `theme.ts` - Theme selection and color scheme

## For AI Agents

### Store Pattern

All stores use Solid.js `createStore` for reactive state:

```typescript
import { createStore } from "solid-js/store";

const [state, setState] = createStore({ ... });

// Export state for reading
export { state };

// Export actions for mutations
export async function doSomething() {
  setState({ ... });
}
```

### Initialization Order

Critical stores must initialize after auth:

1. `initAuth()` - Restore session, fetch current user
2. `initWebSocket()` - Set up event listeners
3. `wsConnect()` - Connect to WebSocket server
4. `initPresence()` - Start presence tracking

### WebSocket Event Routing

`websocket.ts` receives all server events and routes to appropriate stores:

- `message_new` → `messages.ts::addMessage()`
- `typing_start` → internal typing state
- `presence_update` → `presence.ts`
- `voice_*` → `voice.ts` state updates
- `incoming_call` → `call.ts::receiveIncomingCall()`

### Reactive Dependencies

Components import stores and use signals directly:

```typescript
import { authState, isAuthenticated } from "@/stores/auth";
import { selectedChannel } from "@/stores/channels";

const MyComponent = () => {
  return <Show when={isAuthenticated()}>
    <p>Channel: {selectedChannel()?.name}</p>
  </Show>;
};
```

### Message Store

Maintains per-channel message history:

- Keyed by channel_id
- Infinite scroll with `loadMore()`
- Optimistic updates (add message immediately, update on server response)
- Attachment support
- E2EE: automatic decryption of Olm (1:1) and Megolm (group) messages
- Megolm session key auto-processing for inbound key distribution
- Group DMs (3+ participants) route to Megolm, 1:1 DMs use Olm

### E2EE Store
Manages end-to-end encryption state:
- Initialization status, device/identity keys
- **Olm**: `encrypt()`, `decrypt()` for 1:1 DMs
- **Megolm**: `createGroupSession()`, `encryptGroup()`, `decryptGroup()`, `addInboundSession()` for group DMs
- Prekey management and backup operations

### Voice vs Call Stores

- `voice.ts` - Voice channel connections (guild voice chat)
- `call.ts` - DM peer-to-peer calls with ringing/answer flow

### Theme Store

Manages CSS custom properties via `data-theme` attribute:

- Available themes: focused-hybrid, solarized-dark, solarized-light
- Persisted to localStorage
- Updates root element attribute on change

### Presence Tracking

Periodic heartbeat to server (every 30s):

- Updates user's online status
- Receives presence updates via WebSocket
- Tracks last seen timestamps

### Typing Indicators

Auto-timeout after 5 seconds:

- Debounced sending (max once per 3s)
- Automatic cleanup via setTimeout
- Per-channel Set of typing user IDs

### Error Handling

Most stores set `error` field on failure:

- Auth errors shown in login form
- WebSocket errors logged to console
- Failed API calls stored in state for retry
