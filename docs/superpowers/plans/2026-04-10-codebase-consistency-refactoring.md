# Codebase Consistency Refactoring Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor all server modules and client monoliths to match the canonical patterns defined in `docs/superpowers/specs/2026-04-10-codebase-consistency-standards-design.md`.

**Architecture:** Incremental refactoring organized by change type (error standardization, query extraction, handler splitting, etc.) so each phase produces a consistently-structured codebase at its checkpoint. Server-side first, then client.

**Tech Stack:** Rust (axum, thiserror, sqlx, utoipa), Solid.js/TypeScript

**Spec:** `docs/superpowers/specs/2026-04-10-codebase-consistency-standards-design.md`

**Excluded modules (already compliant or infrastructure):**
- `db/` — data layer module, not a feature module. Has its own models.rs/queries.rs pattern that other modules depend on. No changes needed.
- `permissions/` — library module with no HTTP handlers. Already well-structured (resolver.rs, queries.rs, models.rs, helpers.rs).
- `observability/` — infrastructure module. No HTTP handlers, no feature-module patterns apply.
- `email/` — service-only module (237 lines, single mod.rs). Already Tier 1 compliant.
- `vc-common/` and `vc-crypto/` — shared crates. The spec defines conventions for what goes in `vc-common` going forward but does not require retroactive reorganization.
- Test locations — already follow conventions (server: `tests/integration/`, client: `__tests__/` and `e2e/`).

---

## Phase 1: Error Standardization

Move all error enums to `error.rs`, rename to `{ModuleName}Error` convention. Low risk, no logic changes.

### Task 1: Move GuildError from handlers.rs to error.rs

**Files:**
- Create: `server/src/guild/error.rs`
- Modify: `server/src/guild/handlers.rs` — remove GuildError enum + IntoResponse impl
- Modify: `server/src/guild/mod.rs` — add `mod error;`

- [ ] **Step 1: Run tests to establish baseline**

Run: `SQLX_OFFLINE=true cargo test -p vc-server 2>&1 | tail -5`
Expected: All tests pass

- [ ] **Step 2: Create guild/error.rs**

Extract the `GuildError` enum and its `IntoResponse` impl from `guild/handlers.rs` into a new `guild/error.rs`. Keep the exact same variants and error mapping. Add necessary imports (`axum`, `thiserror`, `uuid`, `serde_json`).

- [ ] **Step 3: Update guild/handlers.rs**

Remove the `GuildError` enum and `IntoResponse` impl. Add `use super::error::GuildError;` at the top.

- [ ] **Step 4: Update guild/mod.rs**

Add `mod error;` and `pub use error::GuildError;`.

- [ ] **Step 5: Update all files that import GuildError**

Check all files in `guild/` that reference `GuildError` (categories.rs, roles.rs, emojis.rs, invites.rs, search.rs, limits.rs). Update imports to use `super::error::GuildError`.

- [ ] **Step 6: Verify**

Run: `SQLX_OFFLINE=true cargo test -p vc-server 2>&1 | tail -5`
Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings 2>&1 | tail -10`
Expected: All tests pass, no clippy warnings

- [ ] **Step 7: Commit**

```bash
git add server/src/guild/
git commit -m "refactor(guild): move GuildError to dedicated error.rs"
```

---

### Task 2: Move AdminError from types.rs to error.rs

**Files:**
- Create: `server/src/admin/error.rs`
- Modify: `server/src/admin/types.rs` — remove AdminError enum + IntoResponse impl
- Modify: `server/src/admin/mod.rs` — add `mod error;`

- [ ] **Step 1: Create admin/error.rs**

Extract `AdminError` enum and its `IntoResponse` impl from `admin/types.rs`. Add necessary imports.

- [ ] **Step 2: Update admin/types.rs**

Remove the `AdminError` enum and `IntoResponse` impl.

- [ ] **Step 3: Update admin/mod.rs**

Add `mod error;` and `pub use error::AdminError;`.

- [ ] **Step 4: Update imports**

Update `admin/handlers.rs` and `admin/observability.rs` to import `AdminError` from `super::error`.

- [ ] **Step 5: Verify**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings 2>&1 | tail -10`
Expected: No warnings

- [ ] **Step 6: Commit**

```bash
git add server/src/admin/
git commit -m "refactor(admin): move AdminError to dedicated error.rs"
```

---

### Task 3: Rename ReportError to ModerationError, move to error.rs

**Files:**
- Create: `server/src/moderation/error.rs`
- Modify: `server/src/moderation/types.rs` — remove ReportError
- Modify: `server/src/moderation/mod.rs` — add `mod error;`

- [ ] **Step 1: Create moderation/error.rs**

Extract the error enum from `moderation/types.rs`. Rename `ReportError` to `ModerationError`. Update the `IntoResponse` impl to use `ModerationError`. Keep all variants identical.

- [ ] **Step 2: Update moderation/types.rs**

Remove the error enum and its IntoResponse impl.

- [ ] **Step 3: Update mod.rs and imports**

Add `mod error;` to `moderation/mod.rs`. Update all files in `moderation/` that reference `ReportError` to use `ModerationError` from `super::error`.

