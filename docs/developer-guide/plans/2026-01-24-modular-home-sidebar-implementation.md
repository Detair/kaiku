# Modular Home Sidebar Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add collapsible modules (Pending & Suggestions, Global Pins) to the Home right panel with server-synced collapse state.

**Architecture:** Refactor HomeRightPanel to use a CollapsibleModule wrapper component. Add user_pins table and CRUD API. Extend UserPreferences with homeSidebar.collapsed state.

**Tech Stack:** Rust/Axum (backend), Solid.js/TypeScript (frontend), PostgreSQL (pins storage), server-synced preferences (collapse state)

---

## Task 1: Database Migration - user_pins Table

**Files:**
- Create: `server/migrations/20260124000000_create_user_pins.sql`

**Step 1: Create migration file**

```sql
-- Create user_pins table for global pins/scratchpad
CREATE TABLE user_pins (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    pin_type VARCHAR(20) NOT NULL CHECK (pin_type IN ('note', 'link', 'message')),
    content TEXT NOT NULL,
    title VARCHAR(255),
    metadata JSONB DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    position INT NOT NULL DEFAULT 0
);

-- Index for efficient user queries
CREATE INDEX idx_user_pins_user ON user_pins(user_id, position);

-- Limit pins per user (enforced at application level, but add comment)
COMMENT ON TABLE user_pins IS 'User pins for global scratchpad. Max 50 pins per user enforced in API.';
```

**Step 2: Verify migration syntax**

Run: `cd server && cargo sqlx migrate run --dry-run`
Expected: Migration parses without errors (or skip if no DB connection)

**Step 3: Commit**

```bash
git add server/migrations/
git commit -m "feat(db): add user_pins table for global scratchpad"
```

---

## Task 2: Backend Types - Pins Module

**Files:**
- Create: `server/src/api/pins.rs`
- Modify: `server/src/api/mod.rs`

**Step 1: Create pins types and module structure**

```rust
//! User Pins API
//!
//! CRUD operations for user's global pins (notes, links, pinned messages).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{auth::AuthUser, state::AppState};

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PinType {
    Note,
    Link,
    Message,
}

impl PinType {
    fn as_str(&self) -> &'static str {
        match self {
            PinType::Note => "note",
            PinType::Link => "link",
            PinType::Message => "message",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "note" => Some(PinType::Note),
            "link" => Some(PinType::Link),
            "message" => Some(PinType::Message),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize, FromRow)]
pub struct PinRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub pin_type: String,
    pub content: String,
    pub title: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub position: i32,
}

#[derive(Debug, Serialize)]
pub struct Pin {
    pub id: Uuid,
    pub pin_type: PinType,
    pub content: String,
    pub title: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub position: i32,
}

impl From<PinRow> for Pin {
    fn from(row: PinRow) -> Self {
        Pin {
            id: row.id,
            pin_type: PinType::from_str(&row.pin_type).unwrap_or(PinType::Note),
            content: row.content,
            title: row.title,
            metadata: row.metadata,
            created_at: row.created_at,
            position: row.position,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreatePinRequest {
    pub pin_type: PinType,
    pub content: String,
    pub title: Option<String>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePinRequest {
    pub content: Option<String>,
    pub title: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct ReorderPinsRequest {
    pub pin_ids: Vec<Uuid>,
}

// ============================================================================
// Constants
// ============================================================================

const MAX_PINS_PER_USER: i64 = 50;
const MAX_CONTENT_LENGTH: usize = 2000;
const MAX_TITLE_LENGTH: usize = 255;

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug)]
pub enum PinsError {
    NotFound,
    LimitExceeded,
    ContentTooLong,
    TitleTooLong,
    Database(sqlx::Error),
}

impl IntoResponse for PinsError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            PinsError::NotFound => (StatusCode::NOT_FOUND, "Pin not found"),
            PinsError::LimitExceeded => (StatusCode::BAD_REQUEST, "Maximum pins limit reached (50)"),
            PinsError::ContentTooLong => (StatusCode::BAD_REQUEST, "Content exceeds maximum length"),
            PinsError::TitleTooLong => (StatusCode::BAD_REQUEST, "Title exceeds maximum length"),
            PinsError::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

impl From<sqlx::Error> for PinsError {
    fn from(err: sqlx::Error) -> Self {
        PinsError::Database(err)
    }
}
```

**Step 2: Register module in mod.rs**

Add to `server/src/api/mod.rs`:
```rust
pub mod pins;
```

**Step 3: Verify it compiles**

Run: `cd server && cargo check`
Expected: Compiles without errors

**Step 4: Commit**

```bash
git add server/src/api/pins.rs server/src/api/mod.rs
git commit -m "feat(api): add pins types and error handling"
```

---

## Task 3: Backend Handlers - List and Create Pins

**Files:**
- Modify: `server/src/api/pins.rs`
- Modify: `server/src/api/mod.rs` (routes)

**Step 1: Add list_pins handler**

