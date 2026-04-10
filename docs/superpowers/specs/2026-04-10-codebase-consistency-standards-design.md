# Codebase Consistency Standards

**Date:** 2026-04-10
**Status:** Approved
**Goal:** Define canonical patterns for folder structure, file naming, error handling, handler/query organization, and client conventions so that every module is structured the same way and code review becomes predictable.

## Approach: Tiered Convention

Two tiers based on module complexity. Same canonical names, different minimum requirements. The client side codifies existing patterns and addresses monolith splitting.

---

## 1. Server Module Structure

### Tier 1 — Compact Modules (<=500 lines total, <=2 endpoints)

Everything lives in `mod.rs`. The file follows this order:

```rust
// 1. Imports
// 2. Error enum (with thiserror + IntoResponse)
// 3. Request/Response types (with serde + utoipa)
// 4. Query functions (async fn, take &PgPool)
// 5. Handler functions (async fn, axum extractors)
// 6. Router function: pub fn router() -> Router<AppState>
```

The ordering principle: **dependencies flow downward**. Errors are used by queries and handlers. Types are used by queries and handlers. Queries are used by handlers. The router references handlers.

**Current Tier 1 candidates:** Only `email` (237 lines, single `mod.rs`) is already Tier 1.

**Modules to consolidate into Tier 1:** `connectivity` (466 lines across 2 files), `presence` (556 lines across 2 files — borderline, use judgment). These currently have unnecessary splits (`mod.rs` + `handlers.rs` or `mod.rs` + `types.rs`) and should be collapsed into single `mod.rs` files if they fit.

**Modules that should be Tier 2 despite small size:** `crypto` (708 lines across 2 files), `discovery` (540 lines across 3 files with `types.rs`). These exceed the threshold or already have enough distinct concerns to warrant the standard split.

**Graduation rule:** When a `mod.rs` exceeds ~500 lines or gains a third endpoint group, split into Tier 2 files. This isn't a hard gate — use judgment. A 600-line module with two coherent endpoint groups can stay Tier 1. A 400-line module with three unrelated concerns should split.

### Tier 2 — Standard Modules (>500 lines)

```
feature/
├── mod.rs          # Module doc comment, re-exports, router() function
├── error.rs        # FeatureError enum + IntoResponse impl
├── types.rs        # Request/response DTOs (serde + utoipa derives)
├── queries.rs      # Database query functions (async fn, &PgPool)
├── handlers.rs     # HTTP handler functions
└── [sub-domain].rs # Optional: split handlers by sub-domain when handlers.rs > ~1000 lines
```

**When handlers split into sub-domain files:**
- `handlers.rs` is removed entirely — replaced by the sub-files
- Each sub-file contains handlers for a cohesive group of endpoints
- Example for `auth/`: `login.rs`, `register.rs`, `mfa.rs`, `sessions.rs`, `profile.rs`
- Example for `chat/`: `channels.rs`, `messages.rs`, `dm.rs`, `uploads.rs` (already done)
- `mod.rs` imports and re-exports from each sub-file

**Optional files:**
- `queries.rs` — only if the module accesses the database
- Sub-domain splits — only when handlers would exceed ~1000 lines
- Internal utility files (e.g., `jwt.rs`, `cookies.rs`, `sfu.rs`, `peer.rs`) are fine alongside the standard files. These handle module-internal implementation concerns that don't fit into handlers, queries, types, or error. No naming convention beyond being a descriptive noun.

**`mod.rs` template (Tier 2):**

```rust
//! Feature module documentation
//!
//! Brief description of what this module does.

mod error;
mod handlers;
mod queries;
mod types;

pub use error::FeatureError;
// Re-export only what other modules need

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/path", get(handlers::get_thing))
        .route("/path", post(handlers::create_thing))
}
```

---

## 2. Server Error Handling Convention

### One error enum per module

Every Tier 2 module gets exactly one `FeatureError` enum in `error.rs`. Tier 1 modules define it inline in `mod.rs`.

**Naming:** `{ModuleName}Error` — always matches the module directory name in singular PascalCase.

