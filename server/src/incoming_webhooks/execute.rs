//! Execute Webhook pipeline + token-scoped webhook message CRUD.
//!
//! `POST /api/webhooks/{id}/{token}` is the Discord-compatibility core:
//! whatever a Discord webhook sender posts must work here. Unknown fields are
//! ignored, embeds arrive in Discord wire shape and are adapted, and error /
//! rate-limit bodies match Discord so client libraries behave correctly.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use uuid::Uuid;

use super::error::IncomingWebhookError;
use super::queries::{self, WebhookMessageParams};
use super::types::{
    adapt_embeds, EditWebhookMessageBody, ExecuteQuery, ExecuteWebhookBody, IncomingWebhook,
};
use crate::api::AppState;
use crate::chat::types::{AuthorProfile, MessageResponse};
use crate::db::{self, ChannelType, Message};
use crate::ratelimit::{NormalizedIp, RateLimitCategory};
use crate::ws::{broadcast_to_channel, ServerEvent};

/// Effective (username, `avatar_url`) for one message: per-message override
/// falls back to the webhook's defaults. Snapshotted onto the message row.
fn effective_identity<'a>(
    webhook: &'a IncomingWebhook,
    username: Option<&'a str>,
    avatar_url: Option<&'a str>,
) -> Result<(String, Option<String>), IncomingWebhookError> {
    let name = match username.map(str::trim).filter(|u| !u.is_empty()) {
        Some(u) => {
            if u.chars().count() > 80 {
                return Err(IncomingWebhookError::Validation(
                    "username must be at most 80 characters".to_string(),
                ));
            }
            u.to_string()
        }
        None => webhook.name.clone(),
    };
    let avatar = avatar_url
        .filter(|u| u.starts_with("https://"))
        .map(String::from)
        .or_else(|| webhook.avatar_url.clone());
    Ok((name, avatar))
}

/// Build the API/broadcast response for a webhook-authored message. The
/// author is synthesized from the snapshot columns — there is no user row.
pub(super) fn webhook_message_response(message: Message) -> MessageResponse {
    let name = message
        .webhook_username
        .clone()
        .unwrap_or_else(|| "Webhook".to_string());
    MessageResponse {
        id: message.id,
        channel_id: message.channel_id,
        author: AuthorProfile {
            id: message.webhook_id.unwrap_or(Uuid::nil()),
            username: name.clone(),
            display_name: name,
            avatar_url: message.webhook_avatar_url.clone(),
            status: "offline".to_string(),
        },
        content: message.content,
        encrypted: false,
        attachments: vec![],
        reply_to: message.reply_to,
        parent_id: message.parent_id,
        thread_reply_count: message.thread_reply_count,
        thread_last_reply_at: message.thread_last_reply_at,
        edited_at: message.edited_at,
        created_at: message.created_at,
        // Webhooks never trigger mention notifications (@everyone included).
        mention_type: None,
        reactions: None,
        thread_info: None,
        pinned: false,
        message_type: message.message_type,
        nonce: None,
        embeds: message
            .embeds
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok()),
        components: None,
        webhook_id: message.webhook_id,
    }
}

/// Per-webhook execute rate limit (Discord parity: 5/2s), enforced in-handler
/// so the 429 body carries Discord's float `retry_after`.
async fn check_execute_rate_limit(
    state: &AppState,
    webhook_id: Uuid,
) -> Result<(), IncomingWebhookError> {
    let Some(rate_limiter) = &state.rate_limiter else {
        return Ok(());
    };
    let identifier = format!("incoming_webhook:{webhook_id}");
    match rate_limiter
        .check(RateLimitCategory::WebhookExecute, &identifier)
        .await
    {
        Ok(result) if !result.allowed => Err(IncomingWebhookError::RateLimited {
            #[allow(clippy::cast_precision_loss)]
            retry_after: result.retry_after.max(1) as f64,
        }),
        // Fail-open on limiter errors (same posture as component interactions).
        _ => Ok(()),
    }
}

/// Guild content filter (skip moderation logging — webhook messages have no
/// author user id to attribute; noted as a follow-up).
async fn check_content_filter(
    state: &AppState,
    guild_id: Uuid,
    content: &str,
) -> Result<(), IncomingWebhookError> {
    if content.is_empty() {
        return Ok(());
    }
    if let Ok(engine) = state.filter_cache.get_or_build(&state.db, guild_id).await {
        if engine.check(content).blocked {
            return Err(IncomingWebhookError::ContentFiltered);
        }
    }
    Ok(())
}

