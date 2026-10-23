# Network Module

**Parent:** [Tauri Source](../AGENTS.md)

**Purpose:** HTTP client for REST API calls and WebSocket client for real-time server events. Provides the communication layer between Tauri client and server.

## Architecture

```
SessionManager (access token, refresh)
    ↓ auth_info() / refresh()
WebSocketManager loop (tokio-tungstenite, rustls)
    ↔ Server WebSocket (/ws, token in Sec-WebSocket-Protocol)
    ↓ every text frame, parsed JSON
Tauri event `ws:server_event` → webview `handleServerEvent()` (same as browser)
```

## Module Structure

| File | Purpose | Key Types |
|------|---------|-----------|
| `mod.rs` | Module root | Re-exports `WebSocketManager`, `ClientEvent`, `ConnectionStatus`, `WsTimings` |
| `websocket.rs` | Connection loop, watchdog, forwarding | `WebSocketManager`, `ClientEvent`, `Outbound`, `WsEvents`, `WsTimings` |

## `websocket.rs`

- **Outgoing:** `Outbound::Event(ClientEvent)` from Rust (voice signalling,
  typing, …) or `Outbound::Raw(String)` from the `ws_send` command (must be a
  JSON object with a string `type`). Sending fails while not connected.
- **Incoming:** every text frame is emitted once as `ws:server_event` with
  the parsed JSON. There is no typed server-event enum: unknown event types
  reach the webview unchanged.
- **Status events:** `ws:connecting`, `ws:connected`, `ws:reconnecting`
  (attempt, emitted when a connection is lost or an attempt fails) and
  `ws:disconnected` (the loop ended by itself because there is no session).
  `disconnect()` stops the loop silently.
- **Tokens:** read from `SessionManager::auth_info()` before every attempt;
  refreshed first when they expire within 60 s. HTTP 401/403 on the handshake
  or a server close with 1008 → `refresh()`, then one immediate retry. If the
  fresh token is rejected too, `ws:auth_rejected` is emitted once (hook for
  "show login / password change", C11) and the loop backs off without more
  refreshes until a connection ends normally.
  Losing a live connection always emits `ws:reconnecting` before any retry.
  `SessionError::Expired` → stop (the session manager emits
  `auth:session_expired`).
- **Backoff:** `next_backoff(attempt, jitter)` — 1 s doubling to 30 s, ±20 %,
  same curve as `nextBackoffMs` in `client/src/stores/websocket/connection.ts`.
  `ws_reconnect_now` wakes the backoff sleep. Close codes 1011/1013 are
  ordinary drops (backoff, then the webview catches up).
- **Watchdog:** `ClientEvent::Ping` every `ping_every` (20 s); no inbound frame
  for `silence_limit` (45 s) → drop and reconnect. Handshakes time out after 15 s.
- **TLS:** `rustls-tls-native-roots`; `install_crypto_provider()` picks ring
  once (both ring and aws-lc-rs are in the crate graph).
- **Tests:** `WsEvents` is the emitter; tests record events instead of using an
  `AppHandle` and run against a local `accept_hdr_async` server plus wiremock
  for `/auth/*`.

## HTTP Client

**Location:** `AppState.http` (reqwest client, initialized in `lib.rs`)

**Configuration:**
```rust
HttpClient::builder()
    .timeout(Duration::from_secs(30))
    .build()
```

**Usage in Commands:**
```rust
let response = state.http
    .get(format!("{server_url}/channels"))
    .header("Authorization", format!("Bearer {}", token))
    .send()
    .await?;
```

**Error Handling:**
- Network errors: `map_err(|e| format!("Connection failed: {}", e))`
- HTTP errors: Check `response.status().is_success()`
- Deserialize errors: `response.json::<T>().await.map_err(...)`

## Common Issues

### Connection Refused
- **Cause:** Server not running or wrong URL
- **Debug:** Check server logs, verify URL format (`wss://` not `ws://` in production)

### Authentication Failed
- **Cause:** Invalid/expired token
- **Debug:** Token should be fresh access token, not refresh token
- **Fix:** Re-login to get new token

### Events Not Received
- **Cause:** The webview must listen to `ws:server_event` and switch on `payload.type`;
  there are no per-type `ws:<name>` events any more.

### WebSocket Closes Immediately
- **Cause:** Server rejected connection (auth failure, rate limit)
- **Debug:** Check server logs for close code/reason
- **Fix:** Server should send close frame with reason

## Performance Considerations

### Batching
Not yet implemented. Future optimization:
- Batch multiple `ClientEvent`s into single WebSocket message
- Reduce overhead for high-frequency events (e.g., typing indicators)

### Backpressure
- Outgoing queue is bounded (100); a full queue rejects the send.

### Compression
Not yet implemented. Future:
- Enable permessage-deflate extension
- Reduce bandwidth for text-heavy events

## Security

### TLS
- **Production:** Always use `wss://` (WebSocket over TLS)
- **Development:** `ws://` acceptable for localhost only

### Authentication
- The access token travels in the `Sec-WebSocket-Protocol` header
  (`access_token.<jwt>, access_token`), never in the URL.

### Input Validation
- **Server-side:** All client events must be validated server-side
- **Client-side:** Validate server events to prevent injection (e.g., malicious JSON)

### Rate Limiting
- **Not yet implemented client-side**
- **Server-side:** Implemented (see `server/src/ratelimit/`)

## Future Improvements

1. **Event Replay:** Request missed events after reconnection
2. **Compression:** Enable permessage-deflate
3. **Binary Protocol:** Switch to MessagePack or Protobuf for efficiency
4. **WebSocket Subprotocols:** Structured versioning

## Related Documentation

- [Server WebSocket](../../../../server/src/ws/AGENTS.md) — Server-side WebSocket handling
- [Commands](../commands/AGENTS.md) — Tauri commands that use network layer
- [Frontend Events](../../src/lib/api/events.ts) — TypeScript event listeners