- [ ] **Step 4: Verify**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings 2>&1 | tail -10`
Expected: No warnings

- [ ] **Step 5: Commit**

```bash
git add server/src/moderation/
git commit -m "refactor(moderation): rename ReportError to ModerationError, move to error.rs"
```

---

### Task 4: Move SocialError from types.rs to error.rs

**Files:**
- Create: `server/src/social/error.rs`
- Modify: `server/src/social/types.rs` — remove SocialError enum + IntoResponse impl
- Modify: `server/src/social/mod.rs` — add `mod error;`

- [ ] **Step 1: Create social/error.rs**

Extract `SocialError` enum and its `IntoResponse` impl from `social/types.rs`. Add necessary imports.

- [ ] **Step 2: Update types.rs, friends.rs, and mod.rs**

Remove error from types.rs. Update `friends.rs` to import from `super::error::SocialError`. Add `mod error;` and `pub use error::SocialError;` to mod.rs.

- [ ] **Step 3: Verify and commit**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings 2>&1 | tail -10`

```bash
git add server/src/social/
git commit -m "refactor(social): move SocialError to dedicated error.rs"
```

---

### Task 5: Move WebhookError from types.rs to error.rs

**Files:**
- Create: `server/src/webhooks/error.rs`
- Modify: `server/src/webhooks/types.rs` — remove WebhookError
- Modify: `server/src/webhooks/mod.rs` — add `mod error;`

- [ ] **Step 1: Create webhooks/error.rs**

Extract `WebhookError` enum and IntoResponse impl from `webhooks/types.rs`.

- [ ] **Step 2: Update types.rs, handlers.rs, and mod.rs**

Remove error from types.rs. Update handlers.rs imports. Add `mod error;` to mod.rs.

- [ ] **Step 3: Verify and commit**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings 2>&1 | tail -10`

```bash
git add server/src/webhooks/
git commit -m "refactor(webhooks): move WebhookError to dedicated error.rs"
```

---

### Task 6: (Skipped — connectivity will be consolidated to Tier 1 in Task 13)

Connectivity is a Tier 1 consolidation candidate (466 lines). Creating a separate `error.rs` here would be immediately undone in Phase 2. The error stays inline in the consolidated `mod.rs`.

---

### Task 7: Consolidate chat errors into ChatError

This is the most complex error task — chat has 7 scattered error enums.

**Files:**
- Create: `server/src/chat/error.rs`
- Modify: `server/src/chat/channels.rs` — remove ChannelError, use ChatError
- Modify: `server/src/chat/messages.rs` — remove MessageError, use ChatError
- Modify: `server/src/chat/uploads.rs` — remove UploadError, use ChatError
- Modify: `server/src/chat/overrides.rs` — remove OverrideError, use ChatError
- Modify: `server/src/chat/dm.rs` — remove any inline errors, use ChatError
- Modify: `server/src/chat/mod.rs` — add `mod error;`

- [ ] **Step 1: Inventory all error variants**

Read each file and list every variant from `ChannelError`, `MessageError`, `UploadError`, `OverrideError`, `ProcessingError`, `S3Error`, `DmSearchError`. Note which HTTP status each maps to.

- [ ] **Step 2: Create chat/error.rs with unified ChatError**

Create a single `ChatError` enum with all variants, prefixed by domain where names would collide (e.g., `ChannelNotFound`, `MessageNotFound`). Implement `IntoResponse` with the same status mappings.

- [ ] **Step 3: Update channels.rs**

Remove `ChannelError`. Replace all usages with `ChatError` variants. Update handler return types from `Result<_, ChannelError>` to `Result<_, ChatError>`.

- [ ] **Step 4: Update messages.rs**

Same as step 3 for `MessageError`.

- [ ] **Step 5: Update uploads.rs**

Same for `UploadError`, `ProcessingError`, `S3Error`.

- [ ] **Step 6: Update overrides.rs and dm.rs**

Same for `OverrideError` and `DmSearchError`.

- [ ] **Step 7: Update mod.rs**

Add `mod error;` and `pub use error::ChatError;`.

- [ ] **Step 8: Verify**

Run: `SQLX_OFFLINE=true cargo test -p vc-server 2>&1 | tail -5`
Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings 2>&1 | tail -10`

- [ ] **Step 9: Commit**

```bash
git add server/src/chat/
git commit -m "refactor(chat): consolidate 7 error enums into single ChatError"
```

---

### Task 8: Add error.rs to pages module

Pages currently has no dedicated error type — errors are handled inline.

**Files:**
- Create: `server/src/pages/error.rs`
- Modify: `server/src/pages/handlers.rs` — extract inline error handling
- Modify: `server/src/pages/mod.rs` — add `mod error;`

- [ ] **Step 1: Read pages/handlers.rs to identify inline error patterns**

Determine what error handling exists and define `PagesError` variants for each case.

- [ ] **Step 2: Create pages/error.rs**

Define `PagesError` with appropriate variants and IntoResponse.

- [ ] **Step 3: Update handlers.rs to use PagesError**

Replace inline error construction with `PagesError` variants.

- [ ] **Step 4: Update mod.rs**

Add `mod error;`.

