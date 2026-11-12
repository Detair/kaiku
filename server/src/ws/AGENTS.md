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
4. Server spawns the message sender task (drains the outbound mpsc, writes to the WebSocket)
5. Load presence peers (friends ∪ members of the user's servers, friends first, capped at 500, one query), guild IDs and block sets
6. Spawn the Redis pub/sub task (`handle_pubsub`); it fires a `ready` oneshot once `psubscribe("channel:*")` and all per-user `subscribe` calls have returned **and a `PING` on the same subscriber connection has been answered** (fred 10 resolves `subscribe` when the command is queued, not when Valkey acks it; the PING reply is the barrier that proves Valkey executed every subscription) (every subscription is required: a failed `psubscribe` or any `subscribe` — user, admin, friend presence, guild — ends the pipeline with `SubscribeFailed`, so no `Ready` is sent and the socket closes with 1011)
7. Wait for that `ready` signal (max 5 s; on timeout or early failure the socket is closed with **1011**)
8. Queue `Ready { user_id, server_time }` (server_time = RFC 3339 UTC, taken when Ready is built; clients use it as the catch-up watermark) — so `Ready` always means "subscriptions are live" — then send the `start` oneshot; only then does the pub/sub task begin forwarding (events published in between wait in its broadcast receiver), so `Ready` is always the first frame. Then the friends' presence snapshot
   - After `Ready` and the snapshot, the socket registers as one of the user's devices (see **Presence System**)
   - `Ready` guarantees that the socket's **Valkey** subscriptions are live. It does *not* mean the socket receives `channel:{id}` events yet: those are filtered by the socket's `subscribed_channels`, which is empty at `Ready`. A client only gets a channel's messages after its own `subscribe` for that channel was acknowledged with `subscribed`.
9. Main loop (`select!`): client messages, server heartbeat, and the pub/sub task's exit
10. On disconnect / any exit: abort background tasks, `QUIT` the socket's own subscriber client (bounded by 1 s; fred keeps the connection and its subscriptions alive if the client is merely dropped), remove the socket from the user's devices (the last one announces `offline`)

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

**Access revalidation** (`ws::access`, single node; the module docs are the reference): each socket keeps the channels the client **requested** (`subscribe` — also a refused one — and `dm_created`; at most `MAX_REQUESTED_CHANNELS` = 2 000 server channels and `MAX_REQUESTED_DIRECT` = 2 000 DMs, counted apart; a `subscribe` beyond either answers `TOO_MANY_SUBSCRIPTIONS`, a `dm_created` that no longer fits closes the socket with 1011 `RESYNC` instead of announcing a DM whose messages it would filter) and the subset it may currently view (**allowed** = requested ∩ existing ∩ permitted). A revalidation recomputes allowed from requested in both directions, so access that comes back (re-invite, role granted again, override removed) restores the feed without a client action. A deleted channel also leaves requested. No protocol change: no event tells the client about a dropped channel.
- **Epochs**: a process-wide registry holds one epoch cell per server and per user; values come from one process-wide sequence, cells are dropped when the server or account is deleted. Every write that can reduce or restore access goes through `access::bump_after` / `bump_after_if(change, write, applies)`: the epoch is bumped **in the poll in which the write's future completes** (no `.await` between commit and bump), and also when that future is dropped mid-commit or the commit fails (outcome unknown; a spurious bump costs one revalidation). Only they return the `Bumped` proof (not constructible outside the module) that `access_changed` (voice enforcement + cross-node notice) requires; handlers call `access_changed` right after, **before** other post-commit work (presence revocation, caches). Server epoch: role permission edit or delete, override set/delete, channel delete, server delete, screening disabled. User epoch: join via invite/discovery, screening accepted, kick, leave, ban, role add/remove incl. reaction roles (also for an installed bot, and the role cleanup of a bot uninstall), group-DM leave, admin de-elevation, account deletion (admin and scheduled). The CLI (`kaiku-admin admin-demote`) runs in another process and cannot bump: its effect (admin events only) is bounded by the socket backstop (≤ 70 s).
- **Every frame is decided when it is written** (the sender task), by its `OutboundMsg` variant:
  - `Channel` (everything from `channel:*`) → `SocketAccess::admit`: user scope and the channel's server scope current, channel allowed.
  - `Guild` (everything from `guild:{id}`: member, role, event, emoji updates) → `admit_guild`: the user is a member, read at the current user epoch and that server's epoch.
  - `Voice` (every event of a voice room, built with `OutboundMsg::voice`: room state, joins, departures, mute, media, SDP/ICE, layers) → `admit_voice`: this socket owns the user's peer in that room (in memory) **and** its authorization for the room is current — the epochs of the join check (`Peer::admission`) still hold, or it is revalidated now. A peer that is still in the room after its user was kicked gets nothing.
  - `Admin` → `admit_admin`: the admin stream is a scope like the others (present while the client is subscribed; `admin_subscribe` records its decision only if the user epoch is unchanged, like `subscribe`). The elevation is read from the committed rows inside the normal revalidation, after the epochs were read. A failed read makes the scope *failing* (drops recorded, `RESYNC` on recovery); only a read that answers "not elevated" ends the subscription.
  - `Event` is written as is: replies to the socket's own requests (`ready`, `pong`, `subscribed`, `unsubscribed`, `error`, `voice_error`), `voice_removed` (the socket's own removal: channel id and reason), events on the user's own channel (read state, preferences, friends, workspaces, security notices, `dm_created`, DM call control), and presence (authorized by the presence audience, see below).