/// Validate + adapt an execute body into (content, embeds JSON).
fn prepare_payload(
    content: Option<&str>,
    embeds: Option<Vec<super::types::DiscordEmbedIn>>,
) -> Result<(String, Option<serde_json::Value>), IncomingWebhookError> {
    let content = content.map(str::trim).unwrap_or_default();
    if !content.is_empty() {
        crate::chat::types::validate_message_content(content)
            .map_err(|e| IncomingWebhookError::Validation(e.to_string()))?;
    }

    let embeds = match embeds {
        Some(wire) if !wire.is_empty() => {
            let adapted =
                adapt_embeds(wire).map_err(|e| IncomingWebhookError::Validation(e.to_string()))?;
            if adapted.is_empty() {
                None
            } else {
                Some(serde_json::to_value(&adapted).unwrap_or(serde_json::Value::Null))
            }
        }
        _ => None,
    };

    if content.is_empty() && embeds.is_none() {
        return Err(IncomingWebhookError::EmptyMessage);
    }
    Ok((content.to_string(), embeds))
}

/// Shared send pipeline used by the Discord and Slack execute routes.
pub(super) async fn send_webhook_message(
    state: &AppState,
    webhook: &IncomingWebhook,
    body: ExecuteWebhookBody,
    thread_id: Option<Uuid>,
) -> Result<MessageResponse, IncomingWebhookError> {
    check_execute_rate_limit(state, webhook.id).await?;

    let (content, embeds) = prepare_payload(body.content.as_deref(), body.embeds)?;
    check_content_filter(state, webhook.guild_id, &content).await?;

    let (username, avatar) = effective_identity(
        webhook,
        body.username.as_deref(),
        body.avatar_url.as_deref(),
    )?;

    let channel = db::find_channel_by_id(&state.db, webhook.channel_id)
        .await?
        .ok_or(IncomingWebhookError::UnknownChannel)?;

    let params = WebhookMessageParams {
        channel_id: webhook.channel_id,
        webhook_id: webhook.id,
        username: &username,
        avatar_url: avatar.as_deref(),
        content: &content,
        embeds: embeds.as_ref(),
    };

    // Route by channel type / thread targeting.
    if let Some(thread_id) = thread_id {
        let (root_id, locked) =
            queries::resolve_thread_root(&state.db, thread_id, webhook.channel_id)
                .await?
                .ok_or(IncomingWebhookError::UnknownMessage)?;
        if locked {
            return Err(IncomingWebhookError::ThreadLocked);
        }
        let message = queries::create_webhook_thread_reply(&state.db, root_id, params).await?;
        let response = webhook_message_response(message);

        let thread_info = crate::chat::messages::build_thread_info(&state.db, root_id).await;
        if let Err(e) = broadcast_to_channel(
            &state.redis,
            webhook.channel_id,
            &ServerEvent::ThreadReplyNew {
                channel_id: webhook.channel_id,
                parent_id: root_id,
                message: serde_json::to_value(&response).unwrap_or_default(),
                thread_info: serde_json::to_value(&thread_info).unwrap_or_default(),
            },
        )
        .await
        {
            tracing::warn!(error = %e, "Failed to broadcast webhook thread reply");
        }
        return Ok(response);
    }

    match channel.channel_type {
        ChannelType::Forum => {
            let Some(thread_name) = body
                .thread_name
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
            else {
                return Err(IncomingWebhookError::ThreadRequired);
            };
            let title: String = thread_name.chars().take(128).collect();
            let (message, post_id, created_at, last_activity_at) =
                queries::create_webhook_forum_post(&state.db, &title, params).await?;
            let response = webhook_message_response(message);

            // Same event/payload shape as chat::forum::create_post.
            let post_json = serde_json::json!({
                "id": post_id,
                "channel_id": webhook.channel_id,
                "root_message_id": response.id,
                "title": title,
                "author_id": null,
                "pinned": false,
                "locked": false,
                "reply_count": 0,
                "tag_ids": [],
                "created_at": created_at,
                "last_activity_at": last_activity_at,
            });
            if let Err(e) = broadcast_to_channel(
                &state.redis,
                webhook.channel_id,
                &ServerEvent::ForumPostCreated {
                    channel_id: webhook.channel_id,
                    post: post_json,
                },
            )
            .await
            {
                tracing::warn!(error = %e, "Failed to broadcast webhook forum post");
            }
            Ok(response)
        }
        ChannelType::Text | ChannelType::Announcement => {
            let message = queries::create_webhook_message(&state.db, params).await?;
            let response = webhook_message_response(message);

            if let Err(e) = broadcast_to_channel(
                &state.redis,
                webhook.channel_id,
                &ServerEvent::MessageNew {
                    channel_id: webhook.channel_id,
                    message: serde_json::to_value(&response).unwrap_or_default(),
                },
            )
            .await
            {
                tracing::warn!(error = %e, "Failed to broadcast webhook message");
            }

            // Announcement webhooks fan out to follower channels like user
            // publishes do.
            if channel.channel_type == ChannelType::Announcement {
                crate::chat::announcements::spawn_crosspost(
                    state.clone(),
                    webhook.channel_id,
                    response.content.clone(),
                );
            }
            Ok(response)
        }
        // Unreachable: webhook creation rejects Voice/Dm channels.
        ChannelType::Voice | ChannelType::Dm => Err(IncomingWebhookError::Validation(
            "This channel type cannot receive webhook messages".to_string(),
        )),
    }
}

