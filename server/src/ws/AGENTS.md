<!-- Parent: ../../AGENTS.md -->

# WebSocket Module

**PERFORMANCE CRITICAL** — Real-time bidirectional communication for chat messages, presence, typing indicators, and voice signaling.

## Purpose

- WebSocket upgrade handler with JWT authentication
- Client-to-server and server-to-client event routing
- Redis pub/sub integration for multi-server broadcasting
- Channel subscription management (users subscribe to channels they're viewing)
- Typing indicators and presence updates
- Voice signaling event delegation (to voice module)

## Key Files

- `mod.rs` — module docs and re-exports
- `handlers.rs` — upgrade handler (header and post-connect auth), socket lifecycle (`handle_socket`), `handle_client_message` dispatch, Redis pub/sub forwarder (`handle_pubsub`, `forward_pubsub_messages`), bounded sends (`send_outbound`)
- `events.rs` — `ClientEvent` / `ServerEvent` / `OutboundMsg`, broadcast helpers, Redis channel names
- `bot_gateway.rs`, `bot_events.rs` — separate WebSocket gateway for bots

## For AI Agents

**PERFORMANCE CRITICAL MODULE**: WebSocket is the backbone of real-time features. Message latency should be <100ms. Every connected user holds an open connection, so memory efficiency is critical. Avoid allocations in hot paths, use structured concurrency for pub/sub tasks.

### WebSocket Lifecycle

**Connection Flow**:
1. Client connects to `GET /ws`, authenticating either with `Sec-WebSocket-Protocol: access_token.<jwt>` (validated *before* the upgrade) or by sending `{"type":"authenticate","token":…}` as the first frame (within 5 s after the upgrade)
2. Auth refusals (see **Auth refusal signals** below): header auth answers with a plain HTTP status before the upgrade; frame auth answers on the socket
3. Upgrade to WebSocket protocol
4. Server updates user presence to `online` and spawns the message sender task (drains the outbound mpsc, writes to the WebSocket)
5. Load friends, guild IDs and block sets
6. Spawn the Redis pub/sub task (`handle_pubsub`); it fires a `ready` oneshot once `psubscribe("channel:*")` and all per-user `subscribe` calls have returned **and a `PING` on the same subscriber connection has been answered** (fred 10 resolves `subscribe` when the command is queued, not when Valkey acks it; the PING reply is the barrier that proves Valkey executed every subscription) (every subscription is required: a failed `psubscribe` or any `subscribe` — user, admin, friend presence, guild — ends the pipeline with `SubscribeFailed`, so no `Ready` is sent and the socket closes with 1011)
7. Wait for that `ready` signal (max 5 s; on timeout or early failure the socket is closed with **1011**)
8. Queue `Ready { user_id, server_time }` (server_time = RFC 3339 UTC, taken when Ready is built; clients use it as the catch-up watermark) — so `Ready` always means "subscriptions are live" — then send the `start` oneshot; only then does the pub/sub task begin forwarding (events published in between wait in its broadcast receiver), so `Ready` is always the first frame. Then the friends' presence snapshot
   - `Ready` guarantees that the socket's **Valkey** subscriptions are live. It does *not* mean the socket receives `channel:{id}` events yet: those are filtered by the socket's `subscribed_channels`, which is empty at `Ready`. A client only gets a channel's messages after its own `subscribe` for that channel was acknowledged with `subscribed`.
9. Main loop (`select!`): client messages, server heartbeat, and the pub/sub task's exit
10. On disconnect / any exit: abort background tasks, `QUIT` the socket's own subscriber client (bounded by 1 s; fred keeps the connection and its subscriptions alive if the client is merely dropped), set presence to `offline`

**Pub/sub pipeline failure** (`PubsubExit`): the pub/sub task returns why it stopped. Every reason except `ClientGone` closes the socket with a Close frame (reason `"RESYNC"`), giving the sender task at most 1 s to flush it:
- `ConnectFailed`, `SubscribeFailed`, `StreamEnded`, `Lagged` → **1011** (server-side event pipeline failed). A subscriber reconnect counts as `StreamEnded`, because a plain fred client does not restore its subscriptions.
- `SlowClient` → **1013** (client too slow: the outbound queue stayed full for `SLOW_CLIENT_SEND_TIMEOUT` = 1 s; no silent drops)
- `ClientGone` → no Close frame (the socket is already going away)

Clients treat 1011/1013 as "reconnect with backoff, then catch up" via the REST history endpoints. 1008 stays reserved for auth rejection.

**Gap-free reconnect / catch-up order** (per channel the client shows):
1. Reconnect and wait for `ready`.
2. Send `subscribe` for the channel and wait for its `subscribed` ack. From then on, live `message_new` events for that channel are delivered.
3. `GET /api/messages/channel/{id}?after=<last known message id>` (page until `next_cursor` is absent).
4. Merge live events and the fetched page, **deduplicating by message id** (a message can arrive both ways).

Fetching *before* `subscribed` leaves a gap: messages posted between the fetch and the subscription are neither in the page nor delivered live.

**DMs after reconnect**: `DmCreated` auto-subscribes only sockets that are open when the DM is created, and that auto-subscribe sends no `subscribed` ack. After any reconnect the client must `GET /api/dm` and `subscribe` to its DM channels itself (then catch up as above).
Clients must treat an abnormal close (1006) after a healthy session the same as 1011/1013 (reconnect + catch-up): a 1013 Close frame may never reach a stalled client.

**Auth refusal signals** (client contract): both mean "auth problem — refresh the token, or show login / the password-change screen; never retry blindly":
- Header auth (`Sec-WebSocket-Protocol`): the handshake is refused with an HTTP status before any upgrade — `401` (invalid/revoked token, unknown user) or `403` with body `PASSWORD_CHANGE_REQUIRED` (forced password change pending). Clients see a failed handshake, not a close frame.
- Frame auth (`authenticate` frame): a pending password change gets an `error` event with code `PASSWORD_CHANGE_REQUIRED`, then close **1008**; other auth failures close the socket.

**Authentication** (illustrative; the real code lives in `handlers.rs`):
```rust
// Query param validation (before upgrade)
let claims = jwt::validate_access_token(&query.token, &state.config.jwt_secret)?;
let user_id = Uuid::parse_str(&claims.sub)?;

// Upgrade to WebSocket with user_id
ws.on_upgrade(move |socket| handle_socket(socket, state, user_id))
```

**Why no query-param auth**: tokens in URLs leak into logs and history. Browsers cannot set custom headers on a WebSocket upgrade, so they use `Sec-WebSocket-Protocol` or the post-connect `authenticate` frame.

### Event Types

**Client → Server** (`ClientEvent` enum):
```rust
Ping                             // Keepalive
Subscribe { channel_id }         // Start receiving channel events
Unsubscribe { channel_id }       // Stop receiving channel events
Typing { channel_id }            // Send typing indicator
StopTyping { channel_id }        // Stop typing indicator
VoiceJoin { channel_id }         // Join voice channel (delegated to voice module)
VoiceLeave { channel_id }
VoiceAnswer { channel_id, sdp }
VoiceIceCandidate { channel_id, candidate }
VoiceMute { channel_id }
VoiceUnmute { channel_id }
```

**Server → Client** (`ServerEvent` enum):
```rust
Ready { user_id, server_time }               // Connection authenticated
Pong                                         // Keepalive response
Subscribed { channel_id }                    // Subscription confirmed
Unsubscribed { channel_id }                  // Unsubscription confirmed
MessageNew { channel_id, message }           // New message in channel
MessageEdit { channel_id, message_id, content, edited_at }
MessageDelete { channel_id, message_id }
TypingStart { channel_id, user_id }
TypingStop { channel_id, user_id }
PresenceUpdate { user_id, status }
Error { code, message }
// Voice events (see voice/AGENTS.md)
VoiceOffer { channel_id, sdp }
VoiceIceCandidate { channel_id, candidate }
VoiceUserJoined { channel_id, user_id, username, display_name }
VoiceUserLeft { channel_id, user_id }
VoiceUserMuted { channel_id, user_id }
VoiceUserUnmuted { channel_id, user_id }
VoiceRoomState { channel_id, participants }
VoiceError { code, message }
// DM call events
IncomingCall { channel_id, initiator, initiator_name }
CallStarted { channel_id }
CallEnded { channel_id, reason, duration_secs }
CallParticipantJoined { channel_id, user_id, username }
CallParticipantLeft { channel_id, user_id }
CallDeclined { channel_id, user_id }
// DM lifecycle (user channel)
DmCreated { channel }                        // New DM; `channel` = one GET /api/dm element for the receiver
```

### Channel Subscription

**Purpose**: Clients only receive events for channels they're actively viewing (avoids flooding client with all guild messages).

**Subscription State**:
```rust
let subscribed_channels: Arc<RwLock<HashSet<Uuid>>> = Arc::new(RwLock::new(HashSet::new()));
```

**Subscribe Flow**:
1. Client sends `Subscribe { channel_id }`
2. Server validates channel exists (DB query)
3. Add `channel_id` to `subscribed_channels` set
4. Server responds `Subscribed { channel_id }`
5. Redis pub/sub task filters events by subscribed channels

**Auto-subscribe on `DmCreated`**: `POST /api/dm` publishes `DmCreated` on `user:{id}` of every participant (creator included) when the DM is newly created (an existing 1:1 DM sends nothing; group DMs are always new). In the `user:{id}` branch of `forward_pubsub_messages`, the socket inserts `channel["id"]` into `subscribed_channels` *before* forwarding the event. Redis keeps publish order on the subscriber connection, so the DM's first `channel:{id}` events (e.g. `MessageNew`) reach the socket without a client `Subscribe`.

**Unsubscribe**:
1. Client sends `Unsubscribe { channel_id }` (when navigating away)
2. Remove from `subscribed_channels`
3. No more events forwarded for that channel

**Future Optimization**: Lazy Redis subscription (subscribe to Redis channel only when first client subscribes, unsubscribe when last client leaves).

### Redis Pub/Sub Integration

**Redis Channels** (pub/sub topics, not to be confused with chat channels):
```rust
// Chat channel events
format!("channel:{channel_id}")  // e.g., "channel:123e4567-e89b-12d3-a456-426614174000"

// User presence (future)
format!("presence:{user_id}")

// Global events (future)
"global"
```

**Publisher** (in message/event handlers):
```rust
pub async fn broadcast_to_channel(
    redis: &RedisClient,
    channel_id: Uuid,
    event: &ServerEvent,
) -> Result<(), RedisError> {
    let payload = serde_json::to_string(event)?;
    redis.publish(channels::channel_events(channel_id), payload).await?;
    Ok(())
}
```

**Subscriber** (in `handle_pubsub` task):
```rust
// Pattern subscribe to all channel events
subscriber.psubscribe("channel:*").await?;

// Receive messages (`forward_pubsub_messages`); Lagged/Closed end the pipeline
loop {
    let message = match pubsub_stream.recv().await { /* Lagged → Lagged, Closed → StreamEnded */ };
    let channel_id = parse_channel_id_from_redis_channel(message.channel)?;

    // Only forward if client is subscribed to this channel
    if subscribed_channels.read().await.contains(&channel_id) {
        let event: ServerEvent = serde_json::from_str(&message.value)?;
        // Bounded send: timeout → SlowClient (1013), closed → ClientGone
        if let Err(e) = send_outbound(&tx, OutboundMsg::Event(event)).await {
            return e.into();
        }
    }
}
```

**Multi-Server Scaling**: All servers subscribe to same Redis channels. When one server publishes an event (e.g., new message created on Server A), all servers (A, B, C) receive it and forward to their connected clients. This enables horizontal scaling.

### Message Routing

**Sender Task** (drains mpsc channel, sends to WebSocket):
```rust
let (tx, mut rx) = mpsc::channel::<ServerEvent>(100);  // Buffer 100 events

tokio::spawn(async move {
    while let Some(event) = rx.recv().await {
        let json = serde_json::to_string(&event)?;
        ws_sender.send(Message::Text(json)).await?;
    }
});
```

**Receiver Loop** (main task, handles client messages):
```rust
while let Some(msg) = ws_receiver.next().await {
    match msg {
        Ok(Message::Text(text)) => {
            let event: ClientEvent = serde_json::from_str(&text)?;
            handle_client_message(event, user_id, &state, &tx, &subscribed_channels).await?;
        }
        Ok(Message::Ping(_)) => { /* Axum handles pong automatically */ }
        Ok(Message::Close(_)) => break,
        Err(e) => {
            warn!("WebSocket error: {}", e);
            break;
        }
        _ => {}
    }
}
```

**Backpressure**: `mpsc::channel(256)` outbound buffer. The pub/sub forwarder uses `send()` bounded by `SLOW_CLIENT_SEND_TIMEOUT` (1 s), so short bursts (presence snapshot, bulk replies) just wait. If the queue stays full for the whole timeout, forwarding ends with `PubsubExit::SlowClient` and the socket is closed with 1013 so the client reconnects and catches up (events are never dropped silently). All other outbound sends of a socket (Ready, presence snapshot, client-message replies, error replies, heartbeat Ping, voice replies) go through the same bounded helper `send_outbound`; a timeout ends the socket like `SlowClient` (close 1013), a closed queue like `ClientGone`, so a stalled client can never block the socket loop and its exit handling. Each socket's subscriber client is created with a 1024-slot fred broadcast buffer (`SUBSCRIBER_BROADCAST_CAPACITY`, default 32) so ordinary server-wide traffic during such a wait does not overflow into `Lagged`/1011.

### Typing Indicators

**Flow**:
1. User starts typing in channel (client sends keypress)
2. Client sends `Typing { channel_id }` (throttled client-side, max 1/sec)
3. Server broadcasts `TypingStart { channel_id, user_id }` to all subscribed users
4. Client displays "User is typing..." indicator
5. After 5 seconds of no typing, client sends `StopTyping { channel_id }`
6. Server broadcasts `TypingStop { channel_id, user_id }`

**Server-Side Timeout** (future): If client doesn't send `StopTyping`, server auto-sends after 10 seconds (prevent stuck indicators).

**Optimization**: Don't persist typing events to DB (ephemeral state only).

### Presence System

**Status Values** (in `db::models::UserStatus`):
- `online` — User connected to WebSocket
- `away` — User idle (no activity for 10 minutes, future client-side detection)
- `busy` — User manually set (future: API endpoint)
- `offline` — User disconnected

**Status Updates**:
```rust
// On WebSocket connect
update_presence(&state, user_id, "online").await?;

// On disconnect
update_presence(&state, user_id, "offline").await?;
```

**Broadcasting** (future):
- When user status changes, publish to `presence:{user_id}` Redis channel
- Friends subscribe to each other's presence updates
- Reduces DB queries for presence (cache in Redis with 5min TTL)

### Voice Event Delegation

**Pattern**: WebSocket module routes voice events to voice module (separation of concerns).

```rust
ClientEvent::VoiceJoin { .. }
| ClientEvent::VoiceLeave { .. }
| ClientEvent::VoiceAnswer { .. }
| ClientEvent::VoiceIceCandidate { .. }
| ClientEvent::VoiceMute { .. }
| ClientEvent::VoiceUnmute { .. } => {
    crate::voice::ws_handler::handle_voice_event(
        &state.sfu, &state.db, user_id, event, &tx
    ).await?;
}
```

**Voice Handler** (in `voice::ws_handler`):
- Validates voice channel permissions
- Calls SFU methods (`join_room`, `handle_answer`, etc.)
- Sends `VoiceOffer`, `VoiceIceCandidate` events back via `tx` channel, always through `send_outbound` (a failure surfaces as `VoiceError::Outbound` and ends the socket)
- Fan-out to *other* peers (`Room::broadcast_except` / `broadcast_all`, `SfuServer::renegotiate`) uses `send_outbound_within(.., PEER_SIGNAL_SEND_TIMEOUT)` (250 ms, concurrent per peer): a stalled peer is skipped and logged, never blocking the sender's socket loop; the stalled peer's own loop tears it down

### Error Handling

**Client Errors** (malformed JSON, invalid channel ID):
```rust
send_outbound(tx, OutboundMsg::Event(ServerEvent::Error {
    code: "INVALID_REQUEST".to_string(),
    message: "Channel not found".to_string(),
}))
.await?; // never a raw `tx.send(..).await`: a stalled client would block the socket loop
```

**Server Errors** (DB failure, Redis down):
- Log error server-side (`error!` or `warn!` macros)
- Send generic error to client (don't leak internal details)
- Consider: Automatic reconnect with exponential backoff (client-side)

**Connection Drops**:
- Network interruption: Client reconnects, re-subscribes to channels
- Server restart: All clients disconnect, reconnect to new server instance
- Graceful shutdown: Send `Close` frame with reason code (future)

### Rate Limiting

**WebSocket Connection Limit**: 1 connection per 60 seconds per user (prevents reconnection spam).

**Applied in API router**:
```rust
Router::new()
    .route("/ws", get(ws::handler))
    .layer(from_fn_with_state(state.clone(), rate_limit_by_user))
    .layer(from_fn(with_category(RateLimitCategory::WebSocket)))
```

**Future**: Per-event rate limits (e.g., max 10 `Typing` events per minute).

### Performance Optimization

**Memory per Connection** (estimate; `size_of` measured: `OutboundMsg` 128 B, fred `Message` 136 B):
- Own Valkey subscriber client (`new_subscriber_client`): fred preallocates **every** notification broadcast channel (pub/sub, keyspace, errors, reconnect, connect, …) with `SUBSCRIBER_BROADCAST_CAPACITY` = 1024 slots. The pub/sub buffer alone is ~1024 × ~160 B ≈ 160 KB; all of them together are on the order of **0.5 MB per socket**. There is also one Valkey TCP connection per socket, and each one receives *all* server-wide `channel:*` traffic.
- Outbound `mpsc::channel(256)`: allocated in 32-slot blocks, ~4 KB when idle, up to ~32 KB (plus event payloads) when full.
- WebSocket, `subscribed_channels`, block sets, task stacks: a few KB.
- **Total**: roughly **0.5–0.6 MB per connection**, dominated by the fred subscriber. This is fine for family or small-community scale (hundreds of sockets). Before scaling to many thousands, revisit it: lower the capacity, or share one subscriber per server instance and fan out in-process.

**Latency Targets**:
- Client → Server: <10ms (local processing)
- Server → Redis publish: <5ms
- Redis → Server subscribe: <5ms
- Server → Client: <10ms (local send)
- **Total**: <30ms (excluding network RTT)

**Profiling**:
```rust
// Use tracing for latency measurements
#[tracing::instrument(skip(state, tx, subscribed_channels))]
async fn handle_client_message(...) {
    // Automatically logs timing for this function
}
```

**Metrics to Track**:
- Active WebSocket connections (gauge)
- Messages sent/received per second (counter)
- Average message latency (histogram)
- Redis pub/sub lag (time between publish and receive)

### Testing

**Required Tests**:
- [ ] Connect with valid JWT (receive `Ready` event)
- [ ] Connect with invalid JWT (403 Forbidden)
- [ ] Subscribe to channel, receive `Subscribed` event
- [ ] Publish message to channel (via HTTP), receive `MessageNew` via WebSocket
- [ ] Unsubscribe, verify no more events received
- [ ] Typing indicator broadcast to all channel subscribers
- [ ] Disconnect, verify presence set to `offline`
- [ ] Multiple clients in same channel receive same events

**Load Testing**:
- Simulate 1,000 concurrent connections (measure memory/CPU)
- Send 10,000 messages/sec through Redis pub/sub (measure latency)
- Verify no message loss under load

### Common Pitfalls

**DO NOT**:
- Block async executor (use `spawn` for CPU-heavy tasks)
- Send unbounded events to client (apply backpressure or disconnect)
- Forget to clean up subscriptions on disconnect (memory leak)
- Trust client events without validation (always check channel membership)
- Use `unwrap()` in WebSocket handlers (gracefully handle errors)

**DO**:
- Use structured concurrency (`tokio::spawn` with abort on disconnect)
- Validate all client events server-side (channel exists, user has access)
- Log WebSocket lifecycle events (connect, disconnect, errors)
- Use `tracing` for debugging (structured logging with context)
- Test with flaky networks (simulate packet loss, reconnects)

### Future Enhancements

**Compression**: WebSocket permessage-deflate extension (reduce bandwidth for large messages).

**Binary Protocol**: Use MessagePack or Protocol Buffers instead of JSON (faster serialization, smaller payloads).

**Event Batching**: Send multiple events in single WebSocket frame (reduce overhead).

**Priority Queues**: High-priority events (voice signaling) bypass normal queue.

**Health Checks**: Periodic `Ping`/`Pong` keepalive (detect dead connections, timeout after 60s).

**Reconnection Token**: Server generates short-lived token on disconnect, client can reconnect without full JWT re-auth.

**Partial Subscriptions**: Subscribe to specific event types in channel (e.g., only `MessageNew`, skip typing indicators).

**Federation** (future): Cross-server WebSocket routing for distributed deployments.
