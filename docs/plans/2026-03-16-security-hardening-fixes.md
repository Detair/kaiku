# Security Hardening Fixes — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix 4 security/correctness issues found during live testing of kaiku.pmind.de: password leaking in validation errors, missing HSTS header, guild channel access without membership check, and HTML injection in display names.

**Architecture:** All changes are server-side Rust in the `vc-server` crate. No database migrations, no frontend changes. Each fix is independent and can be tested in isolation.

**Tech Stack:** Rust, axum, validator crate, sqlx

---

### Task 1: Sanitize validation errors to strip sensitive field values

The `validator` crate's `Display` impl includes a `"value"` key in every `ValidationError.params` HashMap, which means passwords and other sensitive inputs are echoed back in HTTP error responses. We need a helper that strips `"value"` keys before formatting.

**Files:**
- Create: `server/src/validation.rs`
- Modify: `server/src/lib.rs` (add `pub mod validation;`)
- Modify: `server/src/auth/handlers.rs:422-423` (use new helper)
- Modify: `server/src/auth/handlers.rs:1374-1375` (use new helper)
- Modify: `server/src/auth/handlers.rs:1476-1477` (use new helper)
- Modify: `server/src/guild/handlers.rs:152-153` (use new helper)
- Modify: `server/src/guild/handlers.rs:390` (use new helper)
- Modify: `server/src/guild/roles.rs:199` (use new helper)
- Modify: `server/src/moderation/handlers.rs:33` (use new helper)
- Modify: `server/src/workspaces/handlers.rs:52,231` (use new helper)

**Step 1: Create `server/src/validation.rs` with the sanitize helper**

```rust
//! Validation helpers that strip sensitive data from error messages.

use validator::ValidationErrors;

/// Format validation errors for HTTP responses, stripping any `"value"` keys
/// from error params so that passwords and other sensitive inputs are never
/// echoed back to the client.
pub fn format_validation_errors(errors: &ValidationErrors) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for (field, field_errors) in errors.field_errors() {
        for err in field_errors {
            if !out.is_empty() {
                out.push_str(", ");
            }
            // Use custom message if the validator provided one
            if let Some(ref msg) = err.message {
                let _ = write!(out, "{field}: {msg}");
            } else {
                // Build a params map WITHOUT the "value" key
                let safe_params: std::collections::HashMap<_, _> = err
                    .params
                    .iter()
                    .filter(|(k, _)| k.as_ref() != "value")
                    .collect();
                let _ = write!(out, "{field}: Validation error: {} {safe_params:?}", err.code);
            }
        }
    }
    out
}
```

**Step 2: Register the module in `server/src/lib.rs`**

Add `pub mod validation;` alongside the other module declarations.

**Step 3: Replace all `.validate().map_err(|e| ...Error::Validation(e.to_string()))?` call sites**

Each call site changes from:
```rust
body.validate()
    .map_err(|e| AuthError::Validation(e.to_string()))?;
```
to:
```rust
body.validate()
    .map_err(|e| AuthError::Validation(crate::validation::format_validation_errors(&e)))?;
```

Apply this to every `.validate()` call site listed in "Files" above.

**Step 4: Build and verify**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
Expected: compiles with no warnings.

**Step 5: Commit**

```
fix(auth): strip sensitive values from validation error responses

The validator crate includes field values in error params by default,
which caused passwords to leak in validation error messages.
```

---

### Task 2: Add HSTS header to security middleware

The Rust server's `security_headers` middleware already sets `X-Content-Type-Options`, `X-Frame-Options`, and `Referrer-Policy`, but is missing `Strict-Transport-Security`.

**Files:**
- Modify: `server/src/api/mod.rs:479-496`

**Step 1: Add HSTS header to the existing middleware**

In `security_headers()` at `server/src/api/mod.rs:479`, add after the existing `referrer-policy` insert (line 494):

```rust
    headers.insert(
        axum::http::HeaderName::from_static("strict-transport-security"),
        axum::http::HeaderValue::from_static("max-age=63072000; includeSubDomains; preload"),
    );
```

**Step 2: Build and verify**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
Expected: compiles with no warnings.

**Step 3: Commit**

```
fix(infra): add Strict-Transport-Security header to API responses
```

---

### Task 3: Add early guild membership check to `list_channels`

The `list_channels` handler fetches ALL channels for a guild, then filters by permission. For non-members, this wastes a DB query and returns `200 []` instead of `403 Forbidden`. Add an early `is_guild_member` check matching the pattern used by every other guild handler.

**Files:**
- Modify: `server/src/guild/handlers.rs:686-687`

**Step 1: Add membership check at the top of `list_channels`**

Insert after `Path(guild_id): Path<Uuid>,` (after line 686, before `let all_channels`):

```rust
    // Verify membership before fetching channels
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }
```

This matches the exact pattern at lines 356-359 (`get_guild`), 513-516 (`update_guild`), 575-578 (`delete_guild`), etc.

**Step 2: Build and verify**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
Expected: compiles with no warnings.

**Step 3: Commit**

```
fix(api): reject non-members from guild channel listing

Previously returned 200 with empty array for non-members.
Now returns 403 Forbidden, matching other guild endpoints.
```

---

### Task 4: Add HTML tag rejection to display name validation

Display names accept arbitrary HTML like `<script>` or `<img onerror=...>`. While the frontend escapes these via JSX, the server should reject them as defense-in-depth. Also apply the existing `validate_unicode_text` check to the registration handler (it's currently only used in `update_profile`).

**Files:**
- Modify: `server/src/presence/types.rs:57-80` (add HTML check)
- Modify: `server/src/auth/handlers.rs:468-469` (add validation to register)

**Step 1: Add HTML tag detection to `validate_unicode_text`**

In `server/src/presence/types.rs`, add a check at the start of `validate_unicode_text` (after the length check on line 59):

```rust
    // Reject HTML tags as defense-in-depth against stored XSS
    if text.contains('<') && text.contains('>') {
        return Err("Text must not contain HTML tags");
    }
```

This is deliberately simple — it catches `<script>`, `<img ...>`, etc. without pulling in an HTML parser. False positives for legitimate `<` and `>` usage in display names are acceptable since users can use the Unicode equivalents.

**Step 2: Add `validate_unicode_text` to the register handler**

In `server/src/auth/handlers.rs`, after line 469 (`let display_name = ...`), add:

```rust
    // Validate display name for unicode safety (control chars, bidi overrides, Zalgo, HTML)
    crate::presence::validate_unicode_text(display_name, 64)
        .map_err(|e| AuthError::Validation(format!("display_name: {e}")))?;
```

**Step 3: Build and verify**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
Expected: compiles with no warnings.

**Step 4: Commit**

```
fix(auth): reject HTML tags in display names and validate on registration

Adds HTML tag detection to validate_unicode_text as defense-in-depth.
Also applies unicode validation to the register handler, which was
previously only done in update_profile.
```

---

## Out of Scope (Caddy Config)

These require changes to the Stoat stack's Caddyfile on the VPS, not code changes:

- **SPA security headers**: Caddy should add `X-Frame-Options`, `X-Content-Type-Options`, `Content-Security-Policy`, `Referrer-Policy`, `Strict-Transport-Security` to the SPA HTML responses.
- **OpenAPI JSON routing**: Caddy's SPA catch-all intercepts `/api/docs/openapi.json` before it reaches the Rust backend. The Caddyfile needs to proxy `/api/docs/*` to the backend.
