# Server-Synced User Preferences - Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Sync user preferences (theme, sound, quiet hours, per-channel notifications) across all devices in real-time.

**Architecture:** REST API for fetch/update, WebSocket broadcast via existing `user:{user_id}` channel, localStorage as offline cache with timestamp-based conflict resolution.

**Tech Stack:** Rust/Axum (API), Redis pub/sub, PostgreSQL JSONB, Solid.js signals, TypeScript

**Design Doc:** `docs/plans/2026-01-23-server-synced-preferences-design.md`

---

## Task 1: Database Migration

**Files:**
- Create: `server/migrations/YYYYMMDDHHMMSS_create_user_preferences.sql`

**Step 1: Create migration file**

```sql
-- Create user_preferences table for syncing settings across devices
CREATE TABLE user_preferences (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    preferences JSONB NOT NULL DEFAULT '{}',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Index for querying by update time (useful for future sync operations)
CREATE INDEX idx_user_preferences_updated ON user_preferences(updated_at);

-- Comment for documentation
COMMENT ON TABLE user_preferences IS 'Stores user preferences (theme, sound, notifications) for cross-device sync';
```

**Step 2: Run migration**

Run: `cd server && sqlx migrate run`
Expected: Migration applies successfully

**Step 3: Verify table exists**

Run: `cd server && sqlx database reset -y && sqlx migrate run`
Expected: Clean migration from scratch works

**Step 4: Commit**

```bash
git add server/migrations/
git commit -m "feat(db): add user_preferences table for cross-device sync"
```

---

## Task 2: Backend Types

**Files:**
- Create: `server/src/api/preferences.rs`
- Modify: `server/src/api/mod.rs`

**Step 1: Create preferences module with types**

Create `server/src/api/preferences.rs`:

```rust
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::auth::Claims;
use crate::error::AppError;

/// Response for preferences endpoints
#[derive(Debug, Serialize)]
pub struct PreferencesResponse {
    pub preferences: serde_json::Value,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// Request body for updating preferences
#[derive(Debug, Deserialize)]
pub struct UpdatePreferencesRequest {
    pub preferences: serde_json::Value,
}

/// Database row for user_preferences
#[derive(Debug, sqlx::FromRow)]
struct UserPreferencesRow {
    user_id: Uuid,
    preferences: serde_json::Value,
    updated_at: OffsetDateTime,
}
```

**Step 2: Add module to mod.rs**

In `server/src/api/mod.rs`, add:

```rust
pub mod preferences;
```

**Step 3: Verify compilation**

Run: `cd server && cargo check`
Expected: Compiles without errors

**Step 4: Commit**

```bash
git add server/src/api/preferences.rs server/src/api/mod.rs
git commit -m "feat(api): add preferences types and module"
```

---

## Task 3: GET /api/me/preferences Endpoint

**Files:**
- Modify: `server/src/api/preferences.rs`
- Modify: `server/src/api/mod.rs` (routes)

**Step 1: Implement get_preferences handler**

Add to `server/src/api/preferences.rs`:

```rust
/// GET /api/me/preferences
/// Returns the current user's preferences
#[tracing::instrument(skip(pool))]
pub async fn get_preferences(
    State(pool): State<PgPool>,
    claims: Claims,
) -> Result<impl IntoResponse, AppError> {
    let row = sqlx::query_as::<_, UserPreferencesRow>(
        r#"
        SELECT user_id, preferences, updated_at
        FROM user_preferences
        WHERE user_id = $1
        "#,
    )
    .bind(claims.sub)
    .fetch_optional(&pool)
    .await?;

    match row {
        Some(row) => Ok(Json(PreferencesResponse {
            preferences: row.preferences,
            updated_at: row.updated_at,
        })),
        None => {
            // Return empty preferences with current timestamp for new users
            Ok(Json(PreferencesResponse {
                preferences: serde_json::json!({}),
                updated_at: OffsetDateTime::now_utc(),
            }))
        }
    }
}
```

**Step 2: Register route**

In `server/src/api/mod.rs`, add the route in the appropriate router:

```rust
use crate::api::preferences;

// In the me_routes or similar:
.route("/me/preferences", get(preferences::get_preferences))
```

**Step 3: Verify compilation**

Run: `cd server && cargo check`
Expected: Compiles without errors

**Step 4: Test endpoint manually**

Run: `cd server && cargo run`
Then: `curl -H "Authorization: Bearer <token>" http://localhost:3000/api/me/preferences`
Expected: Returns `{"preferences": {}, "updated_at": "..."}`

**Step 5: Commit**