Add to `server/src/api/pins.rs`:
```rust
// ============================================================================
// Handlers
// ============================================================================

/// GET /api/me/pins - List user's pins
pub async fn list_pins(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<Vec<Pin>>, PinsError> {
    let rows = sqlx::query_as::<_, PinRow>(
        r#"
        SELECT id, user_id, pin_type, content, title, metadata, created_at, position
        FROM user_pins
        WHERE user_id = $1
        ORDER BY position ASC, created_at DESC
        "#,
    )
    .bind(auth_user.id)
    .fetch_all(&state.pool)
    .await?;

    let pins: Vec<Pin> = rows.into_iter().map(Pin::from).collect();
    Ok(Json(pins))
}

/// POST /api/me/pins - Create a new pin
pub async fn create_pin(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(request): Json<CreatePinRequest>,
) -> Result<Json<Pin>, PinsError> {
    // Validate content length
    if request.content.len() > MAX_CONTENT_LENGTH {
        return Err(PinsError::ContentTooLong);
    }

    // Validate title length
    if let Some(ref title) = request.title {
        if title.len() > MAX_TITLE_LENGTH {
            return Err(PinsError::TitleTooLong);
        }
    }

    // Check pin count limit
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM user_pins WHERE user_id = $1")
        .bind(auth_user.id)
        .fetch_one(&state.pool)
        .await?;

    if count.0 >= MAX_PINS_PER_USER {
        return Err(PinsError::LimitExceeded);
    }

    // Get next position
    let max_pos: Option<(i32,)> =
        sqlx::query_as("SELECT MAX(position) FROM user_pins WHERE user_id = $1")
            .bind(auth_user.id)
            .fetch_optional(&state.pool)
            .await?;

    let next_position = max_pos.and_then(|p| p.0.map(|v| v + 1)).unwrap_or(0);

    // Insert pin
    let row = sqlx::query_as::<_, PinRow>(
        r#"
        INSERT INTO user_pins (user_id, pin_type, content, title, metadata, position)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, user_id, pin_type, content, title, metadata, created_at, position
        "#,
    )
    .bind(auth_user.id)
    .bind(request.pin_type.as_str())
    .bind(&request.content)
    .bind(&request.title)
    .bind(&request.metadata)
    .bind(next_position)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(Pin::from(row)))
}
```

**Step 2: Add routes to mod.rs**

Add to router in `server/src/api/mod.rs`:
```rust
use pins::{list_pins, create_pin};

// In the router builder, add:
.route("/me/pins", get(list_pins).post(create_pin))
```

**Step 3: Verify it compiles**

Run: `cd server && cargo check`
Expected: Compiles without errors

**Step 4: Commit**

```bash
git add server/src/api/pins.rs server/src/api/mod.rs
git commit -m "feat(api): add GET/POST /api/me/pins endpoints"
```

---

## Task 4: Backend Handlers - Update, Delete, Reorder Pins

**Files:**
- Modify: `server/src/api/pins.rs`
- Modify: `server/src/api/mod.rs` (routes)

**Step 1: Add update_pin handler**

Add to `server/src/api/pins.rs`:
```rust
/// PUT /api/me/pins/:id - Update a pin
pub async fn update_pin(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(pin_id): Path<Uuid>,
    Json(request): Json<UpdatePinRequest>,
) -> Result<Json<Pin>, PinsError> {
    // Validate content length if provided
    if let Some(ref content) = request.content {
        if content.len() > MAX_CONTENT_LENGTH {
            return Err(PinsError::ContentTooLong);
        }
    }

    // Validate title length if provided
    if let Some(ref title) = request.title {
        if title.len() > MAX_TITLE_LENGTH {
            return Err(PinsError::TitleTooLong);
        }
    }

    // Check pin exists and belongs to user
    let existing = sqlx::query_as::<_, PinRow>(
        "SELECT * FROM user_pins WHERE id = $1 AND user_id = $2",
    )
    .bind(pin_id)
    .bind(auth_user.id)
    .fetch_optional(&state.pool)
    .await?;

    if existing.is_none() {
        return Err(PinsError::NotFound);
    }

    // Update pin
    let row = sqlx::query_as::<_, PinRow>(
        r#"
        UPDATE user_pins
        SET content = COALESCE($3, content),
            title = COALESCE($4, title),
            metadata = COALESCE($5, metadata)
        WHERE id = $1 AND user_id = $2
        RETURNING id, user_id, pin_type, content, title, metadata, created_at, position
        "#,
    )
    .bind(pin_id)
    .bind(auth_user.id)
    .bind(&request.content)
    .bind(&request.title)
    .bind(&request.metadata)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(Pin::from(row)))
}

/// DELETE /api/me/pins/:id - Delete a pin
pub async fn delete_pin(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(pin_id): Path<Uuid>,
) -> Result<StatusCode, PinsError> {
    let result = sqlx::query("DELETE FROM user_pins WHERE id = $1 AND user_id = $2")
        .bind(pin_id)
        .bind(auth_user.id)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(PinsError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}

/// PUT /api/me/pins/reorder - Reorder pins
pub async fn reorder_pins(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(request): Json<ReorderPinsRequest>,
) -> Result<StatusCode, PinsError> {
    // Update positions based on order in request
    for (position, pin_id) in request.pin_ids.iter().enumerate() {
        sqlx::query("UPDATE user_pins SET position = $3 WHERE id = $1 AND user_id = $2")
            .bind(pin_id)
            .bind(auth_user.id)
            .bind(position as i32)
            .execute(&state.pool)
            .await?;
    }

    Ok(StatusCode::NO_CONTENT)
}
```

**Step 2: Add routes to mod.rs**

Add imports and routes in `server/src/api/mod.rs`:
```rust
use pins::{list_pins, create_pin, update_pin, delete_pin, reorder_pins};

// Add routes:
.route("/me/pins/:id", put(update_pin).delete(delete_pin))
.route("/me/pins/reorder", put(reorder_pins))
```