- **One decision loop** (`SocketAccess::decide`) for every fenced frame; it leaves as *admitted* (scope current and allows), *denied* (scope current and does not, or never asked for) or *undecided* (not current and not revalidatable now: failure, backoff, or epochs moved under 4 revalidations) — the only place a frame is dropped without a decision, always recorded for an outbound frame. The answer is looked at after every revalidation, the last included.
- **Scope states** (four kinds of scope: user, each server, the voice room, the admin stream): *current* (computed at the epoch that still holds) → frames decided from memory; *stale* (epoch moved, or invalidated by the backstop or an undecided subscribe) → the frame waits for a revalidation; *failing* (a revalidation attempt failed or could not be made, none succeeded since) → frames are dropped **and the drop is recorded on the scope**. The forwarder (`may_forward`) drops only on a decision (not requested, or current and not allowed); everything else is decided — and, if dropped, recorded — in one place, the writer.
- **`subscribe`** reads the epochs, checks `require_channel_access`, and records the result only if the epochs are unchanged (else checks again, up to 4 times). A database error (or 4 moves) records the channel undecided (`SERVICE_UNAVAILABLE` error to the client) and the socket revalidates it.
- **Revalidation** (`SocketAccess::refresh`) covers every scope that is not current in one pass: ban state (a banned account is refused everything), one `viewable_and_existing_channels` call for the stale servers' channels (≤ 5 SQL statements incl. the existence check), the user's memberships, after a user bump one DM query, and the voice check when the voice scope is stale. At most `REVALIDATION_CONCURRENCY` (4 of the pool's 20 connections) run at once process-wide; others queue (not a failure). Every bump wakes all sockets (`watch`); a socket with a stale scope revalidates after 150 ms + per-socket jitter of up to 100 ms. Backstop: every `session_recheck_secs` (≤ 60 s), first tick jittered by up to ±10 s; it invalidates every scope, so frames wait for that read. A voice peer whose check fails is removed by its socket (`voice_removed { permission_lost }`) unless the request that made the change was faster (its own reason).
- **Failure**: a failed revalidation (database error, 5 s timeout) makes the scopes it covered *failing* and retries with backoff (250 ms doubling to 5 s); after 30 s of failures the socket closes with 1011 (`AccessCheckFailed`). When a scope recovers and a frame was dropped while it was failing, the socket closes with **1011 `RESYNC`** (at a random moment within 3 s), so the client reconnects and catches up (C11). Refusing what the client *sends* (typing) while failing records nothing.
- Test hooks (each a single relaxed atomic load until a test registers one): `disable_access_wake`, `pause_before_channel_write` (channel and voice writes), `pause_before_feed_write` (server feed and admin writes), `pause_subscribe_after_check`, `pause_voice_join_after_check`, `pause_after_access_bump`, `pause_voice_leave`, `count_access_reads`/`access_reads`, `fail_access_reads`.

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

**Statuses**: `online`, `away`, `busy`, `offline` (`db::UserStatus`). `set_status` accepts `online`, `away` and `busy` (lowercase or capitalized). `offline` is refused with an `error` event `VALIDATION_ERROR`: "appear offline" is not available. A user is `offline` exactly when none of their devices is connected.

**Per device** (`presence::connections`): Valkey sorted set `presence:sockets:{user_id}`, member = socket id, score = expiry (Unix ms). A socket registers once it is fully set up, refreshes its expiry (90 s TTL) when it answers a server heartbeat ping (unsolicited pongs are ignored), and removes itself on close (before the voice cleanup).
- Lifecycle steps are single Lua scripts that prune expired members, update the set, decide the transition and publish in the same atomic step. Concurrent connects or disconnects of one user never announce a transition twice or miss one. `socket_offline` reports 1 → 0 when the set is empty afterwards and the call removed its own socket, expired ones or the user's `presence:due` entry (the "offline still owed" marker, which survives the set key's TTL).
- **Sweeper** (each server, `spawn_presence_sweep`):
  - every 20 s: expired users (`sweep_expired`: atomic expire step, announces `offline`) and dirty status changes (`sweep_dirty`, see below)
  - every 60 s: `reconcile_connected` (self-healing, see below)

