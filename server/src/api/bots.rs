//! Bot Management API
//!
//! Handlers for creating and managing bot applications.

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHasher, SaltString};
use argon2::Argon2;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use thiserror::Error;
use tracing::instrument;
use uuid::Uuid;

use crate::auth::AuthUser;

/// Database row for bot application queries (used with `sqlx::query_as`).
#[derive(sqlx::FromRow)]
struct ApplicationRow {
    id: Uuid,
    name: String,
    description: Option<String>,
    bot_user_id: Option<Uuid>,
    public: bool,
    gateway_intents: Vec<String>,
    created_at: DateTime<Utc>,
}

impl From<ApplicationRow> for ApplicationResponse {
    fn from(r: ApplicationRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            description: r.description,
            bot_user_id: r.bot_user_id,
            public: r.public,
            gateway_intents: r.gateway_intents,
            created_at: r.created_at.to_rfc3339(),
        }
    }
}

/// Errors that can occur during bot operations.
#[derive(Error, Debug)]
pub enum BotError {
    /// Database error.
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    /// Application not found.
    #[error("Application not found")]
    NotFound,
    /// User does not own this application.
    #[error("Forbidden: you don't own this application")]
    Forbidden,
    /// Bot user already created for this application.
    #[error("Bot user already exists for this application")]
    BotAlreadyCreated,
    /// Invalid application name.
    #[error("Application name must be between 2 and 100 characters")]
    InvalidName,
}

impl From<BotError> for (StatusCode, String) {
    fn from(err: BotError) -> Self {
        match err {
            BotError::Database(e) => {
                tracing::error!("Database error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            BotError::NotFound => (StatusCode::NOT_FOUND, err.to_string()),
            BotError::Forbidden => (StatusCode::FORBIDDEN, err.to_string()),
            BotError::BotAlreadyCreated => (StatusCode::CONFLICT, err.to_string()),
            BotError::InvalidName => (StatusCode::BAD_REQUEST, err.to_string()),
        }
    }
}

/// Request body for creating a bot application.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateApplicationRequest {
    /// Application name (2-100 characters).
    pub name: String,
    /// Optional description (max 1000 characters).
    pub description: Option<String>,
}

/// Response for bot application.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ApplicationResponse {
    /// Application ID.
    pub id: Uuid,
    /// Application name.
    pub name: String,
    /// Optional description.
    pub description: Option<String>,
    /// Associated bot user ID (if bot has been created).
    pub bot_user_id: Option<Uuid>,
    /// Whether the bot is publicly listed.
    pub public: bool,
    /// Gateway intents for event filtering.
    pub gateway_intents: Vec<String>,
    /// When the application was created.
    pub created_at: String,
}

/// Response for bot token (only returned once on creation/reset).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct BotTokenResponse {
    /// The bot token (only shown once).
    pub token: String,
    /// Associated bot user ID.
    pub bot_user_id: Uuid,
}

/// Create a new bot application.
#[utoipa::path(
    post,
    path = "/api/applications",
    tag = "bots",
    request_body = CreateApplicationRequest,
    responses(
        (status = 201, body = ApplicationResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[instrument(skip(pool, claims))]
pub async fn create_application(
    State(pool): State<PgPool>,
    claims: AuthUser,
    Json(req): Json<CreateApplicationRequest>,
) -> Result<(StatusCode, Json<ApplicationResponse>), (StatusCode, String)> {
    // Validate name length
    if req.name.len() < 2 || req.name.len() > 100 {
        return Err(BotError::InvalidName.into());
    }

    // Validate description length
    if let Some(ref desc) = req.description {
        if desc.len() > 1000 {
            return Err((
                StatusCode::BAD_REQUEST,
                "Description must be max 1000 characters".to_string(),
            ));
        }
    }

    let app: ApplicationRow = sqlx::query_as(
        r"
        INSERT INTO bot_applications (owner_id, name, description)
        VALUES ($1, $2, $3)
        RETURNING id, name, description, bot_user_id, public, gateway_intents, created_at
        ",
    )
    .bind(claims.id)
    .bind(&req.name)
    .bind(&req.description)
    .fetch_one(&pool)
    .await
    .map_err(BotError::Database)?;

    Ok((StatusCode::CREATED, Json(app.into())))
}

/// List all applications owned by the current user.
#[utoipa::path(
    get,
    path = "/api/applications",
    tag = "bots",
    responses(
        (status = 200, body = Vec<ApplicationResponse>),
    ),
    security(("bearer_auth" = [])),
)]
#[instrument(skip(pool, claims))]
pub async fn list_applications(
    State(pool): State<PgPool>,
    claims: AuthUser,
) -> Result<Json<Vec<ApplicationResponse>>, (StatusCode, String)> {
    let apps: Vec<ApplicationRow> = sqlx::query_as(
        r"
        SELECT id, name, description, bot_user_id, public, gateway_intents, created_at
        FROM bot_applications
        WHERE owner_id = $1
        ORDER BY created_at DESC
        ",
    )
    .bind(claims.id)
    .fetch_all(&pool)
    .await
    .map_err(BotError::Database)?;

    Ok(Json(apps.into_iter().map(Into::into).collect()))
}