**Step 3: Verify it compiles**

Run: `cd server && cargo check`
Expected: Compiles without errors

**Step 4: Commit**

```bash
git add server/src/api/pins.rs server/src/api/mod.rs
git commit -m "feat(api): add PUT/DELETE pins and reorder endpoints"
```

---

## Task 5: Client Types - Pin and HomeSidebar Preferences

**Files:**
- Modify: `client/src/lib/types.ts`
- Modify: `client/src/stores/preferences.ts`

**Step 1: Add Pin types to types.ts**

Add to `client/src/lib/types.ts`:
```typescript
// ============================================================================
// Pins Types
// ============================================================================

export type PinType = "note" | "link" | "message";

export interface Pin {
  id: string;
  pin_type: PinType;
  content: string;
  title?: string;
  metadata: Record<string, unknown>;
  created_at: string;
  position: number;
}

export interface CreatePinRequest {
  pin_type: PinType;
  content: string;
  title?: string;
  metadata?: Record<string, unknown>;
}

export interface UpdatePinRequest {
  content?: string;
  title?: string;
  metadata?: Record<string, unknown>;
}
```

**Step 2: Add homeSidebar to UserPreferences in preferences.ts**

Modify `client/src/stores/preferences.ts` - add to UserPreferences interface and defaults:
```typescript
// Add to UserPreferences interface:
homeSidebar: {
  collapsed: {
    activeNow: boolean;
    pending: boolean;
    pins: boolean;
  };
};

// Add to DEFAULT_PREFERENCES:
homeSidebar: {
  collapsed: {
    activeNow: false,
    pending: false,
    pins: false,
  },
},
```

**Step 3: Verify it compiles**

Run: `cd client && bun tsc --noEmit`
Expected: No type errors

**Step 4: Commit**

```bash
git add client/src/lib/types.ts client/src/stores/preferences.ts
git commit -m "feat(client): add Pin types and homeSidebar preferences"
```

---

## Task 6: Client Store - Pins Management

**Files:**
- Create: `client/src/stores/pins.ts`
- Modify: `client/src/lib/tauri.ts`

**Step 1: Create pins store**

Create `client/src/stores/pins.ts`:
```typescript
/**
 * Pins Store
 *
 * Manages user's global pins (notes, links, pinned messages).
 */

import { createSignal } from "solid-js";
import type { Pin, CreatePinRequest, UpdatePinRequest } from "@/lib/types";

// ============================================================================
// State
// ============================================================================

const [pins, setPins] = createSignal<Pin[]>([]);
const [isLoading, setIsLoading] = createSignal(false);

// ============================================================================
// API Calls
// ============================================================================

async function apiCall<T>(
  endpoint: string,
  options?: RequestInit
): Promise<T> {
  // Check if running in Tauri
  if (window.__TAURI__) {
    const { invoke } = await import("@tauri-apps/api/core");
    const method = options?.method || "GET";
    const body = options?.body ? JSON.parse(options.body as string) : undefined;

    switch (method) {
      case "GET":
        return invoke("fetch_pins") as Promise<T>;
      case "POST":
        return invoke("create_pin", { request: body }) as Promise<T>;
      case "PUT":
        if (endpoint.includes("reorder")) {
          return invoke("reorder_pins", { pinIds: body.pin_ids }) as Promise<T>;
        }
        const pinId = endpoint.split("/").pop();
        return invoke("update_pin", { pinId, request: body }) as Promise<T>;
      case "DELETE":
        const deleteId = endpoint.split("/").pop();
        return invoke("delete_pin", { pinId: deleteId }) as Promise<T>;
      default:
        throw new Error(`Unknown method: ${method}`);
    }
  }

  // HTTP fallback for browser
  const token = localStorage.getItem("vc:token");
  const baseUrl = import.meta.env.VITE_API_URL || "";

  const response = await fetch(`${baseUrl}${endpoint}`, {
    ...options,
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${token}`,
      ...options?.headers,
    },
  });

  if (!response.ok) {
    throw new Error(`API error: ${response.status}`);
  }

  if (response.status === 204) {
    return undefined as T;
  }

  return response.json();
}

// ============================================================================
// Actions
// ============================================================================

export async function loadPins(): Promise<void> {
  setIsLoading(true);
  try {
    const data = await apiCall<Pin[]>("/api/me/pins");
    setPins(data);
  } catch (error) {
    console.error("Failed to load pins:", error);
  } finally {
    setIsLoading(false);
  }
}

export async function createPin(request: CreatePinRequest): Promise<Pin | null> {
  try {
    const pin = await apiCall<Pin>("/api/me/pins", {
      method: "POST",
      body: JSON.stringify(request),
    });
    setPins((prev) => [...prev, pin]);
    return pin;
  } catch (error) {
    console.error("Failed to create pin:", error);
    return null;
  }
}

export async function updatePin(
  pinId: string,
  request: UpdatePinRequest
): Promise<Pin | null> {
  try {
    const pin = await apiCall<Pin>(`/api/me/pins/${pinId}`, {
      method: "PUT",
      body: JSON.stringify(request),
    });
    setPins((prev) => prev.map((p) => (p.id === pinId ? pin : p)));
    return pin;
  } catch (error) {
    console.error("Failed to update pin:", error);
    return null;
  }
}

export async function deletePin(pinId: string): Promise<boolean> {
  try {
    await apiCall(`/api/me/pins/${pinId}`, { method: "DELETE" });
    setPins((prev) => prev.filter((p) => p.id !== pinId));
    return true;
  } catch (error) {
    console.error("Failed to delete pin:", error);
    return false;
  }
}

