//! Scheduled guild events + RSVPs.
//!
//! An organizer (`MANAGE_EVENTS`) schedules an event (voice-channel or external);
//! members RSVP interested/going. A background task (`scheduler`) transitions
//! status and fires reminders. Times are stored UTC; the client localizes.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;
use validator::Validate;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::permissions::{require_guild_permission, GuildPermissions, PermissionError};

pub mod scheduler;

// ============================================================================
// Errors
// ============================================================================

#[derive(Debug, Error)]
pub enum EventError {
    #[error("Event not found")]
    NotFound,
    #[error("Not a member of this guild")]
    NotMember,
    #[error("Start time must be in the future")]
    StartInPast,
    #[error("End time must be after the start time")]
    EndBeforeStart,
    #[error("Invalid RSVP response")]
    InvalidRsvp,
    #[error("{0}")]
    Permission(#[from] PermissionError),
    #[error("Validation failed: {0}")]
    Validation(String),
    #[error("Database error")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for EventError {
    fn into_response(self) -> Response {
        if let Self::Database(e) = &self {
            tracing::error!(error = %e, "Guild event database operation failed");
        }
        let (status, code, msg) = match &self {
            Self::NotFound => (StatusCode::NOT_FOUND, "EVENT_NOT_FOUND", self.to_string()),
            Self::NotMember => (StatusCode::FORBIDDEN, "NOT_MEMBER", self.to_string()),
            Self::StartInPast => (StatusCode::BAD_REQUEST, "START_IN_PAST", self.to_string()),
            Self::EndBeforeStart => (
                StatusCode::BAD_REQUEST,
                "END_BEFORE_START",
                self.to_string(),
            ),
            Self::InvalidRsvp => (StatusCode::BAD_REQUEST, "INVALID_RSVP", self.to_string()),
            Self::Permission(_) => (StatusCode::FORBIDDEN, "PERMISSION_DENIED", self.to_string()),
            Self::Validation(_) => (
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                self.to_string(),
            ),
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "Database error".to_string(),
            ),
        };
        (
            status,
            Json(serde_json::json!({ "error": code, "message": msg })),
        )
            .into_response()
    }
}

