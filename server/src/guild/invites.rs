//! Guild Invite Handlers

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{Duration, Utc};
use rand::Rng;
use uuid::Uuid;

use super::handlers::GuildError;
use super::types::{CreateInviteRequest, GuildInvite, InviteResponse};
use crate::api::AppState;
use crate::auth::AuthUser;

/// Generate a cryptographically random 8-character invite code
fn generate_invite_code() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..8)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// Parse expiry string to duration
fn parse_expiry(expires_in: &str) -> Option<Duration> {
    match expires_in {
        "30m" => Some(Duration::minutes(30)),
        "1h" => Some(Duration::hours(1)),
        "1d" => Some(Duration::days(1)),
        "7d" => Some(Duration::days(7)),
        "never" => None,
        _ => Some(Duration::days(7)), // Default to 7 days
    }
}

/// List invites for a guild (owner only)
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/invites",
    tag = "invites",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, body = Vec<GuildInvite>)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn list_invites(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<GuildInvite>>, GuildError> {
    // Verify ownership
    let guild = sqlx::query_as::<_, (Uuid,)>("SELECT owner_id FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(GuildError::NotFound)?;

    if guild.0 != auth.id {
        return Err(GuildError::Forbidden);
    }

    // Get active invites (not expired)
    let invites = sqlx::query_as::<_, GuildInvite>(
        r"SELECT id, guild_id, code, created_by, expires_at, use_count, created_at
           FROM guild_invites
           WHERE guild_id = $1 AND (expires_at IS NULL OR expires_at > NOW())
           ORDER BY created_at DESC",
    )
    .bind(guild_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(invites))
}

/// Create a new invite (owner only)
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/invites",
    tag = "invites",
    params(("id" = Uuid, Path, description = "Guild ID")),
    request_body = CreateInviteRequest,
    responses((status = 200, body = GuildInvite)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn create_invite(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<CreateInviteRequest>,
) -> Result<Json<GuildInvite>, GuildError> {
    // Verify ownership
    let guild = sqlx::query_as::<_, (Uuid,)>("SELECT owner_id FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(GuildError::NotFound)?;

    if guild.0 != auth.id {
        return Err(GuildError::Forbidden);
    }

    // Check rate limit (max 10 active invites per guild)
    let active_count: (i64,) = sqlx::query_as(
        r"SELECT COUNT(*) FROM guild_invites
           WHERE guild_id = $1 AND (expires_at IS NULL OR expires_at > NOW())",
    )
    .bind(guild_id)
    .fetch_one(&state.db)
    .await?;

    if active_count.0 >= 10 {
        return Err(GuildError::Validation(
            "Maximum 10 active invites per guild".to_string(),
        ));
    }

    // Generate unique code (retry if collision)
    let mut code = generate_invite_code();
    let mut attempts = 0;
    while attempts < 5 {
        let exists: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM guild_invites WHERE code = $1")
                .bind(&code)
                .fetch_optional(&state.db)
                .await?;
        if exists.is_none() {
            break;
        }
        code = generate_invite_code();
        attempts += 1;
    }

    // Calculate expiry
    let expires_at = parse_expiry(&body.expires_in).map(|d| Utc::now() + d);

    // Insert invite
    let invite = sqlx::query_as::<_, GuildInvite>(
        r"INSERT INTO guild_invites (guild_id, code, created_by, expires_at)
           VALUES ($1, $2, $3, $4)
           RETURNING id, guild_id, code, created_by, expires_at, use_count, created_at",
    )
    .bind(guild_id)
    .bind(&code)
    .bind(auth.id)
    .bind(expires_at)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(invite))
}

/// Delete/revoke an invite (owner only)
#[utoipa::path(
    delete,
    path = "/api/guilds/{id}/invites/{code}",
    tag = "invites",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("code" = String, Path, description = "Invite code")
    ),
    responses((status = 204, description = "Invite deleted")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn delete_invite(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, code)): Path<(Uuid, String)>,
) -> Result<StatusCode, GuildError> {
    // Verify ownership
    let guild = sqlx::query_as::<_, (Uuid,)>("SELECT owner_id FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(GuildError::NotFound)?;

    if guild.0 != auth.id {
        return Err(GuildError::Forbidden);
    }

    // Delete the invite
    let result = sqlx::query("DELETE FROM guild_invites WHERE guild_id = $1 AND code = $2")
        .bind(guild_id)
        .bind(&code)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(GuildError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Join a guild via invite code (any authenticated user)
#[utoipa::path(
    post,
    path = "/api/invites/{code}/join",
    tag = "invites",
    params(("code" = String, Path, description = "Invite code")),
    responses((status = 200, body = InviteResponse)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn join_via_invite(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(code): Path<String>,
) -> Result<Json<InviteResponse>, GuildError> {
    // Find the invite
    let invite = sqlx::query_as::<_, GuildInvite>(
        r"SELECT id, guild_id, code, created_by, expires_at, use_count, created_at
           FROM guild_invites
           WHERE code = $1 AND (expires_at IS NULL OR expires_at > NOW())",
    )
    .bind(&code)
    .fetch_optional(&state.db)
    .await?
    .ok_or(GuildError::Validation(
        "Invalid or expired invite code".to_string(),
    ))?;

    let globally_banned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM global_bans WHERE user_id = $1 AND (expires_at IS NULL OR expires_at > NOW()))",
    )
    .bind(auth.id)
    .fetch_one(&state.db)
    .await?;

    if globally_banned {
        return Err(GuildError::Forbidden);
    }

    let mut tx = state.db.begin().await?;

    // Serialize member joins per guild so limit checks are strict under concurrency.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 53))")
        .bind(invite.guild_id)
        .execute(&mut *tx)
        .await?;

    // Check if already a member
    let is_member: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM guild_members WHERE guild_id = $1 AND user_id = $2)",
    )
    .bind(invite.guild_id)
    .bind(auth.id)
    .fetch_one(&mut *tx)
    .await?;
    if is_member {
        tx.commit().await?;

        // Already a member, just return guild info
        let guild_name: (String,) = sqlx::query_as("SELECT name FROM guilds WHERE id = $1")
            .bind(invite.guild_id)
            .fetch_one(&state.db)
            .await?;

        return Ok(Json(InviteResponse {
            id: invite.id,
            code: invite.code,
            guild_id: invite.guild_id,
            guild_name: guild_name.0,
            expires_at: invite.expires_at,
            use_count: invite.use_count,
            created_at: invite.created_at,
        }));
    }

    // Check member limit (best-effort; concurrent joins may overshoot by a small
    // bounded amount due to TOCTOU, but the denormalized member_count self-corrects).
    let member_count: i64 = sqlx::query_scalar("SELECT member_count FROM guilds WHERE id = $1")
        .bind(invite.guild_id)
        .fetch_one(&mut *tx)
        .await?;
    if member_count >= state.config.max_members_per_guild {
        return Err(GuildError::LimitExceeded(format!(
            "Guild has reached the maximum number of members ({})",
            state.config.max_members_per_guild
        )));
    }

    // Add as member (ON CONFLICT DO NOTHING to handle duplicate join attempts)
    let result = sqlx::query(
        "INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(invite.guild_id)
    .bind(auth.id)
    .execute(&mut *tx)
    .await?;

    // If no rows affected, user was already a member (race with the earlier check)
    if result.rows_affected() == 0 {
        tx.commit().await?;

        let guild_name: (String,) = sqlx::query_as("SELECT name FROM guilds WHERE id = $1")
            .bind(invite.guild_id)
            .fetch_one(&state.db)
            .await?;

        return Ok(Json(InviteResponse {
            id: invite.id,
            code: invite.code,
            guild_id: invite.guild_id,
            guild_name: guild_name.0,
            expires_at: invite.expires_at,
            use_count: invite.use_count,
            created_at: invite.created_at,
        }));
    }

    // Increment use count
    sqlx::query("UPDATE guild_invites SET use_count = use_count + 1 WHERE id = $1")
        .bind(invite.id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    // Initialize read state for all text channels (best-effort, non-critical)
    if let Err(err) =
        super::handlers::initialize_channel_read_state(&state.db, invite.guild_id, auth.id).await
    {
        tracing::error!(
            ?err,
            guild_id = %invite.guild_id,
            user_id = %auth.id,
            "Failed to initialize channel read state after invite join"
        );
        // Non-fatal: member was already inserted, read state can be retried on channel access
    }

    // Get guild name for response
    let guild_name: (String,) = sqlx::query_as("SELECT name FROM guilds WHERE id = $1")
        .bind(invite.guild_id)
        .fetch_one(&state.db)
        .await?;

    Ok(Json(InviteResponse {
        id: invite.id,
        code: invite.code,
        guild_id: invite.guild_id,
        guild_name: guild_name.0,
        expires_at: invite.expires_at,
        use_count: invite.use_count + 1,
        created_at: invite.created_at,
    }))
}