**Published state** `presence:state:{user_id}` (hash): `status` + `v`, `activity` + `av`, `custom` + `cv` (the last published event JSON of each kind with its own version). Every publish writes it in the same Lua step as the `PUBLISH`.
- Status goes out as `{"v":N,"event":…}` and rich presence (`publish_rich`) as `{"rv":N,"kind":"activity"|"custom","event":…}`; both are internal envelopes.
- Snapshots and the backstop read only this state, never the database.

**Reconcile** (`connections::reconcile`, Lua) records the status sequence (`users.presence_seq`, bumped by every `set_status`) in `presence:seq:{user_id}` and ignores older ones. It shows `offline` without a live socket, otherwise the status, and publishes only if that differs from the published status.
- `set_status` marks the change dirty (`users.presence_dirty_since`) in the same UPDATE; a successful reconcile clears it (only if no newer change happened). Changes still dirty after 5 s are reconciled by the sweeper on any server.
- **Self-healing**: every 60 s each server reconciles the users with registered sockets from a fresh database read (status, then activity and custom status where they differ from the published state).
  - All of them when they fit the bound (1000); otherwise a rotating window (`reconcile_window`, cursor `presence:reconcile_cursor`) covers everyone within ceil(n / bound) runs.
  - Each socket's backstop (every 60 s plus up to 10 s of jitter) re-sends whatever advanced past what it sent, status and rich presence.
  - So any stale status or rich presence heals within about a minute.
- Socket recovery (a re-registration as the only live device, a failed connect-time lookup, a stale registration read) reconciles from the database too.
- Fails open: Valkey errors are logged at most once a minute.

**Stored vs. shown status**: `users.status` is the chosen status.
- Connecting sets `online` only when the stored status is `offline`.
- The last device closing resets only a stored `online`.
- REST readers use `effective_status(stored, live)` with `live_users`: the member list (blocked users either way show `offline`) and the friends list. Message, pin, thread and bot-gateway author payloads carry no presence (`AuthorProfile.status` is always `"offline"`, see `chat/AGENTS.md`).
- `users.invisible` is deprecated (always `false`).

