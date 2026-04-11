//! User profile handlers (get, update, avatar, password).

use axum::extract::{Multipart, State};
use axum::http::HeaderMap;
use axum::Json;
use axum_extra::extract::CookieJar;
use chrono::Utc;
use validator::Validate;

use super::error::{AuthError, AuthResult};
use super::helpers::extract_current_token_hash;
use super::middleware::AuthUser;
use super::password::{hash_password, verify_password};
use super::queries;
use super::types::{
    UpdatePasswordRequest, UpdateProfileRequest, UpdateProfileResponse, UserProfile,
};
use crate::api::AppState;
use crate::db::{email_exists, find_user_by_id, update_user_avatar, update_user_profile};
use crate::util::format_file_size;
use crate::ws::broadcast_user_patch;

/// Get current user profile.
///
/// GET /auth/me
#[utoipa::path(
    get,
    path = "/auth/me",
    tag = "auth",
    responses(
        (status = 200, description = "Current user profile", body = UserProfile),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(fields(user_id = %auth_user.id))]
pub async fn get_profile(auth_user: AuthUser) -> Json<UserProfile> {
    Json(UserProfile {
        id: auth_user.id.to_string(),
        username: auth_user.username,
        display_name: auth_user.display_name,
        email: auth_user.email,
        avatar_url: crate::api::files::maybe_file_url(auth_user.avatar_url),
        status: "online".to_string(),
        mfa_enabled: auth_user.mfa_enabled,
        deletion_scheduled_at: auth_user.deletion_scheduled_at.map(|dt| dt.to_rfc3339()),
    })
}

/// Upload user avatar.
///
/// POST /auth/me/avatar
#[utoipa::path(
    post,
    path = "/auth/me/avatar",
    tag = "auth",
    request_body(content = Vec<u8>, content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "Avatar uploaded successfully", body = UserProfile),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, multipart), fields(user_id = %auth_user.id))]
pub async fn upload_avatar(
    State(state): State<AppState>,
    auth_user: AuthUser,
    mut multipart: Multipart,
) -> AuthResult<Json<UserProfile>> {
    // Check if S3 is configured
    let s3 = state
        .s3
        .as_ref()
        .ok_or_else(|| AuthError::Internal("File storage not configured".to_string()))?;

    // Get the file from multipart
    let mut file_data = None;
    let mut filename = None;
    let mut content_type = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AuthError::Internal(format!("Multipart error: {e}")))?
    {
        if field.name() == Some("avatar") {
            filename = field.file_name().map(ToString::to_string);
            content_type = field.content_type().map(ToString::to_string);

            let data = field
                .bytes()
                .await
                .map_err(|e| AuthError::Internal(format!("Upload error: {e}")))?;

            file_data = Some(data);
            break; // Only process the first file
        }
    }

    let data = file_data.ok_or(AuthError::Validation("No avatar file provided".to_string()))?;

    // SECURITY: Validate file size before processing to prevent resource exhaustion
    if data.len() > state.config.max_avatar_size {
        return Err(AuthError::Validation(format!(
            "Avatar file too large ({}). Maximum size is {}",
            format_file_size(data.len()),
            format_file_size(state.config.max_avatar_size)
        )));
    }

    // Validate mime type from header
    let mime = content_type.unwrap_or_else(|| "application/octet-stream".to_string());

    if !mime.starts_with("image/") {
        return Err(AuthError::Validation("File must be an image".to_string()));
    }

    // Reject SVG files (potential XSS vector via embedded JavaScript)
    if mime.contains("svg") {
        return Err(AuthError::Validation(
            "SVG files are not allowed for avatars".to_string(),
        ));
    }

    // Validate actual file content using magic bytes (don't trust client-provided MIME type)
    let detected_format = image::guess_format(&data).map_err(|_| {
        AuthError::Validation(
            "Unable to detect image format. File may be corrupted or not a valid image."
                .to_string(),
        )
    })?;

    // Only allow safe raster formats
    match detected_format {
        image::ImageFormat::Png
        | image::ImageFormat::Jpeg
        | image::ImageFormat::Gif
        | image::ImageFormat::WebP => {}
        _ => {
            return Err(AuthError::Validation(format!(
                "Unsupported image format: {detected_format:?}. Only PNG, JPEG, GIF, and WebP are allowed."
            )));
        }
    }

    // Generate S3 key: avatars/{user_id}/{timestamp}_{filename}
    let timestamp = Utc::now().timestamp();
    let safe_filename = filename
        .unwrap_or_else(|| "avatar.png".to_string())
        .replace(|c: char| !c.is_alphanumeric() && c != '.', "_");

    let key = format!("avatars/{}/{}_{}", auth_user.id, timestamp, safe_filename);

    // Upload to S3
    s3.upload(&key, data.to_vec(), &mime)
        .await
        .map_err(|e| AuthError::Internal(format!("S3 upload failed: {e}")))?;

    // Store redirect URL — /api/files/ endpoint generates presigned URLs on-the-fly
    let url = crate::api::files::file_url(&key);

    // Update user in DB
    let user = update_user_avatar(&state.db, auth_user.id, Some(&url))
        .await
        .map_err(|e| AuthError::Internal(format!("Database update failed: {e}")))?;

    // Convert status to string
    let status_str = match user.status {
        crate::db::UserStatus::Online => "online",
        crate::db::UserStatus::Away => "away",
        crate::db::UserStatus::Busy => "busy",
        crate::db::UserStatus::Offline => "offline",
    };

    Ok(Json(UserProfile {
        id: user.id.to_string(),
        username: user.username,
        display_name: user.display_name,
        email: user.email,
        avatar_url: crate::api::files::maybe_file_url(user.avatar_url),
        status: status_str.to_string(),
        mfa_enabled: user.mfa_secret.is_some(),
        deletion_scheduled_at: user.deletion_scheduled_at.map(|dt| dt.to_rfc3339()),
    }))
}

