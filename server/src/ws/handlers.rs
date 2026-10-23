//! WebSocket connection handling and client message dispatch.
//!
//! Contains the HTTP upgrade entry point (`handler`), the per-connection
//! socket loop (`handle_socket`), the Redis pub/sub forwarder (`handle_pubsub`),
//! and the `ClientEvent` dispatch logic (`handle_client_message`).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::{CloseFrame, Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::Response;
use fred::prelude::*;
use futures::{SinkExt, StreamExt};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::events::{
    broadcast_presence_update, broadcast_to_channel, channels, ClientEvent, ClientMessageState,
    OutboundMsg, ServerEvent,
};
use crate::api::AppState;
use crate::auth::jwt;
use crate::db;
use crate::social::block_cache;

/// Minimum interval between activity updates (10 seconds).
const ACTIVITY_UPDATE_INTERVAL: Duration = Duration::from_secs(10);

/// WebSocket protocol header name for authentication.
const WS_PROTOCOL_PREFIX: &str = "access_token.";

/// Capacity of the per-socket outbound queue. Sized to absorb normal bursts;
/// a full queue closes the socket with 1013 so the client resyncs.
const OUTBOUND_QUEUE_CAPACITY: usize = 256;

/// How long a new socket waits for its pub/sub subscriptions before giving up
/// with close code 1011.
const PUBSUB_READY_TIMEOUT: Duration = Duration::from_secs(5);

/// Upper bound for flushing a Close frame to the client before the socket is
/// torn down.
const CLOSE_FLUSH_TIMEOUT: Duration = Duration::from_secs(1);

/// Close code: server-side event pipeline failed; reconnect and catch up.
const CLOSE_INTERNAL_ERROR: u16 = 1011;

/// Close code: client too slow, try again later; reconnect and catch up.
const CLOSE_TRY_AGAIN_LATER: u16 = 1013;

/// How long the pub/sub forwarder waits for room in a full outbound queue
/// before treating the client as too slow (close 1013). Short bursts such as
/// the presence snapshot drain well within this.
const SLOW_CLIENT_SEND_TIMEOUT: Duration = Duration::from_secs(1);

/// Broadcast buffer of each socket's own pub/sub subscriber client (fred's
/// default is 32). The subscriber sees every `channel:*` message server-wide,
/// so the buffer must absorb ordinary traffic while the forwarder waits up to
/// [`SLOW_CLIENT_SEND_TIMEOUT`] for a busy client; overflowing it ends the
/// pipeline with `Lagged` (close 1011).
const SUBSCRIBER_BROADCAST_CAPACITY: usize = 1024;

/// Close reason sent with 1011/1013: the client should reconnect and resync.
const CLOSE_REASON_RESYNC: &str = "resync";

/// Extract JWT token from Sec-WebSocket-Protocol header.
///
/// Expected format: `access_token.<jwt_token>`
///
/// Returns `None` if the header is missing or malformed.
fn extract_token_from_protocol(headers: &HeaderMap) -> Option<String> {
    headers
        .get("sec-websocket-protocol")
        .and_then(|h| h.to_str().ok())
        .and_then(|protocols| {
            // The header may contain multiple protocols separated by commas
            protocols
                .split(',')
                .map(str::trim)
                .find(|p| p.starts_with(WS_PROTOCOL_PREFIX))
                .map(|p| p[WS_PROTOCOL_PREFIX.len()..].to_string())
        })
}

/// Build a plain-text HTTP error response without panicking.
///
/// Falls back to a 500 Internal Server Error if building the requested
/// status fails (which cannot happen with hardcoded status codes, but
/// avoids any `.expect` in the hot path).
fn error_response(status: u16, body: &'static str) -> Response {
    Response::builder()
        .status(status)
        .body(body.into())
        .unwrap_or_else(|_| {
            Response::builder()
                .status(500)
                .body("Internal Server Error".into())
                .expect("fallback response builder")
        })
}

/// WebSocket upgrade handler.
///
/// Supports two authentication methods (dual-auth for transition period):
///
/// 1. **Header-based** (legacy): Client sends `Sec-WebSocket-Protocol: access_token.<jwt_token>`.
///    Token is validated before upgrade; server responds with `Sec-WebSocket-Protocol:
///    access_token`.
///
/// 2. **Post-connect** (new): Client connects without a token header. After upgrade the server
///    waits up to 5 seconds for an `Authenticate` frame carrying the JWT.  On success the normal
///    socket loop starts.
#[tracing::instrument(skip(ws, state, headers))]
pub async fn handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    // Try header-based auth first (backwards compatible)
    if let Some(token) = extract_token_from_protocol(&headers) {
        let claims = match jwt::validate_access_token(&token, &state.config.jwt_public_key) {
            Ok(claims) => claims,
            Err(e) => {
                warn!("Header-based WS auth failed: {}", e);
                return error_response(401, "Invalid token");
            }
        };

        // Reject access tokens whose session was logged out or revoked.
        if let Some(sid) = claims.sid.as_deref() {
            if crate::auth::revocation::is_session_revoked(&state.redis, sid).await {
                return error_response(401, "Session revoked");
            }
        }

        let user_id = match Uuid::parse_str(&claims.sub) {
            Ok(id) => id,
            Err(_) => {
                return error_response(401, "Invalid user ID in token");
            }
        };

        // Verify user still exists (lightweight existence check, not full row fetch)
        match sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)")
            .bind(user_id)
            .fetch_one(&state.db)
            .await
        {
            Ok(true) => {}
            Ok(false) => {
                return error_response(401, "User not found");
            }
            Err(e) => {
                warn!("Database error during WS auth: {}", e);
                return error_response(503, "Service temporarily unavailable");
            }
        }

        // Respond with the protocol to confirm (required for WebSocket handshake)
        return ws
            .protocols(["access_token"])
            .max_message_size(256 * 1024)
            .max_frame_size(64 * 1024)
            .on_upgrade(move |socket| handle_socket(socket, state, user_id));
    }

    // No header token — accept upgrade, authenticate via first frame
    ws.max_message_size(256 * 1024)
        .max_frame_size(64 * 1024)
        .on_upgrade(move |socket| handle_post_connect_auth(socket, state))
}

/// Handle post-connect authentication: wait for an `Authenticate` frame,
/// then hand off to the normal socket loop on success.
async fn handle_post_connect_auth(socket: WebSocket, state: AppState) {
    let (ws_sender, mut ws_receiver) = socket.split();

    // Wait up to 5 seconds for the Authenticate frame
    let user_id = match tokio::time::timeout(
        Duration::from_secs(5),
        wait_for_auth_frame(&mut ws_receiver, &state),
    )
    .await
    {
        Ok(Ok(uid)) => uid,
        Ok(Err(reason)) => {
            warn!("Post-connect auth failed: {}", reason);
            let mut ws_sender = ws_sender;
            let _ = ws_sender.close().await;
            return;
        }
        Err(_) => {
            warn!("Post-connect auth timeout");
            let mut ws_sender = ws_sender;
            let _ = ws_sender.close().await;
            return;
        }
    };

    // Reassemble the socket and continue with normal handler
    let socket = ws_sender
        .reunite(ws_receiver)
        .expect("reunite should never fail for same socket");
    handle_socket(socket, state, user_id).await;
}

/// Read frames until an `Authenticate` event arrives and validate its token.
async fn wait_for_auth_frame(
    receiver: &mut futures::stream::SplitStream<WebSocket>,
    state: &AppState,
) -> Result<Uuid, String> {
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => match serde_json::from_str::<ClientEvent>(&text) {
                Ok(ClientEvent::Authenticate { token }) => {
                    let claims = jwt::validate_access_token(&token, &state.config.jwt_public_key)
                        .map_err(|e| format!("Invalid token: {e}"))?;

                    // Reject access tokens whose session was logged out or revoked.
                    if let Some(sid) = claims.sid.as_deref() {
                        if crate::auth::revocation::is_session_revoked(&state.redis, sid).await {
                            return Err("Session revoked".to_string());
                        }
                    }

                    let user_id = Uuid::parse_str(&claims.sub)
                        .map_err(|_| "Invalid user ID in token".to_string())?;

                    // Verify user exists
                    match sqlx::query_scalar::<_, bool>(
                        "SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)",
                    )
                    .bind(user_id)
                    .fetch_one(&state.db)
                    .await
                    {
                        Ok(true) => return Ok(user_id),
                        Ok(false) => return Err("User not found".to_string()),
                        Err(e) => return Err(format!("Database error: {e}")),
                    }
                }
                Ok(_) => {
                    // Ignore non-Authenticate events during pre-auth
                }
                Err(e) => {
                    return Err(format!("Invalid event: {e}"));
                }
            },
            Ok(Message::Close(_)) => {
                return Err("Connection closed before authentication".to_string());
            }
            Ok(_) => {} // Ignore binary, ping, pong
            Err(e) => {
                return Err(format!("WebSocket error: {e}"));
            }
        }
    }
    Err("Connection closed before authentication".to_string())
}