```bash
git add server/src/api/preferences.rs server/src/api/mod.rs
git commit -m "feat(api): add GET /api/me/preferences endpoint"
```

---

## Task 4: PUT /api/me/preferences Endpoint

**Files:**
- Modify: `server/src/api/preferences.rs`
- Modify: `server/src/api/mod.rs` (routes)

**Step 1: Implement update_preferences handler**

Add to `server/src/api/preferences.rs`:

```rust
/// PUT /api/me/preferences
/// Updates the current user's preferences (full replacement)
#[tracing::instrument(skip(pool))]
pub async fn update_preferences(
    State(pool): State<PgPool>,
    claims: Claims,
    Json(request): Json<UpdatePreferencesRequest>,
) -> Result<impl IntoResponse, AppError> {
    let row = sqlx::query_as::<_, UserPreferencesRow>(
        r#"
        INSERT INTO user_preferences (user_id, preferences, updated_at)
        VALUES ($1, $2, NOW())
        ON CONFLICT (user_id) DO UPDATE
        SET preferences = EXCLUDED.preferences,
            updated_at = NOW()
        RETURNING user_id, preferences, updated_at
        "#,
    )
    .bind(claims.sub)
    .bind(&request.preferences)
    .fetch_one(&pool)
    .await?;

    Ok(Json(PreferencesResponse {
        preferences: row.preferences,
        updated_at: row.updated_at,
    }))
}
```

**Step 2: Register route**

In `server/src/api/mod.rs`, add:

```rust
.route("/me/preferences", get(preferences::get_preferences).put(preferences::update_preferences))
```

**Step 3: Verify compilation**

Run: `cd server && cargo check`
Expected: Compiles without errors

**Step 4: Commit**

```bash
git add server/src/api/preferences.rs server/src/api/mod.rs
git commit -m "feat(api): add PUT /api/me/preferences endpoint"
```

---

## Task 5: WebSocket Event Broadcasting

**Files:**
- Modify: `server/src/ws/events.rs` (add PreferencesUpdated variant)
- Modify: `server/src/api/preferences.rs` (broadcast after update)

**Step 1: Add PreferencesUpdated event**

In `server/src/ws/events.rs`, add to the ServerEvent enum:

```rust
/// User preferences were updated on another device
PreferencesUpdated {
    preferences: serde_json::Value,
    #[serde(with = "time::serde::rfc3339")]
    updated_at: time::OffsetDateTime,
},
```

**Step 2: Update PUT handler to broadcast**

Modify `update_preferences` in `server/src/api/preferences.rs`:

```rust
use crate::ws::events::ServerEvent;
use crate::ws::broadcast::broadcast_to_user;

pub async fn update_preferences(
    State(pool): State<PgPool>,
    State(redis): State<fred::clients::RedisClient>,
    claims: Claims,
    Json(request): Json<UpdatePreferencesRequest>,
) -> Result<impl IntoResponse, AppError> {
    let row = sqlx::query_as::<_, UserPreferencesRow>(
        // ... existing query ...
    )
    .bind(claims.sub)
    .bind(&request.preferences)
    .fetch_one(&pool)
    .await?;

    // Broadcast to all user's devices
    let event = ServerEvent::PreferencesUpdated {
        preferences: row.preferences.clone(),
        updated_at: row.updated_at,
    };
    broadcast_to_user(&redis, claims.sub, &event).await?;

    Ok(Json(PreferencesResponse {
        preferences: row.preferences,
        updated_at: row.updated_at,
    }))
}
```

**Step 3: Verify compilation**

Run: `cd server && cargo check`
Expected: Compiles without errors

**Step 4: Commit**

```bash
git add server/src/ws/events.rs server/src/api/preferences.rs
git commit -m "feat(ws): broadcast PreferencesUpdated to user's devices"
```

---

## Task 6: Client Types

**Files:**
- Modify: `client/src/lib/types.ts`

**Step 1: Add UserPreferences interface**

Add to `client/src/lib/types.ts`:

```typescript
// User Preferences (synced across devices)
export interface UserPreferences {
  // Theme
  theme: "focused-hybrid" | "solarized-dark" | "solarized-light";

  // Sound settings
  sound: {
    enabled: boolean;
    volume: number; // 0-100
    soundType: "default" | "subtle" | "ping" | "chime" | "bell";
    quietHours: {
      enabled: boolean;
      startTime: string; // "HH:MM" format
      endTime: string;
    };
  };

  // Connection display
  connection: {
    displayMode: "circle" | "number";
    showNotifications: boolean;
  };

  // Per-channel notification levels
  channelNotifications: Record<string, "all" | "mentions" | "muted">;
}

export interface PreferencesResponse {
  preferences: Partial<UserPreferences>;
  updated_at: string; // ISO timestamp
}

export interface StoredPreferences {
  data: UserPreferences;
  updated_at: string;
}
```