| Directory | Error Name |
|-----------|-----------|
| `auth/` | `AuthError` |
| `chat/` | `ChatError` |
| `guild/` | `GuildError` |
| `voice/` | `VoiceError` |
| `moderation/` | `ModerationError` |
| `webhooks/` | `WebhookError` |
| `permissions/` | `PermissionError` |
| `governance/` | `GovernanceError` |
| `workspaces/` | `WorkspaceError` |
| `admin/` | `AdminError` |
| `social/` | `SocialError` |
| `connectivity/` | `ConnectivityError` |
| `discovery/` | `DiscoveryError` |
| `ratelimit/` | `RatelimitError` |
| `pages/` | `PagesError` |

All modules follow the same `{ModuleName}Error` convention — the table above is exhaustive for current modules.

### Consolidation rule

One error enum per module — no per-handler error types. Modules that currently scatter errors across handler files (e.g., `chat/` with `MessageError`, `UploadError`, `ChannelError`, `OverrideError`, `ProcessingError`, `S3Error`, `DmSearchError` — 7 total) consolidate into a single flat `ChatError` enum with clearly named variants.

### Standard `error.rs` template

```rust
use axum::response::{IntoResponse, Response};
use axum::http::StatusCode;
use axum::Json;
use serde_json::json;

#[derive(thiserror::Error, Debug)]
pub enum FeatureError {
    #[error("Not found: {0}")]
    NotFound(Uuid),
    #[error("Forbidden")]
    Forbidden,
    #[error("Internal error")]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for FeatureError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        };
        (status, Json(json!({ "error": code, "message": self.to_string() }))).into_response()
    }
}
```

### Rules

1. **Name:** `{ModuleName}Error` — always matches the module directory name
2. **Location:** `error.rs` for Tier 2, inline in `mod.rs` for Tier 1
3. **One per module** — no per-handler error types
4. **Always `thiserror`** — no manual `Display` impls
5. **Always `IntoResponse`** — JSON body with `error` code + `message` fields
6. **Internal errors:** Use `#[from] anyhow::Error` for catch-all internal failures, log at the `IntoResponse` boundary with `tracing::error!`

---

## 3. Server Handler & Query Organization

### Handlers are thin

A handler function should:
1. Extract inputs (axum extractors)
2. Call query/service functions
3. Return a response or error

No raw SQL in handlers. No complex business logic. No multi-step orchestration.

**Standard handler signature:**

```rust
#[utoipa::path(...)]
#[tracing::instrument(skip(state))]
pub async fn get_thing(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    claims: Claims,
) -> Result<Json<ThingResponse>, FeatureError> {
    let thing = queries::get_thing(&state.pool, id).await?;
    Ok(Json(ThingResponse::from(thing)))
}
```

**Splitting rule:** When a single handlers file exceeds ~1000 lines, split by sub-domain. Each sub-file contains handlers for a cohesive group. The split files replace `handlers.rs` entirely.

### All database access goes through `queries.rs`

No inline `sqlx::query!` in handler functions.

**Standard query function:**

```rust
pub async fn get_thing(pool: &PgPool, id: Uuid) -> Result<Thing, FeatureError> {
    sqlx::query_as!(Thing, "SELECT ... FROM things WHERE id = $1", id)
        .fetch_optional(pool)
        .await
        .map_err(|e| FeatureError::Internal(e.into()))?
        .ok_or(FeatureError::NotFound(id))
}
```

**Rules:**
1. Query functions take `&PgPool` (or `&mut PgConnection` for transactions) as first arg
2. Query functions return `Result<T, FeatureError>` — they map DB errors to domain errors
3. One `queries.rs` per module, even when handlers are split into sub-domain files
4. If `queries.rs` exceeds ~800 lines, split into `queries/mod.rs` + sub-files

### Dependency flow

```
Request -> Handler (extract, validate, authorize) -> Query (DB access) -> Response
                                                   /
                                           types.rs (DTOs)
                                           error.rs (error mapping)
```

Handlers import from `queries`, `types`, and `error`. Queries import from `types` and `error`. No circular dependencies.

---

## 4. Server Re-exports & Module Public API

### `mod.rs` is the module's public contract

