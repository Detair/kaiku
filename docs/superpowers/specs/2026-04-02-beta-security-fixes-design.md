# Beta Security Fixes — Design Spec

**Date:** 2026-04-02
**Status:** Draft
**Scope:** 3 security items from the beta checklist (virus scanning deferred)

## Overview

Three security fixes split across two branches:
- **Branch 1 (`fix/beta-security-quick`):** Megolm session cache cleanup on logout + `returnUrl` encoding
- **Branch 2 (`fix/turn-hmac-credentials`):** TURN HMAC time-limited credentials

## Fix 1: Megolm Session Cache Cleanup on Logout

### Problem

`AppState.crypto` (`Arc<Mutex<Option<CryptoManager>>>`) is never cleared on logout. The `CryptoManager` holds a `LocalKeyStore` backed by encrypted SQLite (`keys.db`) containing Olm accounts, Olm sessions, and Megolm inbound/outbound group sessions. If the device is compromised after logout, cached Megolm session keys could decrypt past messages.

### Current Logout Flow

1. Frontend `auth.ts:logout()` calls `wsDisconnect()`, `cleanupWebSocket()`, `stopIdleDetectionCleanup()`, `cleanupPresence()`, `clearAllDrafts()`, `cleanupDrafts()`
2. Then calls `tauri.logout()` which hits `commands/auth.rs:logout()`
3. Tauri `logout()` invalidates refresh token on server, clears `auth` state, clears stored credentials
4. **Neither step touches `state.crypto` or the E2EE store signals**

### Design

**Tauri side (`commands/auth.rs:logout()`):**

After clearing auth state (line 301), drop the `CryptoManager`:

```rust
// Clear crypto state — drops Megolm sessions, Olm account, closes SQLite
if let Ok(mut crypto) = state.crypto.lock() {
    *crypto = None;
}
```

Using `if let Ok` rather than `?` because logout should succeed even if the mutex is poisoned.

**Frontend side (`stores/auth.ts:logout()`):**

After `tauri.logout()` (line 409), reset the E2EE store:

```typescript
resetE2EEState();
```

**New export in `stores/e2ee.ts`:**

```typescript
export function resetE2EEState(): void {
  setStatus({ initialized: false, device_id: null, has_identity_keys: false });
  setIsInitializing(false);
  setError(null);
}
```

### What This Achieves

- Dropping `CryptoManager` closes the SQLite connection and releases all in-memory Olm/Megolm session objects
- The encrypted `keys.db` file remains on disk but is inaccessible without the encryption key (derived from user identity during `initE2EE`)
- On next login, `initE2EE` creates a fresh `CryptoManager`, which opens or creates `keys.db` with the new user's key
- Frontend signals reset to "not initialized" so E2EE UI reflects the logged-out state

### Files

| File | Change |
|------|--------|
| `client/src-tauri/src/commands/auth.rs:301` | Drop `state.crypto` after clearing auth |
| `client/src/stores/e2ee.ts` | Add `resetE2EEState()` export |
| `client/src/stores/auth.ts:409` | Call `resetE2EEState()` after `tauri.logout()` |

---

## Fix 2: `returnUrl` Encoding

### Problem

`AuthGuard.tsx:29` embeds `location.pathname` directly into a query string without URL encoding:

```typescript
navigate(`/login${returnUrl !== "/" ? `?returnUrl=${returnUrl}` : ""}`, { replace: true });
```

If `pathname` contains characters like `?`, `&`, `#`, or spaces, the URL structure breaks. The `Login.tsx` validation (line 83) already prevents open redirect attacks (`startsWith("/")` and `!startsWith("//")`), but the unencoded value could cause parsing issues.

### Design

**`AuthGuard.tsx:29`** — encode the value:

```typescript
navigate(`/login${returnUrl !== "/" ? `?returnUrl=${encodeURIComponent(returnUrl)}` : ""}`, {
  replace: true,
});
```

**`Login.tsx:82-85`** — decode before validating:

```typescript
const raw = searchParams.returnUrl;
const returnUrl = Array.isArray(raw) ? raw[0] : raw;
const decoded = returnUrl ? decodeURIComponent(returnUrl) : null;
const target = decoded && decoded.startsWith("/") && !decoded.startsWith("//")
  ? decoded
  : "/";
navigate(target, { replace: true });
```

### Files

| File | Change |
|------|--------|
| `client/src/components/auth/AuthGuard.tsx:29` | Wrap `returnUrl` with `encodeURIComponent()` |
| `client/src/views/Login.tsx:82-85` | Add `decodeURIComponent()` before validation |

---

## Fix 3: TURN HMAC Time-Limited Credentials

### Problem

`voice/handlers.rs:55-60` returns static TURN credentials from environment variables (`TURN_USERNAME`, `TURN_CREDENTIAL`). These never expire — if leaked, they're valid indefinitely for relay traffic.

### Design

Replace static credentials with per-request HMAC-SHA1 credentials per RFC 5766 / coturn `use-auth-secret` mode.

**Config changes (`config.rs`):**

Replace `turn_username` and `turn_credential` with:

```rust
/// Shared secret for TURN HMAC credential generation (coturn `use-auth-secret` mode).
/// When set, credentials are generated per-request with a TTL.
pub turn_shared_secret: Option<String>,

/// TURN credential TTL in seconds (default: 3600 = 1 hour).
pub turn_credential_ttl: u64,
```

Keep `turn_username` and `turn_credential` as fallback for dev environments without HMAC-capable TURN servers.

**Credential generation (`voice/handlers.rs`):**

The `get_ice_servers` handler needs the authenticated user ID. It currently takes only `State(state)` — add `AuthUser` extractor.

```rust
pub async fn get_ice_servers(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Json<IceServersResponse> {
    // ...
    if let (Some(turn), Some(secret)) = (&state.config.turn_server, &state.config.turn_shared_secret) {
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + state.config.turn_credential_ttl;
        let username = format!("{}:{}", expiry, auth_user.id);

        let mut mac = Hmac::<Sha1>::new_from_slice(secret.as_bytes())
            .expect("HMAC accepts any key length");
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
}
```

**Dependencies:**

- `sha1 = "0.10"` added to workspace `Cargo.toml` (MIT/Apache-2.0 — license-compliant)
- `hmac` already in workspace
- `base64` already in workspace (used via `BASE64_STANDARD`)

**Coturn configuration (VPS):**

```
# /etc/turnserver.conf
use-auth-secret
static-auth-secret=<same-secret-as-TURN_SHARED_SECRET>
# Remove or comment out:
# lt-cred-mech
# user=username:password
```

### Files

| File | Change |
|------|--------|
| `Cargo.toml` (workspace) | Add `sha1 = "0.10"` |
| `server/Cargo.toml` | Add `sha1.workspace = true` |
| `server/src/config.rs:127-133` | Add `turn_shared_secret`, `turn_credential_ttl`; keep old fields as fallback |
| `server/src/voice/handlers.rs:47-66` | HMAC credential generation with `AuthUser` extractor; fallback to static |

---

## Checklist Item Updates

After merging both branches, mark in `docs/developer-guide/plans/2026-03-19-beta-checklist.md`:

- `[x] E2EE Megolm session cache never cleared on logout` — with PR reference
- `[x] returnUrl injection risk` — with PR reference
- `[x] TURN credentials are static/long-lived` — with PR reference

Virus scanning remains deferred: `[ ] No virus scanning for file uploads` — current MIME allowlist + magic bytes is sufficient for beta.
