//! Database queries for the guild module.
//!
//! Each submodule mirrors a source file under `guild/`:
//!
//! - `core` — guild CRUD, members, settings, channel listing/reorder, bots and slash commands (used
//!   by `handlers.rs`)
//! - `categories` — channel category CRUD (used by `categories.rs`)
//! - `roles` — guild role CRUD and member assignment (used by `roles.rs`)
//! - `emojis` — custom emoji CRUD (used by `emojis.rs`)
//! - `invites` — guild invite CRUD and join flow (used by `invites.rs`)
//! - `search` — small helper queries for the search handler (used by `search.rs`)
//! - `limits` — reusable count queries shared by enforcement checks and the usage stats endpoint
//!   (used by `handlers.rs`, formerly `guild/limits.rs`)
//!
//! Functions take `&PgPool` (or `&mut Transaction<Postgres>` for transactional
//! callers) and return `Result<T, ModuleError>` where `ModuleError` is the
//! source-file's local error type, so handlers can use `?` directly.

pub mod categories;
pub mod core;
pub mod emojis;
pub mod invites;
pub mod limits;
pub mod reaction_roles;
pub mod roles;
pub mod search;