**Audience** (`ws::presence_audience`): presence peers = friends ∪ members of the user's servers, without users blocked either way, capped at 500.
- **One lock**: `Audience` holds validated `peers` (forwarded), `subscribed` channels, `blocked` (either way), suspensions, invalidation epochs and the status and rich versions sent. **Every** presence event (live, snapshot, backstop, final `offline`) is authorized and queued while holding it.
- **Notices name the counterpart**: friendship, block and unblock changes notify both users (`notify_pair_changed`); join, leave and kick notify the server channel with the member's id and the member's own channel without one; server deletion has no user.
- **Revoking changes are durable pending revocations**: block, unfriend, leave, kick and server deletion (owner or admin) each run as an operation with an `op` id.
  1. **Before** the database change, the operation opens durable keys in Valkey (`begin_pair_revocation` / `begin_user_revocation`), then publishes a pre-commit notice (`revoking: true`, `op`; for a block also `block: true`) as a fast path. The keys are sorted sets of op ids scored by expiry, with a 5 min TTL as a crash safety net only:
     - `presence:revoking:pair:{viewer}:{peer}`, both directions, for blocks and unfriends
     - `presence:revoking:user:{u}` for the leaving or kicked member, or every member of a deleted server
  2. **After** the change, or on its error, it closes exactly its own op in the keys, then publishes the completion notice with the same `op` (`finish_*_revocation`).
  - A user stays suspended while any operation is pending: announced by a pre-commit notice and not completed (per op, so one completion never ends another), or with an open key.
  - **Every** validation checks the keys after its database read (`open_revocations`: one call for all peers; the viewer's own user key, and per peer the pair key and the peer's user key). This covers the connect-time validation after the subscription barrier, reloads, re-checks and the periodic revalidation. A key opened before the check is seen; a revocation that begins later publishes its pre-commit notice to the socket's live subscriptions.
  - A suspension ends only when no operation is pending **and** a fresh read authorizes. Nothing ends it by elapsed time (there is no grace period).
  - For a block, the pre-commit notice also adds the user to the socket's block sets at once, so channel events (messages, typing) of a user being blocked are dropped like those of a blocked user.
  - Granting changes (accept, join, unblock) notify only after the commit.
- **Revalidation**: a reload (`refresh_full`) or re-check (`recheck`, one query per named user; more than 16 fall back to a full reload) remembers the epoch it started at.
  - Results for users invalidated after that start are discarded and read again; only fresh results authorize and lift suspensions.
  - New peers are subscribed behind a barrier and stay unauthorized until a read after that barrier confirms them.
  - A failed read keeps suspensions and retries with the next heartbeat. A failure after installing subscriptions, or no settled read within 4 rounds, closes the socket with 1011.
  - Dropped peers get a final `offline` only if this socket had sent them anything.
- **Snapshots** (`send_states`: at connect, for new peers, and the backstop) read the published state, then **fence** the forwarder: the socket publishes a `presence_fence` on its own user channel and waits until the forwarder has seen it, so every notice published before is applied. Then each entry is authorized and version-checked under the lock.
- **At connect** the audience is revalidated after the subscription barrier and before `Ready`, the snapshot or any forwarding; a database error there closes with 1011.
- **Periodic revalidation**: each socket's backstop (about every minute) first revalidates the whole audience against the database (`presence_audience::backstop` → `refresh_full`), then re-sends what advanced. This catches an audience notice that was lost after its change committed. Notices are published with a bounded retry (3 attempts).
- A block or unblock processed by the socket (`set_blocked`) also invalidates the user, so a read started earlier cannot overwrite it. Re-authorized peers (a suspension lifted) are re-sent what advanced, not only new peers.
- **Block state is freshness-checked**: a reload or re-check applies a block or unblock it read, both to the presence block set and to the `blocked_users` set used for channel events, only if the read is fresh for that user (no invalidation since it began) and no revocation is pending (`apply_block_read`). A stale read never undoes a block the socket already applied.
- Test hooks (`ws::test_hooks`, `#[doc(hidden)]`):
  - `pause_after_audience_read`: connect, between the audience read and subscribing
  - `pause_in_audience_reload`: after a reload's or re-check's read
  - `pause_after_audience_barrier`: after a round's subscription barrier
  - `pause_in_snapshot`: after the fence, before queuing
  - `fail_next_audience_read`: makes the next audience read fail
  - `run_backstop`: runs the socket backstop on demand

#### Known limits (v1.0)