/// Handle WebSocket connection.
async fn handle_socket(socket: WebSocket, state: AppState, user_id: Uuid) {
    use futures::stream::{SplitSink, SplitStream};
    let (mut ws_sender, mut ws_receiver): (SplitSink<WebSocket, Message>, SplitStream<WebSocket>) =
        socket.split();

    // Channel for sending messages to the WebSocket
    let (tx, mut rx) = mpsc::channel::<OutboundMsg>(OUTBOUND_QUEUE_CAPACITY);

    // Track subscribed channels
    let subscribed_channels: Arc<tokio::sync::RwLock<HashSet<Uuid>>> =
        Arc::new(tokio::sync::RwLock::new(HashSet::new()));

    // Track admin event subscription
    let admin_subscribed: Arc<tokio::sync::RwLock<bool>> =
        Arc::new(tokio::sync::RwLock::new(false));

    // Update user presence to online
    if let Err(e) = update_presence(&state, user_id, "online").await {
        warn!("Failed to update presence: {}", e);
    }

    info!("WebSocket connected: user={}", user_id);
    crate::observability::metrics::record_ws_connect();

    // Spawn task to forward events to WebSocket
    let mut sender_handle: tokio::task::JoinHandle<()> = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let send_result = match msg {
                OutboundMsg::Event(event) => match serde_json::to_string(&event) {
                    Ok(json) => ws_sender.send(Message::Text(json.into())).await,
                    Err(e) => {
                        error!("Failed to serialize event: {}", e);
                        continue;
                    }
                },
                OutboundMsg::Ping => ws_sender.send(Message::Ping(vec![].into())).await,
                OutboundMsg::Close { code, reason } => {
                    let frame = CloseFrame {
                        code,
                        reason: reason.into(),
                    };
                    let _ = ws_sender.send(Message::Close(Some(frame))).await;
                    break;
                }
            };
            if send_result.is_err() {
                break;
            }
        }
    });

    // Fetch user's friends for presence subscriptions
    let friend_ids = match get_user_friends(&state.db, user_id).await {
        Ok(friends) => {
            debug!(
                "User {} has {} friends for presence subscriptions",
                user_id,
                friends.len()
            );
            friends
        }
        Err(e) => {
            warn!("Failed to fetch friends for user {}: {}", user_id, e);
            Vec::new()
        }
    };

    // Fetch user's guild IDs for guild event subscriptions
    let guild_ids = match db::get_user_guild_ids(&state.db, user_id).await {
        Ok(guilds) => {
            debug!(
                "User {} is member of {} guilds for event subscriptions",
                user_id,
                guilds.len()
            );
            guilds
        }
        Err(e) => {
            warn!("Failed to fetch guilds for user {}: {}", user_id, e);
            Vec::new()
        }
    };

    // Load block sets for event filtering
    let blocked_ids = match block_cache::load_blocked_users(&state.db, &state.redis, user_id).await
    {
        Ok(ids) => {
            debug!("User {} has blocked {} users", user_id, ids.len());
            ids
        }
        Err(e) => {
            warn!("Failed to load blocked users for {}: {}", user_id, e);
            HashSet::new()
        }
    };
    let blocked_by_ids = match block_cache::load_blocked_by(&state.db, &state.redis, user_id).await
    {
        Ok(ids) => {
            debug!("User {} is blocked by {} users", user_id, ids.len());
            ids
        }
        Err(e) => {
            warn!("Failed to load blocked-by for {}: {}", user_id, e);
            HashSet::new()
        }
    };

    let blocked_users: Arc<tokio::sync::RwLock<HashSet<Uuid>>> = Arc::new(
        tokio::sync::RwLock::new(blocked_ids.union(&blocked_by_ids).copied().collect()),
    );

    // Spawn task to handle Redis pub/sub
    let (ready_tx, ready_rx) = oneshot::channel();
    let (start_tx, start_rx) = oneshot::channel();
    let mut pubsub_handle: tokio::task::JoinHandle<PubsubExit> = tokio::spawn(handle_pubsub(
        state.redis.clone(),
        HandlePubsubParams {
            tx: tx.clone(),
            subscribed_channels: subscribed_channels.clone(),
            admin_subscribed: admin_subscribed.clone(),
            blocked_users: blocked_users.clone(),
            user_id,
            friend_ids,
            guild_ids,
        },
        ready_tx,
        start_rx,
    ));

    // `Ready` promises that the socket's subscriptions are live, so wait for
    // the pub/sub task before announcing it.
    let pubsub_ready = match tokio::time::timeout(PUBSUB_READY_TIMEOUT, ready_rx).await {
        Ok(Ok(())) => true,
        Ok(Err(_)) => {
            warn!("Pub/sub pipeline failed before ready: user={}", user_id);
            false
        }
        Err(_) => {
            warn!("Pub/sub pipeline not ready in time: user={}", user_id);
            false
        }
    };

    // Why the socket ends, if that warrants a Close frame (see `close_code`).
    let mut exit: Option<PubsubExit> = None;

    if !pubsub_ready {
        exit = Some(PubsubExit::SubscribeFailed);
    } else if let Err(err) = async {
        // `Ready` is the first frame: queue it, then let the pub/sub task start
        // forwarding (events published meanwhile wait in its receiver).
        send_outbound(
            &tx,
            OutboundMsg::Event(ServerEvent::Ready {
                user_id,
                server_time: chrono::Utc::now(),
            }),
        )
        .await?;
        let _ = start_tx.send(());
        send_presence_snapshot(&state, user_id, &tx).await
    }
    .await
    {
        exit = Some(err.into());
    } else {
        // Per-connection mutable state for rate limiting and deduplication
        let mut msg_state = ClientMessageState::default();

        // Server-side heartbeat: detect dead connections via Ping/Pong
        let mut ping_interval = tokio::time::interval(Duration::from_secs(30));
        ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ping_interval.tick().await; // consume the immediate first tick
        let mut awaiting_pong = false;

        // Handle incoming messages with heartbeat
        loop {
            tokio::select! {
                msg = ws_receiver.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            if let Err(e) = handle_client_message(
                                &text,
                                user_id,
                                &state,
                                &tx,
                                &subscribed_channels,
                                &admin_subscribed,
                                &mut msg_state,
                            )
                            .await
                            {
                                if let Some(reason) = outbound_failure(&*e) {
                                    exit = Some(reason);
                                    break;
                                }
                                warn!("Error handling message: {}", e);
                                let reply = OutboundMsg::Event(ServerEvent::Error {
                                    code: "message_error".to_string(),
                                    message: e.to_string(),
                                });
                                if let Err(err) = send_outbound(&tx, reply).await {
                                    exit = Some(err.into());
                                    break;
                                }
                            }
                        }
                        Some(Ok(Message::Ping(_data))) => {
                            // Axum handles pong automatically
                            debug!("Received ping from user={}", user_id);
                        }
                        Some(Ok(Message::Pong(_))) => {
                            awaiting_pong = false;
                        }
                        Some(Ok(Message::Close(_))) => {
                            info!("WebSocket closed: user={}", user_id);
                            break;
                        }
                        Some(Err(e)) => {
                            warn!("WebSocket error: {}", e);
                            break;
                        }
                        None => break,
                        _ => {}
                    }
                }

                _ = ping_interval.tick() => {
                    if awaiting_pong || sender_handle.is_finished() {
                        info!("WebSocket ping timeout: user={}", user_id);
                        break;
                    }
                    awaiting_pong = true;
                    if let Err(err) = send_outbound(&tx, OutboundMsg::Ping).await {
                        exit = Some(err.into());
                        break;
                    }
                }

                pubsub_exit = &mut pubsub_handle => {
                    // A panicked or cancelled pub/sub task is a pipeline failure too.
                    exit = Some(pubsub_exit.unwrap_or(PubsubExit::StreamEnded));
                    break;
                }
            }
        }
    }

    if let Some(code) = exit.and_then(PubsubExit::close_code) {
        warn!(
            "Closing socket ({:?}) with {}: user={}",
            exit, code, user_id
        );
        close_and_flush(&tx, &mut sender_handle, code).await;
    }

    // Cleanup
    pubsub_handle.abort();
    sender_handle.abort();

    // Free per-peer voice rate-limit buckets in case the socket dropped without
    // an explicit VoiceLeave (ping timeout, TCP drop, app crash). The explicit
    // VoiceLeave path also calls `forget_peer`; calling it here too is idempotent.
    state.sfu.voice_rate_limiter().forget_peer(user_id);

    // Update user presence to offline
    if let Err(e) = update_presence(&state, user_id, "offline").await {
        warn!("Failed to update presence on disconnect: {}", e);
    }

    info!("WebSocket disconnected: user={}", user_id);
    crate::observability::metrics::record_ws_disconnect();
}