- [ ] **Step 5: Verify and commit**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings 2>&1 | tail -10`

```bash
git add server/src/pages/
git commit -m "refactor(pages): add dedicated PagesError in error.rs"
```

---

### Task 9: Add error.rs to discovery module

**Files:**
- Create: `server/src/discovery/error.rs`
- Modify: `server/src/discovery/handlers.rs`
- Modify: `server/src/discovery/mod.rs`

- [ ] **Step 1: Read handlers.rs, identify error patterns, create error.rs**

- [ ] **Step 2: Update handlers.rs and mod.rs**

- [ ] **Step 3: Verify and commit**

```bash
git add server/src/discovery/
git commit -m "refactor(discovery): add dedicated DiscoveryError in error.rs"
```

---

### Task 10: (Skipped — crypto will be assessed for Tier 1 consolidation in Task 14)

Crypto is borderline (708 lines, 2 files). Task 14 will assess whether it stays Tier 2 (gets error.rs) or consolidates to Tier 1 (error stays inline).

---

### Task 10a: Verify existing error.rs files match canonical pattern

Modules that already have `error.rs`: `auth`, `voice`, `governance`, `ratelimit`, `workspaces`.

- [ ] **Step 1: Verify naming convention**

Check each module's error enum name matches `{ModuleName}Error`:
- `auth/error.rs` → `AuthError` (expected)
- `voice/error.rs` → `VoiceError` (expected)
- `governance/error.rs` → `GovernanceError` (expected — if named `GovError`, rename)
- `ratelimit/error.rs` → `RatelimitError` (expected)
- `workspaces/error.rs` → `WorkspaceError` (expected)

Run: `grep -n "pub enum" server/src/{auth,voice,governance,ratelimit,workspaces}/error.rs`

- [ ] **Step 2: Verify response format**

Each must use `thiserror`, implement `IntoResponse`, and return JSON with `error` code + `message` fields. Spot-check each file.

- [ ] **Step 3: Fix any violations and commit**

If any renames or format fixes are needed:

```bash
git add server/src/
git commit -m "refactor: align existing error.rs files to canonical naming and format"
```

---

## Phase 1 Checkpoint

Run full test suite and clippy:

```bash
SQLX_OFFLINE=true cargo test -p vc-server
SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
cargo fmt --check
```

All modules now have errors in the canonical location. Review before proceeding.

---

## Phase 2: Types Consolidation

Ensure every Tier 2 module has request/response DTOs in `types.rs`, not scattered in handlers.

### Task 11: Extract types from auth/handlers.rs to auth/types.rs

**Files:**
- Create: `server/src/auth/types.rs`
- Modify: `server/src/auth/handlers.rs` — remove DTO structs
- Modify: `server/src/auth/mod.rs` — add `mod types;`

- [ ] **Step 1: Identify all request/response DTOs in handlers.rs**

Read `auth/handlers.rs` and list all structs with `#[derive(Deserialize)]` or `#[derive(Serialize)]` that represent API DTOs (e.g., `LoginRequest`, `RegisterRequest`, `TokenResponse`, `ProfileResponse`, etc.).

- [ ] **Step 2: Create auth/types.rs**

Move all DTO structs to `auth/types.rs`. Keep serde and utoipa derives.

- [ ] **Step 3: Update handlers.rs imports**

Add `use super::types::{...};` for all moved types. Remove the struct definitions.

- [ ] **Step 4: Update mod.rs**

Add `mod types;`.

- [ ] **Step 5: Verify and commit**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings 2>&1 | tail -10`

```bash
git add server/src/auth/
git commit -m "refactor(auth): extract DTOs to dedicated types.rs"
```

---

### Task 12: Extract types from chat handler files to chat/types.rs

Chat already has DTOs scattered across channels.rs, messages.rs, dm.rs, uploads.rs.

**Files:**
- Create: `server/src/chat/types.rs`
- Modify: `server/src/chat/channels.rs`, `messages.rs`, `dm.rs`, `uploads.rs`
- Modify: `server/src/chat/mod.rs`

- [ ] **Step 1: Inventory all DTO structs across chat files**

- [ ] **Step 2: Create chat/types.rs with all DTOs**

- [ ] **Step 3: Update all chat handler files to import from super::types**

- [ ] **Step 4: Update mod.rs and verify**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings 2>&1 | tail -10`

```bash
git add server/src/chat/
git commit -m "refactor(chat): extract DTOs to dedicated types.rs"
```

---

### Task 13: Extract types from connectivity/handlers.rs

Connectivity is borderline Tier 1 (466 lines). Since it already has 2 files, consolidate into Tier 1 (single mod.rs) instead.

**Files:**
- Modify: `server/src/connectivity/mod.rs` — merge handlers.rs content
- Delete: `server/src/connectivity/handlers.rs`

- [ ] **Step 1: Merge handlers.rs into mod.rs**

Move all content from handlers.rs into mod.rs following Tier 1 ordering: imports, error, types, queries, handlers, router.

- [ ] **Step 2: Delete handlers.rs**

- [ ] **Step 3: Verify and commit**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings 2>&1 | tail -10`

```bash
git add server/src/connectivity/
git commit -m "refactor(connectivity): consolidate to Tier 1 single mod.rs"
```

---

### Task 13a: Consolidate presence to Tier 1

Presence is 556 lines across 2 files (`mod.rs` 5 lines + `types.rs` 551 lines). It has no handlers — just type definitions re-exported from mod.rs. Consolidate into single mod.rs.

**Files:**
- Modify: `server/src/presence/mod.rs` — merge types.rs content
- Delete: `server/src/presence/types.rs`

- [ ] **Step 1: Merge types.rs into mod.rs**

Move all content from `types.rs` into `mod.rs`. Since this module has no handlers or router, mod.rs will just contain the type definitions and their re-exports.

- [ ] **Step 2: Delete types.rs**

- [ ] **Step 3: Update any external imports**

Search for `use crate::presence::types::` or `presence::types::` across the codebase and update to `presence::`.

- [ ] **Step 4: Verify and commit**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings 2>&1 | tail -10`

