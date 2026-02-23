//! Slash Commands API
//!
//! Handlers for registering and managing slash commands.

use std::collections::HashSet;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use thiserror::Error;
use tracing::instrument;
use uuid::Uuid;

use crate::auth::AuthUser;

/// Errors that can occur during command operations.
#[derive(Error, Debug)]
pub enum CommandError {
    /// Database error.
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    /// Command not found.
    #[error("Command not found")]
    NotFound,
    /// Application not found.
    #[error("Application not found")]
    ApplicationNotFound,
    /// User does not own this application.
    #[error("Forbidden: you don't own this application")]
    Forbidden,
    /// Invalid command name.
    #[error("Command name must be 1-32 characters and alphanumeric with hyphens/underscores")]
    InvalidName,
    /// Invalid description.
    #[error("Command description must be 1-100 characters")]
    InvalidDescription,
    /// Duplicate command name in a single registration batch.
    #[error("Duplicate command name in batch: {0}")]
    DuplicateName(String),
}

impl From<CommandError> for (StatusCode, String) {
    fn from(err: CommandError) -> Self {
        match err {
            CommandError::Database(e) => {
                tracing::error!("Database error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            CommandError::NotFound => (StatusCode::NOT_FOUND, err.to_string()),
            CommandError::ApplicationNotFound => (StatusCode::NOT_FOUND, err.to_string()),
            CommandError::Forbidden => (StatusCode::FORBIDDEN, err.to_string()),
            CommandError::InvalidName => (StatusCode::BAD_REQUEST, err.to_string()),
            CommandError::InvalidDescription => (StatusCode::BAD_REQUEST, err.to_string()),
            CommandError::DuplicateName(_) => (StatusCode::CONFLICT, err.to_string()),
        }
    }
}

/// Command option type.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum CommandOptionType {
    /// String input.
    String,
    /// Integer input.
    Integer,
    /// Boolean input.
    Boolean,
    /// User mention.
    User,
    /// Channel mention.
    Channel,
    /// Role mention.
    Role,
}

/// Command option definition.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CommandOption {
    /// Option name.
    pub name: String,
    /// Option description.
    pub description: String,
    /// Option type.
    #[serde(rename = "type")]
    pub option_type: CommandOptionType,
    /// Whether this option is required.
    pub required: bool,
}

/// Request body for registering commands.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RegisterCommandsRequest {
    /// Commands to register.
    pub commands: Vec<RegisterCommandData>,
}

/// Single command registration data.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RegisterCommandData {
    /// Command name (1-32 characters, alphanumeric with hyphens/underscores).
    pub name: String,
    /// Command description (1-100 characters).
    pub description: String,
    /// Command options/parameters.
    #[serde(default)]
    pub options: Vec<CommandOption>,
}

/// Response for a slash command.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CommandResponse {
    /// Command ID.
    pub id: Uuid,
    /// Application ID.
    pub application_id: Uuid,
    /// Guild ID (null for global commands).
    pub guild_id: Option<Uuid>,
    /// Command name.
    pub name: String,
    /// Command description.
    pub description: String,
    /// Command options.
    pub options: Vec<CommandOption>,
    /// When the command was created.
    pub created_at: String,
}

/// Query parameters for listing commands.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ListCommandsQuery {
    /// Guild ID to filter by (omit for global commands).
    pub guild_id: Option<Uuid>,
}

/// Validate command name.
fn validate_command_name(name: &str) -> Result<(), CommandError> {
    if name.is_empty() || name.len() > 32 {
        return Err(CommandError::InvalidName);
    }
    // Command names must be lowercase alphanumeric with hyphens/underscores
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err(CommandError::InvalidName);
    }
    Ok(())
}

/// Validate command description.
const fn validate_command_description(desc: &str) -> Result<(), CommandError> {
    if desc.is_empty() || desc.len() > 100 {
        return Err(CommandError::InvalidDescription);
    }
    Ok(())
}