/// Update current user profile.
///
/// POST /auth/me
///
/// Updates `display_name` and/or email, then broadcasts a patch event
/// to all subscribers so they see the changes in real-time.
#[utoipa::path(
    post,
    path = "/auth/me",
    tag = "auth",
    request_body = UpdateProfileRequest,
    responses(
        (status = 200, description = "Profile updated successfully", body = UpdateProfileResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, body), fields(user_id = %auth_user.id))]
pub async fn update_profile(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(body): Json<UpdateProfileRequest>,
) -> AuthResult<Json<UpdateProfileResponse>> {
    // Validate request
    body.validate()
        .map_err(|e| AuthError::Validation(crate::validation::format_validation_errors(&e)))?;

    // Validate display_name for unicode safety (control chars, bidi overrides, Zalgo)
    if let Some(ref display_name) = body.display_name {
        crate::presence::validate_unicode_text(display_name, 64)
            .map_err(|e| AuthError::Validation(e.to_string()))?;
    }

    // Check if there's anything to update
    if body.display_name.is_none() && body.email.is_none() && body.status_message.is_none() {
        return Err(AuthError::Validation("No fields to update".to_string()));
    }

    // Check email uniqueness if changing email
    if let Some(ref email) = body.email {
        if email_exists(&state.db, email)
            .await
            .map_err(AuthError::Database)?
        {
            // Check if it's the same user's email
            let current_user = find_user_by_id(&state.db, auth_user.id)
                .await
                .map_err(AuthError::Database)?
                .ok_or_else(|| AuthError::NotFound("User".to_string()))?;

            if current_user.email.as_ref() != Some(email) {
                return Err(AuthError::EmailTaken);
            }
        }
    }

    // Build diff for patch event before update
    let mut diff = serde_json::Map::new();
    let mut updated_fields = Vec::new();

    if let Some(ref display_name) = body.display_name {
        diff.insert("display_name".to_string(), serde_json::json!(display_name));
        updated_fields.push("display_name".to_string());
    }
    if let Some(ref email) = body.email {
        diff.insert("email".to_string(), serde_json::json!(email));
        updated_fields.push("email".to_string());
    }

    // Update database
    let _updated_user = update_user_profile(
        &state.db,
        auth_user.id,
        body.display_name.as_deref(),
        body.email.as_ref().map(|e| Some(e.as_str())),
    )
    .await
    .map_err(AuthError::Database)?;

    // Broadcast patch event to subscribers
    if !diff.is_empty() {
        if let Err(e) = broadcast_user_patch(
            &state.redis,
            auth_user.id,
            serde_json::Value::Object(diff.clone()),
        )
        .await
        {
            tracing::error!(
                error = %e,
                user_id = %auth_user.id,
                changed_fields = ?updated_fields,
                diff = ?diff,
                "Failed to broadcast user profile update to Redis - other clients may see stale data. Consider implementing retry queue."
            );
            // Don't fail the request, update was successful
        }
    }

    Ok(Json(UpdateProfileResponse {
        updated: updated_fields,
    }))
}