```bash
git add server/src/presence/
git commit -m "refactor(presence): consolidate to Tier 1 single mod.rs"
```

---

### Task 14: Consolidate crypto to Tier 1

Crypto is 708 lines (borderline) but only has handlers.rs + mod.rs. Read the file to decide — if it has distinct concerns that warrant splitting, keep as Tier 2 with error.rs + types.rs. If cohesive, consolidate to Tier 1.

**Files:**
- Modify: `server/src/crypto/mod.rs`
- Possibly delete: `server/src/crypto/handlers.rs`

- [ ] **Step 1: Read crypto/handlers.rs and assess**

If under 3 endpoint groups and cohesive, merge into mod.rs (Tier 1). If complex, keep split and ensure it has the standard Tier 2 files.

- [ ] **Step 2: Execute the chosen approach**

- [ ] **Step 3: Verify and commit**

```bash
git add server/src/crypto/
git commit -m "refactor(crypto): reorganize to canonical tier structure"
```

---

## Phase 2 Checkpoint

```bash
SQLX_OFFLINE=true cargo test -p vc-server
SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
cargo fmt --check
```

All modules now have types in the canonical location. Review before proceeding.

---

## Phase 3: Query Extraction

Move inline sqlx queries from handler files to dedicated `queries.rs`. This is the highest-effort server change.

### Task 15: Create auth/queries.rs

**Files:**
- Create: `server/src/auth/queries.rs`
- Modify: `server/src/auth/handlers.rs` — replace inline queries with function calls
- Modify: `server/src/auth/mod.rs` — add `mod queries;`

- [ ] **Step 1: Identify all sqlx queries in auth/handlers.rs**

Search for `sqlx::query`, `sqlx::query_as`, `sqlx::query_scalar` in handlers.rs. List each query with its purpose.

- [ ] **Step 2: Create auth/queries.rs**

For each query, create a function:
```rust
pub async fn find_user_by_email(pool: &PgPool, email: &str) -> Result<Option<User>, AuthError> {
    sqlx::query_as!(...)
        .fetch_optional(pool)
        .await
        .map_err(|e| AuthError::Internal(e.into()))
}
```

- [ ] **Step 3: Update handlers.rs**

Replace each inline query with a call to `queries::function_name(...)`.

- [ ] **Step 4: Update mod.rs, verify, and commit**

Add `mod queries;`.

Run: `SQLX_OFFLINE=true cargo test -p vc-server 2>&1 | tail -5`

```bash
git add server/src/auth/
git commit -m "refactor(auth): extract database queries to queries.rs"
```

---

### Task 16: Create guild/queries.rs

**Files:**
- Create: `server/src/guild/queries.rs`
- Modify: `server/src/guild/handlers.rs`, `categories.rs`, `roles.rs`, `emojis.rs`, `invites.rs`
- Modify: `server/src/guild/mod.rs`

- [ ] **Step 1: Inventory all sqlx queries across guild files**

- [ ] **Step 2: Create guild/queries.rs with all query functions**

- [ ] **Step 3: Update all guild handler files to call queries::**

- [ ] **Step 4: Update mod.rs, verify, and commit**

```bash
git add server/src/guild/
git commit -m "refactor(guild): extract database queries to queries.rs"
```

---

### Task 17: Create chat/queries.rs

**Files:**
- Create: `server/src/chat/queries.rs`
- Modify: `server/src/chat/channels.rs`, `messages.rs`, `dm.rs`, `uploads.rs`
- Modify: `server/src/chat/mod.rs`

- [ ] **Step 1: Inventory all sqlx queries across chat files**

- [ ] **Step 2: Create chat/queries.rs**

- [ ] **Step 3: Update all chat handler files**

- [ ] **Step 4: Verify and commit**

```bash
git add server/src/chat/
git commit -m "refactor(chat): extract database queries to queries.rs"
```

---

### Task 18: Create admin/queries.rs

**Files:**
- Create: `server/src/admin/queries.rs`
- Modify: `server/src/admin/handlers.rs`
- Modify: `server/src/admin/mod.rs`

- [ ] **Step 1: Inventory sqlx queries in admin/handlers.rs (2430 lines)**

- [ ] **Step 2: Create admin/queries.rs**

- [ ] **Step 3: Update handlers.rs**

- [ ] **Step 4: Verify and commit**

```bash
git add server/src/admin/
git commit -m "refactor(admin): extract database queries to queries.rs"
```

---

### Task 19: Create moderation/queries.rs

Moderation already has `filter_queries.rs`. Rename to `queries.rs` and add non-filter queries.

**Files:**
- Rename: `server/src/moderation/filter_queries.rs` → merge into `server/src/moderation/queries.rs`
- Modify: `server/src/moderation/handlers.rs`, `admin_handlers.rs`, `filter_handlers.rs`
- Modify: `server/src/moderation/mod.rs`

- [ ] **Step 1: Inventory all queries across moderation files**

- [ ] **Step 2: Create unified queries.rs**

Merge `filter_queries.rs` content and add any queries from handler files.