/// `POST /api/webhooks/{webhook_id}/{token}` — Execute Webhook.
#[utoipa::path(
    post,
    path = "/api/webhooks/{webhook_id}/{token}",
    tag = "incoming-webhooks",
    params(
        ("webhook_id" = Uuid, Path, description = "Webhook ID"),
        ("token" = String, Path, description = "Webhook token"),
        ("wait" = Option<bool>, Query, description = "Return the created message (200) instead of 204"),
        ("thread_id" = Option<Uuid>, Query, description = "Post into an existing thread"),
    ),
    request_body = ExecuteWebhookBody,
    responses(
        (status = 204, description = "Message accepted (wait=false)"),
        (status = 200, body = MessageResponse, description = "Created message (wait=true)"),
    ),
)]
#[tracing::instrument(skip(state, token, ip, body))]
pub async fn execute_webhook(
    State(state): State<AppState>,
    Path((webhook_id, token)): Path<(Uuid, String)>,
    Query(query): Query<ExecuteQuery>,
    ip: Option<Extension<NormalizedIp>>,
    Json(body): Json<ExecuteWebhookBody>,
) -> Result<Response, IncomingWebhookError> {
    let webhook = verify_or_record_failure(&state, webhook_id, &token, ip.as_deref()).await?;
    let response = send_webhook_message(&state, &webhook, body, query.thread_id).await?;
    if query.wait {
        Ok((StatusCode::OK, Json(response)).into_response())
    } else {
        Ok(StatusCode::NO_CONTENT.into_response())
    }
}

/// Token verification that feeds invalid attempts into the failed-auth IP
/// blocker (token guessing gets the same treatment as password guessing).
pub(super) async fn verify_or_record_failure(
    state: &AppState,
    webhook_id: Uuid,
    token: &str,
    ip: Option<&NormalizedIp>,
) -> Result<IncomingWebhook, IncomingWebhookError> {
    match super::handlers::find_webhook_with_token(state, webhook_id, token).await {
        Err(e @ IncomingWebhookError::InvalidToken) => {
            if let (Some(rate_limiter), Some(ip)) = (&state.rate_limiter, ip) {
                rate_limiter.record_failed_auth(&ip.0).await.ok();
            }
            Err(e)
        }
        other => other,
    }
}

// ============================================================================
// Webhook message routes (GET/PATCH/DELETE .../messages/{message_id})
// ============================================================================

/// `GET /api/webhooks/{webhook_id}/{token}/messages/{message_id}`.
#[utoipa::path(
    get,
    path = "/api/webhooks/{webhook_id}/{token}/messages/{message_id}",
    tag = "incoming-webhooks",
    params(
        ("webhook_id" = Uuid, Path, description = "Webhook ID"),
        ("token" = String, Path, description = "Webhook token"),
        ("message_id" = Uuid, Path, description = "Message ID"),
    ),
    responses((status = 200, body = MessageResponse)),
)]
#[tracing::instrument(skip(state, token))]
pub async fn get_webhook_message(
    State(state): State<AppState>,
    Path((webhook_id, token, message_id)): Path<(Uuid, String, Uuid)>,
) -> Result<Json<MessageResponse>, IncomingWebhookError> {
    let webhook = super::handlers::find_webhook_with_token(&state, webhook_id, &token).await?;
    let message = queries::find_webhook_message(&state.db, message_id, webhook.id)
        .await?
        .ok_or(IncomingWebhookError::UnknownMessage)?;
    Ok(Json(webhook_message_response(message)))
}

