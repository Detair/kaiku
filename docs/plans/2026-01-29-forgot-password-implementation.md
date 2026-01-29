# Forgot Password Workflow — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Email-based password reset with secure token generation, rate limiting, single-use tokens, and session invalidation.

**Architecture:** New `password_reset` module in server auth with two public endpoints. Email sent via `lettre` SMTP. Client gets two new views (`/forgot-password`, `/reset-password`). Tokens are SHA-256 hashed before storage (same pattern as refresh tokens). Always returns 200 on forgot-password to prevent email enumeration.

**Tech Stack:** Rust (axum, lettre, sqlx), Solid.js, Argon2id (existing), SHA-256 (existing `hash_token`).

---

## Task 1: Database Migration

**Files:**
- Create: `server/migrations/20260129000000_password_reset.sql`

### Step 1: Create migration file

Create `server/migrations/20260129000000_password_reset.sql`:

```sql
-- Password reset tokens for forgot-password workflow
CREATE TABLE password_reset_tokens (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Partial index: only look up unused tokens
CREATE INDEX idx_password_reset_tokens_hash ON password_reset_tokens(token_hash) WHERE used_at IS NULL;
CREATE INDEX idx_password_reset_tokens_user_id ON password_reset_tokens(user_id);
-- For cleanup job
CREATE INDEX idx_password_reset_tokens_expires ON password_reset_tokens(expires_at);
```

### Step 2: Run migration

```bash
cd server && sqlx migrate run
```

Expected: Migration applied successfully.

### Step 3: Verify table

```bash
psql $DATABASE_URL -c "\d password_reset_tokens"
```

### Step 4: Commit

```bash
git add server/migrations/20260129000000_password_reset.sql
git commit -m "feat(db): add password_reset_tokens table"
```

---

## Task 2: Add `lettre` Email Dependency

**Files:**
- Modify: `server/Cargo.toml`

### Step 1: Add lettre

Add to `[dependencies]` in `server/Cargo.toml`:

```toml
lettre = { version = "0.11", features = ["tokio1-native-tls", "builder", "hostname"] }
```

`lettre` is MIT/Apache-2.0 dual licensed — matches project requirements.

### Step 2: Verify license compliance

```bash
cd server && cargo deny check licenses
```

Expected: No license violations.

### Step 3: Verify it compiles

```bash
cd server && cargo check
```

### Step 4: Commit

```bash
git add server/Cargo.toml server/Cargo.lock
git commit -m "chore(server): add lettre email crate"
```

---

## Task 3: Email Configuration

**Files:**
- Modify: `server/src/config.rs`
- Modify: `server/.env` (or `.env.example` if it exists)

### Step 1: Add SMTP config to Config struct

In `server/src/config.rs`, add fields to the `Config` struct:

```rust
/// SMTP configuration for outbound emails (password reset, etc.)
pub smtp_host: Option<String>,
pub smtp_port: u16,
pub smtp_user: Option<String>,
pub smtp_pass: Option<String>,
pub smtp_from: String,
/// Base URL for reset links (e.g., "https://chat.example.com")
pub app_base_url: String,
```

### Step 2: Parse from environment in `from_env()`

Add to the `Config::from_env()` method:

```rust
smtp_host: env::var("SMTP_HOST").ok(),
smtp_port: env::var("SMTP_PORT")
    .unwrap_or_else(|_| "587".into())
    .parse()
    .unwrap_or(587),
smtp_user: env::var("SMTP_USER").ok(),
smtp_pass: env::var("SMTP_PASS").ok(),
smtp_from: env::var("SMTP_FROM").unwrap_or_else(|_| "noreply@localhost".into()),
app_base_url: env::var("APP_BASE_URL").unwrap_or_else(|_| "http://localhost:5173".into()),
```

### Step 3: Add to .env / .env.example

```env
# Email (SMTP) - Optional, required for password reset
# SMTP_HOST=smtp.example.com
# SMTP_PORT=587
# SMTP_USER=noreply@example.com
# SMTP_PASS=secret
# SMTP_FROM=noreply@example.com
# APP_BASE_URL=https://chat.example.com
```

### Step 4: Verify compilation

```bash
cd server && cargo check
```

### Step 5: Commit

```bash
git add server/src/config.rs server/.env.example
git commit -m "feat(server): add SMTP email configuration"
```