/// Why a bounded outbound send failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OutboundSendError {
    /// The outbound queue stayed full for [`SLOW_CLIENT_SEND_TIMEOUT`].
    #[error("client too slow: outbound queue stayed full")]
    TimedOut,
    /// The outbound queue is closed (the socket is shutting down).
    #[error("outbound queue closed")]
    Closed,
}

impl From<OutboundSendError> for PubsubExit {
    fn from(err: OutboundSendError) -> Self {
        match err {
            OutboundSendError::TimedOut => Self::SlowClient,
            OutboundSendError::Closed => Self::ClientGone,
        }
    }
}

/// Queue a message for the socket, waiting at most [`SLOW_CLIENT_SEND_TIMEOUT`]
/// for room in the outbound queue.
///
/// Every outbound send of a socket goes through here, so a stalled client can
/// never block the socket loop (or the pub/sub forwarder) indefinitely.
pub(crate) async fn send_outbound(
    tx: &mpsc::Sender<OutboundMsg>,
    msg: OutboundMsg,
) -> Result<(), OutboundSendError> {
    send_outbound_within(tx, msg, SLOW_CLIENT_SEND_TIMEOUT).await
}

/// [`send_outbound`] with a caller-chosen bound. Used for sends to *other*
/// sockets' queues (voice fan-out), which must not hold up the caller for the
/// full slow-client timeout.
pub(crate) async fn send_outbound_within(
    tx: &mpsc::Sender<OutboundMsg>,
    msg: OutboundMsg,
    timeout: Duration,
) -> Result<(), OutboundSendError> {
    match tokio::time::timeout(timeout, tx.send(msg)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_closed)) => Err(OutboundSendError::Closed),
        Err(_elapsed) => Err(OutboundSendError::TimedOut),
    }
}