```rust
//! Channel management
//!
//! Handles channel CRUD, membership, permission overrides,
//! and category organization within guilds.

mod error;
mod handlers;
mod queries;
mod types;

pub use error::ChatError;
pub use types::ChannelResponse;  // only if used outside this module

pub fn router() -> Router<AppState> { ... }
```

**Rules:**
1. `mod.rs` contains the `router()` function and `pub use` re-exports — nothing else
2. Only re-export what other modules actually consume
3. Handler functions are never `pub` outside the module
4. Query functions are never `pub` outside the module
5. If another module needs data from your module, expose through `pub use`, don't let it reach into internal files

**Import style within a module:**

Use `super::` for sibling imports:

```rust
// In handlers.rs or login.rs:
use super::error::AuthError;
use super::types::{LoginRequest, LoginResponse};
use super::queries;
```

### `api/mod.rs` — the routing table

After dissolving `api/` endpoints into their feature modules, `api/mod.rs` becomes the top-level router that nests all feature routers. This is the one file that references all modules.

---

## 5. Client Component & Store Conventions

### Component organization

Feature-folder structure:

```
components/
├── feature-name/
│   ├── FeatureMain.tsx
│   ├── FeatureItem.tsx
│   ├── FeatureModal.tsx
│   └── index.ts            # Public exports
└── ui/
    └── SharedComponent.tsx  # Used across 3+ feature folders
```

**Naming suffix conventions:**

| Suffix | Purpose | Example |
|--------|---------|---------|
| `Modal` | Dialog/overlay | `CreateChannelModal` |
| `Panel` | Sidebar or section | `CommandCenterPanel` |
| `Settings` | Configuration UI | `AudioSettings` |
| `Tab` | Tab content | `GeneralTab` |
| `List` | Renders a collection | `MessageList` |
| `Item` | Single entry in a list | `ChannelItem` |
| `Sidebar` | Persistent side panel | `ThreadSidebar` |
| `Picker` | Selection UI | `EmojiPicker` |
| `Indicator` | Small status display | `QualityIndicator` |
| `Button` | Specific action trigger | `ScreenShareButton` |
| `Drawer` | Slide-out panel | `PinDrawer` |

**Rules:**
1. PascalCase for `.tsx`, camelCase for `.ts`
2. Feature folders are lowercase
3. `ui/` is reserved for components used across 3+ feature folders
4. `index.ts` exports only components other folders import
5. No component file should exceed ~1000 lines

### Store conventions

```typescript
// 1. State interface
interface FeatureState { ... }

// 2. Store creation
const [state, setState] = createStore<FeatureState>({ ... });

// 3. Derived accessors (exported)
export const selectedItem = () => state.items.find(...);

// 4. Actions (exported)
export async function loadItems(): Promise<void> { ... }
```

**Rules:**
1. One store per domain: `channels.ts` — not `channelStore.ts`
2. Stores never import from other stores to mutate state (`websocket.ts` is the sole exception as central dispatcher)
3. Components use exported accessors and actions, never raw store state
4. Split when exceeding ~1000 lines
5. Tests in `stores/__tests__/feature.test.ts`

---

## 6. Client Monolith Splitting

### `lib/tauri.ts` -> `lib/tauri/`

```
lib/tauri/
├── index.ts        # Re-exports everything (preserves import paths)
├── auth.ts
├── channels.ts
├── messages.ts
├── guilds.ts
├── voice.ts
├── social.ts
├── admin.ts
├── uploads.ts
└── settings.ts
```

`index.ts` re-exports so existing `import * as tauri from "@/lib/tauri"` continues to work.

### `stores/websocket.ts` -> `stores/websocket/`

```
stores/websocket/
├── index.ts          # Connection + dispatch table
├── messageEvents.ts
├── presenceEvents.ts
├── voiceEvents.ts
├── guildEvents.ts
└── socialEvents.ts
```

Each file exports handler functions. `index.ts` wires them into the dispatch table.

### `lib/types.ts` -> `lib/types/`

```
lib/types/
├── index.ts        # Re-exports everything
├── channel.ts
├── message.ts
├── guild.ts
├── user.ts
├── voice.ts
└── common.ts
```

All three splits use the re-export pattern to preserve existing import paths.

---

## 7. Shared Crate & Testing Conventions

