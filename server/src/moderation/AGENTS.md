<!-- Parent: ../AGENTS.md -->
# Moderation Module

## Purpose
User reporting queue (submit, claim, resolve) and per-guild content filtering (keyword + regex engine, caching, audit log). Security-critical: all filter mutations write to the audit log.

## Key Files

| File | Role |
|------|------|
| `types.rs` | Report enums (`ReportCategory`, `ReportStatus`, `ReportTargetType`) and `ReportError` with `IntoResponse` |
| `handlers.rs` | `POST /api/reports` — user-facing only; enforces 5-reports/hour Redis rate limit and duplicate detection via DB unique index |
| `admin_handlers.rs` | Report queue management (`list`, `get`, `claim`, `resolve`, `stats`); requires `ElevatedAdmin` extension on claim |
| `filter_types.rs` | `FilterCategory` (Slurs/HateSpeech/Spam/AbusiveLanguage/Custom), `FilterAction` (Block/Log/Warn), DB models, request/response types, `FilterError` |
| `filter_engine.rs` | Hybrid Aho-Corasick (keywords, fast path) + `regex::Regex` (patterns); `FilterEngine::build()` compiles once, `check()` runs both passes |
| `filter_cache.rs` | `DashMap`-backed per-guild engine cache; generation counters prevent TOCTOU races on concurrent invalidation |
| `filter_handlers.rs` | CRUD for filter configs and custom patterns under `/api/guilds/{id}/filters`; `test_filter` uses `build_ephemeral` to avoid cache churn |
| `filter_queries.rs` | All DB ops for `guild_filter_configs`, `guild_filter_patterns`, `moderation_actions`; truncates logged content to 200 chars |
| `defaults.rs` | Embeds wordlists via `include_str!` at compile time; `parse_wordlist()` splits lines into keywords vs `regex:`-prefixed patterns |
| `wordlists/` | Four `.txt` files (`slurs.txt`, `hate_speech.txt`, `spam_patterns.txt`, `abusive.txt`) — see TD-26 below |

## For AI Agents

### Two Separate Subsystems
Reports and filters are independent. Reports live in `user_reports` table with their own error type (`ReportError`). Filters live in `guild_filter_configs` / `guild_filter_patterns` / `moderation_actions` with `FilterError`. Don't mix them.

### Filter Architecture Flow
```
message arrives → FilterCache::get_or_build(guild_id)
                → FilterEngine::check(content)
                → if blocked: filter_queries::log_moderation_action()
```
`FilterCache` is stored in `AppState` as `filter_cache: Arc<FilterCache>`. Call `state.filter_cache.invalidate(guild_id)` after every mutation to filter configs or patterns — all three mutating handlers already do this.

### Cache Invalidation Pattern
Every handler that mutates filter state must:
1. Write to DB
2. Call `state.filter_cache.invalidate(guild_id)`
3. Write audit log via `permissions::queries::write_audit_log()`

Missing any step is a bug. The audit log call uses `.ok()` — failures are non-fatal.

### Test Endpoint Uses Ephemeral Engine
`POST /api/guilds/{id}/filters/test` calls `build_ephemeral` instead of `get_or_build`. This builds a fresh engine from DB without inserting into the shared cache, so test runs don't pollute production cache state.

### ReDoS Protection
`validate_regex()` in `filter_handlers.rs` compiles the regex then runs it against 1000 `'a'` chars. If that takes >10ms, the pattern is rejected. Apply this check whenever accepting user-supplied regex.

### Custom Patterns: Hard Limits
- Max 100 patterns per guild (`MAX_CUSTOM_PATTERNS`)
- Max 500 chars per pattern (`MAX_PATTERN_LENGTH`)
- Max 4000 chars for test input (`MAX_TEST_INPUT_LENGTH`)

### Double-Option Deserialization
`UpdatePatternRequest.description` uses `Option<Option<String>>` with a custom deserializer. Absent field = don't change. `null` = clear to NULL. String value = update. The DB query uses a boolean sentinel (`$5`) to distinguish "set null" from "leave unchanged".

### Permission Gate
All filter endpoints require `GuildPermissions::MANAGE_GUILD`. The check is `require_guild_permission(...).map_err(|_| FilterError::Forbidden)`. Admin report endpoints require `ElevatedAdmin` (elevated session, not just guild permission).

### TD-26: Wordlists Are Placeholders
`wordlists/slurs.txt`, `hate_speech.txt`, and `abusive.txt` contain only comment headers — no actual entries. `spam_patterns.txt` has 4 regex patterns. The built-in filter categories (`Slurs`, `HateSpeech`, `AbusiveLanguage`) will match nothing until these files are populated. Custom guild patterns work correctly regardless.

### Report Duplicate Detection
The unique index `idx_reports_no_duplicate_active` on `user_reports` prevents duplicate active reports. The handler catches `sqlx::Error::Database` and checks `db_err.constraint()` to return `ReportError::Duplicate` (409) instead of a generic 500.

### Moderation Log Content Truncation
`log_moderation_action` truncates `original_content` to 200 chars on a valid UTF-8 char boundary before storing. This is intentional data minimization — don't remove it.