---

## Task 4: Email Service Module

**Files:**
- Create: `server/src/auth/email.rs`
- Modify: `server/src/auth/mod.rs` (add `mod email;`)

### Step 1: Create email.rs

Create `server/src/auth/email.rs`:

```rust
use anyhow::{Context, Result};
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::config::Config;

/// Send a password reset email with the given token.
///
/// The email contains a link to the reset page:
/// `{base_url}/reset-password?token={token}`
pub async fn send_password_reset_email(
    config: &Config,
    to_email: &str,
    token: &str,
) -> Result<()> {
    let smtp_host = config
        .smtp_host
        .as_deref()
        .context("SMTP_HOST not configured — cannot send email")?;

    let reset_url = format!(
        "{}/reset-password?token={}",
        config.app_base_url.trim_end_matches('/'),
        token,
    );

    let body = format!(
        "You requested a password reset.\n\n\
         Click the link below to reset your password:\n\
         {reset_url}\n\n\
         This link expires in 1 hour.\n\n\
         If you did not request this, you can safely ignore this email."
    );

    let email = Message::builder()
        .from(config.smtp_from.parse().context("Invalid SMTP_FROM address")?)
        .to(to_email.parse().context("Invalid recipient email")?)
        .subject("Password Reset Request")
        .header(ContentType::TEXT_PLAIN)
        .body(body)
        .context("Failed to build email message")?;

    let mut transport_builder =
        AsyncSmtpTransport::<Tokio1Executor>::relay(smtp_host)
            .context("Failed to create SMTP transport")?
            .port(config.smtp_port);

    if let (Some(user), Some(pass)) = (&config.smtp_user, &config.smtp_pass) {
        transport_builder =
            transport_builder.credentials(Credentials::new(user.clone(), pass.clone()));
    }

    let transport = transport_builder.build();

    transport
        .send(email)
        .await
        .context("Failed to send password reset email")?;

    tracing::info!(to = to_email, "Password reset email sent");
    Ok(())
}
```

### Step 2: Register module

In `server/src/auth/mod.rs`, add after the existing `mod` declarations:

```rust
mod email;
```

### Step 3: Verify

```bash
cd server && cargo check
```

### Step 4: Commit

```bash
git add server/src/auth/email.rs server/src/auth/mod.rs
git commit -m "feat(server): add email service for password reset"
```

---

## Task 5: Password Reset Error Variants

**Files:**
- Modify: `server/src/auth/error.rs`

### Step 1: Add error variants

Add to the `AuthError` enum in `server/src/auth/error.rs`:

```rust
#[error("Email not configured on server")]
EmailNotConfigured,

#[error("Password reset token is invalid or expired")]
ResetTokenInvalid,

#[error("Failed to send email")]
EmailSendFailed,
```

### Step 2: Add HTTP status mappings

In the `IntoResponse` impl for `AuthError`, add mappings:

```rust
AuthError::EmailNotConfigured => (StatusCode::SERVICE_UNAVAILABLE, "EMAIL_NOT_CONFIGURED"),
AuthError::ResetTokenInvalid => (StatusCode::BAD_REQUEST, "RESET_TOKEN_INVALID"),
AuthError::EmailSendFailed => (StatusCode::INTERNAL_SERVER_ERROR, "EMAIL_SEND_FAILED"),
```

### Step 3: Verify

```bash
cd server && cargo check
```

### Step 4: Commit

```bash
git add server/src/auth/error.rs
git commit -m "feat(server): add password reset error variants"
```

---

## Task 6: Password Reset Handlers

**Files:**
- Create: `server/src/auth/password_reset.rs`
- Modify: `server/src/auth/mod.rs` (add `mod password_reset;`, add routes)

This is the core implementation task. Read the existing `handlers.rs` for patterns.

### Step 1: Create password_reset.rs

Create `server/src/auth/password_reset.rs` with two handlers:

**`request_reset` handler:**

