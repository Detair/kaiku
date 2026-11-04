//! User registration handler.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use axum_extra::extract::CookieJar;
use validator::Validate;

use super::client_class::ClientClass;
use super::ctx::AuthCtx;
use super::error::{AuthError, AuthResult};
use super::password::hash_password;
use super::queries;
use super::types::{RegisterRequest, RegisterResponse};
use crate::api::AppState;
use crate::db::{self, email_exists, get_auth_methods_allowed, username_exists};

/// Register a new local user.
///
/// **First User Behavior:** The first user to register is automatically granted
/// system admin permissions within the registration transaction. This is serialized
/// by a FOR UPDATE lock on the `server_config.setup_complete` row to prevent race
/// conditions where multiple concurrent registrations both see `user_count=0`.
///
/// After the first user is created, subsequent registrations will not receive admin
/// permissions unless explicitly granted by an existing admin.
///
/// POST /auth/register
#[utoipa::path(
    post,
    path = "/auth/register",
    tag = "auth",
    request_body = RegisterRequest,
    responses(
        (status = 201, description = "Account created; a registration ticket, never a session", body = RegisterResponse),
        (status = 400, description = "Bad request"),
        (status = 409, description = "Username or email already taken"),
    ),
    security(()),
)]
#[tracing::instrument(skip(state, jar, headers, body), fields(username = %body.username))]
pub async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<RegisterRequest>,
) -> AuthResult<(StatusCode, CookieJar, Json<RegisterResponse>)> {
    // Validate input first
    body.validate()
        .map_err(|e| AuthError::Validation(crate::validation::format_validation_errors(&e)))?;

    // Check if local auth is allowed
    let auth_methods = get_auth_methods_allowed(&state.db).await?;
    if !auth_methods.local {
        return Err(AuthError::AuthMethodDisabled);
    }

    // Check registration policy (fail-closed: deny registration if DB is unreachable)
    let reg_policy_value = db::get_config_value(&state.db, "registration_policy")
        .await
        .map_err(|e| {
            tracing::error!(
                error = %e,
                "Failed to read registration_policy config - denying registration (fail-closed)"
            );
            AuthError::Database(e)
        })?;
    let reg_policy = reg_policy_value.as_str().ok_or_else(|| {
        tracing::error!(
            actual_value = ?reg_policy_value,
            "registration_policy config value is not a string"
        );
        AuthError::Internal("Server configuration error".to_string())
    })?;
    if reg_policy != "open" {
        // Both "closed" and "invite_only" reject direct registration
        return Err(AuthError::RegistrationDisabled);
    }

    // Check username uniqueness (outside transaction - UNIQUE constraint will catch races)
    if username_exists(&state.db, &body.username).await? {
        return Err(AuthError::UserAlreadyExists);
    }

    // Check email uniqueness (if provided)
    if let Some(ref email) = body.email {
        if email_exists(&state.db, email).await? {
            return Err(AuthError::UserAlreadyExists);
        }
    }

    // Set display name (default to username if not provided)
    let display_name = body.display_name.as_deref().unwrap_or(&body.username);

    // Validate display name for unicode safety (control chars, bidi overrides, Zalgo, HTML)
    crate::presence::validate_unicode_text(display_name, 64)
        .map_err(|e| AuthError::Validation(format!("display_name: {e}")))?;

    // The password is optional (B.1); when given, the policy applies.
    let password_hash = match body.password.as_deref().filter(|p| !p.is_empty()) {
        Some(password) => {
            let server_name = {
                let mut conn = state.db.acquire().await?;
                crate::approvals::server_name(&mut conn).await?
            };
            super::password_policy::check(
                password,
                &super::password_policy::PolicyContext {
                    username: &body.username,
                    display_name,
                    server_name: &server_name,
                },
            )
            .map_err(AuthError::PasswordRejected)?;
            let password = password.to_string();
            Some(
                tokio::task::spawn_blocking(move || hash_password(&password))
                    .await
                    .map_err(|e| AuthError::Internal(format!("password hashing failed: {e}")))?
                    .map_err(|_| AuthError::PasswordHash)?,
            )
        }
        None => None,
    };

    // Start transaction for atomic first-user detection and admin grant
    let mut tx = state.db.begin().await.map_err(|e| {
        tracing::error!(
            error = %e,
            username = %body.username,
            "Failed to start registration transaction"
        );
        e
    })?;

    // FOR UPDATE on setup_complete serializes concurrent registrations by acquiring
    // a row-level lock. Multiple transactions can BEGIN concurrently, but they will
    // block at this SELECT FOR UPDATE until the first transaction COMMITS or ROLLS BACK.
    // The lock is held for the entire transaction duration, preventing the race condition
    // where two concurrent registrations both see user_count=0 and both grant admin.
    queries::lock_setup_complete(&mut tx).await.map_err(|e| {
        tracing::error!(
            error = ?e,
            username = %body.username,
            "Failed to lock setup_complete config during registration"
        );
        e
    })?;

    // Now safely count users (serialized by the lock above)
    let user_count = queries::count_users(&mut tx).await.map_err(|e| {
        tracing::error!(
            error = ?e,
            username = %body.username,
            "Failed to count users during registration"
        );
        e
    })?;
    let is_first_user = user_count == 0;

    // Create user (inline to use transaction)
    let user = queries::insert_local_user(
        &mut tx,
        &body.username,
        display_name,
        body.email.as_deref(),
        password_hash.as_deref(),
    )
    .await
    .map_err(|e| {
        tracing::error!(
            error = ?e,
            username = %body.username,
            "Failed to create user during registration - transaction will rollback"
        );
        e
    })?;

    // Grant system admin to first user
    if is_first_user {
        queries::grant_first_user_admin(&mut tx, user.id)
            .await
            .map_err(|e| {
                tracing::error!(
                    error = ?e,
                    user_id = %user.id,
                    username = %user.username,
                    "Failed to grant system admin to first user - transaction will rollback"
                );
                e
            })?;

        tracing::info!(
            user_id = %user.id,
            username = %user.username,
            "First user registered and granted system admin"
        );
    }

    // B.0: a password is activity (the account is no longer new).
    if password_hash.is_some() {
        super::strong::mark_first_activity(&mut tx, user.id).await?;
    }

    // F8: a registration ticket (30 min), never a session.
    let (_, ticket, ticket_expires_at) =
        super::registration::issue_ticket_tx(&mut tx, &AuthCtx::from(&state), user.id).await?;

    // Commit transaction
    tx.commit().await.map_err(|e| {
        tracing::error!(
            error = %e,
            user_id = %user.id,
            username = %user.username,
            "Failed to commit registration transaction - user account rolled back"
        );
        e
    })?;
    if is_first_user {
        // The `admin_role_granted` event now, not at the next sweep (Flash r5).
        crate::security_events::notices::spawn_pending_delivery(&state);
    } else {
        tracing::info!(user_id = %user.id, username = %user.username, "User registered");
    }

    // Native clients keep the ticket in their own storage; browsers get the
    // path-scoped cookie.
    let native = ClientClass::from_headers(&headers) != ClientClass::Web;
    let jar = if native {
        jar
    } else {
        jar.add(super::registration::ticket_cookie(
            &ticket.to_b64u(),
            super::registration::TICKET_MINUTES * 60,
            &state.config,
        ))
    };
    Ok((
        StatusCode::CREATED,
        jar,
        Json(RegisterResponse {
            user_id: user.id,
            username: user.username,
            ticket_expires_at,
            registration_ticket: native.then(|| ticket.to_b64u()),
        }),
    ))
}