fn map_perm(e: PermissionError) -> EventError {
    match e {
        PermissionError::NotGuildMember => EventError::NotMember,
        other => EventError::Permission(other),
    }
}

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct CreateEventRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    #[validate(length(max = 1000))]
    pub description: Option<String>,
    pub channel_id: Option<Uuid>,
    #[validate(length(max = 512))]
    pub location: Option<String>,
    pub starts_at: DateTime<Utc>,
    pub ends_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct UpdateEventRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: Option<String>,
    #[validate(length(max = 1000))]
    pub description: Option<String>,
    #[validate(length(max = 512))]
    pub location: Option<String>,
    pub starts_at: Option<DateTime<Utc>>,
    pub ends_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RsvpRequest {
    pub response: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct EventResponse {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub channel_id: Option<Uuid>,
    pub name: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub starts_at: DateTime<Utc>,
    pub ends_at: Option<DateTime<Utc>>,
    pub status: String,
    pub created_by: Option<Uuid>,
    pub interested_count: i64,
    pub going_count: i64,
    pub my_response: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListEventsQuery {
    #[serde(default)]
    pub scope: Option<String>,
}

// ============================================================================
// Helpers
// ============================================================================

/// A `guild_events` row + aggregate RSVP counts + the caller's response.
type EventRow = (
    Uuid,
    Uuid,
    Option<Uuid>,
    String,
    Option<String>,
    Option<String>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
    String,
    Option<Uuid>,
    i64,
    i64,
    Option<String>,
);

fn to_response(r: EventRow) -> EventResponse {
    EventResponse {
        id: r.0,
        guild_id: r.1,
        channel_id: r.2,
        name: r.3,
        description: r.4,
        location: r.5,
        starts_at: r.6,
        ends_at: r.7,
        status: r.8,
        created_by: r.9,
        interested_count: r.10,
        going_count: r.11,
        my_response: r.12,
    }
}

async fn load_event(
    state: &AppState,
    event_id: Uuid,
    user_id: Uuid,
) -> Result<EventResponse, EventError> {
    // $1 = event_id, $2 = viewer (for my_response).
    let row: Option<EventRow> = sqlx::query_as(
        "SELECT e.id, e.guild_id, e.channel_id, e.name, e.description, e.location,
                e.starts_at, e.ends_at, e.status, e.created_by,
                COUNT(r.user_id) FILTER (WHERE r.response = 'interested') AS interested_count,
                COUNT(r.user_id) FILTER (WHERE r.response = 'going') AS going_count,
                MAX(r.response) FILTER (WHERE r.user_id = $2) AS my_response
         FROM guild_events e
         LEFT JOIN guild_event_rsvps r ON r.event_id = e.id
         WHERE e.id = $1 GROUP BY e.id",
    )
    .bind(event_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;
    row.map(to_response).ok_or(EventError::NotFound)
}

async fn broadcast_event(state: &AppState, guild_id: Uuid, event: &EventResponse, kind: &str) {
    let payload = serde_json::to_value(event).unwrap_or_default();
    let ev = match kind {
        "created" => crate::ws::ServerEvent::GuildEventCreated {
            guild_id,
            event: payload,
        },
        "cancelled" => crate::ws::ServerEvent::GuildEventCancelled {
            guild_id,
            event_id: event.id,
        },
        _ => crate::ws::ServerEvent::GuildEventUpdated {
            guild_id,
            event: payload,
        },
    };
    if let Err(e) = crate::ws::broadcast_to_guild(&state.redis, guild_id, &ev).await {
        tracing::warn!("Failed to broadcast guild event {kind}: {e}");
    }
}

// ============================================================================
// Handlers
// ============================================================================

/// `GET /api/guilds/{id}/events` — upcoming (or `?scope=past`).
#[tracing::instrument(skip(state))]
pub async fn list_events(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Query(q): Query<ListEventsQuery>,
) -> Result<Json<Vec<EventResponse>>, EventError> {
    require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::empty())
        .await
        .map_err(map_perm)?;

    // Two fixed queries (no interpolation) — past history vs upcoming.
    // $1 = guild_id, $2 = viewer.
    const UPCOMING: &str =
        "SELECT e.id, e.guild_id, e.channel_id, e.name, e.description, e.location,
                e.starts_at, e.ends_at, e.status, e.created_by,
                COUNT(r.user_id) FILTER (WHERE r.response = 'interested') AS interested_count,
                COUNT(r.user_id) FILTER (WHERE r.response = 'going') AS going_count,
                MAX(r.response) FILTER (WHERE r.user_id = $2) AS my_response
         FROM guild_events e LEFT JOIN guild_event_rsvps r ON r.event_id = e.id
         WHERE e.guild_id = $1 AND e.status IN ('scheduled', 'active')
         GROUP BY e.id ORDER BY e.starts_at ASC LIMIT 100";
    const PAST: &str = "SELECT e.id, e.guild_id, e.channel_id, e.name, e.description, e.location,
                e.starts_at, e.ends_at, e.status, e.created_by,
                COUNT(r.user_id) FILTER (WHERE r.response = 'interested') AS interested_count,
                COUNT(r.user_id) FILTER (WHERE r.response = 'going') AS going_count,
                MAX(r.response) FILTER (WHERE r.user_id = $2) AS my_response
         FROM guild_events e LEFT JOIN guild_event_rsvps r ON r.event_id = e.id
         WHERE e.guild_id = $1 AND e.status IN ('completed', 'cancelled')
         GROUP BY e.id ORDER BY e.starts_at DESC LIMIT 100";
    let sql = if q.scope.as_deref() == Some("past") {
        PAST
    } else {
        UPCOMING
    };
    let rows: Vec<EventRow> = sqlx::query_as(sql)
        .bind(guild_id)
        .bind(auth.id)
        .fetch_all(&state.db)
        .await?;
    Ok(Json(rows.into_iter().map(to_response).collect()))
}

/// `POST /api/guilds/{id}/events` — schedule an event (`MANAGE_EVENTS`).
#[tracing::instrument(skip(state, body))]
pub async fn create_event(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<CreateEventRequest>,
) -> Result<(StatusCode, Json<EventResponse>), EventError> {
    body.validate()
        .map_err(|e| EventError::Validation(crate::validation::format_validation_errors(&e)))?;
    require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_EVENTS,
    )
    .await
    .map_err(map_perm)?;

    if body.starts_at <= Utc::now() {
        return Err(EventError::StartInPast);
    }
    if let Some(ends) = body.ends_at {
        if ends <= body.starts_at {
            return Err(EventError::EndBeforeStart);
        }
    }

    let id: (Uuid,) = sqlx::query_as(
        "INSERT INTO guild_events
            (guild_id, channel_id, name, description, location, starts_at, ends_at, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
    )
    .bind(guild_id)
    .bind(body.channel_id)
    .bind(&body.name)
    .bind(&body.description)
    .bind(&body.location)
    .bind(body.starts_at)
    .bind(body.ends_at)
    .bind(auth.id)
    .fetch_one(&state.db)
    .await?;

    let event = load_event(&state, id.0, auth.id).await?;
    broadcast_event(&state, guild_id, &event, "created").await;
    Ok((StatusCode::CREATED, Json(event)))
}

/// `PATCH /api/guilds/{id}/events/{event_id}` — edit (`MANAGE_EVENTS`).
#[tracing::instrument(skip(state, body))]
pub async fn update_event(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, event_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateEventRequest>,
) -> Result<Json<EventResponse>, EventError> {
    body.validate()
        .map_err(|e| EventError::Validation(crate::validation::format_validation_errors(&e)))?;
    require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_EVENTS,
    )
    .await
    .map_err(map_perm)?;

    // Validate the resulting start/end ordering against the stored + new values.
    let current = load_event(&state, event_id, auth.id).await?;
    if current.guild_id != guild_id {
        return Err(EventError::NotFound);
    }
    let new_start = body.starts_at.unwrap_or(current.starts_at);
    let new_end = body.ends_at.or(current.ends_at);
    if let Some(end) = new_end {
        if end <= new_start {
            return Err(EventError::EndBeforeStart);
        }
    }

    sqlx::query(
        "UPDATE guild_events SET
            name = COALESCE($2, name),
            description = COALESCE($3, description),
            location = COALESCE($4, location),
            starts_at = COALESCE($5, starts_at),
            ends_at = COALESCE($6, ends_at)
         WHERE id = $1 AND guild_id = $7",
    )
    .bind(event_id)
    .bind(&body.name)
    .bind(&body.description)
    .bind(&body.location)
    .bind(body.starts_at)
    .bind(body.ends_at)
    .bind(guild_id)
    .execute(&state.db)
    .await?;

    let event = load_event(&state, event_id, auth.id).await?;
    broadcast_event(&state, guild_id, &event, "updated").await;
    Ok(Json(event))
}

/// `DELETE /api/guilds/{id}/events/{event_id}` — cancel (`MANAGE_EVENTS`).
#[tracing::instrument(skip(state))]
pub async fn delete_event(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, event_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, EventError> {
    require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_EVENTS,
    )
    .await
    .map_err(map_perm)?;

    let affected =
        sqlx::query("UPDATE guild_events SET status = 'cancelled' WHERE id = $1 AND guild_id = $2")
            .bind(event_id)
            .bind(guild_id)
            .execute(&state.db)
            .await?
            .rows_affected();
    if affected == 0 {
        return Err(EventError::NotFound);
    }

    if let Err(e) = crate::ws::broadcast_to_guild(
        &state.redis,
        guild_id,
        &crate::ws::ServerEvent::GuildEventCancelled { guild_id, event_id },
    )
    .await
    {
        tracing::warn!("Failed to broadcast event cancel: {e}");
    }
    Ok(Json(
        serde_json::json!({ "cancelled": true, "id": event_id }),
    ))
}

/// `PUT /api/guilds/{id}/events/{event_id}/rsvp` — set RSVP (upsert).
#[tracing::instrument(skip(state, body))]
pub async fn set_rsvp(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, event_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<RsvpRequest>,
) -> Result<Json<EventResponse>, EventError> {
    require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::empty())
        .await
        .map_err(map_perm)?;
    if body.response != "interested" && body.response != "going" {
        return Err(EventError::InvalidRsvp);
    }

    sqlx::query(
        "INSERT INTO guild_event_rsvps (event_id, user_id, response)
         SELECT $1, $2, $3 WHERE EXISTS (
            SELECT 1 FROM guild_events WHERE id = $1 AND guild_id = $4
         )
         ON CONFLICT (event_id, user_id) DO UPDATE SET response = EXCLUDED.response",
    )
    .bind(event_id)
    .bind(auth.id)
    .bind(&body.response)
    .bind(guild_id)
    .execute(&state.db)
    .await?;

    let event = load_event(&state, event_id, auth.id).await?;
    if event.guild_id != guild_id {
        return Err(EventError::NotFound);
    }
    broadcast_event(&state, guild_id, &event, "updated").await;
    Ok(Json(event))
}

/// `DELETE /api/guilds/{id}/events/{event_id}/rsvp` — clear RSVP.
#[tracing::instrument(skip(state))]
pub async fn clear_rsvp(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, event_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<EventResponse>, EventError> {
    require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::empty())
        .await
        .map_err(map_perm)?;

    sqlx::query("DELETE FROM guild_event_rsvps WHERE event_id = $1 AND user_id = $2")
        .bind(event_id)
        .bind(auth.id)
        .execute(&state.db)
        .await?;

    let event = load_event(&state, event_id, auth.id).await?;
    broadcast_event(&state, guild_id, &event, "updated").await;
    Ok(Json(event))
}