```rust
use axum::{extract::State, Json};
use chrono::{Duration, Utc};
use rand::RngCore;
use serde::Deserialize;
use validator::Validate;

use crate::api::AppState;
use super::{email, error::AuthError, hash_token, password};

#[derive(Debug, Deserialize, Validate)]
pub struct ForgotPasswordRequest {
    #[validate(email(message = "Invalid email address"))]
    pub email: String,
}

/// POST /auth/forgot-password
///
/// Always returns 200 to prevent email enumeration.
/// If email exists and SMTP is configured, sends a reset link.
#[tracing::instrument(skip(state))]
pub async fn request_reset(
    State(state): State<AppState>,
    Json(body): Json<ForgotPasswordRequest>,
) -> Result<Json<serde_json::Value>, AuthError> {
    body.validate().map_err(|e| AuthError::Validation(e.to_string()))?;

    // Check if SMTP is configured
    if state.config.smtp_host.is_none() {
        tracing::warn!("Password reset requested but SMTP not configured");
        // Still return 200 to not leak server config
        return Ok(Json(serde_json::json!({
            "message": "If an account with that email exists, a reset link has been sent."
        })));
    }

    // Look up user by email
    let user = sqlx::query!(
        r#"SELECT id, email FROM users WHERE email = $1 AND auth_method = 'local'"#,
        body.email,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(AuthError::Database)?;

    if let Some(user) = user {
        // Generate 32-byte random token
        let mut token_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut token_bytes);
        let token = base64_url::encode(&token_bytes);
        let token_hash = hash_token(&token);
        let expires_at = Utc::now() + Duration::hours(1);

        // Store hashed token
        sqlx::query!(
            r#"INSERT INTO password_reset_tokens (user_id, token_hash, expires_at)
               VALUES ($1, $2, $3)"#,
            user.id,
            token_hash,
            expires_at,
        )
        .execute(&state.db)
        .await
        .map_err(AuthError::Database)?;

        // Send email (don't fail the request if email fails)
        if let Some(email_addr) = &user.email {
            if let Err(e) = email::send_password_reset_email(&state.config, email_addr, &token).await {
                tracing::error!(error = %e, "Failed to send password reset email");
            }
        }
    }

    // Always return same response
    Ok(Json(serde_json::json!({
        "message": "If an account with that email exists, a reset link has been sent."
    })))
}
```

**`reset_password` handler:**

```rust
#[derive(Debug, Deserialize, Validate)]
pub struct ResetPasswordRequest {
    pub token: String,
    #[validate(length(min = 8, max = 128, message = "Password must be 8-128 characters"))]
    pub new_password: String,
}

/// POST /auth/reset-password
///
/// Validates token, updates password, invalidates all sessions.
#[tracing::instrument(skip(state, body))]
pub async fn reset_password(
    State(state): State<AppState>,
    Json(body): Json<ResetPasswordRequest>,
) -> Result<Json<serde_json::Value>, AuthError> {
    body.validate().map_err(|e| AuthError::Validation(e.to_string()))?;

    let token_hash = hash_token(&body.token);

    // Find valid (unused, not expired) token
    let reset_record = sqlx::query!(
        r#"SELECT id, user_id FROM password_reset_tokens
           WHERE token_hash = $1 AND used_at IS NULL AND expires_at > NOW()"#,
        token_hash,
    )
    .fetch_optional(&state.db)
    .await
    .map_err(AuthError::Database)?
    .ok_or(AuthError::ResetTokenInvalid)?;

    // Hash new password
    let password_hash = password::hash_password(&body.new_password)
        .map_err(|_| AuthError::PasswordHash)?;

    // Use transaction: update password + mark token used + invalidate sessions
    let mut tx = state.db.begin().await.map_err(AuthError::Database)?;

    sqlx::query!(
        r#"UPDATE users SET password_hash = $1, updated_at = NOW() WHERE id = $2"#,
        password_hash,
        reset_record.user_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(AuthError::Database)?;

    sqlx::query!(
        r#"UPDATE password_reset_tokens SET used_at = NOW() WHERE id = $1"#,
        reset_record.id,
    )
    .execute(&mut *tx)
    .await
    .map_err(AuthError::Database)?;

    // Invalidate all existing sessions for this user
    sqlx::query!(
        r#"DELETE FROM sessions WHERE user_id = $1"#,
        reset_record.user_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(AuthError::Database)?;

    tx.commit().await.map_err(AuthError::Database)?;

    tracing::info!(user_id = %reset_record.user_id, "Password reset completed, all sessions invalidated");

    Ok(Json(serde_json::json!({
        "message": "Password has been reset successfully. Please log in with your new password."
    })))
}
```