**Step 2: Add PreferencesUpdated to ServerEvent union**

Find the ServerEvent type and add:

```typescript
| { type: "preferences_updated"; preferences: Partial<UserPreferences>; updated_at: string }
```

**Step 3: Verify TypeScript**

Run: `cd client && bun run check`
Expected: No type errors

**Step 4: Commit**

```bash
git add client/src/lib/types.ts
git commit -m "feat(client): add UserPreferences types"
```

---

## Task 7: Preferences Store

**Files:**
- Create: `client/src/stores/preferences.ts`

**Step 1: Create unified preferences store**

Create `client/src/stores/preferences.ts`:

```typescript
import { createSignal, createEffect } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { UserPreferences, PreferencesResponse, StoredPreferences } from "../lib/types";

const STORAGE_KEY = "vc:preferences";
const DEBOUNCE_MS = 500;

// Default preferences
const DEFAULT_PREFERENCES: UserPreferences = {
  theme: "focused-hybrid",
  sound: {
    enabled: true,
    volume: 80,
    soundType: "default",
    quietHours: {
      enabled: false,
      startTime: "22:00",
      endTime: "08:00",
    },
  },
  connection: {
    displayMode: "circle",
    showNotifications: true,
  },
  channelNotifications: {},
};

// Signals
const [preferences, setPreferences] = createSignal<UserPreferences>(DEFAULT_PREFERENCES);
const [lastUpdated, setLastUpdated] = createSignal<string>(new Date().toISOString());
const [isSyncing, setIsSyncing] = createSignal(false);

// Debounce timer
let pushTimer: ReturnType<typeof setTimeout> | null = null;

// Load from localStorage
function loadFromLocalStorage(): StoredPreferences | null {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored) {
      return JSON.parse(stored);
    }
  } catch (e) {
    console.error("Failed to load preferences from localStorage:", e);
  }
  return null;
}

// Save to localStorage
function saveToLocalStorage(prefs: UserPreferences, updatedAt: string): void {
  try {
    const stored: StoredPreferences = { data: prefs, updated_at: updatedAt };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(stored));
  } catch (e) {
    console.error("Failed to save preferences to localStorage:", e);
  }
}

// Fetch from server
async function fetchPreferences(): Promise<PreferencesResponse> {
  return invoke<PreferencesResponse>("fetch_preferences");
}

// Push to server
async function pushPreferences(prefs: UserPreferences): Promise<PreferencesResponse> {
  return invoke<PreferencesResponse>("update_preferences", { preferences: prefs });
}

// Initialize preferences on login
export async function initPreferences(): Promise<void> {
  setIsSyncing(true);
  try {
    const local = loadFromLocalStorage();
    const server = await fetchPreferences();

    if (!server.preferences || Object.keys(server.preferences).length === 0) {
      // No server prefs, push local (or defaults)
      const toSync = local?.data ?? DEFAULT_PREFERENCES;
      const result = await pushPreferences(toSync);
      setPreferences(result.preferences as UserPreferences);
      setLastUpdated(result.updated_at);
      saveToLocalStorage(result.preferences as UserPreferences, result.updated_at);
    } else if (!local || new Date(server.updated_at) > new Date(local.updated_at)) {
      // Server is newer, apply
      const merged = { ...DEFAULT_PREFERENCES, ...server.preferences };
      setPreferences(merged);
      setLastUpdated(server.updated_at);
      saveToLocalStorage(merged, server.updated_at);
    } else {
      // Local is newer (edited while offline), push
      const result = await pushPreferences(local.data);
      setPreferences(result.preferences as UserPreferences);
      setLastUpdated(result.updated_at);
      saveToLocalStorage(result.preferences as UserPreferences, result.updated_at);
    }
  } catch (e) {
    console.error("Failed to init preferences:", e);
    // Fall back to local or defaults
    const local = loadFromLocalStorage();
    if (local) {
      setPreferences(local.data);
      setLastUpdated(local.updated_at);
    }
  } finally {
    setIsSyncing(false);
  }
}

// Update a preference value
export function updatePreference<K extends keyof UserPreferences>(
  key: K,
  value: UserPreferences[K]
): void {
  const updated = { ...preferences(), [key]: value };
  const now = new Date().toISOString();

  setPreferences(updated);
  setLastUpdated(now);
  saveToLocalStorage(updated, now);

  // Debounced push to server
  if (pushTimer) clearTimeout(pushTimer);
  pushTimer = setTimeout(async () => {
    try {
      await pushPreferences(updated);
    } catch (e) {
      console.error("Failed to push preferences:", e);
    }
  }, DEBOUNCE_MS);
}

// Handle WebSocket event from other device
export function handlePreferencesUpdated(event: {
  preferences: Partial<UserPreferences>;
  updated_at: string;
}): void {
  const local = loadFromLocalStorage();
  if (!local || new Date(event.updated_at) > new Date(local.updated_at)) {
    const merged = { ...DEFAULT_PREFERENCES, ...event.preferences };
    setPreferences(merged);
    setLastUpdated(event.updated_at);
    saveToLocalStorage(merged, event.updated_at);
  }
}

// Exports
export { preferences, lastUpdated, isSyncing, DEFAULT_PREFERENCES };
```