/// Update current user password.
///
/// POST /auth/me/password
#[utoipa::path(
    post,
    path = "/auth/me/password",
    tag = "auth",
    request_body = UpdatePasswordRequest,
    responses(
        (status = 200, description = "Password updated successfully"),
        (status = 401, description = "Invalid current password"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, body, headers), fields(user_id = %auth_user.id))]
pub async fn update_password(
    State(state): State<AppState>,
    auth_user: AuthUser,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<UpdatePasswordRequest>,
) -> AuthResult<Json<serde_json::Value>> {
    body.validate()
        .map_err(|e| AuthError::Validation(crate::validation::format_validation_errors(&e)))?;

    let user = find_user_by_id(&state.db, auth_user.id)
        .await
        .map_err(AuthError::Database)?
        .ok_or(AuthError::UserNotFound)?;

    let current_hash = user
        .password_hash
        .as_ref()
        .ok_or(AuthError::InvalidCredentials)?;

    let valid = verify_password(&body.current_password, current_hash)
        .map_err(|_| AuthError::PasswordHash)?;

    if !valid {
        return Err(AuthError::InvalidCredentials);
    }

    let new_hash = hash_password(&body.new_password).map_err(|_| AuthError::PasswordHash)?;

    // Transaction: update password + optionally revoke other sessions
    let mut tx = state.db.begin().await.map_err(AuthError::Database)?;

    queries::update_password_hash_tx(&mut tx, auth_user.id, &new_hash).await?;

    let revoked_count = if body.revoke_others {
        let current_token_hash = extract_current_token_hash(&headers, &jar);
        if let Some(ref hash) = current_token_hash {
            queries::delete_other_user_sessions_tx(&mut tx, auth_user.id, hash).await?
        } else {
            // Cannot identify current session — skip revocation rather than
            // accidentally deleting all sessions (including the caller's own).
            tracing::warn!(
                user_id = %auth_user.id,
                "Cannot identify current session for password-change revocation; skipping"
            );
            0u64
        }
    } else {
        0u64
    };

    tx.commit().await.map_err(AuthError::Database)?;
    tracing::info!(user_id = %auth_user.id, revoked_others = body.revoke_others, revoked_count, "Password updated");

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Password updated successfully.",
        "revoked_count": revoked_count
    })))
}

#[cfg(test)]
mod tests {
    use super::UpdateProfileRequest;

    #[test]
    fn update_profile_request_distinguishes_missing_and_null_status_message() {
        let missing: UpdateProfileRequest =
            serde_json::from_str("{}").expect("missing status_message should deserialize");
        assert_eq!(missing.status_message, None);

        let explicit_null: UpdateProfileRequest =
            serde_json::from_str(r#"{"status_message":null}"#)
                .expect("null status_message should deserialize");
        assert_eq!(explicit_null.status_message, Some(None));

        let value: UpdateProfileRequest = serde_json::from_str(r#"{"status_message":"In queue"}"#)
            .expect("string status_message should deserialize");
        assert_eq!(value.status_message, Some(Some("In queue".to_string())));
    }
}