export async function reorderPins(pinIds: string[]): Promise<boolean> {
  try {
    await apiCall("/api/me/pins/reorder", {
      method: "PUT",
      body: JSON.stringify({ pin_ids: pinIds }),
    });
    // Reorder local state
    setPins((prev) => {
      const pinMap = new Map(prev.map((p) => [p.id, p]));
      return pinIds
        .map((id, index) => {
          const pin = pinMap.get(id);
          return pin ? { ...pin, position: index } : null;
        })
        .filter((p): p is Pin => p !== null);
    });
    return true;
  } catch (error) {
    console.error("Failed to reorder pins:", error);
    return false;
  }
}

// ============================================================================
// Selectors
// ============================================================================

export { pins, isLoading };

export function getPinsByType(type: Pin["pin_type"]): Pin[] {
  return pins().filter((p) => p.pin_type === type);
}
```

**Step 2: Verify it compiles**

Run: `cd client && bun tsc --noEmit`
Expected: No type errors

**Step 3: Commit**

```bash
git add client/src/stores/pins.ts
git commit -m "feat(client): add pins store with CRUD operations"
```

---

## Task 7: Tauri Commands - Pins API

**Files:**
- Create: `client/src-tauri/src/commands/pins.rs`
- Modify: `client/src-tauri/src/commands/mod.rs`
- Modify: `client/src-tauri/src/lib.rs`

**Step 1: Create pins commands**

Create `client/src-tauri/src/commands/pins.rs`:
```rust
//! Tauri commands for pins API

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct Pin {
    pub id: String,
    pub pin_type: String,
    pub content: String,
    pub title: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: String,
    pub position: i32,
}

#[derive(Debug, Deserialize)]
pub struct CreatePinRequest {
    pub pin_type: String,
    pub content: String,
    pub title: Option<String>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePinRequest {
    pub content: Option<String>,
    pub title: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[tauri::command]
pub async fn fetch_pins(state: State<'_, AppState>) -> Result<Vec<Pin>, String> {
    let client = state.client.lock().await;
    let token = state.token.lock().await;

    let token = token.as_ref().ok_or("Not authenticated")?;
    let base_url = &state.base_url;

    let response = client
        .get(format!("{}/api/me/pins", base_url))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("Failed to fetch pins: {}", response.status()));
    }

    response.json().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_pin(
    state: State<'_, AppState>,
    request: CreatePinRequest,
) -> Result<Pin, String> {
    let client = state.client.lock().await;
    let token = state.token.lock().await;

    let token = token.as_ref().ok_or("Not authenticated")?;
    let base_url = &state.base_url;

    let response = client
        .post(format!("{}/api/me/pins", base_url))
        .bearer_auth(token)
        .json(&request)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("Failed to create pin: {}", response.status()));
    }

    response.json().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_pin(
    state: State<'_, AppState>,
    pin_id: String,
    request: UpdatePinRequest,
) -> Result<Pin, String> {
    let client = state.client.lock().await;
    let token = state.token.lock().await;

    let token = token.as_ref().ok_or("Not authenticated")?;
    let base_url = &state.base_url;

    let response = client
        .put(format!("{}/api/me/pins/{}", base_url, pin_id))
        .bearer_auth(token)
        .json(&request)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("Failed to update pin: {}", response.status()));
    }

    response.json().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_pin(state: State<'_, AppState>, pin_id: String) -> Result<(), String> {
    let client = state.client.lock().await;
    let token = state.token.lock().await;

    let token = token.as_ref().ok_or("Not authenticated")?;
    let base_url = &state.base_url;

    let response = client
        .delete(format!("{}/api/me/pins/{}", base_url, pin_id))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("Failed to delete pin: {}", response.status()));
    }

    Ok(())
}

#[tauri::command]
pub async fn reorder_pins(state: State<'_, AppState>, pin_ids: Vec<String>) -> Result<(), String> {
    let client = state.client.lock().await;
    let token = state.token.lock().await;

    let token = token.as_ref().ok_or("Not authenticated")?;
    let base_url = &state.base_url;

    let response = client
        .put(format!("{}/api/me/pins/reorder", base_url))
        .bearer_auth(token)
        .json(&serde_json::json!({ "pin_ids": pin_ids }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("Failed to reorder pins: {}", response.status()));
    }

    Ok(())
}
```

**Step 2: Register module and commands**

Add to `client/src-tauri/src/commands/mod.rs`:
```rust
pub mod pins;
```

Add to command registration in `client/src-tauri/src/lib.rs`:
```rust
use commands::pins::{fetch_pins, create_pin, update_pin, delete_pin, reorder_pins};

// In invoke_handler, add:
fetch_pins,
create_pin,
update_pin,
delete_pin,
reorder_pins,
```

**Step 3: Verify it compiles**

Run: `cd client/src-tauri && cargo check`
Expected: Compiles without errors

**Step 4: Commit**

```bash
git add client/src-tauri/src/commands/
git commit -m "feat(tauri): add pins commands for CRUD operations"
```

---

## Task 8: CollapsibleModule Component

**Files:**
- Create: `client/src/components/home/modules/CollapsibleModule.tsx`

**Step 1: Create the component**

```typescript
/**
 * CollapsibleModule Component
 *
 * Generic wrapper for collapsible sidebar modules.
 */

