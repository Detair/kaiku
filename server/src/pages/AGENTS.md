<!-- Parent: ../AGENTS.md -->
# Pages Module

## Purpose
Static information pages for the platform and guilds. Two scopes: platform-level (Terms of Service, Privacy Policy) and guild-level (rules, FAQ, welcome). Not chat channels.

## Key Files

- `types.rs` - Core structs: `Page` (full content), `PageListItem` (no content, for listings), `PageRevision`, `PageAcceptance`, `PageCategory`. Contains `deserialize_double_option` for `Option<Option<Uuid>>` on `category_id` — handles absent/null/value distinction.
- `queries.rs` - All DB access. `hash_content()` computes SHA-256 of content stored as `content_hash`. `slugify()` lowercases + replaces non-alphanumeric with `-`. Position assigned atomically via inline `COUNT(*)` subquery on insert.
- `handlers.rs` - 1888 lines. Platform handlers require `is_system_admin`. Guild handlers require `GuildPermissions::MANAGE_PAGES`. Read endpoints (list, get-by-slug) require auth but no special permission. `get_pending_acceptance` returns pages the calling user hasn't accepted yet.
- `constants.rs` - Hard limits: 100KB content, 100-char title/slug, 50-char category name, 10 pages/scope (configurable via `config.max_pages_per_guild`), 25 revisions/page, 20 categories/guild, 7-day slug cooldown after deletion.
- `router.rs` - Three routers: `platform_pages_router` (`/api/pages`), `guild_pages_router` (`/api/guilds/{guild_id}/pages`), `guild_page_categories_router` (`/api/guilds/{guild_id}/page-categories`).

## For AI Agents

### Scope Distinction
`guild_id: Option<Uuid>` is the scope discriminator throughout. `None` = platform page, `Some(id)` = guild page. All queries branch on this. Platform pages don't support categories.

### Permission Model
- **Read** (list, get-by-slug): any authenticated user, no permission check beyond `require_auth` middleware.
- **Write** (create, update, delete, reorder, restore): platform pages require `is_system_admin`; guild pages require `GuildPermissions::MANAGE_PAGES`.
- **Accept**: any authenticated user can accept any page (`POST /{id}/accept`).

### Revision System
Every create and update snapshots a `PageRevision`. Revisions are numbered sequentially per page. Restore (`POST /{page_id}/revisions/{n}/restore`) creates a new revision rather than mutating history. Max revisions enforced at create time; oldest revision is pruned when limit is hit.

### Slug Rules
- Auto-generated from title via `slugify()` if not provided.
- Must not match `RESERVED_SLUGS` (includes `admin`, `api`, `new`, `edit`, `settings`, etc.).
- 7-day cooldown after deletion — slug can't be reused immediately.
- Slug uniqueness is scoped: same slug can exist in different guilds or on platform.

### Content Format
Markdown with Mermaid diagram support (rendering is client-side). Stored as raw text. `content_hash` (SHA-256 hex) tracks versions for acceptance records — if content changes, prior acceptances are stale.

### Acceptance Tracking
`PageAcceptance` records `(user_id, page_id, content_hash)`. `get_pending_acceptance` returns pages with `requires_acceptance = true` where the user has no acceptance record matching the current `content_hash`.

### Error Handling
Handlers use `type PageResult<T> = Result<T, (StatusCode, String)>`. Permission check DB errors fail-fast (500, not 403) to avoid security ambiguity. Slug/limit checks default to "assume at limit" on DB error.

### Adding a New Page Endpoint
1. Add query function in `queries.rs`.
2. Add handler in `handlers.rs` — check scope, check permission, validate, call query.
3. Register route in the appropriate router in `router.rs`.
4. Annotate with `#[utoipa::path(...)]` matching existing handler patterns.
