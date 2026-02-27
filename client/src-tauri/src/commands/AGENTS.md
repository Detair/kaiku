# Commands Module

**Parent:** [Tauri Source](../AGENTS.md)

**Purpose:** Tauri IPC command handlers that bridge the Solid.js frontend with Rust backend functionality. All user interactions (login, send message, join voice) flow through these commands.

## Architecture

```
Frontend (TypeScript)
    ↓ invoke('command_name', { args })
Tauri IPC (JSON serialization)
    ↓
Command Handler (Rust)
    ↓ uses AppState
Backend Services (HTTP, WebSocket, Audio, WebRTC)
    ↓ returns Result<T, String>
Frontend receives Promise<T>
```

## Module Structure

| File | Purpose | Key Commands |
|------|---------|--------------|
| `auth.rs` | Authentication, registration, logout | `login`, `register`, `logout`, `get_current_user` |
| `chat.rs` | Text channels and messages | `get_channels`, `get_messages`, `send_message` |
| `crypto.rs` | E2EE encryption operations (Olm + Megolm) | `init_e2ee`, `encrypt_message`, `decrypt_message`, `create_megolm_session`, `encrypt_group_message`, `decrypt_group_message` |
| `voice.rs` | Voice channel join/leave, mute/deafen | `join_voice`, `leave_voice`, `set_mute`, `handle_voice_offer` |
| `settings.rs` | User preferences (audio, theme, etc.) | `get_settings`, `update_settings` |
| `websocket.rs` | WebSocket lifecycle and subscriptions | `ws_connect`, `ws_disconnect`, `ws_subscribe` |
| `mod.rs` | Module root (exports all command modules) | — |

## Key Patterns

### Command Signature
All commands follow this pattern:

```rust
#[command]
pub async fn command_name(
    state: State<'_, AppState>,        // Injected by Tauri
    app: AppHandle,                     // Optional: for emitting events
    param1: String,                     // User input (JSON deserialized)
    param2: SomeStruct,                 // Serde-deserializable types
) -> Result<ReturnType, String> {      // Result for error handling
    // Implementation
}
```

**Rules:**
- Always `pub async fn`
- Return `Result<T, String>` (String is frontend error message)
- Use `#[command]` macro
- Register in `lib.rs` `invoke_handler!` macro

### Error Handling
- **Frontend-friendly**: `Err("User already exists")` not `Err(DatabaseError::Conflict)`
- **Log details**: `error!("Failed to insert user {}: {}", username, e)`
- **Security**: Never leak internal paths or stack traces

### State Access
All state is shared `Arc<RwLock<T>>`:

```rust
// Read access
let auth = state.auth.read().await;
let token = auth.access_token.clone();

// Write access
let mut auth = state.auth.write().await;
auth.user = Some(user);
```

**Gotcha:** Avoid holding locks across `.await` points (can deadlock).

### Event Emission
Commands can emit events to frontend:

```rust
app.emit("voice:state_change", "Connected")?;
```

Frontend listens:
```typescript
import { listen } from '@tauri-apps/api/event';
listen('voice:state_change', (event) => console.log(event.payload));
```

## Module Details

### `auth.rs`
**Purpose:** User authentication and session management.

**Key Types:**
- `LoginRequest` / `RegisterRequest`: Frontend input
- `TokenResponse`: Server `/auth/login` response
- `UserResponse`: Server `/auth/me` response

**Flow (Login):**
1. `POST {server_url}/auth/login` with credentials
2. Receive `access_token` + `refresh_token`
3. `GET {server_url}/auth/me` to fetch user profile
4. Store tokens in `AppState.auth`
5. Store refresh token in OS keyring (keyring-rs)

**Security:**
- Access token: In-memory only (15min expiry)
- Refresh token: OS keyring (persistent across restarts)
- Never log tokens
- Clear keyring on logout

### `chat.rs`
**Purpose:** Text channel operations.

**Key Commands:**
- `get_channels()`: Fetch list of accessible channels
- `get_messages(channel_id, limit, before)`: Paginated message history
- `send_message(channel_id, content)`: Send text message

**Pattern:** All API calls use `state.http` (reqwest) with `Authorization: Bearer {token}` header.