- [ ] **Step 3: Update handler files and mod.rs**

- [ ] **Step 4: Verify and commit**

```bash
git add server/src/moderation/
git commit -m "refactor(moderation): consolidate queries into queries.rs"
```

---

### Task 20: Create social/queries.rs

**Files:**
- Create: `server/src/social/queries.rs`
- Modify: `server/src/social/friends.rs`
- Modify: `server/src/social/mod.rs`

- [ ] **Step 1: Extract queries from friends.rs to queries.rs**

- [ ] **Step 2: Update friends.rs and mod.rs**

- [ ] **Step 3: Verify and commit**

```bash
git add server/src/social/
git commit -m "refactor(social): extract database queries to queries.rs"
```

---

### Task 21: Create workspaces/queries.rs

**Files:**
- Create: `server/src/workspaces/queries.rs`
- Modify: `server/src/workspaces/handlers.rs`
- Modify: `server/src/workspaces/mod.rs`

- [ ] **Step 1: Extract queries from handlers.rs to queries.rs**

- [ ] **Step 2: Update handlers.rs and mod.rs**

- [ ] **Step 3: Verify and commit**

```bash
git add server/src/workspaces/
git commit -m "refactor(workspaces): extract database queries to queries.rs"
```

---

### Task 21a: Create governance/queries.rs

Governance has inline queries across `deletion.rs`, `export.rs`, and `handlers.rs`.

**Files:**
- Create: `server/src/governance/queries.rs`
- Modify: `server/src/governance/deletion.rs`, `export.rs`, `handlers.rs`
- Modify: `server/src/governance/mod.rs`

- [ ] **Step 1: Inventory queries across governance files**

- [ ] **Step 2: Create queries.rs with all query functions**

- [ ] **Step 3: Update handler files to call queries::**

- [ ] **Step 4: Verify and commit**

```bash
git add server/src/governance/
git commit -m "refactor(governance): extract database queries to queries.rs"
```

---

### Task 21b: Create voice/queries.rs (if applicable)

Voice uses an in-memory SFU — most state is not in the database. Check whether there are any sqlx queries in the voice module.

