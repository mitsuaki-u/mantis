use crate::infrastructure::errors::Result;
use async_trait::async_trait;
use std::future::Future;
use std::sync::Arc;

use super::Cache;

/// Trait for repositories that support caching
#[async_trait]
pub trait CachingRepository {
    /// Get a value from cache, falling back to the database fetch function
    async fn get_from_cache_or_db<T, F, FutDb>(
        &self,
        cache_key: &str,
        db_fetch: F,
    ) -> Result<Option<T>>
    where
        F: FnOnce() -> FutDb + Send + 'static,
        FutDb: Future<Output = Result<Option<T>>> + Send,
        T: serde::de::DeserializeOwned + serde::Serialize + Send + 'static;

    /// Store a value in both the cache and database
    async fn store_in_cache_and_db<T, F, Fut>(
        &self,
        cache_key: &str,
        value: &T,
        db_store: F,
    ) -> Result<()>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<()>> + Send,
        T: serde::Serialize + Sync + Send;

    /// Check if the cache is enabled
    fn is_cache_enabled(&self) -> bool;

    /// Get the cache reference if available
    fn get_cache(&self) -> Option<Arc<Cache>>;

    /// Invalidate a specific cache entry
    async fn invalidate_cache(&self, cache_key: &str) -> Result<()>;

    /// Prioritize a specific entity for cache flushing
    async fn prioritize_for_flush(&self, entity_id: &str) -> Result<()>;
}
