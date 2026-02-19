//! Per-Guild Filter Engine Cache
//!
//! Caches compiled `FilterEngine` instances per guild using `DashMap`
//! for lock-free concurrent access. Engines are lazily built on first
//! message and invalidated when filter config changes.
//!
//! A generation counter prevents stale engines from overwriting fresh
//! invalidations (TOCTOU protection).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use sqlx::PgPool;
use uuid::Uuid;

use super::filter_engine::FilterEngine;
use super::filter_queries;

/// Cached engine paired with the generation it was built at.
struct CachedEngine {
    engine: Arc<FilterEngine>,
    generation: u64,
}

/// Thread-safe cache of per-guild filter engines.
pub struct FilterCache {
    engines: DashMap<Uuid, CachedEngine>,
    generation: AtomicU64,
}

impl Default for FilterCache {
    fn default() -> Self {
        Self::new()
    }
}

impl FilterCache {
    /// Create a new empty cache.
    pub fn new() -> Self {
        Self {
            engines: DashMap::new(),
            generation: AtomicU64::new(0),
        }
    }

    /// Get the filter engine for a guild, building it if not cached.
    #[tracing::instrument(skip(self, pool))]
    pub async fn get_or_build(
        &self,
        pool: &PgPool,
        guild_id: Uuid,
    ) -> Result<Arc<FilterEngine>, String> {
        // Fast path: engine already cached
        if let Some(entry) = self.engines.get(&guild_id) {
            return Ok(Arc::clone(&entry.engine));
        }

        // Capture generation before DB reads
        let gen_before = self.generation.load(Ordering::Acquire);

        // Slow path: build from database
        let configs = filter_queries::list_filter_configs(pool, guild_id)
            .await
            .map_err(|e| format!("Failed to load filter configs: {e}"))?;

        let patterns = filter_queries::list_custom_patterns(pool, guild_id)
            .await
            .map_err(|e| format!("Failed to load custom patterns: {e}"))?;

        let engine = Arc::new(FilterEngine::build(&configs, &patterns)?);

        // Only insert if no invalidation happened since we started building.
        // If generation changed, our data is potentially stale — skip insert
        // and let the next caller rebuild with fresh data.
        let gen_after = self.generation.load(Ordering::Acquire);
        if gen_before == gen_after {
            self.engines.insert(
                guild_id,
                CachedEngine {
                    engine: Arc::clone(&engine),
                    generation: gen_before,
                },
            );
        }

        Ok(engine)
    }

    /// Build a fresh engine from the database without touching the shared cache.
    ///
    /// Used by the test endpoint to avoid cache churn.
    #[tracing::instrument(skip(self, pool))]
    pub async fn build_ephemeral(
        &self,
        pool: &PgPool,
        guild_id: Uuid,
    ) -> Result<Arc<FilterEngine>, String> {
        let configs = filter_queries::list_filter_configs(pool, guild_id)
            .await
            .map_err(|e| format!("Failed to load filter configs: {e}"))?;

        let patterns = filter_queries::list_custom_patterns(pool, guild_id)
            .await
            .map_err(|e| format!("Failed to load custom patterns: {e}"))?;

        Ok(Arc::new(FilterEngine::build(&configs, &patterns)?))
    }

    /// Invalidate the cached engine for a guild.
    ///
    /// Increments the generation counter so in-flight builds from stale
    /// data will not overwrite the invalidation.
    pub fn invalidate(&self, guild_id: Uuid) {
        self.generation.fetch_add(1, Ordering::Release);
        self.engines.remove(&guild_id);
    }
}