### `voice.rs`
**Purpose:** Voice channel lifecycle and WebRTC signaling.

**Key Commands:**
- `join_voice(channel_id)`: Initialize WebRTC + Audio, send `VoiceJoin` event
- `leave_voice()`: Stop audio, disconnect WebRTC, send `VoiceLeave` event
- `handle_voice_offer(sdp)`: Process server SDP offer, return SDP answer
- `handle_voice_ice_candidate(candidate)`: Add ICE candidate from server
- `set_mute(muted)` / `set_deafen(deafened)`: Audio state
- `get_audio_devices()`: Enumerate microphones/speakers
- `start_mic_test()` / `get_mic_level()`: Microphone test UI

**Flow (Join Voice):**
1. Initialize `VoiceState` (WebRTC + Audio)
2. Set up callbacks: ICE candidates → WebSocket, state changes → frontend events
3. Send `VoiceJoin` event to server via WebSocket
4. Server responds with `VoiceOffer` (SDP)
5. `handle_voice_offer()` creates SDP answer, sends via `VoiceAnswer` event
6. Server sends ICE candidates via `VoiceIceCandidate` events
7. WebRTC connects, audio starts flowing

**Critical:** Audio and WebRTC initialized lazily on first `join_voice()` call.

### `settings.rs`
**Purpose:** User preferences persistence.

**Not Yet Implemented:** Currently stubs. Future: Store in local DB or JSON file.

**Planned Settings:**
- Audio device selection
- Input/output volume
- PTT keybind
- Theme
- Notification preferences

### `websocket.rs`
**Purpose:** WebSocket lifecycle and event subscriptions.

**Key Commands:**
- `ws_connect(server_url, token)`: Establish WebSocket connection
- `ws_disconnect()`: Close connection
- `ws_status()`: Get connection state
- `ws_subscribe(channel_id)` / `ws_unsubscribe(channel_id)`: Message events
- `ws_typing(channel_id)` / `ws_stop_typing(channel_id)`: Typing indicators
- `ws_ping()`: Keepalive

**Pattern:** Commands interact with `AppState.websocket` (WebSocketManager). Server events are forwarded to frontend via `app.emit()`.

## Testing

### Unit Testing
Not yet implemented for commands (require mock AppState).

**Future Pattern:**
```rust
#[cfg(test)]
mod tests {
    async fn mock_app_state() -> AppState { /* ... */ }

    #[tokio::test]
    async fn test_login_success() {
        let state = mock_app_state().await;
        // ...
    }
}
```

### Integration Testing
Test from frontend (TypeScript):
```typescript
import { invoke } from '@tauri-apps/api/core';

test('login flow', async () => {
    const user = await invoke('login', {
        request: { server_url: 'https://example.com', username: 'test', password: 'pass' }
    });
    expect(user.username).toBe('test');
});
```

## Common Issues

### "Failed to invoke command" Error
- **Cause:** Command not registered in `lib.rs` `invoke_handler!` macro
- **Fix:** Add command to the macro list

### Serialization Errors
- **Cause:** Type mismatch between frontend and Rust (e.g., `number` vs `string`)
- **Fix:** Use `serde` `#[serde(rename)]` / `#[serde(default)]` attributes
- **Debug:** Check browser console for JSON payload

### Deadlocks
- **Cause:** Holding `RwLock` across `.await` point
- **Bad:**
  ```rust
  let auth = state.auth.read().await;
  do_async_thing().await; // Still holding lock!
  ```
- **Good:**
  ```rust
  let token = {
      let auth = state.auth.read().await;
      auth.access_token.clone()
  }; // Lock dropped
  do_async_thing().await;
  ```

## Future Improvements

1. **Command Middleware**: Rate limiting, logging, auth checks
2. **Mock Testing**: AppState builder for unit tests
3. **Command Documentation**: Auto-generate TypeScript types from Rust signatures
4. **Streaming Responses**: For large data (e.g., message history)

## Related Documentation

- [Frontend Commands](../../src/lib/api/commands.ts) — TypeScript wrappers
- [AppState](../lib.rs) — Shared application state
- [WebSocket Events](../network/websocket.rs) — Real-time event types
