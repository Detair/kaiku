//! HTTP handlers for bot application management.

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHasher, SaltString};
use argon2::Argon2;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tracing::instrument;
use uuid::Uuid;

use super::error::BotError;
use super::types::{
    ApplicationResponse, ApplicationRow, BotTokenResponse, CreateApplicationRequest,
    UpdateIntentsRequest,
};
use crate::auth::AuthUser;

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