- [ ] **Step 1: Search for sqlx usage in voice/**

Run: `grep -r "sqlx::query" server/src/voice/`

If queries exist, extract them to `voice/queries.rs`. If no sqlx queries, skip this task.

- [ ] **Step 2: If applicable, create queries.rs and update**

- [ ] **Step 3: Verify and commit if changes were made**

```bash
git add server/src/voice/
git commit -m "refactor(voice): extract database queries to queries.rs"
```

---

### Task 21c: Create discovery/queries.rs (if applicable)

- [ ] **Step 1: Search for sqlx usage in discovery/**

Run: `grep -r "sqlx::query" server/src/discovery/`

If queries exist, extract to `discovery/queries.rs`. Discovery is Tier 2 per the spec, so queries should go in a dedicated file.

- [ ] **Step 2: If applicable, create queries.rs and update**

- [ ] **Step 3: Verify and commit if changes were made**

```bash
git add server/src/discovery/
git commit -m "refactor(discovery): extract database queries to queries.rs"
```

---

## Phase 3 Checkpoint

```bash
SQLX_OFFLINE=true cargo test -p vc-server
SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
cargo fmt --check
```

All database queries now live in `queries.rs`. Review before proceeding.

---

## Phase 4: Handler Splitting

Break up monolithic handler files exceeding ~1000 lines.

### Task 22: Split auth/handlers.rs (2716 lines)

**Files:**
- Create: `server/src/auth/login.rs`, `register.rs`, `mfa.rs`, `sessions.rs`, `profile.rs`
- Delete: `server/src/auth/handlers.rs`
- Modify: `server/src/auth/mod.rs` — replace `mod handlers;` with sub-modules

- [ ] **Step 1: Map handler functions to sub-domains**

Read `auth/handlers.rs` and group handlers:
- `login.rs`: login, logout, refresh_token
- `register.rs`: register, verify_email, resend_verification
- `mfa.rs`: mfa_setup, mfa_verify, mfa_disable, backup codes
- `sessions.rs`: list_sessions, revoke_session, revoke_all
- `profile.rs`: get_profile, update_profile, upload_avatar, update_password

- [ ] **Step 2: Create each sub-domain file**

Move handlers to their respective files. Each file imports from `super::error`, `super::types`, `super::queries`.

- [ ] **Step 3: Delete handlers.rs**

- [ ] **Step 4: Update mod.rs**

Replace `mod handlers;` with:
```rust
mod login;
mod register;
mod mfa;
mod sessions;
mod profile;
```

Update the `router()` function to reference `login::login_handler`, `register::register_handler`, etc.

- [ ] **Step 5: Verify and commit**

Run: `SQLX_OFFLINE=true cargo test -p vc-server 2>&1 | tail -5`
Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings 2>&1 | tail -10`

```bash
git add server/src/auth/
git commit -m "refactor(auth): split monolithic handlers.rs into sub-domain files"
```

---

### Task 23: Split admin/handlers.rs (2430 lines)

**Files:**
- Create: `server/src/admin/users.rs`, `guilds.rs`, `audit.rs`, `system.rs`
- Delete: `server/src/admin/handlers.rs`
- Modify: `server/src/admin/mod.rs`

- [ ] **Step 1: Map handlers to sub-domains**

- `users.rs`: user listing, banning, deletion, details
- `guilds.rs`: guild listing, suspension, deletion, details
- `audit.rs`: audit log, CSV exports, bulk actions
- `system.rs`: status, elevation, cache management

- [ ] **Step 2: Create sub-domain files and move handlers**

- [ ] **Step 3: Delete handlers.rs, update mod.rs**

- [ ] **Step 4: Verify and commit**

```bash
git add server/src/admin/
git commit -m "refactor(admin): split monolithic handlers.rs into sub-domain files"
```

---

### Task 24: Split pages/handlers.rs (1991 lines)

**Files:**
- Create: `server/src/pages/platform.rs`, `guild_pages.rs`, `revisions.rs`, `categories.rs`
- Delete: `server/src/pages/handlers.rs`
- Modify: `server/src/pages/mod.rs`

- [ ] **Step 1: Map handlers to sub-domains**

- `platform.rs`: platform page CRUD
- `guild_pages.rs`: guild page CRUD, acceptance
- `revisions.rs`: revision listing, restore
- `categories.rs`: page category CRUD

- [ ] **Step 2: Create sub-domain files, move router from router.rs to mod.rs**

Also remove the separate `router.rs` file — move its router functions into `mod.rs` to match the canonical pattern.

- [ ] **Step 3: Delete handlers.rs and router.rs, update mod.rs**

- [ ] **Step 4: Verify and commit**

```bash
git add server/src/pages/
git commit -m "refactor(pages): split handlers, move router to mod.rs"
```

---

### Task 25: Split guild/handlers.rs (1643 lines)

**Files:**
- Create: `server/src/guild/core.rs`, `members.rs`, `settings.rs`, `bots.rs`
- Delete: `server/src/guild/handlers.rs`
- Modify: `server/src/guild/mod.rs`

- [ ] **Step 1: Map handlers to sub-domains**

Guild already has some splits (categories.rs, roles.rs, emojis.rs, invites.rs). The remaining handlers.rs needs splitting:
- `core.rs`: create, update, delete guild
- `members.rs`: member listing, kicking, guild membership
- `settings.rs`: guild settings, usage stats
- `bots.rs`: bot management within guilds

- [ ] **Step 2: Create sub-domain files**

- [ ] **Step 3: Delete handlers.rs, update mod.rs**

- [ ] **Step 4: Verify and commit**

```bash
git add server/src/guild/
git commit -m "refactor(guild): split monolithic handlers.rs into sub-domain files"
```

---

### Task 26: Split ws/mod.rs (2237 lines)

The WebSocket gateway module has all logic in mod.rs. This is a special case — it's not REST handlers but a WebSocket message dispatcher. Note: `bot_gateway.rs` (688 lines) and `bot_events.rs` (158 lines) already exist as separate files and should remain.

**Files:**
- Create: `server/src/ws/handlers.rs`, `server/src/ws/events.rs`
- Modify: `server/src/ws/mod.rs` — keep only re-exports and connection setup
- Keep: `server/src/ws/bot_gateway.rs`, `server/src/ws/bot_events.rs` (already split)

- [ ] **Step 1: Analyze ws/mod.rs structure**

Read and identify logical groups: connection setup, authentication, event dispatching, message handling.

- [ ] **Step 2: Split into focused files**

- `mod.rs`: WebSocket upgrade handler, connection setup, re-exports
- `handlers.rs`: message dispatch logic, event processing
- `events.rs`: event type definitions if any

- [ ] **Step 3: Update mod.rs, verify and commit**

```bash
git add server/src/ws/
git commit -m "refactor(ws): split monolithic mod.rs into focused files"
```

---

## Phase 4 Checkpoint

```bash
SQLX_OFFLINE=true cargo test -p vc-server
SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
cargo fmt --check
```

All handler files are now under ~1000 lines. Review before proceeding.

---

## Phase 5: api/ Module Dissolution

Move endpoint files from `api/` into their feature modules.

### Task 27: Move api/ endpoints to feature modules

**Files to move:**

| Source | Target | Notes |
|--------|--------|-------|
| `api/pins.rs` | `chat/pins.rs` | Existing chat module |
| `api/channel_pins.rs` | `chat/channel_pins.rs` | Existing chat module |
| `api/reactions.rs` | `chat/reactions.rs` | Existing chat module |
| `api/unread.rs` | `chat/unread.rs` | Existing chat module |
| `api/files.rs` | `chat/files.rs` | Existing chat module |
| `api/favorites.rs` | New `favorites/mod.rs` (Tier 1) | New module |
| `api/global_search.rs` | New `search/mod.rs` (Tier 1) | New module |
| `api/setup.rs` | New `setup/mod.rs` (Tier 1) | New module |
| `api/preferences.rs` | New `preferences/mod.rs` (Tier 1) | New module |
| `api/settings.rs` | New `settings/mod.rs` (Tier 1) | New module |
| `api/bots.rs` | `guild/bots.rs` or new `bots/` module | Assess scope |
| `api/commands.rs` | With bots | Assess scope |

- [ ] **Step 1: Move chat-related files**

Move `pins.rs`, `channel_pins.rs`, `reactions.rs`, `unread.rs`, `files.rs` to `chat/`. Update imports in each file to use `super::` instead of `super::` (adjust for new module context). Add `mod` declarations to `chat/mod.rs`. Add router entries for these endpoints.

- [ ] **Step 2: Verify chat moves**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings 2>&1 | tail -10`

- [ ] **Step 3: Create new Tier 1 modules**

For `favorites`, `search`, `setup`, `preferences`, `settings`: create `server/src/{module}/mod.rs` with the file content restructured to Tier 1 format (imports, error, types, queries, handlers, router — all in mod.rs).

- [ ] **Step 4: Register new modules**

Add `mod favorites;`, `mod search;`, `mod setup;`, `mod preferences;`, `mod settings;` to `server/src/lib.rs` or wherever modules are declared.

- [ ] **Step 5: Move bot-related files**

Assess whether `bots.rs` and `commands.rs` belong in `guild/` or need a dedicated `bots/` module. Move accordingly.

- [ ] **Step 6: Clean up api/mod.rs**

Remove all `mod` declarations for moved files. `api/mod.rs` should now only contain:
- `AppState` definition (if it lives here)
- The top-level `app_router()` function that nests all feature routers

- [ ] **Step 7: Update the top-level router**

Update `api/mod.rs` router to use the new module paths:
```rust
.nest("/api/favorites", favorites::router())
.nest("/api/search", search::router())
// etc.
```

- [ ] **Step 8: Verify full build**

Run: `SQLX_OFFLINE=true cargo test -p vc-server 2>&1 | tail -5`
Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings 2>&1 | tail -10`

- [ ] **Step 9: Commit**

```bash
git add server/src/
git commit -m "refactor(api): dissolve api/ endpoints into feature modules"
```

---

## Phase 5 Checkpoint

```bash
SQLX_OFFLINE=true cargo test -p vc-server
SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
cargo fmt --check
```

`api/` is now just the routing table. Review before proceeding to client.

---

## Phase 6: mod.rs Cleanup

Ensure every Tier 2 module's mod.rs only contains re-exports and router(). Move any utility functions, logic, or type definitions out.

### Task 28: Audit and clean all mod.rs files

- [ ] **Step 1: Check each module's mod.rs**

For each Tier 2 module, read mod.rs and verify it only contains:
- Module doc comment
- `mod` declarations
- `pub use` re-exports
- `router()` function

If it contains anything else (helper functions, utility code, type definitions), move that code to the appropriate file.

- [ ] **Step 2: Fix any violations found**

Common violations to look for:
- `admin/mod.rs`: may have cache helper functions → move to a utility file
- `auth/mod.rs`: may have middleware setup → keep in mod.rs if it's part of router setup, otherwise extract
- `ws/mod.rs`: should now be clean from Task 26

- [ ] **Step 3: Verify and commit**

```bash
git add server/src/
git commit -m "refactor: clean mod.rs files to canonical re-exports-only pattern"
```

---

## Phase 6 Checkpoint

Server refactoring complete. Full verification:

```bash
SQLX_OFFLINE=true cargo test -p vc-server
SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
cargo fmt --check
```

---

## Phase 7: Client Monolith Splitting

### Task 29: Split lib/tauri.ts into lib/tauri/

**Files:**
- Create: `client/src/lib/tauri/index.ts`, `auth.ts`, `channels.ts`, `messages.ts`, `guilds.ts`, `voice.ts`, `social.ts`, `admin.ts`, `uploads.ts`, `settings.ts`, `pages.ts`, `e2ee.ts`, `websocket.ts`, `search.ts`
- Delete: `client/src/lib/tauri.ts`

- [ ] **Step 1: Create lib/tauri/ directory**

- [ ] **Step 2: Split functions by domain**

Move functions from `tauri.ts` to domain files based on the audit:
- `auth.ts`: login, register, logout, sessions, MFA, QR login, OIDC (~lines 444–1091)
- `channels.ts`: channel CRUD, permissions (~lines 1152–1188, 3420–3457)
- `messages.ts`: message CRUD, threads, reactions, pins (~lines 1211–1401, 4186–4289)
- `guilds.ts`: guild CRUD, invites, categories, emojis, roles, members (~lines 1473–2073, 3291–3399)
- `voice.ts`: voice join/leave, mute/deafen, DM calls (~lines 2446–2552)
- `social.ts`: friends, blocking (~lines 2087–2171)
- `admin.ts`: admin users, guilds, audit, reports, observability, auth settings (~lines 2241–2291, 3500+)
- `uploads.ts`: upload limits, file upload (~lines 235–343)
- `settings.ts`: settings, preferences, UI state (~lines 2561–2616)
- `pages.ts`: platform pages, guild pages, revisions, page categories (~lines 2865–3266)
- `e2ee.ts`: encryption functions (~lines 3937–4177)
- `websocket.ts`: WS connect, subscribe, typing, media (~lines 2630–2853)
- `search.ts`: global search, guild search, DM search
- `common.ts`: shared helpers, base URL config, fetch wrappers

- [ ] **Step 3: Create index.ts with re-exports**

```typescript
export * from './auth';
export * from './channels';
export * from './messages';
// ... all domain files
```

- [ ] **Step 4: Verify no import breakage**

Run: `cd client && bun run check`
Run: `cd client && bun run test:run`

- [ ] **Step 5: Commit**

```bash
git add client/src/lib/tauri/
git rm client/src/lib/tauri.ts
git commit -m "refactor(client): split monolithic tauri.ts into domain modules"
```

---

### Task 30: Split stores/websocket.ts into stores/websocket/

**Files:**
- Create: `client/src/stores/websocket/index.ts`, `messageEvents.ts`, `presenceEvents.ts`, `voiceEvents.ts`, `guildEvents.ts`, `socialEvents.ts`, `adminEvents.ts`, `threadEvents.ts`, `mediaEvents.ts`
- Delete: `client/src/stores/websocket.ts`

- [ ] **Step 1: Create stores/websocket/ directory**

- [ ] **Step 2: Split event handlers by domain**

Based on the audit:
- `messageEvents.ts`: message_new, message_edit, message_delete
- `presenceEvents.ts`: presence_update, rich_presence, custom_status, typing
- `voiceEvents.ts`: voice_*, ice_candidate, room_state, mute/unmute
- `guildEvents.ts`: guild_emoji_updated, channel/role updates
- `socialEvents.ts`: friend requests, blocking, DM events
- `adminEvents.ts`: admin_user_*, admin_guild_*, admin_report_*
- `threadEvents.ts`: thread_message_*, thread_reply_count
- `mediaEvents.ts`: screen_share_*, webcam_*, voice_user_stats

Each file exports handler functions.

- [ ] **Step 3: Create index.ts**

Contains WebSocket connection logic and the dispatch table that imports and wires all event handlers.

- [ ] **Step 4: Verify**

Run: `cd client && bun run check`
Run: `cd client && bun run test:run`

- [ ] **Step 5: Commit**

```bash
git add client/src/stores/websocket/
git rm client/src/stores/websocket.ts
git commit -m "refactor(client): split websocket store into domain event modules"
```

---

### Task 31: Split lib/types.ts into lib/types/

**Files:**
- Create: `client/src/lib/types/index.ts`, `user.ts`, `guild.ts`, `channel.ts`, `message.ts`, `voice.ts`, `auth.ts`, `admin.ts`, `pages.ts`, `social.ts`, `e2ee.ts`, `common.ts`, `events.ts`, `preferences.ts`
- Delete: `client/src/lib/types.ts`

- [ ] **Step 1: Create lib/types/ directory**

- [ ] **Step 2: Split types by domain**

Based on the audit:
- `user.ts`: User, UserProfile, UserPresence, Activity, CustomStatus, SessionInfo (~6 types)
- `guild.ts`: Guild, GuildSettings, GuildMember, GuildInvite, GuildRole, GuildEmoji (~12 types)
- `channel.ts`: Channel, ChannelType, ChannelCategory, ChannelOverride (~6 types)
- `message.ts`: Message, Attachment, Reaction, ThreadInfo, PaginatedMessages, ChannelPin (~8 types)
- `voice.ts`: VoiceParticipant, WebcamServerInfo, ScreenShareServerInfo, AudioSettings (~5 types)
- `auth.ts`: LoginRequest, RegisterRequest, TokenResponse (~4 types)
- `admin.ts`: AdminStats, UserSummary, GuildSummary, AuditLogEntry, paginated responses (~15 types)
- `pages.ts`: Page, PageRevision, PageCategory, CreatePageRequest (~8 types)
- `social.ts`: Friend, DMChannel, DMListItem, FriendshipStatus (~7 types)
- `e2ee.ts`: E2EEStatus, PrekeyData, DeviceKeys, EncryptedMessage (~10 types)
- `events.ts`: ClientEvent, ServerEvent (the large union types)
- `preferences.ts`: UserPreferences, FocusMode, NotificationPreferences, DisplayPreferences (~12 types)
- `common.ts`: ThemeName, UserStatus, QualityLevel, shared enums and utility types

- [ ] **Step 3: Create index.ts with re-exports**

```typescript
export * from './user';
export * from './guild';
// ... all domain files
```

- [ ] **Step 4: Verify**

Run: `cd client && bun run check`
Run: `cd client && bun run test:run`

- [ ] **Step 5: Commit**

```bash
git add client/src/lib/types/
git rm client/src/lib/types.ts
git commit -m "refactor(client): split monolithic types.ts into domain modules"
```

---

## Phase 7 Checkpoint

Full client verification:

```bash
cd client && bun run check
cd client && bun run test:run
```

---

## Phase 8: Module Documentation

### Task 32: Add module doc comments to all mod.rs files

- [ ] **Step 1: Add doc comments to modules missing them**

Every module's `mod.rs` must start with:
```rust
//! Short noun phrase
//!
//! What this module handles (1-2 lines).
```

Go through each module and add or update the doc comment.

- [ ] **Step 2: Verify and commit**

Run: `cargo fmt --check`

```bash
git add server/src/
git commit -m "docs: add module doc comments to all server modules"
```

---

## Phase 9: Standards Integration

### Task 33: Update CLAUDE.md with standards reference

- [ ] **Step 1: Add reference to standards doc in CLAUDE.md**

Add a pointer under the Documentation Pointers section:
```markdown
- `docs/superpowers/specs/2026-04-10-codebase-consistency-standards-design.md` — Canonical module structure and code organization patterns
```

- [ ] **Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: add reference to codebase consistency standards"
```

---

## Final Verification

```bash
# Server
SQLX_OFFLINE=true cargo test -p vc-server
SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
cargo fmt --check

# Client
cd client && bun run check
cd client && bun run test:run

# Workspace
SQLX_OFFLINE=true cargo clippy -- -D warnings
```

All 33 tasks complete. Codebase now follows the canonical patterns from the spec.
