# Forgot Password Feature — Completed

## Status: MERGED (PR #129)

The forgot password feature has been fully implemented, reviewed, and merged to `main`.

## What Was Done

Implemented a complete password reset workflow per the plan in `~/.claude/plans/structured-wiggling-wand.md`:

### Backend (Rust/Axum)
- **SMTP Email Service** (`server/src/email/mod.rs`): `EmailService` using lettre crate, supports STARTTLS/TLS/plain
- **Config** (`server/src/config.rs`): 6 SMTP env vars (`SMTP_HOST`, `SMTP_PORT`, `SMTP_USERNAME`, `SMTP_PASSWORD`, `SMTP_FROM`, `SMTP_TLS`) + `has_smtp()` method
- **DB Migration** (`server/migrations/20260131000000_password_reset_tokens.sql`): `password_reset_tokens` table with indexes
- **DB Model/Queries** (`server/src/db/models.rs`, `queries.rs`): `PasswordResetToken` model + 6 query functions
- **Auth Handlers** (`server/src/auth/handlers.rs`): `forgot_password` and `reset_password` handlers
- **Auth Routes** (`server/src/auth/mod.rs`): Two rate-limited public routes under `AuthPasswordReset` category
- **Background Cleanup** (`server/src/main.rs`): Hourly cleanup of expired tokens
- **Error Variant** (`server/src/auth/error.rs`): `EmailNotConfigured` → 503

### Frontend (Solid.js)
- `client/src/views/ForgotPassword.tsx` — email input form
- `client/src/views/ResetPassword.tsx` — code + new password form
- `client/src/App.tsx` — routes for `/forgot-password` and `/reset-password`
- `client/src/views/Login.tsx` — "Forgot password?" link

### Docs
- `CHANGELOG.md` — entry under `[Unreleased] > Added`
- `LICENSE_COMPLIANCE.md` — lettre (MIT license)

## Security Design
- Tokens: 256-bit random, base64url encoded, SHA256-hashed in DB
- 1-hour expiry, single-use
- All sessions invalidated on successful reset
- No user enumeration: generic 200 response for all cases (including SMTP failure)
- Rate limiting via `AuthPasswordReset` category
- SMTP optional: server starts without it, returns 503

## Review Fixes Applied
1. Email send failure now returns generic 200 (was returning error, breaking anti-enumeration)
2. `invalidate_user_reset_tokens` errors are logged (was silent `let _ =`)
3. lettre license corrected from "MIT OR Apache-2.0" to "MIT"

## Open Follow-Up Issues
- **#130**: Handle orphaned reset token when email send fails (token saved to DB before email, not cleaned up on failure)
- **#131**: Remove or document unused `update_user_password` function in `db/queries.rs` (dead code, not caught by compiler because `pub`)

## Key Files
- `server/src/email/mod.rs` (new)
- `server/src/auth/handlers.rs` (forgot_password ~line 1050, reset_password ~line 1135)
- `server/src/config.rs` (SMTP config fields)
- `server/src/db/queries.rs` (6 new query functions)
- `server/migrations/20260131000000_password_reset_tokens.sql` (new)