import { Component, JSX, Show, createSignal, onMount } from "solid-js";
import { ChevronDown, ChevronRight } from "lucide-solid";
import { preferences, updateNestedPreference } from "@/stores/preferences";

interface CollapsibleModuleProps {
  id: "activeNow" | "pending" | "pins";
  title: string;
  badge?: number;
  children: JSX.Element;
}

const CollapsibleModule: Component<CollapsibleModuleProps> = (props) => {
  // Get collapsed state from preferences
  const isCollapsed = () => preferences().homeSidebar?.collapsed?.[props.id] ?? false;

  const toggleCollapse = () => {
    const currentCollapsed = preferences().homeSidebar?.collapsed ?? {};
    updateNestedPreference("homeSidebar", "collapsed", {
      ...currentCollapsed,
      [props.id]: !isCollapsed(),
    });
  };

  return (
    <div class="border-b border-white/10 last:border-b-0">
      {/* Header */}
      <button
        onClick={toggleCollapse}
        class="w-full flex items-center justify-between px-4 py-3 hover:bg-white/5 transition-colors"
      >
        <div class="flex items-center gap-2">
          <Show when={isCollapsed()} fallback={<ChevronDown class="w-4 h-4 text-text-secondary" />}>
            <ChevronRight class="w-4 h-4 text-text-secondary" />
          </Show>
          <span class="font-semibold text-text-primary">{props.title}</span>
          <Show when={props.badge && props.badge > 0}>
            <span class="px-1.5 py-0.5 text-xs font-medium bg-accent-primary text-white rounded-full">
              {props.badge}
            </span>
          </Show>
        </div>
      </button>

      {/* Content */}
      <Show when={!isCollapsed()}>
        <div class="px-4 pb-4 animate-in slide-in-from-top-2 duration-150">
          {props.children}
        </div>
      </Show>
    </div>
  );
};

export default CollapsibleModule;
```

**Step 2: Verify it compiles**

Run: `cd client && bun tsc --noEmit`
Expected: No type errors

**Step 3: Commit**

```bash
git add client/src/components/home/modules/
git commit -m "feat(ui): add CollapsibleModule component"
```

---

## Task 9: PendingModule Component

**Files:**
- Create: `client/src/components/home/modules/PendingModule.tsx`

**Step 1: Create the component**

```typescript
/**
 * PendingModule Component
 *
 * Shows pending friend requests and guild invites with quick actions.
 */

import { Component, Show, For } from "solid-js";
import { UserPlus, Users, Check, X } from "lucide-solid";
import { friendsState, acceptFriendRequest, declineFriendRequest, cancelFriendRequest } from "@/stores/friends";
import CollapsibleModule from "./CollapsibleModule";
import { Avatar } from "@/components/ui";

const PendingModule: Component = () => {
  const pendingCount = () =>
    friendsState.pendingIncoming.length + friendsState.pendingOutgoing.length;

  return (
    <CollapsibleModule id="pending" title="Pending" badge={pendingCount()}>
      <Show
        when={pendingCount() > 0}
        fallback={
          <div class="text-center py-4">
            <UserPlus class="w-8 h-8 text-text-secondary mx-auto mb-2 opacity-50" />
            <p class="text-sm text-text-secondary">No pending requests</p>
            <p class="text-xs text-text-muted mt-1">
              Add friends by their username
            </p>
          </div>
        }
      >
        <div class="space-y-2">
          {/* Incoming Requests */}
          <Show when={friendsState.pendingIncoming.length > 0}>
            <div class="text-xs font-medium text-text-secondary uppercase tracking-wide mb-2">
              Incoming
            </div>
            <For each={friendsState.pendingIncoming}>
              {(request) => (
                <div class="flex items-center justify-between py-2 px-2 rounded-lg hover:bg-white/5">
                  <div class="flex items-center gap-2">
                    <Avatar
                      src={request.avatar_url}
                      name={request.display_name}
                      size="sm"
                    />
                    <div>
                      <div class="text-sm font-medium text-text-primary">
                        {request.display_name}
                      </div>
                      <div class="text-xs text-text-secondary">
                        @{request.username}
                      </div>
                    </div>
                  </div>
                  <div class="flex items-center gap-1">
                    <button
                      onClick={() => acceptFriendRequest(request.user_id)}
                      class="p-1.5 rounded-full bg-status-success/20 text-status-success hover:bg-status-success/30 transition-colors"
                      title="Accept"
                    >
                      <Check class="w-4 h-4" />
                    </button>
                    <button
                      onClick={() => declineFriendRequest(request.user_id)}
                      class="p-1.5 rounded-full bg-status-error/20 text-status-error hover:bg-status-error/30 transition-colors"
                      title="Decline"
                    >
                      <X class="w-4 h-4" />
                    </button>
                  </div>
                </div>
              )}
            </For>
          </Show>

          {/* Outgoing Requests */}
          <Show when={friendsState.pendingOutgoing.length > 0}>
            <div class="text-xs font-medium text-text-secondary uppercase tracking-wide mb-2 mt-3">
              Outgoing
            </div>
            <For each={friendsState.pendingOutgoing}>
              {(request) => (
                <div class="flex items-center justify-between py-2 px-2 rounded-lg hover:bg-white/5">
                  <div class="flex items-center gap-2">
                    <Avatar
                      src={request.avatar_url}
                      name={request.display_name}
                      size="sm"
                    />
                    <div>
                      <div class="text-sm font-medium text-text-primary">
                        {request.display_name}
                      </div>
                      <div class="text-xs text-text-muted">Pending...</div>
                    </div>
                  </div>
                  <button
                    onClick={() => cancelFriendRequest(request.user_id)}
                    class="p-1.5 rounded-full bg-white/10 text-text-secondary hover:bg-white/20 transition-colors"
                    title="Cancel"
                  >
                    <X class="w-4 h-4" />
                  </button>
                </div>
              )}
            </For>
          </Show>
        </div>
      </Show>
    </CollapsibleModule>
  );
};