/// `PATCH /api/webhooks/{webhook_id}/{token}/messages/{message_id}`.
#[utoipa::path(
    patch,
    path = "/api/webhooks/{webhook_id}/{token}/messages/{message_id}",
    tag = "incoming-webhooks",
    params(
        ("webhook_id" = Uuid, Path, description = "Webhook ID"),
        ("token" = String, Path, description = "Webhook token"),
        ("message_id" = Uuid, Path, description = "Message ID"),
    ),
    request_body = EditWebhookMessageBody,
    responses((status = 200, body = MessageResponse)),
)]
#[tracing::instrument(skip(state, token, body))]
pub async fn edit_webhook_message(
    State(state): State<AppState>,
    Path((webhook_id, token, message_id)): Path<(Uuid, String, Uuid)>,
    Json(body): Json<EditWebhookMessageBody>,
) -> Result<Json<MessageResponse>, IncomingWebhookError> {
    let webhook = super::handlers::find_webhook_with_token(&state, webhook_id, &token).await?;
    let existing = queries::find_webhook_message(&state.db, message_id, webhook.id)
        .await?
        .ok_or(IncomingWebhookError::UnknownMessage)?;

    let content = match body.content.as_deref().map(str::trim) {
        Some(c) if !c.is_empty() => {
            crate::chat::types::validate_message_content(c)
                .map_err(|e| IncomingWebhookError::Validation(e.to_string()))?;
            check_content_filter(&state, webhook.guild_id, c).await?;
            Some(c.to_string())
        }
        _ => None,
    };
    let embeds = match body.embeds {
        Some(wire) => {
            let adapted =
                adapt_embeds(wire).map_err(|e| IncomingWebhookError::Validation(e.to_string()))?;
            Some(serde_json::to_value(&adapted).unwrap_or(serde_json::Value::Null))
        }
        None => None,
    };
    if content.is_none() && embeds.is_none() {
        return Err(IncomingWebhookError::EmptyMessage);
    }

    let updated = queries::update_webhook_message(
        &state.db,
        existing.id,
        content.as_deref(),
        embeds.as_ref(),
    )
    .await?;
    let response = webhook_message_response(updated);

    if let Err(e) = broadcast_to_channel(
        &state.redis,
        response.channel_id,
        &ServerEvent::MessageEdit {
            channel_id: response.channel_id,
            message_id: response.id,
            content: response.content.clone(),
            edited_at: response
                .edited_at
                .map(|t| t.to_rfc3339())
                .unwrap_or_default(),
        },
    )
    .await
    {
        tracing::warn!(error = %e, "Failed to broadcast webhook message edit");
    }
    Ok(Json(response))
}

/// `DELETE /api/webhooks/{webhook_id}/{token}/messages/{message_id}`.
#[utoipa::path(
    delete,
    path = "/api/webhooks/{webhook_id}/{token}/messages/{message_id}",
    tag = "incoming-webhooks",
    params(
        ("webhook_id" = Uuid, Path, description = "Webhook ID"),
        ("token" = String, Path, description = "Webhook token"),
        ("message_id" = Uuid, Path, description = "Message ID"),
    ),
    responses((status = 204)),
)]
#[tracing::instrument(skip(state, token))]
pub async fn delete_webhook_message(
    State(state): State<AppState>,
    Path((webhook_id, token, message_id)): Path<(Uuid, String, Uuid)>,
) -> Result<StatusCode, IncomingWebhookError> {
    let webhook = super::handlers::find_webhook_with_token(&state, webhook_id, &token).await?;
    let message = queries::find_webhook_message(&state.db, message_id, webhook.id)
        .await?
        .ok_or(IncomingWebhookError::UnknownMessage)?;

    queries::delete_webhook_message(&state.db, message.id).await?;
    if let Some(parent_id) = message.parent_id {
        db::decrement_thread_counters(&state.db, parent_id)
            .await
            .ok();
    }

    if let Err(e) = broadcast_to_channel(
        &state.redis,
        message.channel_id,
        &ServerEvent::MessageDelete {
            channel_id: message.channel_id,
            message_id: message.id,
        },
    )
    .await
    {
        tracing::warn!(error = %e, "Failed to broadcast webhook message delete");
    }
    Ok(StatusCode::NO_CONTENT)
}