### `vc-common` — what goes in

**In `vc-common`:**
- Domain entities shared between server and `src-tauri` (`User`, `Channel`, `Message`, `Guild`)
- WebSocket protocol types (event enums, payloads both sides serialize/deserialize)
- Shared constants (permission bitflags, limits, protocol version)

**Stays in the feature module:**
- API request/response DTOs
- Server-internal types (DB row mappings, intermediate processing)
- Client-internal types (UI state, component props)

**Rule of thumb:** If changing the type requires changes to both `server/` and `src-tauri/`, it belongs in `vc-common`. Otherwise it stays local.

### Testing conventions

**Server unit tests:** `#[cfg(test)]` at the bottom of source files. Standard Rust convention.

**Server integration tests:** `/server/tests/integration/{feature}_http.rs`. One file per feature area.

**Client store/lib tests:** `__tests__/feature.test.ts` colocated next to the code.

**Client E2E tests:** `/client/e2e/{feature}.spec.ts`. One file per feature area.

No coverage targets prescribed. The standard covers structure and naming, not quantity.

---

## 8. Naming Conventions & Documentation

### File naming — server

| File | Naming |
|------|--------|
| Module root | `mod.rs` |
| Error enum | `error.rs` (never in `types.rs`) |
| Request/response DTOs | `types.rs` |
| Database queries | `queries.rs` |
| HTTP handlers (unsplit) | `handlers.rs` |
| Handler sub-domain splits | Noun: `login.rs`, `mfa.rs` — not `login_handlers.rs` |

### File naming — client

| File | Naming |
|------|--------|
| Components | PascalCase `.tsx`: `ChannelList.tsx` |
| Utilities | camelCase `.ts`: `contextMenuBuilders.ts` |
| Stores | camelCase domain noun `.ts`: `channels.ts` |
| Unit tests | `.test.ts` |
| E2E tests | `.spec.ts` |
| Re-export index | `index.ts` — re-exports only, no logic |

### Module documentation

Every module's `mod.rs` starts with:

```rust
//! Short noun phrase (shows in cargo doc listing)
//!
//! What this module handles, max 2-3 lines.
```

No mandatory comments on every function. `#[utoipa::path]` covers the public API. Only add comments explaining "why" when the reason isn't obvious.

---

## 9. `api/` Module Dissolution

Endpoints currently in `api/` that belong in feature modules:

| Current file | Target module |
|-------------|--------------|
| `api/bots.rs` | `guild/bots.rs` or dedicated `bots/` module |
| `api/commands.rs` | `guild/commands.rs` or `bots/commands.rs` |
| `api/pins.rs` | `chat/pins.rs` |
| `api/reactions.rs` | `chat/reactions.rs` |
| `api/preferences.rs` | `auth/preferences.rs` or dedicated `preferences/` |
| `api/favorites.rs` | dedicated `favorites/` (Tier 1) |
| `api/unread.rs` | `chat/unread.rs` |
| `api/channel_pins.rs` | `chat/channel_pins.rs` |
| `api/global_search.rs` | dedicated `search/` (Tier 1) |
| `api/settings.rs` | dedicated `settings/` (Tier 1) or `auth/settings.rs` |
| `api/setup.rs` | dedicated `setup/` (Tier 1) |
| `api/files.rs` | `chat/files.rs` |

After dissolution, `api/mod.rs` retains only the top-level router that nests all feature routers.

---

## Summary: The Canonical Checklist

When reviewing any server module, check:

- [ ] Error enum in `error.rs` (or inline for Tier 1), named `{ModuleName}Error`
- [ ] DTOs in `types.rs`, not scattered in handlers
- [ ] All SQL in `queries.rs`, not in handlers
- [ ] Handlers are thin — extract, call query, return response
- [ ] `mod.rs` has doc comment, re-exports, and `router()` only
- [ ] No handler file exceeds ~1000 lines
- [ ] `super::` imports within the module

When reviewing any client code, check:

- [ ] Component in correct feature folder with correct naming suffix
- [ ] Store follows the standard pattern (interface, store, accessors, actions)
- [ ] No component/store file exceeds ~1000 lines
- [ ] Tests colocated in `__tests__/`