- **Infrastructure failures:** the pending-revocation keys and notices live in Valkey. If Valkey is unreachable when a revoking change starts (no key, no notice) or a server crashes mid-operation, an observer can keep receiving that user's presence until its next revalidation (about 60 s plus jitter) or reconnect. A key left by a crash expires after 5 min, and until then the pair stays suspended. If only the completion notice is lost, the users stay suspended until the next revalidation sees the key closed.
- **Concurrency residual (accepted for v1.0):** what remains only occurs with concurrent operations, connects or lock waits in the millisecond-to-second range and affects only status or activity, never message content. Examples: an update published in the instant between a connecting socket's key check and its subscriptions going live, or a reload racing a notice that is still in flight on another server.
- **Stale but not leaking:**
  - A status or rich presence can be stale until the next periodic reconcile or backstop (about 60 s). Examples: concurrent rich updates from two devices published out of database order, or a reconcile that briefly publishes the rich value it read just before a newer one.
  - REST readers (member list, friends) derive status from the database plus live sockets, not from the published state.
- **Reconcile cursor:** above the bound of 1000 connected users the periodic reconcile walks the due set by rank. Ranks shift as users connect and disconnect, so coverage within ceil(n/bound) runs holds for a stable population; with churn a user can occasionally wait one extra run.
- **Notice churn:** if a reload or re-check cannot settle within four rounds (the audience keeps changing, or the same user is invalidated again and again), the socket closes with 1011 and the client reconnects with a fresh state.
- **Pending block at connect (accepted):** a block that is still pending while a socket connects is applied to presence immediately (the durable key). Hiding that user's channel messages starts only with the block's completion notice, or, in a rare case of a concurrent leave and block, only after a reconnect.
- **Revoking write longer than the key TTL (accepted):** if a revoking database write waits longer than the 5 min TTL of its pending-revocation key, the key expires first and the user's status can be disclosed briefly until the write commits and its completion notice arrives.
- **Channel-message hiding on block is a UX feature, not a confidentiality boundary.** Blocking filters the blocked user's events from the socket's live stream. Their messages in shared server channels remain reachable through pins and search.

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
- Graceful shutdown (`shutdown::serve`), bounded to 5 s **from the signal**: the first action, with nothing awaited before it, cancels `WsConnectionTracker::shutdown_token`; each socket — also one waiting for its `authenticate` frame, one validating it, one still in its setup, and one inside a long operation (all interruptible) — closes with **1012** `SERVER_RESTART` and runs its normal cleanup, which removes its voice peer. Concurrently and under the same deadline the voice rooms are emptied (`leave_all`), the sockets finish and HTTP requests drain; 250 ms before the deadline, request handlers still running are cancelled (503). Clients reconnect on 1012 like on 1011/1013 (C11: only 1008 is special). A socket is not promised `voice_user_left` for others before its own close frame.

### Rate Limiting

**Connection attempts**: `/ws` is rate limited per client IP with `RateLimitCategory::WsConnect` (`rate_limit_by_ip`, default 60 per 60 s: a household of 15 devices behind one address reconnects at once after a restart).

**Concurrent connections** (`connection_limit::WsConnectionTracker`, `AppState.ws_connections`, in-process because the server is a single node):
- At most `WS_MAX_CONNECTIONS_PER_USER` (10) sockets per user and `WS_MAX_CONNECTIONS_PER_IP` (30) per client IP. The IP is derived exactly like the rate limiter's (`extract_client_ip` with the trusted proxies, `normalize_ip`: IPv4-mapped IPv6 as IPv4, IPv6 per /64). A socket that has not authenticated holds an IP slot and must send `authenticate` within 5 s. Without `RATE_LIMIT_TRUSTED_PROXIES` on a non-loopback listener, `main` logs a startup warning (behind a proxy every client would count as the proxy).
- The IP slot is taken before the upgrade (HTTP 429 `TOO_MANY_CONNECTIONS`). The user slot is taken before the upgrade with header auth (429), or right after the `authenticate` frame (close **1013** `TOO_MANY_CONNECTIONS`), in both cases before `handle_socket` creates the Valkey subscriber.
- Each slot is a `ConnectionPermit` that gives it back on `Drop`, held until `handle_socket` returns.

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