### Step 2: Register module and routes

In `server/src/auth/mod.rs`:

1. Add module declaration:
```rust
mod password_reset;
```

2. Add routes to the router function. Find the public (unauthenticated) section and add:
```rust
.route("/forgot-password", post(password_reset::request_reset))
.route("/reset-password", post(password_reset::reset_password))
```

Apply the `AuthPasswordReset` rate limit category to these routes (follow the existing pattern for `AuthLogin`).

### Step 3: Add `base64-url` dependency if not already present

Check `Cargo.toml` for `base64-url` or `base64`. If neither exists:

```toml
base64-url = "3"
```

Alternatively, use the `base64` crate with URL-safe encoding if already present. Check which base64 crate exists and use it.

### Step 4: Verify

```bash
cd server && cargo check
```

Fix any compilation errors.

### Step 5: Commit

```bash
git add server/src/auth/password_reset.rs server/src/auth/mod.rs server/Cargo.toml server/Cargo.lock
git commit -m "feat(server): add forgot-password and reset-password endpoints"
```

---

## Task 7: Client — Forgot Password View

**Files:**
- Create: `client/src/views/ForgotPassword.tsx`
- Modify: `client/src/views/Login.tsx` (add "Forgot Password?" link)
- Modify: `client/src/App.tsx` (add route)
- Modify: `client/src/lib/tauri.ts` (add API function)

### Step 1: Add API function

In `client/src/lib/tauri.ts`, add:

```typescript
export async function requestPasswordReset(serverUrl: string, email: string): Promise<void> {
  const res = await fetch(`${serverUrl}/auth/forgot-password`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ email }),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.message || "Request failed");
  }
}
```

### Step 2: Create ForgotPassword.tsx

Create `client/src/views/ForgotPassword.tsx`:

```tsx
import { Component, createSignal } from "solid-js";
import { A } from "@solidjs/router";
import { requestPasswordReset } from "@/lib/tauri";

const ForgotPassword: Component = () => {
  const [serverUrl, setServerUrl] = createSignal(localStorage.getItem("serverUrl") || "");
  const [email, setEmail] = createSignal("");
  const [submitted, setSubmitted] = createSignal(false);
  const [error, setError] = createSignal("");
  const [isLoading, setIsLoading] = createSignal(false);

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    setError("");
    setIsLoading(true);
    try {
      await requestPasswordReset(serverUrl(), email());
      setSubmitted(true);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Something went wrong");
    } finally {
      setIsLoading(false);
    }
  };

  return (
    // Match Login.tsx layout and styling
    // Show email input form, or success message if submitted
    // Include "Back to Login" link
  );
};

export default ForgotPassword;
```

Match the exact styling from `Login.tsx` (read it for the form structure, input classes, button classes, error display pattern).

### Step 3: Add "Forgot Password?" link to Login.tsx

In `client/src/views/Login.tsx`, add a link after the password input and before the submit button (or in the footer area):

```tsx
<A href="/forgot-password" class="text-sm text-accent-primary hover:underline">
  Forgot Password?
</A>
```

### Step 4: Add route to App.tsx

In `client/src/App.tsx`:

1. Import: `import ForgotPassword from "./views/ForgotPassword";`
2. Add wrapped component: `const ForgotPasswordPage = () => <Layout><ForgotPassword /></Layout>;`
3. Add route: `<Route path="/forgot-password" component={ForgotPasswordPage} />`

### Step 5: Verify

```bash
cd client && bunx tsc --noEmit
```

### Step 6: Commit

```bash
git add client/src/views/ForgotPassword.tsx client/src/views/Login.tsx client/src/App.tsx client/src/lib/tauri.ts
git commit -m "feat(client): add forgot password view and login link"
```

---

## Task 8: Client — Reset Password View

**Files:**
- Create: `client/src/views/ResetPassword.tsx`
- Modify: `client/src/App.tsx` (add route)
- Modify: `client/src/lib/tauri.ts` (add API function)

### Step 1: Add API function

In `client/src/lib/tauri.ts`, add:

```typescript
export async function resetPassword(serverUrl: string, token: string, newPassword: string): Promise<void> {
  const res = await fetch(`${serverUrl}/auth/reset-password`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ token, new_password: newPassword }),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.message || "Reset failed");
  }
}
```