export default PendingModule;
```

**Step 2: Verify it compiles**

Run: `cd client && bun tsc --noEmit`
Expected: No type errors

**Step 3: Commit**

```bash
git add client/src/components/home/modules/PendingModule.tsx
git commit -m "feat(ui): add PendingModule for friend requests"
```

---

## Task 10: PinsModule Component

**Files:**
- Create: `client/src/components/home/modules/PinsModule.tsx`

**Step 1: Create the component**

```typescript
/**
 * PinsModule Component
 *
 * Shows user's pinned notes, links, and messages.
 */

import { Component, Show, For, createSignal } from "solid-js";
import { Pin, FileText, Link, MessageSquare, Plus, Trash2, ExternalLink } from "lucide-solid";
import { pins, loadPins, createPin, deletePin, updatePin } from "@/stores/pins";
import type { Pin as PinType, PinType as PinTypeEnum } from "@/lib/types";
import CollapsibleModule from "./CollapsibleModule";

const PinsModule: Component = () => {
  const [isAdding, setIsAdding] = createSignal(false);
  const [addType, setAddType] = createSignal<PinTypeEnum>("note");
  const [newContent, setNewContent] = createSignal("");
  const [newTitle, setNewTitle] = createSignal("");
  const [editingId, setEditingId] = createSignal<string | null>(null);
  const [editContent, setEditContent] = createSignal("");

  // Load pins on mount
  loadPins();

  const handleCreate = async () => {
    if (!newContent().trim()) return;

    await createPin({
      pin_type: addType(),
      content: newContent(),
      title: newTitle() || undefined,
    });

    setNewContent("");
    setNewTitle("");
    setIsAdding(false);
  };

  const handleDelete = async (pinId: string) => {
    await deletePin(pinId);
  };

  const startEdit = (pin: PinType) => {
    setEditingId(pin.id);
    setEditContent(pin.content);
  };

  const saveEdit = async (pinId: string) => {
    await updatePin(pinId, { content: editContent() });
    setEditingId(null);
  };

  const cancelEdit = () => {
    setEditingId(null);
    setEditContent("");
  };

  const getPinIcon = (type: PinTypeEnum) => {
    switch (type) {
      case "note":
        return <FileText class="w-4 h-4" />;
      case "link":
        return <Link class="w-4 h-4" />;
      case "message":
        return <MessageSquare class="w-4 h-4" />;
    }
  };

  return (
    <CollapsibleModule id="pins" title="Pins" badge={pins().length}>
      <div class="space-y-2">
        {/* Add button */}
        <Show when={!isAdding()}>
          <button
            onClick={() => setIsAdding(true)}
            class="w-full flex items-center justify-center gap-2 py-2 px-3 rounded-lg border border-dashed border-white/20 text-text-secondary hover:border-white/40 hover:text-text-primary transition-colors"
          >
            <Plus class="w-4 h-4" />
            <span class="text-sm">Add Pin</span>
          </button>
        </Show>

        {/* Add form */}
        <Show when={isAdding()}>
          <div class="p-3 rounded-lg bg-white/5 space-y-2">
            <div class="flex gap-2">
              <button
                onClick={() => setAddType("note")}
                class={`flex-1 py-1.5 px-2 rounded text-xs font-medium transition-colors ${
                  addType() === "note"
                    ? "bg-accent-primary text-white"
                    : "bg-white/10 text-text-secondary hover:bg-white/20"
                }`}
              >
                Note
              </button>
              <button
                onClick={() => setAddType("link")}
                class={`flex-1 py-1.5 px-2 rounded text-xs font-medium transition-colors ${
                  addType() === "link"
                    ? "bg-accent-primary text-white"
                    : "bg-white/10 text-text-secondary hover:bg-white/20"
                }`}
              >
                Link
              </button>
            </div>
            <Show when={addType() === "link"}>
              <input
                type="text"
                placeholder="Title (optional)"
                value={newTitle()}
                onInput={(e) => setNewTitle(e.currentTarget.value)}
                class="w-full px-3 py-1.5 rounded bg-surface-base border border-white/10 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:border-accent-primary"
              />
            </Show>
            <textarea
              placeholder={addType() === "link" ? "URL" : "Note content..."}
              value={newContent()}
              onInput={(e) => setNewContent(e.currentTarget.value)}
              rows={addType() === "note" ? 3 : 1}
              class="w-full px-3 py-1.5 rounded bg-surface-base border border-white/10 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:border-accent-primary resize-none"
            />
            <div class="flex justify-end gap-2">
              <button
                onClick={() => setIsAdding(false)}
                class="px-3 py-1.5 rounded text-sm text-text-secondary hover:bg-white/10"
              >
                Cancel
              </button>
              <button
                onClick={handleCreate}
                disabled={!newContent().trim()}
                class="px-3 py-1.5 rounded text-sm bg-accent-primary text-white hover:bg-accent-primary/80 disabled:opacity-50 disabled:cursor-not-allowed"
              >
                Save
              </button>
            </div>
          </div>
        </Show>

        {/* Pins list */}
        <For each={pins()}>
          {(pin) => (
            <div class="group relative py-2 px-2 rounded-lg hover:bg-white/5">
              <Show
                when={editingId() === pin.id}
                fallback={
                  <div class="flex items-start gap-2">
                    <span class="text-text-secondary mt-0.5">
                      {getPinIcon(pin.pin_type)}
                    </span>
                    <div class="flex-1 min-w-0">
                      <Show when={pin.title}>
                        <div class="text-sm font-medium text-text-primary truncate">
                          {pin.title}
                        </div>
                      </Show>
                      <Show
                        when={pin.pin_type === "link"}
                        fallback={
                          <p
                            class="text-sm text-text-secondary line-clamp-2 cursor-pointer"
                            onClick={() => startEdit(pin)}
                          >
                            {pin.content}
                          </p>
                        }
                      >
                        <a
                          href={pin.content}
                          target="_blank"
                          rel="noopener noreferrer"
                          class="text-sm text-accent-primary hover:underline flex items-center gap-1"
                        >
                          <span class="truncate">{pin.content}</span>
                          <ExternalLink class="w-3 h-3 flex-shrink-0" />
                        </a>
                      </Show>
                    </div>
                    <button
                      onClick={() => handleDelete(pin.id)}
                      class="opacity-0 group-hover:opacity-100 p-1 rounded text-text-muted hover:text-status-error hover:bg-status-error/10 transition-all"
                      title="Delete"
                    >
                      <Trash2 class="w-4 h-4" />
                    </button>
                  </div>
                }
              >
                {/* Edit mode */}
                <div class="space-y-2">
                  <textarea
                    value={editContent()}
                    onInput={(e) => setEditContent(e.currentTarget.value)}
                    rows={3}
                    class="w-full px-3 py-1.5 rounded bg-surface-base border border-white/10 text-sm text-text-primary focus:outline-none focus:border-accent-primary resize-none"
                  />
                  <div class="flex justify-end gap-2">
                    <button
                      onClick={cancelEdit}
                      class="px-2 py-1 rounded text-xs text-text-secondary hover:bg-white/10"
                    >
                      Cancel
                    </button>
                    <button
                      onClick={() => saveEdit(pin.id)}
                      class="px-2 py-1 rounded text-xs bg-accent-primary text-white hover:bg-accent-primary/80"
                    >
                      Save
                    </button>
                  </div>
                </div>
              </Show>
            </div>
          )}
        </For>

        {/* Empty state */}
        <Show when={pins().length === 0 && !isAdding()}>
          <div class="text-center py-4">
            <Pin class="w-8 h-8 text-text-secondary mx-auto mb-2 opacity-50" />
            <p class="text-sm text-text-secondary">No pins yet</p>
            <p class="text-xs text-text-muted mt-1">
              Save notes and links for quick access
            </p>
          </div>
        </Show>
      </div>
    </CollapsibleModule>
  );
};