/// Register or update slash commands for an application.
/// This will replace all existing commands for the scope (guild or global).
#[utoipa::path(
    put,
    path = "/api/applications/{id}/commands",
    tag = "commands",
    params(
        ("id" = Uuid, Path, description = "Application ID"),
    ),
    request_body = RegisterCommandsRequest,
    responses(
        (status = 200, description = "Commands registered"),
    ),
    security(("bearer_auth" = [])),
)]
#[instrument(skip(pool, claims))]
pub async fn register_commands(
    State(pool): State<PgPool>,
    Path(app_id): Path<Uuid>,
    Query(query): Query<ListCommandsQuery>,
    claims: AuthUser,
    Json(req): Json<RegisterCommandsRequest>,
) -> Result<(StatusCode, Json<Vec<CommandResponse>>), (StatusCode, String)> {
    // Check if application exists and user owns it
    let app = sqlx::query!(
        r#"
        SELECT owner_id
        FROM bot_applications
        WHERE id = $1
        "#,
        app_id
    )
    .fetch_optional(&pool)
    .await
    .map_err(CommandError::Database)?
    .ok_or_else(|| CommandError::ApplicationNotFound)?;

    // Check ownership
    if app.owner_id != claims.id {
        return Err(CommandError::Forbidden.into());
    }

    // Validate all commands
    for cmd in &req.commands {
        validate_command_name(&cmd.name)?;
        validate_command_description(&cmd.description)?;
    }

    // Check for duplicate names within the batch
    let mut seen_names = HashSet::with_capacity(req.commands.len());
    for cmd in &req.commands {
        if !seen_names.insert(&cmd.name) {
            return Err(CommandError::DuplicateName(cmd.name.clone()).into());
        }
    }

    // Start transaction
    let mut tx = pool.begin().await.map_err(CommandError::Database)?;

    // Delete existing commands for this scope
    sqlx::query!(
        r#"
        DELETE FROM slash_commands
        WHERE application_id = $1
          AND (($2::uuid IS NULL AND guild_id IS NULL) OR guild_id = $2)
        "#,
        app_id,
        query.guild_id
    )
    .execute(&mut *tx)
    .await
    .map_err(CommandError::Database)?;

    // Insert new commands
    let mut responses = Vec::new();
    for cmd in req.commands {
        let options_json = serde_json::to_value(&cmd.options).map_err(|e| {
            tracing::error!("Failed to serialize command options: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to serialize options".to_string(),
            )
        })?;

        let result = sqlx::query!(
            r#"
            INSERT INTO slash_commands (application_id, guild_id, name, description, options)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, created_at
            "#,
            app_id,
            query.guild_id,
            cmd.name,
            cmd.description,
            options_json
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(CommandError::Database)?;

        responses.push(CommandResponse {
            id: result.id,
            application_id: app_id,
            guild_id: query.guild_id,
            name: cmd.name,
            description: cmd.description,
            options: cmd.options,
            created_at: result.created_at.to_rfc3339(),
        });
    }

    tx.commit().await.map_err(CommandError::Database)?;

    Ok((StatusCode::OK, Json(responses)))
}

/// List all commands for an application.
#[utoipa::path(
    get,
    path = "/api/applications/{id}/commands",
    tag = "commands",
    params(
        ("id" = Uuid, Path, description = "Application ID"),
    ),
    responses(
        (status = 200, description = "List of commands"),
    ),
    security(("bearer_auth" = [])),
)]
#[instrument(skip(pool, claims))]
pub async fn list_commands(
    State(pool): State<PgPool>,
    Path(app_id): Path<Uuid>,
    Query(query): Query<ListCommandsQuery>,
    claims: AuthUser,
) -> Result<Json<Vec<CommandResponse>>, (StatusCode, String)> {
    // Check if application exists and user owns it
    let app = sqlx::query!(
        r#"
        SELECT owner_id
        FROM bot_applications
        WHERE id = $1
        "#,
        app_id
    )
    .fetch_optional(&pool)
    .await
    .map_err(CommandError::Database)?
    .ok_or_else(|| CommandError::ApplicationNotFound)?;

    // Check ownership
    if app.owner_id != claims.id {
        return Err(CommandError::Forbidden.into());
    }

    // Fetch commands
    let commands = sqlx::query!(
        r#"
        SELECT id, application_id, guild_id, name, description, options, created_at
        FROM slash_commands
        WHERE application_id = $1
          AND (($2::uuid IS NULL AND guild_id IS NULL) OR guild_id = $2)
        ORDER BY name
        "#,
        app_id,
        query.guild_id
    )
    .fetch_all(&pool)
    .await
    .map_err(CommandError::Database)?;

    let responses = commands
        .into_iter()
        .map(|cmd| {
            let options: Vec<CommandOption> = cmd
                .options
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default();

            CommandResponse {
                id: cmd.id,
                application_id: cmd.application_id,
                guild_id: cmd.guild_id,
                name: cmd.name,
                description: cmd.description,
                options,
                created_at: cmd.created_at.to_rfc3339(),
            }
        })
        .collect();

    Ok(Json(responses))
}