### Step 2: Create ResetPassword.tsx

Create `client/src/views/ResetPassword.tsx`:

```tsx
import { Component, createSignal, onMount } from "solid-js";
import { useSearchParams, useNavigate } from "@solidjs/router";
import { resetPassword } from "@/lib/tauri";

const ResetPassword: Component = () => {
  const [searchParams] = useSearchParams();
  const navigate = useNavigate();
  const [serverUrl] = createSignal(localStorage.getItem("serverUrl") || "");
  const [password, setPassword] = createSignal("");
  const [confirmPassword, setConfirmPassword] = createSignal("");
  const [error, setError] = createSignal("");
  const [success, setSuccess] = createSignal(false);
  const [isLoading, setIsLoading] = createSignal(false);

  const token = () => searchParams.token || "";

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    if (password() !== confirmPassword()) {
      setError("Passwords do not match");
      return;
    }
    if (password().length < 8) {
      setError("Password must be at least 8 characters");
      return;
    }
    setError("");
    setIsLoading(true);
    try {
      await resetPassword(serverUrl(), token(), password());
      setSuccess(true);
      // Redirect to login after 3 seconds
      setTimeout(() => navigate("/login"), 3000);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Reset failed");
    } finally {
      setIsLoading(false);
    }
  };

  return (
    // Match Login.tsx layout and styling
    // Show new password + confirm password inputs
    // If no token in URL, show error
    // On success, show message + auto-redirect to login
  );
};

export default ResetPassword;
```

### Step 3: Add route to App.tsx

1. Import: `import ResetPassword from "./views/ResetPassword";`
2. Add wrapped: `const ResetPasswordPage = () => <Layout><ResetPassword /></Layout>;`
3. Add route: `<Route path="/reset-password" component={ResetPasswordPage} />`

### Step 4: Verify

```bash
cd client && bunx tsc --noEmit
```

### Step 5: Commit

```bash
git add client/src/views/ResetPassword.tsx client/src/App.tsx client/src/lib/tauri.ts
git commit -m "feat(client): add reset password view with token validation"
```

---

## Task 9: CHANGELOG Update

**Files:**
- Modify: `CHANGELOG.md`

### Step 1: Add entry

Under `## [Unreleased]` → `### Added`:

```markdown
- Forgot password workflow with email-based secure token reset
  - `POST /auth/forgot-password` sends reset link (always returns 200 to prevent enumeration)
  - `POST /auth/reset-password` validates token and updates password
  - SHA-256 hashed tokens with 1-hour expiry and single-use enforcement
  - All existing sessions invalidated on password reset
  - Rate-limited: 2 requests per 60 seconds per IP
  - SMTP email configuration via environment variables
```

### Step 2: Commit

```bash
git add CHANGELOG.md
git commit -m "docs: add forgot password to changelog"
```

---

## Verification

### Server
```bash
cd server && cargo check && cargo test
```

### Client
```bash
cd client && bunx tsc --noEmit
```

### Manual Testing

**Prerequisites:** Configure SMTP env vars (can use Mailpit for local testing):
```bash
SMTP_HOST=localhost
SMTP_PORT=1025
SMTP_FROM=noreply@test.local
APP_BASE_URL=http://localhost:5173
```

**Test flow:**
1. Navigate to `/login`, click "Forgot Password?"
2. Enter email, submit → should see success message
3. Check Mailpit inbox → should have email with reset link
4. Click reset link → should open `/reset-password?token=...`
5. Enter new password + confirm → submit
6. Should redirect to `/login` after success
7. Login with new password → should work
8. Login with old password → should fail
9. Try using same reset link again → should fail (single-use)
10. Try requesting reset with non-existent email → same success message (no enumeration)

### Security Checklist
- [ ] Token is 32 bytes random, base64url encoded
- [ ] Token stored as SHA-256 hash (not plaintext)
- [ ] Token expires after 1 hour
- [ ] Token marked as used after successful reset
- [ ] All sessions invalidated after reset
- [ ] `forgot-password` always returns 200 regardless of email existence
- [ ] Rate limited (2 req/60s per IP)
- [ ] Password validation (min 8 chars)
- [ ] SMTP credentials not logged