/// If a (boxed) handler error is a failed outbound send, the socket must end:
/// returns the matching exit reason.
fn outbound_failure(err: &(dyn std::error::Error + 'static)) -> Option<PubsubExit> {
    if let Some(e) = err.downcast_ref::<OutboundSendError>() {
        return Some(PubsubExit::from(*e));
    }
    match err.downcast_ref::<crate::voice::VoiceError>() {
        Some(crate::voice::VoiceError::Outbound(e)) => Some(PubsubExit::from(*e)),
        _ => None,
    }
}

/// Queue a Close frame (reason "resync") and give the sender task up to
/// [`CLOSE_FLUSH_TIMEOUT`] to deliver it.
async fn close_and_flush(
    tx: &mpsc::Sender<OutboundMsg>,
    sender_handle: &mut tokio::task::JoinHandle<()>,
    code: u16,
) {
    let flush = async {
        if tx
            .send(OutboundMsg::Close {
                code,
                reason: CLOSE_REASON_RESYNC,
            })
            .await
            .is_ok()
        {
            let _ = sender_handle.await;
        }
    };
    let _ = tokio::time::timeout(CLOSE_FLUSH_TIMEOUT, flush).await;
}

/// Send the friends' presence snapshot (status, activity, custom status).
async fn send_presence_snapshot(
    state: &AppState,
    user_id: Uuid,
    tx: &mpsc::Sender<OutboundMsg>,
) -> Result<(), OutboundSendError> {
    match get_friends_presence(&state.db, user_id).await {
        Ok(snapshots) => {
            for snap in snapshots {
                // Always send base presence
                let presence_event = ServerEvent::PresenceUpdate {
                    user_id: snap.user_id,
                    status: snap.status.clone(),
                };
                send_outbound(tx, OutboundMsg::Event(presence_event)).await?;

                let is_offline = snap.status == "offline";

                // Send activity if present and user is not offline
                if !is_offline {
                    if let Some(activity_json) = snap.activity {
                        if let Ok(activity) =
                            serde_json::from_value::<crate::presence::Activity>(activity_json)
                        {
                            let activity_event = ServerEvent::RichPresenceUpdate {
                                user_id: snap.user_id,
                                activity: Some(activity),
                            };
                            send_outbound(tx, OutboundMsg::Event(activity_event)).await?;
                        }
                    }

                    // Send custom status if present and user is not offline
                    if let Some(cs_json) = snap.custom_status {
                        if let Ok(cs) =
                            serde_json::from_value::<crate::presence::CustomStatus>(cs_json)
                        {
                            let cs_event = ServerEvent::CustomStatusUpdate {
                                user_id: snap.user_id,
                                custom_status: Some(cs),
                            };
                            send_outbound(tx, OutboundMsg::Event(cs_event)).await?;
                        }
                    }
                }
            }
        }
        Err(e) => {
            warn!(
                "Failed to fetch initial friend presence for {}: {}",
                user_id, e
            );
        }
    }

    Ok(())
}

/// Handle a client message.
///
/// **Internal:** Exposed for integration tests only.
#[allow(clippy::implicit_hasher)]
#[tracing::instrument(
    skip(state, tx, subscribed_channels, admin_subscribed, msg_state, text),
    fields(user_id = %user_id)
)]
pub async fn handle_client_message(
    text: &str,
    user_id: Uuid,
    state: &AppState,
    tx: &mpsc::Sender<OutboundMsg>,
    subscribed_channels: &Arc<tokio::sync::RwLock<HashSet<Uuid>>>,
    admin_subscribed: &Arc<tokio::sync::RwLock<bool>>,
    msg_state: &mut ClientMessageState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let event: ClientEvent = serde_json::from_str(text)?;
    crate::observability::metrics::record_ws_message(event.variant_name());

    // Per-connection flood ceiling across every message type. Most events
    // (typing, subscribe, presence/status, voice signaling) have no other
    // per-message limit, so a single spamming connection is bounded here.
    // Excess messages are dropped silently — replying with an error per dropped
    // message would turn the flood into an outbound amplifier.
    if !msg_state.flood.allow() {
        debug!("Dropping WS message from user={user_id} over per-connection flood limit");
        return Ok(());
    }

    match event {
        ClientEvent::Ping => {
            send_outbound(tx, OutboundMsg::Event(ServerEvent::Pong)).await?;
        }

        ClientEvent::Subscribe { channel_id } => {
            // Verify channel exists
            if db::find_channel_by_id(&state.db, channel_id)
                .await?
                .is_none()
            {
                send_outbound(
                    tx,
                    OutboundMsg::Event(ServerEvent::Error {
                        code: "channel_not_found".to_string(),
                        message: "Channel not found".to_string(),
                    }),
                )
                .await?;
                return Ok(());
            }

            // Check if user has VIEW_CHANNEL permission
            if crate::permissions::require_channel_access(&state.db, user_id, channel_id)
                .await
                .is_err()
            {
                send_outbound(
                    tx,
                    OutboundMsg::Event(ServerEvent::Error {
                        code: "forbidden".to_string(),
                        message: "You don't have permission to view this channel".to_string(),
                    }),
                )
                .await?;
                return Ok(());
            }

            // Add to subscribed channels
            subscribed_channels.write().await.insert(channel_id);

            send_outbound(
                tx,
                OutboundMsg::Event(ServerEvent::Subscribed { channel_id }),
            )
            .await?;
            debug!("User {} subscribed to channel {}", user_id, channel_id);
        }

        ClientEvent::Unsubscribe { channel_id } => {
            subscribed_channels.write().await.remove(&channel_id);
            send_outbound(
                tx,
                OutboundMsg::Event(ServerEvent::Unsubscribed { channel_id }),
            )
            .await?;
            debug!("User {} unsubscribed from channel {}", user_id, channel_id);
        }

        ClientEvent::Typing { channel_id } => {
            // Only allow typing in channels the user is subscribed to
            // (subscription is already permission-gated)
            if !subscribed_channels.read().await.contains(&channel_id) {
                return Ok(());
            }

            // Server-side throttle: max 1 typing event per second per channel
            let now = Instant::now();
            if let Some(last) = msg_state.last_typing.get(&channel_id) {
                if now.duration_since(*last) < Duration::from_secs(1) {
                    return Ok(());
                }
            }
            msg_state.last_typing.insert(channel_id, now);
            msg_state
                .last_typing
                .retain(|_, last| now.duration_since(*last) < Duration::from_secs(2));

            // Broadcast typing indicator
            broadcast_to_channel(
                &state.redis,
                channel_id,
                &ServerEvent::TypingStart {
                    channel_id,
                    user_id,
                },
            )
            .await?;
        }

        ClientEvent::StopTyping { channel_id } => {
            // Only allow in channels the user is subscribed to
            if !subscribed_channels.read().await.contains(&channel_id) {
                return Ok(());
            }

            // No throttle on StopTyping — throttling it causes ghost typing indicators

            // Broadcast stop typing
            broadcast_to_channel(
                &state.redis,
                channel_id,
                &ServerEvent::TypingStop {
                    channel_id,
                    user_id,
                },
            )
            .await?;
        }

        // Voice events - delegate to voice handler
        ClientEvent::VoiceJoin { .. }
        | ClientEvent::VoiceLeave { .. }
        | ClientEvent::VoicePublisherOffer { .. }
        | ClientEvent::VoiceSubscriberAnswer { .. }
        | ClientEvent::VoiceIceCandidate { .. }
        | ClientEvent::VoiceMute { .. }
        | ClientEvent::VoiceUnmute { .. }
        | ClientEvent::VoiceStats { .. }
        | ClientEvent::VoiceScreenShareStart { .. }
        | ClientEvent::VoiceScreenShareStop { .. }
        | ClientEvent::VoiceWebcamStart { .. }
        | ClientEvent::VoiceWebcamStop { .. }
        | ClientEvent::VoiceSetLayerPreference { .. } => {
            if let Err(e) = crate::voice::ws_handler::handle_voice_event(
                &state.sfu,
                &state.db,
                user_id,
                event,
                tx,
                state.screen_share_limiter.as_ref(),
            )
            .await
            {
                // A reply to this socket that could not be queued ends the
                // socket like any other failed outbound send.
                if matches!(e, crate::voice::VoiceError::Outbound(_)) {
                    return Err(Box::new(e));
                }
                warn!("Voice event error: {}", e);
                send_outbound(
                    tx,
                    OutboundMsg::Event(ServerEvent::VoiceError {
                        code: "voice_error".to_string(),
                        message: e.to_string(),
                    }),
                )
                .await?;
            }
        }

        ClientEvent::SetActivity { activity } => {
            // Validate activity data if present
            if let Some(ref act) = activity {
                act.validate()
                    .map_err(|e| format!("Invalid activity: {e}"))?;
            }

            // Rate limiting: enforce minimum interval between updates
            let now = Instant::now();
            if let Some(last_update) = msg_state.activity.last_update {
                let elapsed = now.duration_since(last_update);
                if elapsed < ACTIVITY_UPDATE_INTERVAL {
                    let remaining = ACTIVITY_UPDATE_INTERVAL.saturating_sub(elapsed);
                    return Err(format!(
                        "Rate limited: wait {} seconds before next activity update",
                        remaining.as_secs() + 1
                    )
                    .into());
                }
            }

            // Deduplication: skip update if activity is unchanged
            if activity == msg_state.activity.last_activity {
                debug!("Skipping activity update: unchanged for user={}", user_id);
                return Ok(());
            }

            // Update database
            sqlx::query("UPDATE users SET activity = $1 WHERE id = $2")
                .bind(serde_json::to_value(&activity).ok())
                .bind(user_id)
                .execute(&state.db)
                .await
                .map_err(|e| format!("Failed to update activity: {e}"))?;

            // Update state for rate limiting and deduplication
            msg_state.activity.last_update = Some(now);
            activity.clone_into(&mut msg_state.activity.last_activity);

            // Broadcast to user's presence subscribers
            let event = ServerEvent::RichPresenceUpdate { user_id, activity };
            broadcast_presence_update(state, user_id, &event).await;
        }

        ClientEvent::SetStatus { status } => {
            let status_str = match status {
                crate::db::UserStatus::Online => "online",
                crate::db::UserStatus::Away => "away",
                crate::db::UserStatus::Busy => "busy",
                crate::db::UserStatus::Offline => "offline",
            };
            update_presence(state, user_id, status_str).await?;

            let event = ServerEvent::PresenceUpdate {
                user_id,
                status: status_str.to_string(),
            };
            broadcast_presence_update(state, user_id, &event).await;

            // Hide custom status when going offline/invisible
            if matches!(status, crate::db::UserStatus::Offline) {
                let hide_event = ServerEvent::CustomStatusUpdate {
                    user_id,
                    custom_status: None,
                };
                broadcast_presence_update(state, user_id, &hide_event).await;
            }

            debug!("User {} set status to {}", user_id, status_str);
        }

        ClientEvent::SetCustomStatus { custom_status } => {
            // Validate if setting (not clearing)
            if let Some(ref cs) = custom_status {
                cs.validate()
                    .map_err(|e| format!("Invalid custom status: {e}"))?;
            }

            // Rate limiting
            let now = Instant::now();
            if let Some(last_update) = msg_state.custom_status.last_update {
                let elapsed = now.duration_since(last_update);
                if elapsed < ACTIVITY_UPDATE_INTERVAL {
                    let remaining = ACTIVITY_UPDATE_INTERVAL.saturating_sub(elapsed);
                    return Err(format!(
                        "Rate limited: wait {} seconds before next custom status update",
                        remaining.as_secs() + 1
                    )
                    .into());
                }
            }

            // Deduplication
            if msg_state.custom_status.last_custom_status.as_ref() == Some(&custom_status) {
                debug!(
                    "Skipping custom status update: unchanged for user={}",
                    user_id
                );
                return Ok(());
            }

            // Persist to database
            let json_value = custom_status
                .as_ref()
                .and_then(|cs| serde_json::to_value(cs).ok());
            sqlx::query("UPDATE users SET custom_status = $1 WHERE id = $2")
                .bind(&json_value)
                .bind(user_id)
                .execute(&state.db)
                .await
                .map_err(|e| format!("Failed to update custom status: {e}"))?;

            // Update rate limiting state
            msg_state.custom_status.last_update = Some(now);
            msg_state.custom_status.last_custom_status = Some(custom_status.clone());

            // Broadcast to presence subscribers
            let event = ServerEvent::CustomStatusUpdate {
                user_id,
                custom_status,
            };
            broadcast_presence_update(state, user_id, &event).await;
            debug!("User {} updated custom status", user_id);
        }

        ClientEvent::AdminSubscribe => {
            // Check if user is an elevated admin
            let is_elevated =
                crate::admin::is_elevated_admin(&state.redis, &state.db, user_id).await;
            if !is_elevated {
                send_outbound(
                    tx,
                    OutboundMsg::Event(ServerEvent::Error {
                        code: "admin_not_elevated".to_string(),
                        message: "Must be an elevated admin to subscribe to admin events"
                            .to_string(),
                    }),
                )
                .await?;
                return Ok(());
            }

            *admin_subscribed.write().await = true;
            debug!("Admin {} subscribed to admin events", user_id);
        }

        ClientEvent::AdminUnsubscribe => {
            *admin_subscribed.write().await = false;
            debug!("Admin {} unsubscribed from admin events", user_id);
        }

        ClientEvent::ComponentInteraction {
            message_id,
            custom_id,
            values,
        } => {
            handle_component_interaction(state, tx, user_id, message_id, &custom_id, values)
                .await?;
        }

        ClientEvent::Authenticate { .. } => {
            // Already authenticated — ignore duplicate
        }
    }

    Ok(())
}

