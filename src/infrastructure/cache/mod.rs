// Module declarations
pub mod batch;
pub mod config;
pub mod connection;
pub mod flush;
pub mod operations;
pub mod token_cache;
pub mod traits;
pub mod types;

// Re-exports for public API
pub use traits::CachingRepository;
pub use types::{CachedMarketData, CachedTokenMetadata, TOKEN_METADATA_TTL};

use crate::infrastructure::database::repositories::TokenRepository;
use crate::infrastructure::errors::Result;
use log::{debug, info};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use batch::BatchOperations;
use connection::ConnectionManager;
use flush::FlushManager;
use operations::CacheOperations;
use token_cache::TokenCache;

/// Main cache structure that orchestrates all cache operations
#[derive(Clone)]
pub struct Cache {
    connection_manager: ConnectionManager,
    prefix: String,
    batch_interval: Duration,
    token_repo: Option<Arc<TokenRepository>>,
    /// Flag to track if cache has been modified since last flush
    dirty: Arc<AtomicBool>,
}

impl Cache {
    /// Create a new cache instance
    pub async fn new(redis_url: &str, batch_interval_secs: u64) -> Self {
        let connection_manager = ConnectionManager::new(redis_url).await;
        let batch_interval = Duration::from_secs(batch_interval_secs);

        Self {
            connection_manager,
            prefix: String::new(),
            batch_interval,
            token_repo: None,
            dirty: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Add token repository for database operations
    pub fn with_token_repository(mut self, repo: Arc<TokenRepository>) -> Self {
        self.token_repo = Some(repo);
        self
    }

    /// Check if the cache is enabled and initialized properly
    pub fn is_enabled(&self) -> bool {
        self.connection_manager.is_enabled()
    }

    /// Initialize the cache
    pub fn initialize(&self) -> Result<()> {
        self.connection_manager.initialize()
    }

    /// Start background flush task
    pub async fn start_flush_task(self) -> Result<()> {
        let flush_manager = FlushManager::new(
            self.connection_manager.clone(),
            self.token_repo.clone(),
            self.batch_interval,
        );
        flush_manager.start_flush_task().await
    }

    // === Basic Cache Operations ===

    /// Get a value from cache
    pub async fn get(&self, key: &str) -> Result<String> {
        let ops = CacheOperations::new(self.connection_manager.clone(), self.prefix.clone());
        ops.get(key).await
    }

    /// Set a value in cache with optional TTL
    pub async fn set<T: serde::Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl_seconds: Option<usize>,
    ) -> Result<()> {
        let ops = CacheOperations::new(self.connection_manager.clone(), self.prefix.clone());
        let result = ops.set(key, value, ttl_seconds).await;
        if result.is_ok() {
            self.dirty.store(true, Ordering::Relaxed);
        }
        result
    }

    /// Delete a value from cache
    pub async fn delete(&self, key: &str) -> Result<()> {
        let ops = CacheOperations::new(self.connection_manager.clone(), self.prefix.clone());
        ops.delete(key).await
    }

    // === Token-Specific Operations ===

    /// Cache token metadata
    pub async fn cache_token_metadata(
        &self,
        token_id: &str,
        symbol: &str,
        decimals: u8,
    ) -> Result<()> {
        let token_cache = TokenCache::new(self.connection_manager.clone());
        let result = token_cache
            .cache_token_metadata(token_id, symbol, decimals)
            .await;
        if result.is_ok() {
            self.dirty.store(true, Ordering::Relaxed);
        }
        result
    }

    /// Get cached token metadata
    pub async fn get_token_metadata(&self, token_id: &str) -> Result<Option<CachedTokenMetadata>> {
        let token_cache = TokenCache::new(self.connection_manager.clone());
        token_cache.get_token_metadata(token_id).await
    }

    /// Cache price and volume data
    pub async fn cache_price_data(&self, token_id: &str, price: f64, volume: f64) -> Result<()> {
        let token_cache = TokenCache::new(self.connection_manager.clone());
        let result = token_cache.cache_price_data(token_id, price, volume).await;
        if result.is_ok() {
            self.dirty.store(true, Ordering::Relaxed);
        }
        result
    }

    /// Get cached price and volume data
    pub async fn get_price_data(&self, token_id: &str) -> Result<Option<CachedMarketData>> {
        let token_cache = TokenCache::new(self.connection_manager.clone());
        token_cache.get_price_data(token_id).await
    }

    /// Set token metadata using database TokenMetadata
    pub async fn set_token_metadata(
        &self,
        token_id: &str,
        metadata: &crate::infrastructure::database::pool::TokenMetadata,
    ) -> Result<()> {
        let token_cache = TokenCache::new(self.connection_manager.clone());
        let result = token_cache.set_token_metadata(token_id, metadata).await;
        if result.is_ok() {
            self.dirty.store(true, Ordering::Relaxed);
        }
        result
    }

    // === Flush Operations ===

    /// Manual flush to database
    /// Only flushes if cache has been modified since last flush
    pub async fn manual_flush(&self) -> Result<()> {
        // Check if cache has any dirty data
        if !self.dirty.load(Ordering::Relaxed) {
            debug!("Cache flush skipped - no changes since last flush");
            return Ok(());
        }

        info!("Performing cache flush (changes detected)");
        let flush_manager = FlushManager::new(
            self.connection_manager.clone(),
            self.token_repo.clone(),
            self.batch_interval,
        );

        let result = flush_manager.flush_to_database().await;

        // Clear dirty flag only if flush was successful
        if result.is_ok() {
            self.dirty.store(false, Ordering::Relaxed);
            debug!("Cache flush completed, dirty flag cleared");
        }

        result
    }

    /// Prioritize a token for flushing
    pub async fn prioritize_token_flush(&self, token_id: &str) -> Result<()> {
        let flush_manager = FlushManager::new(
            self.connection_manager.clone(),
            self.token_repo.clone(),
            self.batch_interval,
        );
        flush_manager.prioritize_token_flush(token_id).await
    }

    // === Batch Operations ===

    /// Batch get token data for multiple tokens
    pub async fn batch_get_token_data(
        &self,
        token_ids: &[String],
    ) -> Result<std::collections::HashMap<String, (Option<(String, String)>, Option<(f64, f64)>)>>
    {
        let batch_ops =
            BatchOperations::new(self.connection_manager.clone(), self.token_repo.clone());
        batch_ops.batch_get_token_data(token_ids).await
    }

    /// Preload common token data
    pub async fn preload_common_data(&self, token_ids: &[String]) -> Result<()> {
        let batch_ops =
            BatchOperations::new(self.connection_manager.clone(), self.token_repo.clone());
        batch_ops.preload_common_data(token_ids).await
    }
}