export default PinsModule;
```

**Step 2: Verify it compiles**

Run: `cd client && bun tsc --noEmit`
Expected: No type errors

**Step 3: Commit**

```bash
git add client/src/components/home/modules/PinsModule.tsx
git commit -m "feat(ui): add PinsModule for global pins"
```

---

## Task 11: ActiveNowModule Component

**Files:**
- Create: `client/src/components/home/modules/ActiveNowModule.tsx`

**Step 1: Extract Active Now into module**

```typescript
/**
 * ActiveNowModule Component
 *
 * Shows friends who are currently playing games.
 */

import { Component, Show, For } from "solid-js";
import { Coffee } from "lucide-solid";
import { getOnlineFriends } from "@/stores/friends";
import { getUserActivity } from "@/stores/presence";
import CollapsibleModule from "./CollapsibleModule";
import ActiveActivityCard from "../ActiveActivityCard";

const ActiveNowModule: Component = () => {
  const activeFriends = () => {
    return getOnlineFriends().filter((f) => getUserActivity(f.user_id));
  };

  return (
    <CollapsibleModule id="activeNow" title="Active Now" badge={activeFriends().length}>
      <Show
        when={activeFriends().length > 0}
        fallback={
          <div class="flex flex-col items-center justify-center py-4 text-center">
            <Coffee class="w-8 h-8 text-text-secondary mb-2 opacity-50" />
            <p class="text-sm text-text-secondary">It's quiet for now...</p>
            <p class="text-xs text-text-muted mt-1">
              When friends start playing, they'll show here!
            </p>
          </div>
        }
      >
        <div class="space-y-3">
          <For each={activeFriends()}>
            {(friend) => (
              <ActiveActivityCard
                userId={friend.user_id}
                displayName={friend.display_name}
                username={friend.username}
                avatarUrl={friend.avatar_url}
                activity={getUserActivity(friend.user_id)!}
              />
            )}
          </For>
        </div>
      </Show>
    </CollapsibleModule>
  );
};