/// Route a component click to the bot that authored the message. Mints an
/// interaction (reusing the slash-command registry: Redis owner+context keys
/// with a short TTL) and publishes `ComponentInvoked` to the owning bot. The
/// bot replies over its gateway using the existing interaction-response path.
async fn handle_component_interaction(
    state: &AppState,
    tx: &mpsc::Sender<OutboundMsg>,
    user_id: Uuid,
    message_id: Uuid,
    custom_id: &str,
    values: Vec<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let send_err = |code: &'static str, msg: &'static str| {
        let ev = ServerEvent::Error {
            code: code.to_string(),
            message: msg.to_string(),
        };
        async move { send_outbound(tx, OutboundMsg::Event(ev)).await }
    };

    // Rate-limit clicks per user — each interaction does Redis writes + a
    // publish + DB reads, so spammed clicks must not flood a bot or Redis.
    if let Some(rate_limiter) = &state.rate_limiter {
        let identifier = format!("component_interaction:{user_id}");
        let allowed = rate_limiter
            .check(crate::ratelimit::RateLimitCategory::WsMessage, &identifier)
            .await
            .map(|r| r.allowed)
            .unwrap_or(true); // fail-open on limiter error (availability)
        if !allowed {
            send_err("rate_limited", "You're interacting too quickly").await?;
            return Ok(());
        }
    }

    // The message must exist and the user must be able to view its channel.
    let Some(message) = db::find_message_by_id(&state.db, message_id).await? else {
        send_err("message_not_found", "Message not found").await?;
        return Ok(());
    };
    if crate::permissions::require_channel_access(&state.db, user_id, message.channel_id)
        .await
        .is_err()
    {
        send_err("forbidden", "You cannot interact with this message").await?;
        return Ok(());
    }

    // The message must actually carry a component with this custom_id.
    let rows: Vec<crate::chat::components::ActionRow> = message
        .components
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    if !crate::chat::components::custom_id_present(&rows, custom_id) {
        send_err("component_not_found", "No such component on this message").await?;
        return Ok(());
    }

    // The owning bot is the message author; it must be a bot.
    let Some(bot_id) = message.user_id else {
        send_err("component_not_found", "Message has no author").await?;
        return Ok(());
    };
    let is_bot = db::find_user_by_id(&state.db, bot_id)
        .await?
        .map(|u| u.is_bot)
        .unwrap_or(false);
    if !is_bot {
        send_err("component_not_found", "Message is not bot-authored").await?;
        return Ok(());
    }

    let guild_id = db::find_channel_by_id(&state.db, message.channel_id)
        .await?
        .and_then(|c| c.guild_id);

    // Mint the interaction (owner + context) with a short TTL, then publish.
    let interaction_id = Uuid::new_v4();
    let owner_key = format!("interaction:{interaction_id}:owner");
    let context_key = format!("interaction:{interaction_id}:context");
    let context = serde_json::json!({
        "user_id": user_id,
        "channel_id": message.channel_id,
        "guild_id": guild_id,
        "message_id": message_id,
        "command_name": format!("component:{custom_id}"),
    });
    state
        .redis
        .set::<(), _, _>(
            &owner_key,
            bot_id.to_string(),
            Some(fred::types::Expiration::EX(300)),
            None,
            false,
        )
        .await?;
    state
        .redis
        .set::<(), _, _>(
            &context_key,
            context.to_string(),
            Some(fred::types::Expiration::EX(300)),
            None,
            false,
        )
        .await?;

    let event = crate::ws::bot_gateway::BotServerEvent::ComponentInvoked {
        interaction_id,
        custom_id: custom_id.to_string(),
        message_id,
        guild_id,
        channel_id: message.channel_id,
        user_id,
        values,
    };
    let payload = serde_json::to_string(&event)?;
    state
        .redis
        .publish::<(), _, _>(format!("bot:{bot_id}"), payload)
        .await?;

    Ok(())
}

/// Parameters for the Redis pub/sub handler.
pub(crate) struct HandlePubsubParams {
    tx: mpsc::Sender<OutboundMsg>,
    subscribed_channels: Arc<tokio::sync::RwLock<HashSet<Uuid>>>,
    admin_subscribed: Arc<tokio::sync::RwLock<bool>>,
    blocked_users: Arc<tokio::sync::RwLock<HashSet<Uuid>>>,
    user_id: Uuid,
    friend_ids: Vec<Uuid>,
    guild_ids: Vec<Uuid>,
}

/// Why a connection's pub/sub pipeline stopped.
///
/// Every variant except [`PubsubExit::ClientGone`] means the socket can no
/// longer see all of its events, so the socket is closed and the client
/// reconnects and catches up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PubsubExit {
    /// The subscriber connection to Valkey could not be established.
    ConnectFailed,
    /// The `channel:*` pattern subscription failed.
    SubscribeFailed,
    /// The pub/sub message stream ended.
    StreamEnded,
    /// The pub/sub receiver fell behind and messages were skipped.
    Lagged,
    /// The outbound queue to the client was full.
    SlowClient,
    /// The outbound queue was closed (the socket is already shutting down).
    ClientGone,
}

impl PubsubExit {
    /// WebSocket close code to send for this exit, or `None` if the socket is
    /// already going away.
    ///
    /// 1013 = client too slow, try again later; 1011 = server-side event
    /// pipeline failed. Both mean "reconnect and catch up".
    pub(crate) const fn close_code(self) -> Option<u16> {
        match self {
            Self::SlowClient => Some(CLOSE_TRY_AGAIN_LATER),
            Self::ClientGone => None,
            Self::ConnectFailed | Self::SubscribeFailed | Self::StreamEnded | Self::Lagged => {
                Some(CLOSE_INTERNAL_ERROR)
            }
        }
    }
}

/// Handle Redis pub/sub messages.
///
/// Fires `ready` once the `channel:*` pattern subscription and all per-user
/// subscriptions have succeeded (any failure returns `SubscribeFailed`), then waits for `start`
/// (sent by the socket after it queued `Ready`) and forwards messages until the pipeline stops.
/// The returned [`PubsubExit`] says why.
async fn handle_pubsub(
    redis: Client,
    params: HandlePubsubParams,
    ready: oneshot::Sender<()>,
    start: oneshot::Receiver<()>,
) -> PubsubExit {
    // Create a subscriber client
    let subscriber = new_subscriber_client(&redis);

    // Connect (fred 8.x returns JoinHandle)
    let _connect_handle = subscriber.connect();

    if let Err(e) = subscriber.wait_for_connect().await {
        error!("Subscriber connection failed: {}", e);
        return PubsubExit::ConnectFailed;
    }

    // Watch for reconnects from here on (the initial connect has already been
    // announced, so only later reconnects arrive on this receiver).
    let reconnect_rx = subscriber.reconnect_rx();

    // Subscribe to pattern for all channel events
    let pubsub_stream = subscriber.message_rx();

    // Subscribe to channel pattern
    if let Err(e) = subscriber.psubscribe("channel:*").await {
        error!("Failed to psubscribe: {}", e);
        return PubsubExit::SubscribeFailed;
    }

    // Subscribe to user's own events channel (for preferences sync, etc.)
    let user_channel = channels::user_events(params.user_id);
    // Cross-device sync depends on this channel; like every subscription below,
    // a failure ends the pipeline (no `Ready`, close 1011).
    if let Err(e) = subscriber.subscribe(&user_channel).await {
        error!("Failed to subscribe to user events channel: {}", e);
        return PubsubExit::SubscribeFailed;
    }
    debug!("Subscribed to user events channel: {}", user_channel);

    // Every other subscription is required too: `Ready` promises that the
    // socket sees all of its events, so any failure ends the pipeline.
    let other_channels = std::iter::once(channels::ADMIN_EVENTS.to_string())
        .chain(
            params
                .friend_ids
                .iter()
                .map(|id| channels::user_presence(*id)),
        )
        .chain(
            params
                .guild_ids
                .iter()
                .map(|id| channels::guild_events(*id)),
        );
    for channel in other_channels {
        if let Err(e) = subscriber.subscribe(&channel).await {
            error!("Failed to subscribe to {}: {}", channel, e);
            return PubsubExit::SubscribeFailed;
        }
        debug!("Subscribed to {}", channel);
    }

    // Subscriptions are live: the socket may now announce `Ready`.
    let _ = ready.send(());

    forward_after_start(start, pubsub_stream, reconnect_rx, &params).await
}

/// Create the per-socket subscriber client: same connection settings and
/// reconnect policy as `redis`, but with a [`SUBSCRIBER_BROADCAST_CAPACITY`]
/// pub/sub buffer. The shared client is not modified.
fn new_subscriber_client(redis: &Client) -> Client {
    let mut perf = redis.perf_config();
    perf.broadcast_channel_capacity = SUBSCRIBER_BROADCAST_CAPACITY;
    Client::new(
        redis.client_config(),
        Some(perf),
        Some(redis.connection_config().clone()),
        redis.client_reconnect_policy(),
    )
}