/// Delete a specific command.
#[utoipa::path(
    delete,
    path = "/api/applications/{id}/commands/{command_id}",
    tag = "commands",
    params(
        ("id" = Uuid, Path, description = "Application ID"),
        ("command_id" = Uuid, Path, description = "Command ID"),
    ),
    responses(
        (status = 204, description = "Command deleted"),
    ),
    security(("bearer_auth" = [])),
)]
#[instrument(skip(pool, claims))]
pub async fn delete_command(
    State(pool): State<PgPool>,
    Path((app_id, cmd_id)): Path<(Uuid, Uuid)>,
    claims: AuthUser,
) -> Result<StatusCode, (StatusCode, String)> {
    // Check if application exists and user owns it
    let app = sqlx::query!(
        r#"
        SELECT owner_id
        FROM bot_applications
        WHERE id = $1
        "#,
        app_id
    )
    .fetch_optional(&pool)
    .await
    .map_err(CommandError::Database)?
    .ok_or_else(|| CommandError::ApplicationNotFound)?;

    // Check ownership
    if app.owner_id != claims.id {
        return Err(CommandError::Forbidden.into());
    }

    // Delete command
    let result = sqlx::query!(
        r#"
        DELETE FROM slash_commands
        WHERE id = $1 AND application_id = $2
        "#,
        cmd_id,
        app_id
    )
    .execute(&pool)
    .await
    .map_err(CommandError::Database)?;

    if result.rows_affected() == 0 {
        return Err(CommandError::NotFound.into());
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Delete all commands for a scope (guild or global).
#[utoipa::path(
    delete,
    path = "/api/applications/{id}/commands",
    tag = "commands",
    params(
        ("id" = Uuid, Path, description = "Application ID"),
    ),
    responses(
        (status = 204, description = "All commands deleted"),
    ),
    security(("bearer_auth" = [])),
)]
#[instrument(skip(pool, claims))]
pub async fn delete_all_commands(
    State(pool): State<PgPool>,
    Path(app_id): Path<Uuid>,
    Query(query): Query<ListCommandsQuery>,
    claims: AuthUser,
) -> Result<StatusCode, (StatusCode, String)> {
    // Check if application exists and user owns it
    let app = sqlx::query!(
        r#"
        SELECT owner_id
        FROM bot_applications
        WHERE id = $1
        "#,
        app_id
    )
    .fetch_optional(&pool)
    .await
    .map_err(CommandError::Database)?
    .ok_or_else(|| CommandError::ApplicationNotFound)?;

    // Check ownership
    if app.owner_id != claims.id {
        return Err(CommandError::Forbidden.into());
    }

    // Delete all commands for this scope
    sqlx::query!(
        r#"
        DELETE FROM slash_commands
        WHERE application_id = $1
          AND (($2::uuid IS NULL AND guild_id IS NULL) OR guild_id = $2)
        "#,
        app_id,
        query.guild_id
    )
    .execute(&pool)
    .await
    .map_err(CommandError::Database)?;

    Ok(StatusCode::NO_CONTENT)
}
