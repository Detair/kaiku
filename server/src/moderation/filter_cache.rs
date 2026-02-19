//! Per-Guild Filter Engine Cache
//!
//! Caches compiled `FilterEngine` instances per guild using `DashMap`
//! for lock-free concurrent access. Engines are lazily built on first
//! message and invalidated when filter config changes.
//!
//! Per-guild generation counters prevent stale engines from overwriting
//! fresh invalidations (TOCTOU protection) without causing cross-guild
//! cache misses.

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
    /// Per-guild generation counters. Incremented on invalidation so
    /// in-flight builds from stale data are discarded on insert.
    generations: DashMap<Uuid, Arc<AtomicU64>>,
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
            generations: DashMap::new(),
        }
    }

    /// Get or create the generation counter for a guild.
    fn guild_generation(&self, guild_id: Uuid) -> Arc<AtomicU64> {
        self.generations
            .entry(guild_id)
            .or_insert_with(|| Arc::new(AtomicU64::new(0)))
            .clone()
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

        // Capture per-guild generation before DB reads
        let gen = self.guild_generation(guild_id);
        let gen_before = gen.load(Ordering::Acquire);

        // Slow path: build from database
        let configs = filter_queries::list_filter_configs(pool, guild_id)
            .await
            .map_err(|e| format!("Failed to load filter configs: {e}"))?;

        let patterns = filter_queries::list_custom_patterns(pool, guild_id)
            .await
            .map_err(|e| format!("Failed to load custom patterns: {e}"))?;

        let engine = Arc::new(FilterEngine::build(&configs, &patterns)?);

        // Only insert if no invalidation happened for THIS guild since we started.
        let gen_after = gen.load(Ordering::Acquire);
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
    /// Increments the guild's generation counter so in-flight builds
    /// from stale data will not overwrite the invalidation.
    pub fn invalidate(&self, guild_id: Uuid) {
        self.guild_generation(guild_id)
            .fetch_add(1, Ordering::Release);
        self.engines.remove(&guild_id);
    }
}