/// Wait for `start` (the socket has queued `Ready`), then forward messages.
///
/// Messages published before `start` stay buffered in `rx`, so `Ready` is
/// always the socket's first frame and nothing is lost. If `start` is dropped
/// the socket closed during setup and [`PubsubExit::ClientGone`] is returned.
async fn forward_after_start(
    start: oneshot::Receiver<()>,
    rx: tokio::sync::broadcast::Receiver<fred::types::Message>,
    reconnect_rx: tokio::sync::broadcast::Receiver<fred::types::config::Server>,
    params: &HandlePubsubParams,
) -> PubsubExit {
    if start.await.is_err() {
        return PubsubExit::ClientGone;
    }
    forward_until_reconnect(rx, reconnect_rx, params).await
}

/// Run [`forward_pubsub_messages`] until it stops or the subscriber connection
/// reconnects.
///
/// A plain fred client does not restore its subscriptions after a reconnect,
/// so a reconnect ends the pipeline with [`PubsubExit::StreamEnded`].
async fn forward_until_reconnect(
    rx: tokio::sync::broadcast::Receiver<fred::types::Message>,
    mut reconnect_rx: tokio::sync::broadcast::Receiver<fred::types::config::Server>,
    params: &HandlePubsubParams,
) -> PubsubExit {
    use tokio::sync::broadcast::error::RecvError;

    let reconnected = async {
        match reconnect_rx.recv().await {
            Ok(_) | Err(RecvError::Lagged(_)) => {}
            // No reconnect notifications can arrive any more.
            Err(RecvError::Closed) => std::future::pending::<()>().await,
        }
    };

    tokio::select! {
        exit = forward_pubsub_messages(rx, params) => exit,
        () = reconnected => {
            warn!(
                "Pub/sub subscriber reconnected, subscriptions lost (user {})",
                params.user_id
            );
            PubsubExit::StreamEnded
        }
    }
}

/// Forward pub/sub messages to the socket's outbound queue until the pipeline
/// stops, and report why it stopped.
///
/// Never drops an event silently: a lagging receiver, or an outbound queue that
/// stays full for [`SLOW_CLIENT_SEND_TIMEOUT`], ends forwarding so the socket
/// can be closed and the client can resync.
pub(crate) async fn forward_pubsub_messages(
    mut rx: tokio::sync::broadcast::Receiver<fred::types::Message>,
    params: &HandlePubsubParams,
) -> PubsubExit {
    use tokio::sync::broadcast::error::RecvError;

    macro_rules! try_forward {
        ($event:expr) => {
            if let Err(err) = send_outbound(&params.tx, OutboundMsg::Event($event)).await {
                if err == OutboundSendError::TimedOut {
                    warn!(
                        "Closing slow WebSocket client (user {}): outbound queue full for {:?}",
                        params.user_id, SLOW_CLIENT_SEND_TIMEOUT
                    );
                }
                return err.into();
            }
        };
    }

    let user_channel = channels::user_events(params.user_id);
    loop {
        let message = match rx.recv().await {
            Ok(message) => message,
            Err(RecvError::Closed) => {
                warn!("Pub/sub stream ended (user {})", params.user_id);
                return PubsubExit::StreamEnded;
            }
            Err(RecvError::Lagged(skipped)) => {
                warn!(
                    "Pub/sub receiver lagged by {} messages (user {})",
                    skipped, params.user_id
                );
                return PubsubExit::Lagged;
            }
        };
        let channel_name = message.channel.to_string();

        // Handle channel events (channel:{uuid})
        if let Some(uuid_str) = channel_name.strip_prefix("channel:") {
            if let Ok(channel_id) = Uuid::parse_str(uuid_str) {
                // Check if we're subscribed to this channel
                if params
                    .subscribed_channels
                    .read()
                    .await
                    .contains(&channel_id)
                {
                    // Parse and forward the event (with block filtering)
                    if let Some(payload) = message.value.as_str() {
                        if let Ok(event) = serde_json::from_str::<ServerEvent>(&payload) {
                            // Filter events from blocked users
                            let blocked = params.blocked_users.read().await;
                            let should_filter = match &event {
                                ServerEvent::MessageNew { message, .. } => message
                                    .get("author")
                                    .and_then(|a| a.get("id"))
                                    .and_then(|id| id.as_str())
                                    .and_then(|id| Uuid::parse_str(id).ok())
                                    .is_some_and(|author_id| blocked.contains(&author_id)),
                                ServerEvent::TypingStart { user_id: uid, .. }
                                | ServerEvent::TypingStop { user_id: uid, .. }
                                | ServerEvent::VoiceUserJoined { user_id: uid, .. }
                                | ServerEvent::VoiceUserLeft { user_id: uid, .. }
                                | ServerEvent::CallParticipantJoined { user_id: uid, .. }
                                | ServerEvent::CallParticipantLeft { user_id: uid, .. } => {
                                    blocked.contains(uid)
                                }
                                _ => false,
                            };
                            drop(blocked);

                            if !should_filter {
                                try_forward!(event);
                            }
                        }
                    }
                }
            }
        }
        // Handle user events (user:{uuid}) - for preferences sync across devices
        else if channel_name == user_channel {
            if let Some(payload) = message.value.as_str() {
                if let Ok(event) = serde_json::from_str::<ServerEvent>(&payload) {
                    // Handle block/unblock events to update in-memory set
                    match &event {
                        ServerEvent::UserBlocked {
                            user_id: blocked_id,
                        } => {
                            params.blocked_users.write().await.insert(*blocked_id);
                        }
                        ServerEvent::UserUnblocked {
                            user_id: unblocked_id,
                        } => {
                            params.blocked_users.write().await.remove(unblocked_id);
                        }
                        // Subscribe before forwarding: Redis keeps publish
                        // order on this connection, so the DM's first
                        // `channel:{id}` events are not filtered out.
                        ServerEvent::DmCreated { channel } => {
                            let channel_id = channel
                                .get("id")
                                .and_then(|id| id.as_str())
                                .and_then(|id| Uuid::parse_str(id).ok());
                            if let Some(channel_id) = channel_id {
                                params.subscribed_channels.write().await.insert(channel_id);
                            } else {
                                warn!(
                                    "DmCreated without a valid channel id (user {})",
                                    params.user_id
                                );
                            }
                        }
                        _ => {}
                    }

                    try_forward!(event);
                }
            }
        }
        // Handle admin events
        else if channel_name == channels::ADMIN_EVENTS {
            // Only forward if user is subscribed to admin events
            if *params.admin_subscribed.read().await {
                if let Some(payload) = message.value.as_str() {
                    if let Ok(event) = serde_json::from_str::<ServerEvent>(&payload) {
                        try_forward!(event);
                    }
                }
            }
        }
        // Handle presence events (presence:{uuid})
        else if channel_name.starts_with("presence:") {
            // Forward presence updates from friends (filter blocked users)
            if let Some(payload) = message.value.as_str() {
                if let Ok(event) = serde_json::from_str::<ServerEvent>(&payload) {
                    let should_filter = match &event {
                        ServerEvent::PresenceUpdate { user_id: uid, .. }
                        | ServerEvent::RichPresenceUpdate { user_id: uid, .. }
                        | ServerEvent::CustomStatusUpdate { user_id: uid, .. } => {
                            params.blocked_users.read().await.contains(uid)
                        }
                        _ => false,
                    };

                    if !should_filter {
                        try_forward!(event);
                    }
                }
            }
        }
        // Handle user events (user:{uuid}) for cross-device sync
        else if channel_name.starts_with("user:") {
            // Forward all user-targeted events (read sync, etc.)
            if let Some(payload) = message.value.as_str() {
                if let Ok(event) = serde_json::from_str::<ServerEvent>(&payload) {
                    try_forward!(event);
                }
            }
        }
        // Handle guild events (guild:{uuid}) for state sync
        else if channel_name.starts_with("guild:") {
            // Forward guild/member patch events to all guild members
            if let Some(payload) = message.value.as_str() {
                if let Ok(event) = serde_json::from_str::<ServerEvent>(&payload) {
                    try_forward!(event);
                }
            }
        }
    }
}

