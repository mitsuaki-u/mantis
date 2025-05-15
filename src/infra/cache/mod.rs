use crate::core::config::Config;
use crate::core::error::{Error, Result};
use crate::core::models::token::TokenData;
// Use full paths from database module
use crate::infra::db::database::{PriceData, TokenMetadata};
use crate::infra::db::repositories::TokenRepository;
use chrono::{DateTime, Utc};
use deadpool_redis::{
    Config as RedisConfig, Connection, Pool as RedisPool, Runtime as RedisRuntime,
};
use log::{debug, error, info, trace, warn};
use redis::{AsyncCommands, RedisResult}; // Add Value for pipeline results
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::RwLock;

// Cache TTL values in seconds
const TOKEN_METADATA_TTL: usize = 3600; // 1 hour
const PRICE_DATA_TTL: usize = 60; // 1 minute
const TOKEN_LIST_TTL: usize = 60 * 5; // 5 minutes

// Define the pool type using the import
// type RedisPool = RedisPool; // Removed redundant type alias

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedTokenMetadata {
    name: String,
    symbol: String,
    decimals: i32,
    timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedPrice {
    pub price: f64,
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone)]
pub struct Cache {
    pool: Option<RedisPool>,
    prefix: String,
    flush_priority_set: String, // Redis set key for tokens needing priority flush
    last_flush_attempt: Arc<Mutex<Option<std::time::Instant>>>,
    flush_in_progress: Arc<RwLock<bool>>,
    batch_interval: Duration,
    enabled: bool,
    token_repo: Option<Arc<TokenRepository>>,
}

impl Cache {
    pub async fn new(redis_url: &str, batch_interval_secs: u64) -> Self {
        let mut enabled = false;
        let mut pool = None;
        let batch_interval = Duration::from_secs(batch_interval_secs);
        let mut final_url = redis_url.to_string(); // Store the URL used

        let mut cfg = RedisConfig::from_url(redis_url);

        match cfg.create_pool(Some(RedisRuntime::Tokio1)) {
            Ok(p) => {
                // Test connection from pool
                match p.get().await {
                    Ok(_) => {
                        info!("✅ Redis connection pool initialized successfully for {}. Cache enabled.", redis_url);
                        pool = Some(p);
                        enabled = true;
                    }
                    Err(e) => {
                        error!(
                            "❌ Failed to get initial connection from Redis pool for {}: {}.",
                            redis_url, e
                        );
                        // Fall through to try localhost fallback if applicable
                    }
                }
            }
            Err(e) => {
                error!(
                    "❌ Failed to create Redis config/pool for {}: {}.",
                    redis_url, e
                );
                // Fall through to try localhost fallback if applicable
            }
        }

        // Handle fallback for localhost if initial connection failed
        if !enabled && redis_url.contains("localhost") {
            let ip_url = redis_url.replace("localhost", "127.0.0.1");
            warn!(
                "Initial Redis connection failed. Trying fallback URL: {}",
                ip_url
            );
            final_url = ip_url.clone(); // Update final URL for logging
            cfg = RedisConfig::from_url(&ip_url);
            match cfg.create_pool(Some(RedisRuntime::Tokio1)) {
                Ok(p) => match p.get().await {
                    Ok(_) => {
                        info!("✅ Redis connection pool initialized successfully for {}. Cache enabled.", ip_url);
                        pool = Some(p);
                        enabled = true;
                    }
                    Err(e) => {
                        error!("❌ Failed to get initial connection from Redis pool (fallback URL {}): {}. Cache remains disabled.", ip_url, e);
                    }
                },
                Err(e) => {
                    error!("❌ Failed to create Redis config/pool (fallback URL {}): {}. Cache remains disabled.", ip_url, e);
                }
            }
        }

        if !enabled {
            warn!("❌ All Redis connection attempts failed. Cache will be disabled.");
        }

        Self {
            pool,
            prefix: String::new(),
            flush_priority_set: String::new(),
            last_flush_attempt: Arc::new(Mutex::new(None)),
            flush_in_progress: Arc::new(RwLock::new(false)),
            batch_interval,
            enabled,
            token_repo: None,
        }
    }

    pub fn with_token_repository(mut self, repo: Arc<TokenRepository>) -> Self {
        self.token_repo = Some(repo);
        self
    }

    /// Check if the cache is enabled and initialized properly.
    pub fn is_enabled(&self) -> bool {
        self.enabled && self.pool.is_some()
    }

