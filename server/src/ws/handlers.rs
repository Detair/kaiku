//! WebSocket connection handling and client message dispatch.
//!
//! Contains the HTTP upgrade entry point (`handler`), the per-connection
//! socket loop (`handle_socket`), the Redis pub/sub forwarder (`handle_pubsub`),
//! and the `ClientEvent` dispatch logic (`handle_client_message`).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::Response;
use fred::prelude::*;
use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;
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
    let (tx, mut rx) = mpsc::channel::<OutboundMsg>(100);

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

    // Send ready event
    let _ = tx
        .send(OutboundMsg::Event(ServerEvent::Ready { user_id }))
        .await;

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

    match get_friends_presence(&state.db, user_id).await {
        Ok(snapshots) => {
            for snap in snapshots {
                // Always send base presence
                let presence_event = ServerEvent::PresenceUpdate {
                    user_id: snap.user_id,
                    status: snap.status.clone(),
                };
                if tx.send(OutboundMsg::Event(presence_event)).await.is_err() {
                    break;
                }

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
                            if tx.send(OutboundMsg::Event(activity_event)).await.is_err() {
                                break;
                            }
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
                            if tx.send(OutboundMsg::Event(cs_event)).await.is_err() {
                                break;
                            }
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
    let redis_client = state.redis.clone();
    let tx_clone = tx.clone();
    let subscribed_clone = subscribed_channels.clone();
    let admin_subscribed_clone = admin_subscribed.clone();
    let blocked_clone = blocked_users.clone();
    let pubsub_handle = tokio::spawn(async move {
        handle_pubsub(
            redis_client,
            HandlePubsubParams {
                tx: tx_clone,
                subscribed_channels: subscribed_clone,
                admin_subscribed: admin_subscribed_clone,
                blocked_users: blocked_clone,
                user_id,
                friend_ids,
                guild_ids,
            },
        )
        .await;
    });

    // Spawn task to forward events to WebSocket
    let sender_handle: tokio::task::JoinHandle<()> = tokio::spawn(async move {
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
            };
            if send_result.is_err() {
                break;
            }
        }
    });

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
                            warn!("Error handling message: {}", e);
                            let _ = tx
                                .send(OutboundMsg::Event(ServerEvent::Error {
                                    code: "message_error".to_string(),
                                    message: e.to_string(),
                                }))
                                .await;
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
                if tx.send(OutboundMsg::Ping).await.is_err() {
                    break;
                }
            }
        }
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

    match event {
        ClientEvent::Ping => {
            tx.send(OutboundMsg::Event(ServerEvent::Pong)).await?;
        }

        ClientEvent::Subscribe { channel_id } => {
            // Verify channel exists
            if db::find_channel_by_id(&state.db, channel_id)
                .await?
                .is_none()
            {
                tx.send(OutboundMsg::Event(ServerEvent::Error {
                    code: "channel_not_found".to_string(),
                    message: "Channel not found".to_string(),
                }))
                .await?;
                return Ok(());
            }

            // Check if user has VIEW_CHANNEL permission
            if crate::permissions::require_channel_access(&state.db, user_id, channel_id)
                .await
                .is_err()
            {
                tx.send(OutboundMsg::Event(ServerEvent::Error {
                    code: "forbidden".to_string(),
                    message: "You don't have permission to view this channel".to_string(),
                }))
                .await?;
                return Ok(());
            }

            // Add to subscribed channels
            subscribed_channels.write().await.insert(channel_id);

            tx.send(OutboundMsg::Event(ServerEvent::Subscribed { channel_id }))
                .await?;
            debug!("User {} subscribed to channel {}", user_id, channel_id);
        }

        ClientEvent::Unsubscribe { channel_id } => {
            subscribed_channels.write().await.remove(&channel_id);
            tx.send(OutboundMsg::Event(ServerEvent::Unsubscribed { channel_id }))
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
                warn!("Voice event error: {}", e);
                tx.send(OutboundMsg::Event(ServerEvent::VoiceError {
                    code: "voice_error".to_string(),
                    message: e.to_string(),
                }))
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
                tx.send(OutboundMsg::Event(ServerEvent::Error {
                    code: "admin_not_elevated".to_string(),
                    message: "Must be an elevated admin to subscribe to admin events".to_string(),
                }))
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
        async move { tx.send(OutboundMsg::Event(ev)).await }
    };

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
struct HandlePubsubParams {
    tx: mpsc::Sender<OutboundMsg>,
    subscribed_channels: Arc<tokio::sync::RwLock<HashSet<Uuid>>>,
    admin_subscribed: Arc<tokio::sync::RwLock<bool>>,
    blocked_users: Arc<tokio::sync::RwLock<HashSet<Uuid>>>,
    user_id: Uuid,
    friend_ids: Vec<Uuid>,
    guild_ids: Vec<Uuid>,
}

/// Handle Redis pub/sub messages.
async fn handle_pubsub(redis: Client, params: HandlePubsubParams) {
    // Create a subscriber client
    let subscriber = redis.clone_new();

    // Connect (fred 8.x returns JoinHandle)
    let _connect_handle = subscriber.connect();

    if let Err(e) = subscriber.wait_for_connect().await {
        error!("Subscriber connection failed: {}", e);
        return;
    }

    // Subscribe to pattern for all channel events
    let mut pubsub_stream = subscriber.message_rx();

    // Subscribe to channel pattern
    if let Err(e) = subscriber.psubscribe("channel:*").await {
        error!("Failed to psubscribe: {}", e);
        return;
    }

    // Subscribe to user's own events channel (for preferences sync, etc.)
    let user_channel = channels::user_events(params.user_id);
    if let Err(e) = subscriber.subscribe(&user_channel).await {
        warn!("Failed to subscribe to user events channel: {}", e);
    } else {
        debug!("Subscribed to user events channel: {}", user_channel);
    }

    // Subscribe to admin events channel
    if let Err(e) = subscriber.subscribe(channels::ADMIN_EVENTS).await {
        warn!("Failed to subscribe to admin events: {}", e);
    } else {
        debug!("Subscribed to admin events channel");
    }

    // Subscribe to friends' presence channels
    for friend_id in &params.friend_ids {
        let presence_channel = channels::user_presence(*friend_id);
        if let Err(e) = subscriber.subscribe(&presence_channel).await {
            warn!(
                "Failed to subscribe to presence channel for friend {}: {}",
                friend_id, e
            );
        } else {
            debug!("Subscribed to presence channel: {}", presence_channel);
        }
    }

    // Subscribe to guild event channels for state sync
    for guild_id in &params.guild_ids {
        let guild_channel = channels::guild_events(*guild_id);
        if let Err(e) = subscriber.subscribe(&guild_channel).await {
            warn!(
                "Failed to subscribe to guild events channel for guild {}: {}",
                guild_id, e
            );
        } else {
            debug!("Subscribed to guild events channel: {}", guild_channel);
        }
    }

    macro_rules! try_forward {
        ($tx:expr, $event:expr, $drops:ident, $user_id:expr) => {
            match $tx.try_send(OutboundMsg::Event($event)) {
                Ok(()) => {
                    $drops = 0;
                }
                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                    $drops += 1;
                    if $drops > 10 {
                        warn!(
                            "Disconnecting slow WebSocket client (user {}): {} consecutive drops",
                            $user_id, $drops
                        );
                        break;
                    }
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    break;
                }
            }
        };
    }

    let mut backpressure_drops: u32 = 0;
    while let Ok(message) = pubsub_stream.recv().await {
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
                                try_forward!(params.tx, event, backpressure_drops, params.user_id);
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
                        _ => {}
                    }

                    try_forward!(params.tx, event, backpressure_drops, params.user_id);
                }
            }
        }
        // Handle admin events
        else if channel_name == channels::ADMIN_EVENTS {
            // Only forward if user is subscribed to admin events
            if *params.admin_subscribed.read().await {
                if let Some(payload) = message.value.as_str() {
                    if let Ok(event) = serde_json::from_str::<ServerEvent>(&payload) {
                        try_forward!(params.tx, event, backpressure_drops, params.user_id);
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
                        try_forward!(params.tx, event, backpressure_drops, params.user_id);
                    }
                }
            }
        }
        // Handle user events (user:{uuid}) for cross-device sync
        else if channel_name.starts_with("user:") {
            // Forward all user-targeted events (read sync, etc.)
            if let Some(payload) = message.value.as_str() {
                if let Ok(event) = serde_json::from_str::<ServerEvent>(&payload) {
                    try_forward!(params.tx, event, backpressure_drops, params.user_id);
                }
            }
        }
        // Handle guild events (guild:{uuid}) for state sync
        else if channel_name.starts_with("guild:") {
            // Forward guild/member patch events to all guild members
            if let Some(payload) = message.value.as_str() {
                if let Ok(event) = serde_json::from_str::<ServerEvent>(&payload) {
                    try_forward!(params.tx, event, backpressure_drops, params.user_id);
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