/// Spawn a background task that periodically clears expired custom statuses.
///
/// Runs every 60 seconds. For each expired status:
/// 1. Clears `custom_status` to NULL in the database
/// 2. Broadcasts `CustomStatusUpdate { custom_status: None }` to friends
pub fn spawn_custom_status_sweep(
    db: sqlx::PgPool,
    redis: fred::clients::Client,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_mins(1));
        loop {
            interval.tick().await;

            // Find expired custom statuses
            let expired: Vec<(Uuid,)> = match sqlx::query_as(
                r"
                SELECT id FROM users
                WHERE custom_status IS NOT NULL
                  AND custom_status->>'expires_at' IS NOT NULL
                  AND (custom_status->>'expires_at')::timestamptz <= NOW()
                ",
            )
            .fetch_all(&db)
            .await
            {
                Ok(rows) => rows,
                Err(e) => {
                    warn!(error = %e, "Custom status sweep: query failed");
                    continue;
                }
            };

            if expired.is_empty() {
                continue;
            }

            let user_ids: Vec<Uuid> = expired.into_iter().map(|(id,)| id).collect();
            debug!(count = user_ids.len(), "Clearing expired custom statuses");

            // Clear in database
            if let Err(e) = sqlx::query("UPDATE users SET custom_status = NULL WHERE id = ANY($1)")
                .bind(&user_ids)
                .execute(&db)
                .await
            {
                warn!(error = %e, "Custom status sweep: clear failed");
                continue;
            }

            // Broadcast to friends
            for uid in &user_ids {
                let event = ServerEvent::CustomStatusUpdate {
                    user_id: *uid,
                    custom_status: None,
                };
                let json = match serde_json::to_string(&event) {
                    Ok(j) => j,
                    Err(_) => continue,
                };
                let channel = format!("presence:{uid}");
                let _: Result<(), _> = redis.publish(&channel, &json).await;
            }
        }
    })
}

/// Update user presence in the database.
async fn update_presence(state: &AppState, user_id: Uuid, status: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET status = $1::user_status WHERE id = $2")
        .bind(status)
        .bind(user_id)
        .execute(&state.db)
        .await?;

    Ok(())
}