/// Get a specific application by ID.
#[utoipa::path(
    get,
    path = "/api/applications/{id}",
    tag = "bots",
    params(
        ("id" = Uuid, Path, description = "Application ID"),
    ),
    responses(
        (status = 200, body = ApplicationResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[instrument(skip(pool, claims))]
pub async fn get_application(
    State(pool): State<PgPool>,
    Path(app_id): Path<Uuid>,
    claims: AuthUser,
) -> Result<Json<ApplicationResponse>, (StatusCode, String)> {
    #[derive(sqlx::FromRow)]
    struct AppWithOwner {
        id: Uuid,
        name: String,
        description: Option<String>,
        bot_user_id: Option<Uuid>,
        public: bool,
        gateway_intents: Vec<String>,
        created_at: DateTime<Utc>,
        owner_id: Uuid,
    }

    let app: AppWithOwner = sqlx::query_as(
        r"
        SELECT id, name, description, bot_user_id, public, gateway_intents, created_at, owner_id
        FROM bot_applications
        WHERE id = $1
        ",
    )
    .bind(app_id)
    .fetch_optional(&pool)
    .await
    .map_err(BotError::Database)?
    .ok_or_else(|| BotError::NotFound)?;

    // Check ownership
    if app.owner_id != claims.id {
        return Err(BotError::Forbidden.into());
    }

    Ok(Json(ApplicationResponse {
        id: app.id,
        name: app.name,
        description: app.description,
        bot_user_id: app.bot_user_id,
        public: app.public,
        gateway_intents: app.gateway_intents,
        created_at: app.created_at.to_rfc3339(),
    }))
}

/// Create a bot user for an application and generate a token.
#[utoipa::path(
    post,
    path = "/api/applications/{id}/bot",
    tag = "bots",
    params(
        ("id" = Uuid, Path, description = "Application ID"),
    ),
    responses(
        (status = 201, body = BotTokenResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[instrument(skip(pool, claims))]
pub async fn create_bot(
    State(pool): State<PgPool>,
    Path(app_id): Path<Uuid>,
    claims: AuthUser,
) -> Result<(StatusCode, Json<BotTokenResponse>), (StatusCode, String)> {
    // Start a transaction to prevent race conditions
    let mut tx = pool.begin().await.map_err(BotError::Database)?;

    // Check if application exists, user owns it, and bot doesn't exist yet (within transaction)
    let app = sqlx::query!(
        r#"
        SELECT id, name, bot_user_id, owner_id
        FROM bot_applications
        WHERE id = $1
        FOR UPDATE
        "#,
        app_id
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(BotError::Database)?
    .ok_or_else(|| BotError::NotFound)?;

    // Check ownership
    if app.owner_id != claims.id {
        return Err(BotError::Forbidden.into());
    }

    // Check if bot user already exists (inside transaction to prevent TOCTOU)
    if app.bot_user_id.is_some() {
        return Err(BotError::BotAlreadyCreated.into());
    }

    // Create bot user first to get bot_user_id
    let bot_username = format!("bot_{}", &app.id.simple().to_string()[..12]);
    let bot_display_name = format!("{} (Bot)", app.name);

    let bot_user = sqlx::query!(
        r#"
        INSERT INTO users (username, display_name, password_hash, is_bot, bot_owner_id, status)
        VALUES ($1, $2, $3, true, $4, 'offline')
        RETURNING id
        "#,
        bot_username,
        bot_display_name,
        "bot_token_only",
        claims.id
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(BotError::Database)?;

    // Generate token secret
    let token_secret = Uuid::new_v4().to_string();

    // Create full token: "bot_user_id.secret" for indexed authentication
    let token = format!("{}.{token_secret}", bot_user.id);

    // Hash the full token using Argon2id with proper CSPRNG salt
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let token_hash = argon2
        .hash_password(token.as_bytes(), &salt)
        .map_err(|e| {
            tracing::error!("Failed to hash bot token: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to hash token".to_string(),
            )
        })?
        .to_string();

    // Update application with bot_user_id and token_hash
    sqlx::query!(
        r#"
        UPDATE bot_applications
        SET bot_user_id = $1, token_hash = $2, updated_at = NOW()
        WHERE id = $3
        "#,
        bot_user.id,
        token_hash,
        app_id
    )
    .execute(&mut *tx)
    .await
    .map_err(BotError::Database)?;

    tx.commit().await.map_err(BotError::Database)?;

    Ok((
        StatusCode::CREATED,
        Json(BotTokenResponse {
            token,
            bot_user_id: bot_user.id,
        }),
    ))
}

/// Reset the bot token for an application.
#[utoipa::path(
    post,
    path = "/api/applications/{id}/reset-token",
    tag = "bots",
    params(
        ("id" = Uuid, Path, description = "Application ID"),
    ),
    responses(
        (status = 200, body = BotTokenResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[instrument(skip(pool, claims))]
pub async fn reset_bot_token(
    State(pool): State<PgPool>,
    Path(app_id): Path<Uuid>,
    claims: AuthUser,
) -> Result<Json<BotTokenResponse>, (StatusCode, String)> {
    // Start transaction to prevent race conditions
    let mut tx = pool.begin().await.map_err(BotError::Database)?;

    // Check if application exists and user owns it (with lock to prevent TOCTOU)
    let app = sqlx::query!(
        r#"
        SELECT id, bot_user_id, owner_id
        FROM bot_applications
        WHERE id = $1
        FOR UPDATE
        "#,
        app_id
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(BotError::Database)?
    .ok_or_else(|| BotError::NotFound)?;

    // Check ownership
    if app.owner_id != claims.id {
        return Err(BotError::Forbidden.into());
    }

    // Check if bot user exists
    let bot_user_id = app.bot_user_id.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "Bot user not created yet".to_string(),
        )
    })?;

    // Generate token secret
    let token_secret = Uuid::new_v4().to_string();

    // Create full token: "bot_user_id.secret" for indexed authentication
    let token = format!("{bot_user_id}.{token_secret}");

    // Hash the token with proper CSPRNG salt
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let token_hash = argon2
        .hash_password(token.as_bytes(), &salt)
        .map_err(|e| {
            tracing::error!("Failed to hash bot token: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to hash token".to_string(),
            )
        })?
        .to_string();

    // Update token_hash within transaction
    sqlx::query!(
        r#"
        UPDATE bot_applications
        SET token_hash = $1, updated_at = NOW()
        WHERE id = $2
        "#,
        token_hash,
        app_id
    )
    .execute(&mut *tx)
    .await
    .map_err(BotError::Database)?;

    tx.commit().await.map_err(BotError::Database)?;

    Ok(Json(BotTokenResponse { token, bot_user_id }))
}

/// Delete a bot application.
#[utoipa::path(
    delete,
    path = "/api/applications/{id}",
    tag = "bots",
    params(
        ("id" = Uuid, Path, description = "Application ID"),
    ),
    responses(
        (status = 204, description = "Application deleted"),
    ),
    security(("bearer_auth" = [])),
)]
#[instrument(skip(pool, claims))]
pub async fn delete_application(
    State(pool): State<PgPool>,
    Path(app_id): Path<Uuid>,
    claims: AuthUser,
) -> Result<StatusCode, (StatusCode, String)> {
    // Check ownership and delete in one query
    let result = sqlx::query!(
        r#"
        DELETE FROM bot_applications
        WHERE id = $1 AND owner_id = $2
        "#,
        app_id,
        claims.id
    )
    .execute(&pool)
    .await
    .map_err(BotError::Database)?;

    if result.rows_affected() == 0 {
        return Err(BotError::NotFound.into());
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Request to update gateway intents.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateIntentsRequest {
    /// List of intent names (e.g., `["messages", "members", "commands"]`).
    pub intents: Vec<String>,
}

/// Update gateway intents for an application.
/// PUT /api/applications/{id}/intents
#[utoipa::path(
    put,
    path = "/api/applications/{id}/intents",
    tag = "bots",
    params(
        ("id" = Uuid, Path, description = "Application ID"),
    ),
    request_body = UpdateIntentsRequest,
    responses(
        (status = 200, description = "Gateway intents updated"),
    ),
    security(("bearer_auth" = [])),
)]
#[instrument(skip(pool, claims))]
pub async fn update_gateway_intents(
    State(pool): State<PgPool>,
    Path(app_id): Path<Uuid>,
    claims: AuthUser,
    Json(req): Json<UpdateIntentsRequest>,
) -> Result<Json<ApplicationResponse>, (StatusCode, String)> {
    // Validate intent names
    for intent in &req.intents {
        if !crate::webhooks::events::GatewayIntent::ALL.contains(&intent.as_str()) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "Invalid intent: '{}'. Valid intents: {}",
                    intent,
                    crate::webhooks::events::GatewayIntent::ALL.join(", ")
                ),
            ));
        }
    }

    // Check ownership
    let row: Option<(Uuid,)> =
        sqlx::query_as("SELECT owner_id FROM bot_applications WHERE id = $1")
            .bind(app_id)
            .fetch_optional(&pool)
            .await
            .map_err(BotError::Database)?;

    let (owner_id,) = row.ok_or_else(|| BotError::NotFound)?;
    if owner_id != claims.id {
        return Err(BotError::Forbidden.into());
    }

    // Update intents
    let updated: ApplicationRow = sqlx::query_as(
        r"
        UPDATE bot_applications
        SET gateway_intents = $1, updated_at = NOW()
        WHERE id = $2
        RETURNING id, name, description, bot_user_id, public, gateway_intents, created_at
        ",
    )
    .bind(&req.intents)
    .bind(app_id)
    .fetch_one(&pool)
    .await
    .map_err(BotError::Database)?;

    Ok(Json(updated.into()))
}