**Step 2: Verify TypeScript**

Run: `cd client && bun run check`
Expected: No type errors

**Step 3: Commit**

```bash
git add client/src/stores/preferences.ts
git commit -m "feat(client): add unified preferences store with sync"
```

---

## Task 8: Tauri Commands

**Files:**
- Modify: `client/src-tauri/src/commands/mod.rs` (or appropriate file)

**Step 1: Add fetch_preferences command**

```rust
#[tauri::command]
pub async fn fetch_preferences(
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let client = &state.http_client;
    let token = state.get_token().await.ok_or("Not authenticated")?;

    let response = client
        .get(&format!("{}/api/me/preferences", state.api_url))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if response.status().is_success() {
        response.json().await.map_err(|e| e.to_string())
    } else {
        Err(format!("Failed to fetch preferences: {}", response.status()))
    }
}
```

**Step 2: Add update_preferences command**

```rust
#[tauri::command]
pub async fn update_preferences(
    state: tauri::State<'_, AppState>,
    preferences: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let client = &state.http_client;
    let token = state.get_token().await.ok_or("Not authenticated")?;

    let response = client
        .put(&format!("{}/api/me/preferences", state.api_url))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "preferences": preferences }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if response.status().is_success() {
        response.json().await.map_err(|e| e.to_string())
    } else {
        Err(format!("Failed to update preferences: {}", response.status()))
    }
}
```

**Step 3: Register commands**

Add to the Tauri builder:

```rust
.invoke_handler(tauri::generate_handler![
    // ... existing commands ...
    fetch_preferences,
    update_preferences,
])
```

**Step 4: Verify compilation**

Run: `cd client/src-tauri && cargo check`
Expected: Compiles without errors

**Step 5: Commit**

```bash
git add client/src-tauri/
git commit -m "feat(tauri): add preferences sync commands"
```

---

## Task 9: WebSocket Event Handler

**Files:**
- Modify: `client/src/stores/websocket.ts`

**Step 1: Import preferences handler**

Add import:

```typescript
import { handlePreferencesUpdated } from "./preferences";
```

**Step 2: Add case for preferences_updated event**

In the event handler switch/if block:

```typescript
case "preferences_updated":
  handlePreferencesUpdated(event);
  break;
```

**Step 3: Verify TypeScript**

Run: `cd client && bun run check`
Expected: No type errors

**Step 4: Commit**

```bash
git add client/src/stores/websocket.ts
git commit -m "feat(ws): handle preferences_updated event"
```

---

## Task 10: Migrate Existing Stores

**Files:**
- Modify: `client/src/stores/theme.ts`
- Modify: `client/src/stores/sound.ts`
- Modify: `client/src/stores/connection.ts`

**Step 1: Update theme store to use preferences**

Modify `client/src/stores/theme.ts` to read/write through preferences store:

```typescript
import { createEffect } from "solid-js";
import { preferences, updatePreference } from "./preferences";

// Derived signal for theme
export const theme = () => preferences().theme;

// Update theme through preferences
export function setTheme(newTheme: "focused-hybrid" | "solarized-dark" | "solarized-light") {
  updatePreference("theme", newTheme);
}

// Apply theme to document
createEffect(() => {
  const currentTheme = theme();
  document.documentElement.setAttribute("data-theme", currentTheme);
});
```

**Step 2: Update sound store similarly**

Update `client/src/stores/sound.ts` to delegate to preferences store.

**Step 3: Update connection store similarly**

Update `client/src/stores/connection.ts` to delegate to preferences store.

**Step 4: Verify TypeScript**

Run: `cd client && bun run check`
Expected: No type errors

**Step 5: Commit**

```bash
git add client/src/stores/theme.ts client/src/stores/sound.ts client/src/stores/connection.ts
git commit -m "refactor(stores): migrate theme/sound/connection to preferences store"
```

---