/// Get list of user's accepted friend IDs.
async fn get_user_friends(db: &sqlx::PgPool, user_id: Uuid) -> Result<Vec<Uuid>, sqlx::Error> {
    let friends: Vec<(Uuid,)> = sqlx::query_as(
        r"
        SELECT CASE
            WHEN requester_id = $1 THEN addressee_id
            ELSE requester_id
        END as friend_id
        FROM friendships
        WHERE (requester_id = $1 OR addressee_id = $1)
        AND status = 'accepted'
        ",
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;

    Ok(friends.into_iter().map(|(id,)| id).collect())
}

/// Snapshot of a friend's full presence state for the connect flow.
#[derive(Debug, sqlx::FromRow)]
struct FriendPresenceSnapshot {
    user_id: Uuid,
    status: String,
    activity: Option<serde_json::Value>,
    custom_status: Option<serde_json::Value>,
}

async fn get_friends_presence(
    db: &sqlx::PgPool,
    user_id: Uuid,
) -> Result<Vec<FriendPresenceSnapshot>, sqlx::Error> {
    let rows: Vec<FriendPresenceSnapshot> = sqlx::query_as(
        r"
        SELECT
            CASE
                WHEN f.requester_id = $1 THEN f.addressee_id
                ELSE f.requester_id
            END as user_id,
            u.status::text as status,
            u.activity,
            u.custom_status
        FROM friendships f
        JOIN users u ON u.id = CASE
            WHEN f.requester_id = $1 THEN f.addressee_id
            ELSE f.requester_id
        END
        WHERE (f.requester_id = $1 OR f.addressee_id = $1)
          AND f.status = 'accepted'
        ",
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;

    Ok(rows)
}

#[cfg(test)]
mod pubsub_tests {
    use fred::types::config::Server;
    use fred::types::{Message as PubsubMessage, MessageKind, Value as PubsubValue};

    use super::*;

    const CHANNEL: Uuid = Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_0001);

    fn test_params(capacity: usize) -> (HandlePubsubParams, mpsc::Receiver<OutboundMsg>) {
        let (tx, rx) = mpsc::channel(capacity);
        let params = HandlePubsubParams {
            tx,
            subscribed_channels: Arc::new(tokio::sync::RwLock::new(HashSet::new())),
            admin_subscribed: Arc::new(tokio::sync::RwLock::new(false)),
            blocked_users: Arc::new(tokio::sync::RwLock::new(HashSet::new())),
            user_id: Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_00aa),
            friend_ids: Vec::new(),
            guild_ids: Vec::new(),
        };
        (params, rx)
    }

    fn typing_event() -> ServerEvent {
        ServerEvent::TypingStart {
            channel_id: CHANNEL,
            user_id: Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_00bb),
        }
    }

    fn channel_message(channel_id: Uuid, event: &ServerEvent) -> PubsubMessage {
        PubsubMessage {
            channel: channels::channel_events(channel_id).into(),
            value: PubsubValue::from(serde_json::to_string(event).expect("serialize event")),
            kind: MessageKind::PMessage,
            server: Server::new("127.0.0.1", 6379),
        }
    }

    #[tokio::test]
    async fn dm_created_subscribes_before_forwarding() {
        let (btx, brx) = tokio::sync::broadcast::channel(8);
        let (params, mut rx) = test_params(16);
        let dm_created = ServerEvent::DmCreated {
            channel: serde_json::json!({ "id": CHANNEL }),
        };
        btx.send(PubsubMessage {
            channel: channels::user_events(params.user_id).into(),
            value: PubsubValue::from(serde_json::to_string(&dm_created).expect("serialize")),
            kind: MessageKind::Message,
            server: Server::new("127.0.0.1", 6379),
        })
        .unwrap();
        btx.send(channel_message(CHANNEL, &typing_event())).unwrap();
        drop(btx);

        assert!(matches!(
            forward_pubsub_messages(brx, &params).await,
            PubsubExit::StreamEnded
        ));
        assert!(params.subscribed_channels.read().await.contains(&CHANNEL));
        assert!(matches!(
            rx.recv().await,
            Some(OutboundMsg::Event(ServerEvent::DmCreated { .. }))
        ));
        assert!(matches!(
            rx.recv().await,
            Some(OutboundMsg::Event(ServerEvent::TypingStart { .. }))
        ));
    }

    #[tokio::test]
    async fn forward_returns_stream_ended_when_sender_dropped() {
        let (btx, brx) = tokio::sync::broadcast::channel::<PubsubMessage>(8);
        let (params, _rx) = test_params(16);
        drop(btx);
        assert!(matches!(
            forward_pubsub_messages(brx, &params).await,
            PubsubExit::StreamEnded
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn forward_returns_slow_client_when_send_times_out() {
        let (btx, brx) = tokio::sync::broadcast::channel(8);
        let (params, _rx_never_drained) = test_params(1); // outbound capacity 1
        params.subscribed_channels.write().await.insert(CHANNEL);
        for _ in 0..2 {
            btx.send(channel_message(CHANNEL, &typing_event())).unwrap();
        }
        let started = tokio::time::Instant::now();
        assert!(matches!(
            forward_pubsub_messages(brx, &params).await,
            PubsubExit::SlowClient
        ));
        assert!(started.elapsed() >= SLOW_CLIENT_SEND_TIMEOUT);
    }

    #[tokio::test(start_paused = true)]
    async fn forward_waits_for_briefly_full_queue_instead_of_closing() {
        // A burst (e.g. the presence snapshot) can fill the queue; as long as
        // the client drains it within the timeout, nothing is dropped and the
        // socket stays open.
        let (btx, brx) = tokio::sync::broadcast::channel(8);
        let (params, mut rx) = test_params(1);
        params.subscribed_channels.write().await.insert(CHANNEL);
        for _ in 0..3 {
            btx.send(channel_message(CHANNEL, &typing_event())).unwrap();
        }
        let forward = forward_pubsub_messages(brx, &params);
        tokio::pin!(forward);
        let drain = async {
            for _ in 0..3 {
                tokio::time::sleep(SLOW_CLIENT_SEND_TIMEOUT / 2).await;
                assert!(matches!(
                    rx.recv().await,
                    Some(OutboundMsg::Event(ServerEvent::TypingStart { .. }))
                ));
            }
        };
        tokio::select! {
            exit = &mut forward => panic!("forwarding stopped early: {exit:?}"),
            () = drain => {}
        }
    }

    #[tokio::test]
    async fn forward_returns_lagged_when_broadcast_overflows() {
        let (btx, brx) = tokio::sync::broadcast::channel(1);
        let (params, _rx) = test_params(16);
        params.subscribed_channels.write().await.insert(CHANNEL);
        for _ in 0..3 {
            btx.send(channel_message(CHANNEL, &typing_event())).unwrap();
        }
        assert!(matches!(
            forward_pubsub_messages(brx, &params).await,
            PubsubExit::Lagged
        ));
    }

    #[tokio::test]
    async fn forward_returns_client_gone_when_outbound_closed() {
        let (btx, brx) = tokio::sync::broadcast::channel(8);
        let (params, rx) = test_params(16);
        params.subscribed_channels.write().await.insert(CHANNEL);
        drop(rx);
        btx.send(channel_message(CHANNEL, &typing_event())).unwrap();
        assert!(matches!(
            forward_pubsub_messages(brx, &params).await,
            PubsubExit::ClientGone
        ));
    }

    #[tokio::test]
    async fn forward_returns_stream_ended_on_subscriber_reconnect() {
        // A plain fred client does not restore subscriptions after a reconnect,
        // so a reconnect must end forwarding instead of leaving a silent socket.
        let (_btx, brx) = tokio::sync::broadcast::channel::<PubsubMessage>(8);
        let (reconnect_tx, reconnect_rx) = tokio::sync::broadcast::channel::<Server>(1);
        let (params, _rx) = test_params(16);
        reconnect_tx.send(Server::new("127.0.0.1", 6379)).unwrap();
        let exit = tokio::time::timeout(
            Duration::from_secs(1),
            forward_until_reconnect(brx, reconnect_rx, &params),
        )
        .await
        .expect("forwarding must stop on reconnect");
        assert!(matches!(exit, PubsubExit::StreamEnded));
    }

    #[tokio::test]
    async fn forward_keeps_running_without_reconnect() {
        let (btx, brx) = tokio::sync::broadcast::channel::<PubsubMessage>(8);
        let (_reconnect_tx, reconnect_rx) = tokio::sync::broadcast::channel::<Server>(1);
        let (params, mut rx) = test_params(16);
        params.subscribed_channels.write().await.insert(CHANNEL);
        btx.send(channel_message(CHANNEL, &typing_event())).unwrap();
        let forward = forward_until_reconnect(brx, reconnect_rx, &params);
        tokio::pin!(forward);
        tokio::select! {
            _ = &mut forward => panic!("forwarding stopped without a reason"),
            msg = rx.recv() => assert!(matches!(
                msg,
                Some(OutboundMsg::Event(ServerEvent::TypingStart { .. }))
            )),
        }
    }

    #[tokio::test]
    async fn forwarding_waits_for_start_so_ready_is_first() {
        let (btx, brx) = tokio::sync::broadcast::channel::<PubsubMessage>(8);
        let (_reconnect_tx, reconnect_rx) = tokio::sync::broadcast::channel::<Server>(1);
        let (start_tx, start_rx) = oneshot::channel();
        let (params, mut rx) = test_params(16);
        params.subscribed_channels.write().await.insert(CHANNEL);
        let user_id = params.user_id;
        let tx = params.tx.clone();

        // Subscriptions are live; an event arrives before the socket queued Ready.
        btx.send(channel_message(CHANNEL, &typing_event())).unwrap();

        let forward = forward_after_start(start_rx, brx, reconnect_rx, &params);
        tokio::pin!(forward);
        // Give the forwarder every chance to run: it must not forward yet.
        tokio::select! {
            biased;
            exit = &mut forward => panic!("forwarding stopped early: {exit:?}"),
            () = tokio::task::yield_now() => {}
        }
        assert!(rx.try_recv().is_err(), "event forwarded before start");

        tx.send(OutboundMsg::Event(ServerEvent::Ready {
            user_id,
            server_time: chrono::Utc::now(),
        }))
        .await
        .unwrap();
        start_tx.send(()).unwrap();

        tokio::select! {
            exit = &mut forward => panic!("forwarding stopped early: {exit:?}"),
            () = async {
                assert!(matches!(
                    rx.recv().await,
                    Some(OutboundMsg::Event(ServerEvent::Ready { .. }))
                ));
                assert!(matches!(
                    rx.recv().await,
                    Some(OutboundMsg::Event(ServerEvent::TypingStart { .. }))
                ));
            } => {}
        }
    }

    #[tokio::test]
    async fn forwarding_returns_client_gone_when_start_never_comes() {
        let (_btx, brx) = tokio::sync::broadcast::channel::<PubsubMessage>(8);
        let (_reconnect_tx, reconnect_rx) = tokio::sync::broadcast::channel::<Server>(1);
        let (start_tx, start_rx) = oneshot::channel::<()>();
        let (params, _rx) = test_params(16);
        drop(start_tx);
        assert!(matches!(
            forward_after_start(start_rx, brx, reconnect_rx, &params).await,
            PubsubExit::ClientGone
        ));
    }

    #[test]
    fn subscriber_client_gets_larger_broadcast_capacity() {
        let base = Client::new(
            fred::types::config::Config::default(),
            None,
            None,
            Some(ReconnectPolicy::new_constant(0, 100)),
        );
        assert!(base.perf_config().broadcast_channel_capacity < SUBSCRIBER_BROADCAST_CAPACITY);

        let subscriber = new_subscriber_client(&base);
        assert_eq!(
            subscriber.perf_config().broadcast_channel_capacity,
            SUBSCRIBER_BROADCAST_CAPACITY
        );
        assert!(subscriber.has_reconnect_policy());
        // The shared base client is left untouched.
        assert!(base.perf_config().broadcast_channel_capacity < SUBSCRIBER_BROADCAST_CAPACITY);
    }

    #[tokio::test(start_paused = true)]
    async fn bounded_send_gives_up_when_queue_stays_full() {
        let (tx, _rx_never_drained) = mpsc::channel(1);
        send_outbound(&tx, OutboundMsg::Ping).await.unwrap(); // fills the queue
        let started = tokio::time::Instant::now();
        let err = send_outbound(&tx, OutboundMsg::Ping).await.unwrap_err();
        assert!(matches!(err, OutboundSendError::TimedOut));
        assert!(started.elapsed() >= SLOW_CLIENT_SEND_TIMEOUT);
        assert_eq!(PubsubExit::from(err), PubsubExit::SlowClient);
    }

    #[tokio::test]
    async fn bounded_send_reports_closed_queue() {
        let (tx, rx) = mpsc::channel(1);
        drop(rx);
        let err = send_outbound(&tx, OutboundMsg::Ping).await.unwrap_err();
        assert!(matches!(err, OutboundSendError::Closed));
        assert_eq!(PubsubExit::from(err), PubsubExit::ClientGone);
    }

    #[tokio::test(start_paused = true)]
    async fn bounded_send_failure_is_recognisable_through_boxed_errors() {
        // handle_client_message returns Box<dyn Error>; the socket loop must
        // still tell an outbound failure apart from an ordinary handler error.
        let (tx, _rx_never_drained) = mpsc::channel(1);
        send_outbound(&tx, OutboundMsg::Ping).await.unwrap();
        let boxed: Box<dyn std::error::Error + Send + Sync> = send_outbound(&tx, OutboundMsg::Ping)
            .await
            .unwrap_err()
            .into();
        assert_eq!(outbound_failure(&*boxed), Some(PubsubExit::SlowClient));
        let other: Box<dyn std::error::Error + Send + Sync> = "bad input".into();
        assert_eq!(outbound_failure(&*other), None);
    }

    #[tokio::test(start_paused = true)]
    async fn timed_out_voice_reply_is_an_outbound_failure() {
        let (tx, _rx_never_drained) = mpsc::channel(1);
        send_outbound(&tx, OutboundMsg::Ping).await.unwrap();
        let voice_err: crate::voice::VoiceError = send_outbound(&tx, OutboundMsg::Ping)
            .await
            .unwrap_err()
            .into();
        let boxed: Box<dyn std::error::Error + Send + Sync> = Box::new(voice_err);
        assert_eq!(outbound_failure(&*boxed), Some(PubsubExit::SlowClient));

        let other: Box<dyn std::error::Error + Send + Sync> =
            Box::new(crate::voice::VoiceError::NotInChannel);
        assert_eq!(outbound_failure(&*other), None);
    }

    #[test]
    fn close_codes() {
        assert_eq!(PubsubExit::SlowClient.close_code(), Some(1013));
        assert_eq!(PubsubExit::StreamEnded.close_code(), Some(1011));
        assert_eq!(PubsubExit::Lagged.close_code(), Some(1011));
        assert_eq!(PubsubExit::ConnectFailed.close_code(), Some(1011));
        assert_eq!(PubsubExit::SubscribeFailed.close_code(), Some(1011));
        assert_eq!(PubsubExit::ClientGone.close_code(), None);
    }
}