export default ActiveNowModule;
```

**Step 2: Create index.ts for modules**

Create `client/src/components/home/modules/index.ts`:
```typescript
export { default as CollapsibleModule } from "./CollapsibleModule";
export { default as ActiveNowModule } from "./ActiveNowModule";
export { default as PendingModule } from "./PendingModule";
export { default as PinsModule } from "./PinsModule";
```

**Step 3: Verify it compiles**

Run: `cd client && bun tsc --noEmit`
Expected: No type errors

**Step 4: Commit**

```bash
git add client/src/components/home/modules/
git commit -m "feat(ui): add ActiveNowModule and module exports"
```

---

## Task 12: Refactor HomeRightPanel to Use Modules

**Files:**
- Modify: `client/src/components/home/HomeRightPanel.tsx`

**Step 1: Refactor to use module components**

Replace the content of `HomeRightPanel.tsx`:
```typescript
/**
 * HomeRightPanel Component
 *
 * Context-aware right panel for Home view.
 * Shows modular sidebar when in Friends view, user profile for DMs.
 */

import { Component, Show, For } from "solid-js";
import { dmsState, getSelectedDM } from "@/stores/dms";
import { getUserActivity } from "@/stores/presence";
import { ActivityIndicator } from "@/components/ui";
import { ActiveNowModule, PendingModule, PinsModule } from "./modules";

const HomeRightPanel: Component = () => {
  const dm = () => getSelectedDM();
  const isGroupDM = () => dm()?.participants && dm()!.participants.length > 1;

  return (
    <aside class="hidden xl:flex w-[360px] flex-col bg-surface-layer1 border-l border-white/10 h-full">
      <Show
        when={!dmsState.isShowingFriends && dm()}
        fallback={
          // Modular Sidebar (Friends View)
          <div class="flex-1 flex flex-col overflow-y-auto">
            <ActiveNowModule />
            <PendingModule />
            <PinsModule />
          </div>
        }
      >
        <Show
          when={isGroupDM()}
          fallback={
            // 1:1 DM - show user profile
            <div class="p-4">
              <div class="flex flex-col items-center">
                <div class="w-20 h-20 rounded-full bg-accent-primary flex items-center justify-center mb-3">
                  <span class="text-2xl font-bold text-surface-base">
                    {dm()?.participants[0]?.display_name?.charAt(0).toUpperCase()}
                  </span>
                </div>
                <h3 class="text-lg font-semibold text-text-primary">
                  {dm()?.participants[0]?.display_name}
                </h3>
                <p class="text-sm text-text-secondary">
                  @{dm()?.participants[0]?.username}
                </p>
                <Show when={dm()?.participants[0]?.user_id && getUserActivity(dm()!.participants[0].user_id)}>
                  <div class="mt-3 w-full px-3 py-2 rounded-lg bg-white/5">
                    <ActivityIndicator activity={getUserActivity(dm()!.participants[0].user_id)!} />
                  </div>
                </Show>
              </div>
            </div>
          }
        >
          {/* Group DM - show participants */}
          <div class="p-4">
            <h3 class="text-sm font-semibold text-text-secondary uppercase tracking-wide mb-3">
              Members — {dm()?.participants.length}
            </h3>
            <div class="space-y-2">
              <For each={dm()?.participants}>
                {(p) => (
                  <div class="flex items-start gap-2 py-1">
                    <div class="w-8 h-8 rounded-full bg-accent-primary flex items-center justify-center flex-shrink-0">
                      <span class="text-xs font-semibold text-surface-base">
                        {p.display_name.charAt(0).toUpperCase()}
                      </span>
                    </div>
                    <div class="min-w-0 flex-1">
                      <span class="text-sm text-text-primary">{p.display_name}</span>
                      <Show when={p.user_id && getUserActivity(p.user_id)}>
                        <ActivityIndicator activity={getUserActivity(p.user_id)!} compact />
                      </Show>
                    </div>
                  </div>
                )}
              </For>
            </div>
          </div>
        </Show>
      </Show>
    </aside>
  );
};

export default HomeRightPanel;
```

**Step 2: Verify it compiles**

Run: `cd client && bun tsc --noEmit`
Expected: No type errors

**Step 3: Commit**

```bash
git add client/src/components/home/HomeRightPanel.tsx
git commit -m "refactor(ui): use modular sidebar in HomeRightPanel"
```

---

## Task 13: Update Roadmap and Changelog

**Files:**
- Modify: `docs/project/roadmap.md`
- Modify: `CHANGELOG.md`

**Step 1: Update roadmap**

Mark Modular Home Sidebar as complete in `docs/project/roadmap.md`:
```markdown
- [x] **[UX] Modular Home Sidebar** ✅
  - Collapsible module framework with server-synced state
  - Active Now module showing friends' game activity
  - Pending module for friend requests and guild invites
  - Pins module for notes, links, and bookmarks
  - **Design:** `docs/plans/2026-01-24-modular-home-sidebar-design.md`
```

**Step 2: Update changelog**

Add to `CHANGELOG.md` under `[Unreleased]`:
```markdown
- Modular Home Sidebar
  - Collapsible modules (Active Now, Pending, Pins) in Home right panel
  - Global Pins feature for saving notes and links across devices
  - Pending module shows friend requests with quick accept/decline
  - Module collapse state syncs across devices via preferences
```

**Step 3: Commit**

```bash
git add docs/project/roadmap.md CHANGELOG.md
git commit -m "docs: mark modular home sidebar complete"
```

---

## Verification

After all tasks complete:

1. **Backend tests:** `cd server && cargo test`
2. **Frontend types:** `cd client && bun tsc --noEmit`
3. **Manual test:**
   - Open Home view, verify modules display
   - Collapse/expand modules, refresh, verify state persists
   - Create note pin, verify appears
   - Create link pin, verify opens in new tab
   - Delete pin, verify removed
   - Accept/decline friend request (if available)