## Task 11: Initialize on Login

**Files:**
- Modify: `client/src/App.tsx` or login handler

**Step 1: Call initPreferences after login**

Find where authentication completes and add:

```typescript
import { initPreferences } from "./stores/preferences";

// After successful login:
await initPreferences();
```

**Step 2: Verify build**

Run: `cd client && bun run build`
Expected: Builds without errors

**Step 3: Commit**

```bash
git add client/src/
git commit -m "feat(auth): initialize preferences sync on login"
```

---

## Task 12: Migration for Existing Users

**Files:**
- Modify: `client/src/stores/preferences.ts`

**Step 1: Add migration logic**

Add migration function to `preferences.ts`:

```typescript
// Migrate old localStorage keys to new unified format
function migrateOldPreferences(): Partial<UserPreferences> | null {
  const migrated: Partial<UserPreferences> = {};
  let hasMigration = false;

  // Migrate theme
  const oldTheme = localStorage.getItem("theme");
  if (oldTheme) {
    migrated.theme = oldTheme as UserPreferences["theme"];
    localStorage.removeItem("theme");
    hasMigration = true;
  }

  // Migrate sound settings
  const oldSound = localStorage.getItem("vc:soundSettings");
  if (oldSound) {
    try {
      migrated.sound = JSON.parse(oldSound);
      localStorage.removeItem("vc:soundSettings");
      hasMigration = true;
    } catch {}
  }

  // Migrate connection settings
  const oldConnection = localStorage.getItem("vc:connectionSettings");
  if (oldConnection) {
    try {
      migrated.connection = JSON.parse(oldConnection);
      localStorage.removeItem("vc:connectionSettings");
      hasMigration = true;
    } catch {}
  }

  // Migrate per-channel notifications
  const oldChannelNotifs = localStorage.getItem("vc:channelNotifications");
  if (oldChannelNotifs) {
    try {
      migrated.channelNotifications = JSON.parse(oldChannelNotifs);
      localStorage.removeItem("vc:channelNotifications");
      hasMigration = true;
    } catch {}
  }

  return hasMigration ? migrated : null;
}
```

**Step 2: Call migration in initPreferences**

At the start of `initPreferences()`:

```typescript
// Check for and apply migrations from old localStorage keys
const migrated = migrateOldPreferences();
if (migrated) {
  const current = loadFromLocalStorage()?.data ?? DEFAULT_PREFERENCES;
  const merged = { ...current, ...migrated };
  saveToLocalStorage(merged, new Date().toISOString());
}
```

**Step 3: Verify build**

Run: `cd client && bun run build`
Expected: Builds without errors

**Step 4: Commit**

```bash
git add client/src/stores/preferences.ts
git commit -m "feat(client): migrate old localStorage keys to unified preferences"
```

---

## Task 13: Integration Testing

**Step 1: Manual test flow**

1. Login on device A
2. Change theme to "solarized-dark"
3. Open device B (or incognito tab)
4. Login on device B
5. Verify theme is "solarized-dark"
6. Change theme on B to "solarized-light"
7. Verify A updates within 2 seconds

**Step 2: Test offline scenario**

1. Disconnect network
2. Change preference
3. Reconnect
4. Verify preference syncs to server

**Step 3: Verify migration**

1. Set old localStorage keys manually
2. Clear vc:preferences
3. Login
4. Verify old keys migrated to new format

---

## Task 14: Update Roadmap and Changelog

**Files:**
- Modify: `docs/project/roadmap.md`
- Modify: `CHANGELOG.md`

**Step 1: Update roadmap**

Mark Server-Synced User Preferences as complete in Phase 4.

**Step 2: Update changelog**

Add under `[Unreleased] > Added`:

```markdown
- Server-synced user preferences
  - Theme, sound settings, quiet hours, and per-channel notifications sync across all devices
  - Real-time updates via WebSocket when preferences change on another device
  - Offline support with automatic sync on reconnect
  - Migration from legacy localStorage keys
```

**Step 3: Commit**

```bash
git add docs/project/roadmap.md CHANGELOG.md
git commit -m "docs: mark server-synced preferences complete"
```

---

## Verification Checklist

- [ ] Database migration runs cleanly
- [ ] GET /api/me/preferences returns empty object for new users
- [ ] PUT /api/me/preferences upserts and returns updated data
- [ ] WebSocket broadcasts PreferencesUpdated event
- [ ] Client receives and applies preferences on login
- [ ] Theme/sound/connection changes sync to server (debounced)
- [ ] Other devices receive real-time updates
- [ ] Old localStorage keys migrate on first sync
- [ ] Offline changes sync when reconnected
