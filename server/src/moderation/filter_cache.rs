//! Per-Guild Filter Engine Cache
//!
//! Caches compiled `FilterEngine` instances per guild using `DashMap`
//! for lock-free concurrent access. Engines are lazily built on first
//! message and invalidated when filter config changes.

use std::sync::Arc;

use dashmap::DashMap;
use sqlx::PgPool;
use uuid::Uuid;

use super::filter_engine::FilterEngine;
use super::filter_queries;

/// Thread-safe cache of per-guild filter engines.
#[derive(Default)]
pub struct FilterCache {
    engines: DashMap<Uuid, Arc<FilterEngine>>,
}

impl FilterCache {
    /// Create a new empty cache.
    pub fn new() -> Self {
        Self {
            engines: DashMap::new(),
        }
    }

    /// Get the filter engine for a guild, building it if not cached.
    pub async fn get_or_build(
        &self,
        pool: &PgPool,
        guild_id: Uuid,
    ) -> Result<Arc<FilterEngine>, String> {
        // Fast path: engine already cached
        if let Some(engine) = self.engines.get(&guild_id) {
            return Ok(Arc::clone(engine.value()));
        }

        // Slow path: build from database
        let configs = filter_queries::list_filter_configs(pool, guild_id)
            .await
            .map_err(|e| format!("Failed to load filter configs: {e}"))?;

        let patterns = filter_queries::list_custom_patterns(pool, guild_id)
            .await
            .map_err(|e| format!("Failed to load custom patterns: {e}"))?;

        let engine = Arc::new(FilterEngine::build(&configs, &patterns)?);
        self.engines.insert(guild_id, Arc::clone(&engine));

        Ok(engine)
    }

    /// Invalidate the cached engine for a guild.
    ///
    /// The next call to `get_or_build` will rebuild from the database.
    pub fn invalidate(&self, guild_id: Uuid) {
        self.engines.remove(&guild_id);
    }
}
