//! The security log (contract C28): `GET /api/me/security-events` and the
//! elevated admin view `GET /api/admin/security-events`. There is no
//! delete route.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use super::queries::{page, EventRow};
use super::types::{Action, SecurityEvent};
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::chat::types::CursorPaginatedResponse;

/// Default and maximum page size.
pub const DEFAULT_LIMIT: i64 = 50;
pub const MAX_LIMIT: i64 = 100;

/// Errors of the security log routes.
#[derive(Debug, thiserror::Error)]
pub enum SecurityEventsError {
    #[error("{0}")]
    Validation(String),
    #[error("Database error")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for SecurityEventsError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            Self::Validation(m) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", m.clone()),
            Self::Database(e) => {
                tracing::error!(error = %e, "security log query failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "Internal server error".to_string(),
                )
            }
        };
        (
            status,
            Json(serde_json::json!({ "error": code, "message": message })),
        )
            .into_response()
    }
}

/// Raw query parameters, parsed by hand so bad values answer
/// `VALIDATION_ERROR` in the usual JSON body.
#[derive(Debug, Default, Deserialize, utoipa::IntoParams)]
pub struct LogQuery {
    /// Id of the last event of the previous page.
    pub before: Option<String>,
    /// 1..100, default 50.
    pub limit: Option<String>,
    /// Admin log only: one account.
    pub user_id: Option<String>,
}

fn parse_uuid(raw: Option<&str>, field: &str) -> Result<Option<Uuid>, SecurityEventsError> {
    raw.map(|s| {
        Uuid::parse_str(s)
            .map_err(|_| SecurityEventsError::Validation(format!("{field} must be a UUID")))
    })
    .transpose()
}

fn parse_limit(raw: Option<&str>) -> Result<i64, SecurityEventsError> {
    match raw {
        None => Ok(DEFAULT_LIMIT),
        Some(s) => match s.parse::<i64>() {
            Ok(n) if (1..=MAX_LIMIT).contains(&n) => Ok(n),
            _ => Err(SecurityEventsError::Validation(format!(
                "limit must be between 1 and {MAX_LIMIT}"
            ))),
        },
    }
}

/// Actions the reader can take on an event (C28: computed per reader, only
/// where usable). L1a has none; L1b Tasks 21 and 24 fill this.
pub async fn actions_for(
    _conn: &mut sqlx::PgConnection,
    _reader: &AuthUser,
    _reader_is_admin: bool,
    _ev: &SecurityEvent,
) -> sqlx::Result<Vec<Action>> {
    Ok(Vec::new())
}

async fn render(
    state: &AppState,
    reader: &AuthUser,
    reader_is_admin: bool,
    subject: Option<Uuid>,
    before: Option<Uuid>,
    limit: i64,
) -> Result<CursorPaginatedResponse<SecurityEvent>, SecurityEventsError> {
    let mut conn = state.db.acquire().await?;
    let mut rows: Vec<EventRow> = page(&mut conn, subject, before, limit).await?;
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    let has_more = rows.len() > limit;
    rows.truncate(limit);
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        let label = super::notices::authenticator_label(&mut conn, row).await?;
        // The owner's own log and the admin log show IP and place.
        let mut ev = row.to_wire(true, label);
        ev.actions = actions_for(&mut conn, reader, reader_is_admin, &ev).await?;
        items.push(ev);
    }
    let next_cursor = if has_more {
        items.last().map(|e| e.id)
    } else {
        None
    };
    Ok(CursorPaginatedResponse {
        items,
        has_more,
        next_cursor,
    })
}

/// The caller's own security log, newest first (any session).
#[utoipa::path(
    get,
    path = "/api/me/security-events",
    tag = "security",
    params(LogQuery),
    responses((status = 200, body = CursorPaginatedResponse<SecurityEvent>)),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, auth, q), fields(user_id = %auth.id))]
pub async fn list_own(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<LogQuery>,
) -> Result<Json<CursorPaginatedResponse<SecurityEvent>>, SecurityEventsError> {
    let limit = parse_limit(q.limit.as_deref())?;
    let before = parse_uuid(q.before.as_deref(), "before")?;
    Ok(Json(
        render(&state, &auth, false, Some(auth.id), before, limit).await?,
    ))
}

/// Every account's security log, or one account's (`user_id`), newest
/// first. Mounted behind `require_elevated`.
#[utoipa::path(
    get,
    path = "/api/admin/security-events",
    tag = "admin",
    params(LogQuery),
    responses((status = 200, body = CursorPaginatedResponse<SecurityEvent>)),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, auth, q), fields(user_id = %auth.id))]
pub async fn list_admin(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<LogQuery>,
) -> Result<Json<CursorPaginatedResponse<SecurityEvent>>, SecurityEventsError> {
    let limit = parse_limit(q.limit.as_deref())?;
    let before = parse_uuid(q.before.as_deref(), "before")?;
    let subject = parse_uuid(q.user_id.as_deref(), "user_id")?;
    Ok(Json(
        render(&state, &auth, true, subject, before, limit).await?,
    ))
}

/// `/api/me/security-events` (GET only).
pub fn router() -> axum::Router<AppState> {
    axum::Router::new().route("/", axum::routing::get(list_own))
}
