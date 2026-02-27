# Phase 3 Implementation Agent Prompt

Use this prompt to instruct a Claude Sonnet agent to implement Phase 3 features.

---

## Agent Prompt

```
You are implementing Phase 3 (Guild Architecture) for the Canis VoiceChat platform.

## Project Context

Canis is a self-hosted Discord-like voice and text chat platform built with:
- **Server:** Rust (Axum, SQLx, PostgreSQL, Redis)
- **Client:** Solid.js + TypeScript + UnoCSS (Tauri desktop app)
- **Patterns:** JWT auth, WebSocket for real-time, REST API

## Your Task

Implement the Guild (Server) system following the plan at:
`docs/plans/PHASE_3_IMPLEMENTATION.md`

## Execution Rules

1. **Read the plan first** - Understand all tasks before starting
2. **One task at a time** - Complete each task fully before moving to the next
3. **Build after each task** - Run `cargo build` and `bun run build` to verify
4. **Follow existing patterns** - Match the code style in existing files:
   - `server/src/chat/` for backend patterns
   - `client/src/stores/` for frontend store patterns
   - `client/src/components/` for UI components

## Task Order

Execute in this exact order:
1. Database migration (Task 1)
2. Guild backend API (Task 2)
3. Channel guild scope (Task 3)
4. Frontend guild store (Task 6)
5. Server Rail UI (Task 7)
6. Context switching (Task 8)
7. Friends backend (Task 4)
8. DM backend (Task 5)
9. Friends UI (Task 9)
10. Home view (Task 10)
11. Rate limiting (Task 11)

## Code Style Requirements

### Rust (Server)
- Use `thiserror` for custom errors
- Use `#[tracing::instrument]` on handlers
- Use `sqlx::query_as!` with type annotations
- Return `Result<Json<T>, CustomError>`
- Follow axum handler patterns from `server/src/chat/`

### TypeScript (Client)
- Use Solid.js signals and stores
- Use `createStore` from `solid-js/store`
- Follow component patterns from `client/src/components/`
- Use UnoCSS utility classes matching existing theme

## Key Files to Reference

Before implementing, read these files to understand patterns:
- `server/src/chat/channels.rs` - Handler patterns
- `server/src/db/queries.rs` - Database query patterns
- `client/src/stores/channels.ts` - Store patterns
- `client/src/stores/guilds.ts` - Guild store skeleton (already exists)
- `client/src/components/layout/AppShell.tsx` - Layout structure

## Constraints

- **License:** Only use MIT/Apache-2.0/BSD licensed crates
- **Security:** Validate all inputs server-side
- **Backwards Compatible:** Existing features must continue working
- **No Breaking Changes:** Existing API endpoints remain functional

## Verification

After completing all tasks:
1. Run `cargo build && cargo test`
2. Run `cd client && bun run build`
3. Test manually: create guild, switch guilds, send friend request, start DM

## Start

Begin with Task 1: Create the database migration file at:
`server/migrations/20240201000000_guilds.sql`

Follow the schema in the implementation plan exactly.
```

---

## Alternative: Task-Specific Prompts

For more granular control, use these task-specific prompts:

### Task 1: Database Migration
```
Create the Phase 3 database migration for Canis at server/migrations/20240201000000_guilds.sql

Include tables for:
- guilds (id, name, owner_id, icon_url, description, timestamps)
- guild_members (guild_id, user_id, nickname, joined_at)
- guild_member_roles (guild_id, user_id, role_id)
- friendships (id, requester_id, addressee_id, status enum, timestamps)
- dm_participants (channel_id, user_id, joined_at)

Modify existing tables:
- Add guild_id to channels, roles, channel_categories

Add appropriate indexes and constraints.

Reference: docs/plans/PHASE_3_IMPLEMENTATION.md Task 1
```

### Task 2: Guild Backend
```
Implement the Guild API for Canis in Rust/Axum.

Create files:
- server/src/guild/mod.rs (module + router)
- server/src/guild/handlers.rs (CRUD handlers)
- server/src/guild/types.rs (request/response types)

Endpoints:
- POST /api/guilds - Create guild
- GET /api/guilds - List user's guilds
- GET /api/guilds/:id - Get guild
- PATCH /api/guilds/:id - Update guild
- DELETE /api/guilds/:id - Delete guild (owner only)
- POST /api/guilds/:id/join - Join guild
- POST /api/guilds/:id/leave - Leave guild
- GET /api/guilds/:id/members - List members
- GET /api/guilds/:id/channels - List channels

Follow patterns in server/src/chat/channels.rs.
Reference: docs/plans/PHASE_3_IMPLEMENTATION.md Task 2
```

### Task 7: Server Rail UI
```
Create the Server Rail component for Canis (Solid.js).

Create files:
- client/src/components/layout/ServerRail.tsx
- client/src/components/layout/ServerIcon.tsx

ServerRail should:
- Be a 72px wide vertical sidebar on the left
- Show Home button at top (for DMs/friends)
- Show list of guild icons (from guildsState.guilds)
- Show "Add Server" button at bottom
- Highlight active guild
- Support tooltips on hover

Use existing theme colors (bg-surface-base, text-text-primary, etc.)
Reference: docs/plans/PHASE_3_IMPLEMENTATION.md Task 7
```

---

## Usage

### Full Implementation
```bash
# In Claude Code or similar
claude "Read docs/plans/PHASE_3_IMPLEMENTATION.md and implement all tasks in order. Start with Task 1."
```

### Single Task
```bash
claude "Implement Task 2 (Guild Backend) from docs/plans/PHASE_3_IMPLEMENTATION.md"
```

### With Verification
```bash
claude "Implement Task 7 (Server Rail UI). After implementation, run 'bun run build' to verify it compiles."
```
