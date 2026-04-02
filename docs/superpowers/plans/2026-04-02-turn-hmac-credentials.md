# TURN HMAC Time-Limited Credentials — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace static TURN credentials with per-request HMAC-SHA1 time-limited credentials (RFC 5766 / coturn `use-auth-secret` mode).

**Architecture:** Add `sha1` crate, extend config with `turn_shared_secret` and `turn_credential_ttl`, generate HMAC credentials in `get_ice_servers` handler with `AuthUser` extractor. Fall back to static credentials when shared secret is not set.

**Tech Stack:** Rust (axum, hmac, sha1, base64), coturn

**Spec:** `docs/superpowers/specs/2026-04-02-beta-security-fixes-design.md`

---

## File Map

| File | Responsibility |
|------|---------------|
| `Cargo.toml` (workspace root) | Add `sha1 = "0.10"` workspace dependency |
| `server/Cargo.toml` | Add `sha1.workspace = true` |
| `server/src/config.rs` | Add `turn_shared_secret`, `turn_credential_ttl` fields + env parsing |
| `server/src/voice/handlers.rs` | HMAC credential generation with `AuthUser` extractor |

---

## Task 1: Add `sha1` dependency

**Files:**
- Modify: `Cargo.toml` (workspace root, dependencies section)
- Modify: `server/Cargo.toml`

- [ ] **Step 1: Add sha1 to workspace Cargo.toml**

In the root `Cargo.toml`, in the `[workspace.dependencies]` section (near `sha2 = "0.10"` at line 62), add:

```toml
sha1 = "0.10"
```

- [ ] **Step 2: Add sha1 to server Cargo.toml**

In `server/Cargo.toml`, in the `[dependencies]` section (near `hmac.workspace = true` at line 47), add:

```toml
sha1.workspace = true
```

- [ ] **Step 3: Verify it compiles**

Run: `SQLX_OFFLINE=true cargo check -p vc-server`
Expected: PASS

---

## Task 2: Extend config with TURN HMAC fields

**Files:**
- Modify: `server/src/config.rs:127-133` (struct fields)
- Modify: `server/src/config.rs:313-315` (env parsing)
- Modify: `server/src/config.rs:501-503` (test defaults)

- [ ] **Step 1: Add struct fields**

At `server/src/config.rs`, after `turn_credential` (line 133), add:

```rust
    /// Shared secret for TURN HMAC credential generation (coturn `use-auth-secret` mode).
    /// When set, time-limited credentials are generated per-request instead of using static ones.
    pub turn_shared_secret: Option<String>,

    /// TURN credential TTL in seconds (default: 3600 = 1 hour).
    pub turn_credential_ttl: u64,
```

- [ ] **Step 2: Add env parsing**

At `server/src/config.rs`, after `turn_credential` env parsing (line 315), add:

```rust
            turn_shared_secret: env::var("TURN_SHARED_SECRET").ok().filter(|s| !s.is_empty()),
            turn_credential_ttl: env::var("TURN_CREDENTIAL_TTL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3600),
```

- [ ] **Step 3: Add test defaults**

At `server/src/config.rs`, after `turn_credential: None` in the test defaults (line 503), add:

```rust
            turn_shared_secret: None,
            turn_credential_ttl: 3600,
```

- [ ] **Step 4: Build and verify**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml server/Cargo.toml server/src/config.rs
git commit -m "feat(voice): add TURN HMAC config fields and sha1 dependency

Add turn_shared_secret and turn_credential_ttl to AppConfig for
coturn use-auth-secret mode. Add sha1 0.10 workspace dependency
for HMAC-SHA1 credential generation."
```

---

## Task 3: HMAC credential generation in `get_ice_servers`

**Files:**
- Modify: `server/src/voice/handlers.rs:1-10` (imports)
- Modify: `server/src/voice/handlers.rs:47-66` (handler)

- [ ] **Step 1: Add imports**

At `server/src/voice/handlers.rs`, replace the imports section (lines 1-10):

```rust
//! Voice HTTP Handlers
//!
//! HTTP endpoints for voice-related operations.
//! Voice signaling (join/leave/offer/answer/ice) is handled via WebSocket.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::Json;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use hmac::{Hmac, Mac};
use serde::Serialize;
use sha1::Sha1;

use crate::api::AppState;
use crate::auth::AuthUser;
```

- [ ] **Step 2: Replace get_ice_servers handler**

At `server/src/voice/handlers.rs`, replace the `get_ice_servers` function (lines 47-66):

```rust
pub async fn get_ice_servers(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Json<IceServersResponse> {
    let mut servers = vec![IceServer {
        urls: vec![state.config.stun_server.clone()],
        username: None,
        credential: None,
    }];

    // Prefer HMAC time-limited credentials when shared secret is configured
    if let (Some(turn), Some(secret)) = (&state.config.turn_server, &state.config.turn_shared_secret) {
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_secs()
            + state.config.turn_credential_ttl;
        let username = format!("{}:{}", expiry, auth_user.id);

        let mut mac = Hmac::<Sha1>::new_from_slice(secret.as_bytes())
            .expect("HMAC-SHA1 accepts any key length");
        mac.update(username.as_bytes());
        let credential = BASE64_STANDARD.encode(mac.finalize().into_bytes());

        servers.push(IceServer {
            urls: vec![turn.clone()],
            username: Some(username),
            credential: Some(credential),
        });
    } else if let Some(turn) = &state.config.turn_server {
        // Fallback: static credentials for dev environments
        servers.push(IceServer {
            urls: vec![turn.clone()],
            username: state.config.turn_username.clone(),
            credential: state.config.turn_credential.clone(),
        });
    }

    Json(IceServersResponse {
        ice_servers: servers,
    })
}
```

- [ ] **Step 3: Build and verify**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add server/src/voice/handlers.rs
git commit -m "feat(voice): generate HMAC time-limited TURN credentials

Replace static TURN credentials with per-request HMAC-SHA1
credentials when TURN_SHARED_SECRET is configured. Credentials
expire after turn_credential_ttl seconds (default 1 hour).
Falls back to static credentials for dev environments."
```

---

## Task 4: Docs — CHANGELOG, checklist, push, PR

**Files:**
- Modify: `CHANGELOG.md`
- Modify: `docs/developer-guide/plans/2026-03-19-beta-checklist.md`

- [ ] **Step 1: Add CHANGELOG entry**

Under `### Security` (or `### Changed`) in `[Unreleased]`:

```markdown
- TURN relay credentials are now time-limited (1-hour HMAC-SHA1) instead of static when `TURN_SHARED_SECRET` is configured
```

- [ ] **Step 2: Mark checklist item done**

In `docs/developer-guide/plans/2026-03-19-beta-checklist.md`:

Line 124: `- [ ] TURN credentials are static/long-lived`
→ `- [x] TURN credentials are static/long-lived (#PR_NUMBER)`

- [ ] **Step 3: Commit, push, create PR**

```bash
git add CHANGELOG.md docs/developer-guide/plans/2026-03-19-beta-checklist.md
git commit -m "docs: update CHANGELOG and checklist for TURN HMAC credentials"
git push -u origin fix/turn-hmac-credentials
gh pr create --title "feat(voice): TURN HMAC time-limited credentials" --body "..."
```
