//! Bot Management API
//!
//! Handlers for creating and managing bot applications.

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use thiserror::Error;
use tracing::instrument;
use uuid::Uuid;

use crate::auth::Claims;

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
#[derive(Debug, Deserialize)]
pub struct CreateApplicationRequest {
    /// Application name (2-100 characters).
    pub name: String,
    /// Optional description (max 1000 characters).
    pub description: Option<String>,
}

/// Response for bot application.
#[derive(Debug, Serialize)]
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
    /// When the application was created.
    pub created_at: String,
}

/// Response for bot token (only returned once on creation/reset).
#[derive(Debug, Serialize)]
pub struct BotTokenResponse {
    /// The bot token (only shown once).
    pub token: String,
    /// Associated bot user ID.
    pub bot_user_id: Uuid,
}

/// Create a new bot application.
#[instrument(skip(pool, claims))]
pub async fn create_application(
    State(pool): State<PgPool>,
    claims: Claims,
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

    let app = sqlx::query!(
        r#"
        INSERT INTO bot_applications (owner_id, name, description)
        VALUES ($1, $2, $3)
        RETURNING id, name, description, bot_user_id, public, created_at
        "#,
        claims.sub,
        req.name,
        req.description
    )
    .fetch_one(&pool)
    .await
    .map_err(BotError::Database)?;

    Ok((
        StatusCode::CREATED,
        Json(ApplicationResponse {
            id: app.id,
            name: app.name,
            description: app.description,
            bot_user_id: app.bot_user_id,
            public: app.public,
            created_at: app.created_at.to_rfc3339(),
        }),
    ))
}

/// List all applications owned by the current user.
#[instrument(skip(pool, claims))]
pub async fn list_applications(
    State(pool): State<PgPool>,
    claims: Claims,
) -> Result<Json<Vec<ApplicationResponse>>, (StatusCode, String)> {
    let apps = sqlx::query!(
        r#"
        SELECT id, name, description, bot_user_id, public, created_at
        FROM bot_applications
        WHERE owner_id = $1
        ORDER BY created_at DESC
        "#,
        claims.sub
    )
    .fetch_all(&pool)
    .await
    .map_err(BotError::Database)?;

    let response = apps
        .into_iter()
        .map(|app| ApplicationResponse {
            id: app.id,
            name: app.name,
            description: app.description,
            bot_user_id: app.bot_user_id,
            public: app.public,
            created_at: app.created_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(response))
}

/// Get a specific application by ID.
#[instrument(skip(pool, claims))]
pub async fn get_application(
    State(pool): State<PgPool>,
    Path(app_id): Path<Uuid>,
    claims: Claims,
) -> Result<Json<ApplicationResponse>, (StatusCode, String)> {
    let app = sqlx::query!(
        r#"
        SELECT id, name, description, bot_user_id, public, created_at, owner_id
        FROM bot_applications
        WHERE id = $1
        "#,
        app_id
    )
    .fetch_optional(&pool)
    .await
    .map_err(BotError::Database)?
    .ok_or_else(|| BotError::NotFound)?;

    // Check ownership
    if app.owner_id != claims.sub {
        return Err(BotError::Forbidden.into());
    }

    Ok(Json(ApplicationResponse {
        id: app.id,
        name: app.name,
        description: app.description,
        bot_user_id: app.bot_user_id,
        public: app.public,
        created_at: app.created_at.to_rfc3339(),
    }))
}

/// Create a bot user for an application and generate a token.
#[instrument(skip(pool, claims))]
pub async fn create_bot(
    State(pool): State<PgPool>,
    Path(app_id): Path<Uuid>,
    claims: Claims,
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
    if app.owner_id != claims.sub {
        return Err(BotError::Forbidden.into());
    }

    // Check if bot user already exists (inside transaction to prevent TOCTOU)
    if app.bot_user_id.is_some() {
        return Err(BotError::BotAlreadyCreated.into());
    }

    // Create bot user first to get bot_user_id
    let bot_username = format!("bot-{}", app.name.to_lowercase().replace(' ', "-"));
    let bot_display_name = format!("{} (Bot)", app.name);

    let bot_user = sqlx::query!(
        r#"
        INSERT INTO users (username, display_name, is_bot, bot_owner_id, status)
        VALUES ($1, $2, true, $3, 'offline')
        RETURNING id
        "#,
        bot_username,
        bot_display_name,
        claims.sub
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(BotError::Database)?;

    // Generate token secret
    let token_secret = Uuid::new_v4().to_string();

    // Create full token: "bot_user_id.secret" for indexed authentication
    let token = format!("{}.{}", bot_user.id, token_secret);

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
#[instrument(skip(pool, claims))]
pub async fn reset_bot_token(
    State(pool): State<PgPool>,
    Path(app_id): Path<Uuid>,
    claims: Claims,
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
    if app.owner_id != claims.sub {
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
    let token = format!("{}.{}", bot_user_id, token_secret);

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
#[instrument(skip(pool, claims))]
pub async fn delete_application(
    State(pool): State<PgPool>,
    Path(app_id): Path<Uuid>,
    claims: Claims,
) -> Result<StatusCode, (StatusCode, String)> {
    // Check ownership and delete in one query
    let result = sqlx::query!(
        r#"
        DELETE FROM bot_applications
        WHERE id = $1 AND owner_id = $2
        "#,
        app_id,
        claims.sub
    )
    .execute(&pool)
    .await
    .map_err(BotError::Database)?;

    if result.rows_affected() == 0 {
        return Err(BotError::NotFound.into());
    }

    Ok(StatusCode::NO_CONTENT)
}