    pub fn initialize(&self) -> Result<()> {
        // Use the new public method
        if self.is_enabled() {
            info!("Cache is enabled and pool available.");
            Ok(())
        } else if !self.enabled {
            // Keep check for logging disabled state
            info!("Cache is disabled.");
            Ok(())
        } else {
            error!("Cache pool failed to initialize properly.");
            Err(Error::Other("Cache pool failed to initialize".to_string()))
        }
    }

    pub async fn start_flush_task(self, db: crate::db::Database) -> Result<()> {
        if !self.enabled || self.pool.is_none() {
            info!("Cache is disabled or pool not initialized, not starting flush task");
            return Ok(());
        }

        info!(
            "Starting cache flush task with interval: {:?}",
            self.batch_interval
        );

        let cache_clone = self.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(cache_clone.batch_interval);

            loop {
                interval.tick().await;
                debug!("Running scheduled cache flush");

                if let Err(e) = cache_clone.flush_to_database(&db).await {
                    error!("Error flushing cache to database: {}", e);
                }
            }
        });

        Ok(())
    }

    async fn get_connection(&self) -> Result<Connection> {
        if !self.is_enabled() {
            return Err(Error::Cache("Cache is disabled".to_string()));
        }
        match &self.pool {
            Some(p) => p.get().await.map_err(|e| {
                error!("Failed to get connection from Redis pool: {}", e);
                Error::Cache(format!("Failed to get Redis connection: {}", e))
            }),
            None => Err(Error::Cache(
                "Cache is disabled or pool not initialized".to_string(),
            )),
        }
    }

    pub async fn cache_token_metadata(&self, token_id: &str, symbol: &str) -> Result<()> {
        if !self.enabled {
            debug!("Cache disabled, skipping metadata caching for {}", token_id);
            return Ok(());
        }

        let key = format!("token:metadata:{}", token_id);
        let metadata = CachedTokenMetadata {
            name: token_id.to_string(),
            symbol: symbol.to_string(),
            decimals: 0,
            timestamp: Utc::now(),
        };

        let serialized =
            serde_json::to_string(&metadata).map_err(|e| Error::Serialization(e.to_string()))?;

        let mut conn = self.get_connection().await?;

        conn.set_ex(&key, serialized, TOKEN_METADATA_TTL as u64)
            .await
            .map_err(|e| Error::Cache(format!("Redis SETEX error: {}", e)))?;

        debug!("Cached metadata for token {}", token_id);
        Ok(())
    }

    pub async fn get_token_metadata(&self, token_id: &str) -> Result<Option<CachedTokenMetadata>> {
        if !self.enabled {
            debug!("Cache disabled, not checking metadata for {}", token_id);
            return Ok(None);
        }

        let key = format!("token:metadata:{}", token_id);

        let mut conn = self.get_connection().await?;
        let result: RedisResult<Option<String>> = conn.get(&key).await;

        match result {
            Ok(Some(value)) => match serde_json::from_str::<CachedTokenMetadata>(&value) {
                Ok(metadata) => {
                    debug!("💾 Cache HIT for token metadata: {}", token_id);
                    Ok(Some(metadata))
                }
                Err(e) => {
                    warn!("Failed to deserialize cached token metadata for {}: {}. Removing invalid entry.", token_id, e);
                    let _: RedisResult<()> = conn.del(&key).await;
                    Ok(None)
                }
            },
            Ok(None) => {
                debug!("💾 Cache MISS for token metadata: {}", token_id);
                Ok(None)
            }
            Err(e) => {
                error!(
                    "Redis GET error retrieving token metadata for {}: {}",
                    token_id, e
                );
                Err(Error::Cache(format!("Redis GET error: {}", e)))
            }
        }
    }

    pub async fn cache_price_data(&self, token_id: &str, price: f64, volume: f64) -> Result<()> {
        if !self.enabled {
            debug!("Cache disabled, skipping price caching for {}", token_id);
            return Ok(());
        }

        let key = format!("token:price:{}", token_id);
        let price_data = CachedPrice {
            price,
            timestamp: Utc::now(),
        };

        let serialized =
            serde_json::to_string(&price_data).map_err(|e| Error::Serialization(e.to_string()))?;

        let mut conn = self.get_connection().await?;
        conn.set_ex(&key, serialized, PRICE_DATA_TTL as u64)
            .await
            .map_err(|e| Error::Cache(format!("Redis SETEX error: {}", e)))?;

        debug!(
            "Cached price data for token {}: ${:.4} vol ${:.2}",
            token_id, price, volume
        );
        Ok(())
    }

    pub async fn get_price_data(&self, token_id: &str) -> Result<Option<CachedPrice>> {
        if !self.enabled {
            debug!("Cache disabled, not checking price data for {}", token_id);
            return Ok(None);
        }

        let key = format!("token:price:{}", token_id);
        let mut conn = self.get_connection().await?;

        let result: RedisResult<Option<String>> = conn.get(&key).await;

        match result {
            Ok(Some(value)) => match serde_json::from_str::<CachedPrice>(&value) {
                Ok(price_data) => {
                    debug!("💾 Cache HIT for price data: {}", token_id);
                    Ok(Some(price_data))
                }
                Err(e) => {
                    warn!("Failed to deserialize cached price data for {}: {}. Removing invalid entry.", token_id, e);
                    let _: RedisResult<()> = conn.del(&key).await;
                    Ok(None)
                }
            },
            Ok(None) => {
                debug!("💾 Cache MISS for price data: {}", token_id);
                Ok(None)
            }
            Err(e) => {
                error!(
                    "Redis GET error retrieving price data for {}: {}",
                    token_id, e
                );
                Err(Error::Cache(format!("Redis GET error: {}", e)))
            }
        }
    }

    async fn flush_to_database(&self, db: &crate::db::Database) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let priority_tokens = self.get_priority_flush_tokens().await?;
        let pending_tokens = self.get_pending_flush_tokens().await?;

        // Combine lists, ensuring priority tokens are flushed first
        let mut all_tokens: Vec<_> = priority_tokens
            .into_iter()
            .chain(pending_tokens.into_iter())
            .collect();
        all_tokens.dedup(); // Remove duplicates if a token was in both

        if all_tokens.is_empty() {
            debug!("No tokens pending flush");
            return Ok(());
        }

        info!(
            "Flushing cache data for {} tokens to database",
            all_tokens.len()
        );
        let mut conn = self.get_connection().await?;

        // Assuming TokenRepository is available via self.token_repo
        let token_repo = match &self.token_repo {
            Some(repo) => repo.clone(),
            None => {
                return Err(Error::Config(
                    "TokenRepository not available in Cache".to_string(),
                ))
            }
        };

        // Use a local block for the batch operation
        let mut price_updates: Vec<(String, f64, f64)> = Vec::new();
        let mut metadata_updates: HashMap<String, String> = HashMap::new();
        let mut keys_to_clear: Vec<String> = Vec::new();

        for token_id in &all_tokens {
            let price_key = format!("token:price:{}", token_id);
            let metadata_key = format!("token:metadata:{}", token_id);
            keys_to_clear.push(price_key.clone());
            keys_to_clear.push(metadata_key.clone());

            let price_result: RedisResult<Option<String>> = conn.get(&price_key).await;
            let metadata_result: RedisResult<Option<String>> = conn.get(&metadata_key).await;

            let price_data_opt: Option<CachedPrice> = price_result
                .ok()
                .flatten()
                .and_then(|s| serde_json::from_str(&s).ok());
            let metadata_opt: Option<CachedTokenMetadata> = metadata_result
                .ok()
                .flatten()
                .and_then(|s| serde_json::from_str(&s).ok());

            if let Some(price_data) = price_data_opt {
                let symbol = metadata_opt
                    .map(|m| m.symbol)
                    .unwrap_or_else(|| token_id.to_string());
                price_updates.push((token_id.clone(), price_data.price, price_data.price));
                // Use update_token_metadata_with_price which handles both
            } else {
                warn!(
                    "No price data found in cache for token {} during flush.",
                    token_id
                );
            }
        }

        // Store combined updates in the database using TokenRepository
        // This needs to be adapted based on TokenRepository's methods
        if !price_updates.is_empty() {
            // Assuming TokenRepository has a method like batch_update_token_data
            // Or call update_token_metadata_with_price for each
            for (token_id, price, volume) in price_updates {
                // Call the async function directly
                if let Err(e) = token_repo
                    .update_token_metadata_with_price(
                        &token_id, &token_id, &token_id, price, volume,
                    )
                    .await
                {
                    error!(
                        "Failed to update token metadata/price in cache flush for {}: {}",
                        token_id, e
                    );
                }
            }
        }

        // Clear the flushed entries from Redis sets
        self.clear_priority_flush().await?;
        self.clear_pending_flush().await?;

        info!(
            "Finished flushing cache data for {} tokens",
            all_tokens.len()
        );
        Ok(())
    }

    async fn get_pending_flush_tokens(&self) -> Result<Vec<String>> {
        if !self.enabled {
            return Ok(Vec::new());
        }
        let mut conn = self.get_connection().await?;
        let tokens: Vec<String> = conn
            .smembers("pending_flush")
            .await
            .map_err(|e| Error::Cache(format!("Redis SMEMBERS error: {}", e)))?;
        Ok(tokens)
    }

    async fn clear_pending_flush(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let mut conn = self.get_connection().await?;
        let _: () = conn
            .del("pending_flush")
            .await
            .map_err(|e| Error::Cache(format!("Redis DEL error: {}", e)))?;
        Ok(())
    }

    pub async fn manual_flush(&self, db: &crate::db::Database) -> Result<()> {
        info!("Performing manual cache flush");
        self.flush_to_database(db).await
    }

    pub async fn prioritize_token_flush(&self, token_id: &str) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let mut conn = self.get_connection().await?;
        let _: () = conn
            .sadd("priority_flush", token_id)
            .await
            .map_err(|e| Error::Cache(format!("Redis SADD error: {}", e)))?;
        debug!("Prioritized token {} for flushing", token_id);
        Ok(())
    }

    async fn get_priority_flush_tokens(&self) -> Result<Vec<String>> {
        if !self.enabled {
            return Ok(Vec::new());
        }
        let mut conn = self.get_connection().await?;
        let tokens: Vec<String> = conn
            .smembers("priority_flush")
            .await
            .map_err(|e| Error::Cache(format!("Redis SMEMBERS error: {}", e)))?;
        Ok(tokens)
    }

    async fn clear_priority_flush(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let mut conn = self.get_connection().await?;
        let _: () = conn
            .del("priority_flush")
            .await
            .map_err(|e| Error::Cache(format!("Redis DEL error: {}", e)))?;
        Ok(())
    }

    pub async fn batch_get_token_data(
        &self,
        token_ids: &[String],
    ) -> Result<HashMap<String, (Option<(String, String)>, Option<(f64, f64)>)>> {
        if !self.enabled || token_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut conn = self.get_connection().await?;
        let mut pipe = redis::pipe();
        pipe.atomic(); // Make the pipeline atomic
        let mut commands_added = false; // Flag to track if pipe has commands

        for token_id in token_ids {
            pipe.get(format!("token:metadata:{}", token_id))
                .get(format!("token:price:{}", token_id));
            commands_added = true; // Mark that commands were added
        }

        if !commands_added {
            // Check the flag instead of pipe.len()
            return Ok(HashMap::new());
        }

        let redis_results: Vec<Option<String>> = pipe
            .query_async(&mut *conn)
            .await
            .map_err(|e| Error::Cache(format!("Redis pipeline error: {}", e)))?;

        let mut result_map = HashMap::new();
        for (i, token_id) in token_ids.iter().enumerate() {
            let metadata_str_opt = redis_results.get(i * 2).cloned().flatten();
            let price_str_opt = redis_results.get(i * 2 + 1).cloned().flatten();

            let metadata = metadata_str_opt.and_then(|s| {
                serde_json::from_str::<CachedTokenMetadata>(&s)
                    .map_err(|e| warn!("Failed to parse metadata for {}: {}", token_id, e))
                    .ok()
                    .map(|m| (m.name, m.symbol))
            });

            let price_data = price_str_opt.and_then(|s| {
                serde_json::from_str::<CachedPrice>(&s)
                    .map_err(|e| warn!("Failed to parse price data for {}: {}", token_id, e))
                    .ok()
                    .map(|p| (p.price, p.price))
            });

            result_map.insert(token_id.clone(), (metadata, price_data));
        }

        Ok(result_map)
    }

    pub async fn preload_common_data(
        &self,
        db: &crate::db::Database,
        token_ids: &[String],
    ) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        info!(
            "Preloading cache with data for {} common tokens",
            token_ids.len()
        );

        let mut conn = self.get_connection().await?;
        let mut pipe = redis::pipe();
        pipe.atomic();

        let mut preload_count = 0;
        for token_id in token_ids {
            let metadata_key = format!("token:metadata:{}", token_id);
            let price_key = format!("token:price:{}", token_id);

            // Check existence using EXISTS command for efficiency - Separate calls
            let metadata_exists: bool = redis::cmd("EXISTS")
                .arg(&metadata_key)
                .query_async(&mut *conn)
                .await
                .map_err(|e| Error::Cache(format!("Redis EXISTS metadata error: {}", e)))
                .map(|v: i64| v > 0)?; // Convert integer result to bool

            let price_exists: bool = redis::cmd("EXISTS")
                .arg(&price_key)
                .query_async(&mut *conn)
                .await
                .map_err(|e| Error::Cache(format!("Redis EXISTS price error: {}", e)))
                .map(|v: i64| v > 0)?; // Convert integer result to bool

            if !metadata_exists || !price_exists {
                // Fetch from DB if not in cache
                if let Some(token_repo) = &self.token_repo {
                    match token_repo.get_token_price_stats(token_id).await {
                        Ok(token_data) => {
                            let metadata = CachedTokenMetadata {
                                name: token_data.id.clone(),
                                symbol: token_data.symbol.clone(),
                                decimals: 0,
                                timestamp: Utc::now(),
                            };
                            let price = CachedPrice {
                                price: token_data.price_usd,
                                timestamp: Utc::now(),
                            };

                            if let (Ok(ser_meta), Ok(ser_price)) = (
                                serde_json::to_string(&metadata),
                                serde_json::to_string(&price),
                            ) {
                                pipe.set_ex(&metadata_key, ser_meta, TOKEN_METADATA_TTL as u64)
                                    .set_ex(&price_key, ser_price, PRICE_DATA_TTL as u64);
                                preload_count += 1;
                            } else {
                                warn!("Failed to serialize data for preloading token {}", token_id);
                            }
                        }
                        Err(e) => {
                            warn!(
                                "Failed to fetch token {} from DB for preloading: {}",
                                token_id, e
                            );
                        }
                    }
                } else {
                    warn!(
                        "TokenRepository not available, cannot preload data for {}",
                        token_id
                    );
                }
            }
        }

        if preload_count > 0 {
            let _: () = pipe
                .query_async(&mut *conn)
                .await
                .map_err(|e| Error::Cache(format!("Redis preload pipeline error: {}", e)))?;
            info!(
                "Successfully preloaded cache with data for {} tokens.",
                preload_count
            );
        } else {
            info!("Cache already contained data for common tokens, no preload needed.");
        }

        Ok(())
    }

    pub async fn get(&self, key: &str) -> Result<String> {
        if !self.enabled {
            return Err(Error::Cache("Cache is disabled".to_string()));
        }
        let mut conn = self.get_connection().await?;
        let value: String = conn
            .get(key)
            .await
            .map_err(|e| Error::Cache(format!("Redis GET error for key {}: {}", key, e)))?;
        Ok(value)
    }

    pub async fn set<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl_seconds: Option<usize>,
    ) -> Result<()> {
        if !self.enabled {
            return Ok(()); // Silently ignore if cache is disabled
        }
        let mut conn = self.get_connection().await?;
        let value = serde_json::to_string(value)?;
        let key = format!("{}:{}", self.prefix, key);

        if let Some(ttl) = ttl_seconds {
            conn.set_ex(&key, value, ttl as u64)
                .await
                .map_err(|e| Error::Cache(format!("Redis SETEX error for key {}: {}", key, e)))?;
        } else {
            conn.set(&key, value)
                .await
                .map_err(|e| Error::Cache(format!("Redis SET error for key {}: {}", key, e)))?;
        }
        Ok(())
    }

    pub async fn delete(&self, key: &str) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let full_key = format!("{}:{}", self.prefix, key);
        let mut conn = self.get_connection().await?;
        let key_clone_for_err = full_key.clone(); // Clone for error message
        let _: () = conn
            .del(&full_key) // Borrow full_key here
            .await
            .map_err(|e| {
                Error::Cache(format!(
                    "Redis DEL error for key {}: {}",
                    key_clone_for_err, e
                ))
            })?;
        Ok(())
    }

    /// Set metadata for a specific token
    pub async fn set_token_metadata(
        &self,
        token_id: &str,
        metadata: &TokenMetadata, // Use the DB/core TokenMetadata model
    ) -> Result<()> {
        let key = format!("token:meta:{}", token_id);
        let cache_data = CachedTokenMetadata {
            name: metadata.name.clone(),
            symbol: metadata.symbol.clone(),
            decimals: metadata.decimals, // Use decimals from TokenMetadata
            timestamp: Utc::now(),
        };
        self.set(&key, &cache_data, Some(TOKEN_METADATA_TTL)).await
    }
}
