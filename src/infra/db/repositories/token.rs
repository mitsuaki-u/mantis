use super::{CachingRepository, Repository};
use crate::core::error::{Error, Result};
use crate::core::models::token::TokenData;
use crate::infra::cache::Cache;
// use crate::infra::db::database::Client as PgClient; // Removed unused
use crate::infra::db::database::{Database, TokenMetadata};
use crate::infra::db::queries;
use async_trait;
use chrono::{DateTime, Utc};
use log::{debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};
use serde_json;
use std::future::Future;
use std::sync::Arc;
use tokio_postgres::types::ToSql;

// Helper structs for caching
#[derive(Serialize, Deserialize)]
struct CachedTokenMetadata {
    token_id: String,
    symbol: String,
}

#[derive(Serialize, Deserialize)]
struct CachedPriceData {
    token_id: String,
    price: f64,
    volume: f64,
    timestamp: chrono::DateTime<Utc>,
}

#[derive(Clone)]
pub struct TokenRepository {
    db: Database,
    cache: Option<Arc<Cache>>,
    is_paper_trade: bool,
}

impl TokenRepository {
    pub fn new(db: Database, is_paper_trade: bool) -> Self {
        Self {
            db,
            cache: None,
            is_paper_trade,
        }
    }

    pub fn with_paper_trading(mut self, is_paper_trade: bool) -> Self {
        self.is_paper_trade = is_paper_trade;
        self
    }

    pub fn with_cache(db: Database, cache: Cache, is_paper_trade: bool) -> Self {
        Self {
            db,
            cache: Some(Arc::new(cache)),
            is_paper_trade,
        }
    }

    /// Update token metadata and optionally the latest price (async)
    pub async fn update_token_metadata_with_price(
        &self,
        token_id: &str,
        symbol: &str,
        name: &str,
        price: f64,
        volume: f64,
    ) -> Result<()> {
        // Note: Caching might be handled by CachingRepository trait now
        debug!(
            "Updating token metadata and price for {} ({})",
            token_id, symbol
        );

        let mut client = self.db.get_connection().await?; // Made client mutable
        let transaction = client
            .transaction()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        let now = Utc::now();

        // Upsert token metadata
        transaction
            .execute(
                queries::token::UPSERT_TOKEN,
                &[
                    &token_id as &(dyn ToSql + Sync),
                    &name,
                    &symbol,
                    &now,
                    &true,
                    &true,
                ], // Assuming decimals=0, active=true, tracked=true for now
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        // Insert price data
        transaction
            .execute(
                queries::price::INSERT_PRICE,
                &[&token_id as &(dyn ToSql + Sync), &price, &volume, &now],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        transaction
            .commit()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        info!(
            "Successfully updated token metadata and price data for {}",
            token_id
        );
        Ok(())
    }

    /// Ensure token has price data - if not, add default price data
    pub async fn ensure_token_has_price_data(&self, token_id: &str) -> Result<()> {
        let token_id = token_id.to_string();

        let cached_data_opt = if let Some(cache) = &self.cache {
            cache.get_price_data(&token_id).await?
        } else {
            None
        };

        let has_price_data = if cached_data_opt.is_some() {
            true
        } else {
            // Not in cache, check DB (now async)
            let history = self.get_price_history(&token_id, 1).await?;
            !history.is_empty()
        };

        if !has_price_data {
            info!(
                "Token {} exists but has no price data, adding default price data",
                token_id
            );
            self.update_token_metadata_with_price(&token_id, &token_id, &token_id, 0.0, 0.0)
                .await?;
            info!(
                "Successfully added default price data for token {}",
                token_id
            );
        } else {
            debug!("Token {} already has price data", token_id);
        }
        Ok(())
    }

    /// Get the underlying database pool access
    pub fn get_db(&self) -> Database {
        self.db.clone()
    }

    /// Update token metadata only
    pub async fn update_token_metadata(
        &self,
        token_id: &str,
        symbol: &str,
        name: &str,
    ) -> Result<()> {
        debug!(
            "Updating token metadata for {} with symbol {} / name {}",
            token_id, symbol, name
        );
        let client = self.db.get_connection().await?;
        let now = Utc::now();

        let rows_affected = client
            .execute(
                queries::token::UPDATE_TOKEN_METADATA,
                &[&token_id as &(dyn ToSql + Sync), &symbol, &name, &now],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        if rows_affected == 0 {
            warn!(
                "Attempted to update metadata for non-existent token: {}",
                token_id
            );
            client
                .execute(
                    queries::token::INSERT_TOKEN_SIMPLE,
                    &[&token_id as &(dyn ToSql + Sync), &now],
                )
                .await
                .map_err(|e| Error::Database(e.to_string()))?;
        }

        Ok(())
    }

    /// Store price data (simplified, assumes token exists)
    pub async fn store_price_data(&self, token_id: &str, price: f64, volume: f64) -> Result<()> {
        debug!(
            "Storing price data for {}: ${:.4}, vol: ${:.2}",
            token_id, price, volume
        );

        if self.is_cache_enabled() {
            let cached_price_data = CachedPriceData {
                token_id: token_id.to_string(),
                price,
                volume,
                timestamp: Utc::now(),
            };

            let price_key = format!("token:price:{}", token_id);
            let db_clone = self.db.clone();
            let token_id_clone = token_id.to_string();

            let db_store_closure = move || async move {
                let client = db_clone.get_connection().await?;
                let now = Utc::now();
                client
                    .execute(
                        queries::price::INSERT_PRICE,
                        &[
                            &token_id_clone as &(dyn ToSql + Sync),
                            &price,
                            &volume,
                            &now,
                        ],
                    )
                    .await
                    .map_err(|e| Error::Database(e.to_string()))?;
                Ok(())
            };

            self.store_in_cache_and_db(&price_key, &cached_price_data, db_store_closure)
                .await?
        }

        let client = self.db.get_connection().await?;
        let now = Utc::now();
        client
            .execute(
                queries::price::INSERT_PRICE,
                &[&token_id as &(dyn ToSql + Sync), &price, &volume, &now],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }

    /// Batch store price data for multiple tokens using a single transaction
    pub async fn batch_store_price_data(&self, price_data: &[(String, f64, f64)]) -> Result<()> {
        if price_data.is_empty() {
            return Ok(());
        }

        let mut client = self.db.get_connection().await?;
        let transaction = client
            .transaction()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        let stmt_token = transaction
            .prepare(queries::token::INSERT_TOKEN_SIMPLE)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;
        let stmt_price = transaction
            .prepare(queries::price::INSERT_PRICE)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        let now = Utc::now();
        for (token_id, price, volume) in price_data {
            transaction
                .execute(&stmt_token, &[token_id as &(dyn ToSql + Sync), &now])
                .await
                .map_err(|e| Error::Database(e.to_string()))?;
            transaction
                .execute(
                    &stmt_price,
                    &[token_id as &(dyn ToSql + Sync), price, volume, &now],
                )
                .await
                .map_err(|e| Error::Database(e.to_string()))?;
        }

        transaction
            .commit()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(())
    }

    /// Calculate price change statistics for a token (now async)
    pub async fn get_token_price_stats(&self, token_id: &str) -> Result<TokenData> {
        trace!("Getting price stats for token: {}", token_id);
        let overall_start_time = std::time::Instant::now();

        let token_id_string = token_id.to_string(); // Use a consistent string form
        let client = self.db.get_connection().await?;

        // 1. Ensure token exists (INSERT_TOKEN_SIMPLE)
        let query1_start_time = std::time::Instant::now();
        debug!(
            "➡️ {}: DB Query 1/4 (Ensure Token Exists - INSERT_TOKEN_SIMPLE)...",
            token_id_string
        );
        let now_check = Utc::now();
        let insert_result = client
            .execute(
                queries::token::INSERT_TOKEN_SIMPLE,
                &[&token_id_string as &(dyn ToSql + Sync), &now_check],
            )
            .await;
        debug!(
            "⬅️ {}: DB Query 1/4 finished. Duration: {:?}. Result: {}",
            token_id_string,
            query1_start_time.elapsed(),
            if insert_result.is_ok() { "Ok" } else { "Err" }
        );
        insert_result.map_err(|e| Error::Database(e.to_string()))?;

        // 2. Get basic token info (GET_TOKEN_INFO)
        let query2_start_time = std::time::Instant::now();
        debug!(
            "➡️ {}: DB Query 2/4 (Get Basic Info - GET_TOKEN_INFO)...",
            token_id_string
        );
        let basic_info_result = client
            .query_one(
                queries::token::GET_TOKEN_INFO,
                &[&token_id_string as &(dyn ToSql + Sync)],
            )
            .await;
        debug!(
            "⬅️ {}: DB Query 2/4 finished. Duration: {:?}. Result: {}",
            token_id_string,
            query2_start_time.elapsed(),
            if basic_info_result.is_ok() {
                "Ok"
            } else {
                "Err"
            }
        );

        let (symbol, name) = match basic_info_result {
            Ok(row) => (row.get::<_, String>(0), row.get::<_, String>(1)),
            Err(e) => {
                warn!(
                    "Failed to get basic token info for {}: {}. Using token_id as fallback.",
                    token_id_string, e
                );
                (token_id_string.clone(), token_id_string.clone())
            }
        };

        // 3. Get latest price (GET_LATEST_PRICE)
        let query3_start_time = std::time::Instant::now();
        debug!(
            "➡️ {}: DB Query 3/4 (Get Latest Price - GET_LATEST_PRICE)...",
            token_id_string
        );
        let latest_price_result = client
            .query_opt(
                queries::price::GET_LATEST_PRICE,
                &[&token_id_string as &(dyn ToSql + Sync)],
            )
            .await;
        debug!(
            "⬅️ {}: DB Query 3/4 finished. Duration: {:?}. Result: {}",
            token_id_string,
            query3_start_time.elapsed(),
            if latest_price_result.is_ok() {
                "Ok"
            } else {
                "Err"
            }
        );

        let latest_price = match latest_price_result {
            Ok(Some(row)) => row.get::<_, f64>(0),
            Ok(None) => 0.0, // Default if no price found
            Err(e) => {
                error!(
                    "Failed to fetch latest price for {}: {}",
                    token_id_string, e
                );
                return Err(Error::Database(e.to_string())); // Return error if query failed
            }
        };

        // 4. Get 24h price stats (GET_TOKEN_PRICE_STATS)
        let query4_start_time = std::time::Instant::now();
        debug!(
            "➡️ {}: DB Query 4/4 (Get 24h Stats - GET_TOKEN_PRICE_STATS)...",
            token_id_string
        );
        let stats_result = client
            .query_one(
                queries::price::GET_TOKEN_PRICE_STATS,
                &[&token_id_string as &(dyn ToSql + Sync)],
            )
            .await;
        debug!(
            "⬅️ {}: DB Query 4/4 finished. Duration: {:?}. Result: {}",
            token_id_string,
            query4_start_time.elapsed(),
            if stats_result.is_ok() { "Ok" } else { "Err" }
        );

        let (price_change_24h, volume_24h) = match stats_result {
            Ok(row) => (row.get(0), row.get(1)),
            Err(e) => {
                error!("Failed to fetch price stats for {}: {}", token_id_string, e);
                (0.0, 0.0)
            }
        };

        debug!(
            "Finished all DB queries for {}. Total time in get_token_price_stats: {:?}",
            token_id_string,
            overall_start_time.elapsed()
        );

        Ok(TokenData {
            id: token_id_string.clone(),
            symbol,
            name,
            price_usd: latest_price,
            price_change_24h,
            volume_24h,
            market_cap: None,
            market_cap_rank: None,
            chain: String::new(),
            address: None,
            latest_news: None,
            last_updated: Some(Utc::now()),
        })
    }

    /// Get price history and token data (now async)
    pub async fn get_token_history(
        &self,
        token_id: &str,
        limit: usize,
    ) -> Result<Option<(TokenData, Vec<(f64, f64, chrono::DateTime<Utc>)>)>> {
        let token_data = self.get_token_price_stats(token_id).await?;

        let history = self.get_price_history(token_id, limit).await?;

        if history.is_empty() {
            Ok(None)
        } else {
            Ok(Some((token_data, history)))
        }
    }

    /// Get latest market data for all tokens (async)
    pub async fn get_latest_market_data(&self) -> Result<Vec<TokenData>> {
        debug!("Getting latest market data for all tokens");
        let client = self.db.get_connection().await?;

        let rows = client
            .query(queries::price::GET_LATEST_MARKET_DATA, &[])
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        let mut tokens = Vec::with_capacity(rows.len());
        for row in rows {
            tokens.push(TokenData {
                id: row.get(0),
                symbol: row.get(1),
                name: row.get(2),
                price_usd: row.get(3),
                price_change_24h: row.get(4),
                volume_24h: row.get(5),
                market_cap: None,
                market_cap_rank: None,
                chain: String::new(),
                address: None,
                latest_news: None,
                last_updated: Some(Utc::now()),
            });
        }

        tokens.sort_by(|a, b| {
            b.volume_24h
                .partial_cmp(&a.volume_24h)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(tokens)
    }

    /// Check if a token exists in the database (async)
    pub async fn token_exists(&self, token_id: &str) -> Result<bool> {
        trace!("Checking token existence (async) for {}", token_id);
        let client = self.db.get_connection().await?;
        let row = client
            .query_one(
                queries::token::CHECK_TOKEN_EXISTS,
                &[&token_id as &(dyn ToSql + Sync)],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;
        let count: i64 = row.get(0);
        Ok(count > 0)
    }

    /// Check if a token exists in the database (alias for token_exists)
    pub async fn check_token_exists(&self, token_id: &str) -> Result<bool> {
        self.token_exists(token_id).await
    }

    /// Ensure a token exists in the database (async)
    pub async fn ensure_token_exists(&self, token_id: &str) -> Result<()> {
        if self.token_exists(token_id).await? {
            debug!("Token {} already exists in the database", token_id);
            Ok(())
        } else {
            debug!("Creating new token record for {} (async)", token_id);
            let client = self.db.get_connection().await?;
            let now = Utc::now();
            client
                .execute(
                    queries::token::INSERT_TOKEN_SIMPLE,
                    &[&token_id as &(dyn ToSql + Sync), &now],
                )
                .await
                .map_err(|e| Error::Database(e.to_string()))?;
            info!("Created new token record for {}", token_id);
            Ok(())
        }
    }

    /// Get count of tokens in the database (async)
    pub async fn get_token_count(&self) -> Result<usize> {
        let client = self.db.get_connection().await?;
        let row = client
            .query_one(queries::verify::COUNT_TOKENS, &[])
            .await
            .map_err(|e| Error::Database(e.to_string()))?;
        let count: i64 = row.get(0);
        Ok(count as usize)
    }

    /// Get count of trades in the database (async)
    pub async fn get_trade_count(&self) -> Result<usize> {
        let client = self.db.get_connection().await?;
        let paper_row = client
            .query_one("SELECT COUNT(*) FROM paper_trades", &[])
            .await;
        let live_row = client
            .query_one("SELECT COUNT(*) FROM live_trades", &[])
            .await;

        let paper_count = paper_row.map(|r| r.get::<_, i64>(0)).unwrap_or(0);
        let live_count = live_row.map(|r| r.get::<_, i64>(0)).unwrap_or(0);

        Ok((paper_count + live_count) as usize)
    }

    /// Get the latest price for a token (async)
    pub async fn get_latest_price(&self, token_id: &str) -> Result<Option<f64>> {
        let _price_key = format!("token:price:{}", token_id); // Prefixed price_key with _
                                                              // Placeholder: In a real scenario, this would query a cache or a fast data store
                                                              // For now, returning None to indicate no cached price found
        Ok(None)
    }

    pub async fn get_token_metrics(
        &self,
        token_id: &str,
    ) -> Result<Option<crate::types::market::TokenMetrics>> {
        debug!("Getting token metrics for {}", token_id);
        match self.get_token_price_stats(token_id).await {
            Ok(token_data) => {
                let metrics = crate::types::market::TokenMetrics::from(&token_data);
                Ok(Some(metrics))
            }
            Err(Error::NotFound(_)) => Ok(None),
            Err(e) => {
                error!("Failed to get token metrics for {}: {:?}", token_id, e);
                Err(e)
            }
        }
    }

    /// Check if a token has price data in the database (async)
    pub async fn check_if_token_has_price_data(&self, token_id: &str) -> Result<bool> {
        let client = self.db.get_connection().await?;
        let row = client
            .query_one(
                "SELECT COUNT(*) FROM price_history WHERE token_id = $1",
                &[&token_id as &(dyn ToSql + Sync)],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;
        let count: i64 = row.get(0);
        Ok(count > 0)
    }

    /// Get price history for a token (async)
    pub async fn get_price_history(
        &self,
        token_id: &str,
        limit: usize,
    ) -> Result<Vec<(f64, f64, chrono::DateTime<Utc>)>> {
        debug!(
            "Getting price history for token {} with limit {}",
            token_id, limit
        );
        let client = self.db.get_connection().await?;

        let rows = client
            .query(
                queries::price::GET_PRICE_HISTORY,
                &[&token_id as &(dyn ToSql + Sync), &(limit as i64)],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        let history: Vec<(f64, f64, DateTime<Utc>)> = rows
            .iter()
            .map(|row| (row.get(0), row.get(1), row.get(2)))
            .collect();

        Ok(history)
    }

    /// Update token metadata with a TokenMetadata object (async)
    pub async fn update_token_metadata_full(
        &self,
        token_id: &str,
        metadata: &TokenMetadata,
    ) -> Result<()> {
        let client = self.db.get_connection().await?;
        client
            .execute(
                queries::token::UPSERT_TOKEN,
                &[
                    &token_id as &(dyn ToSql + Sync),
                    &metadata.symbol,
                    &metadata.name,
                    &metadata.updated_at,
                    &true,
                    &true,
                ],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        debug!(
            "Upserted token metadata for {} with name {}, symbol {}",
            token_id, metadata.name, metadata.symbol
        );
        Ok(())
    }
}

// Implement the CachingRepository trait for TokenRepository
#[async_trait::async_trait]
impl CachingRepository for TokenRepository {
    // Correct implementation for get_from_cache_or_db using get/set and serde
    async fn get_from_cache_or_db<T, F, FutDb>(
        &self,
        cache_key: &str,
        db_fetch: F,
    ) -> Result<Option<T>>
    where
        F: FnOnce() -> FutDb + Send + 'static,
        FutDb: Future<Output = Result<Option<T>>> + Send,
        T: serde::de::DeserializeOwned + serde::Serialize + Send + 'static,
    {
        trace!("Getting from cache or DB for key: {}", cache_key);
        if let Some(cache) = self.get_cache() {
            match cache.get(cache_key).await {
                // Use cache.get()
                Ok(value_str) => match serde_json::from_str::<T>(&value_str) {
                    Ok(value) => {
                        trace!("Cache hit and deserialized for key: {}", cache_key);
                        return Ok(Some(value));
                    }
                    Err(e) => {
                        warn!(
                                "Cache hit for key {}, but failed to deserialize: {}. Fetching from DB.",
                                cache_key, e
                            );
                        if let Err(del_e) = cache.delete(cache_key).await {
                            warn!(
                                "Failed to delete invalid cache entry {}: {}",
                                cache_key, del_e
                            );
                        }
                    }
                },
                Err(Error::Cache(msg)) if msg.contains("nil") || msg.contains("key not found") => {
                    trace!("Cache miss for key: {}", cache_key);
                }
                Err(e) => {
                    warn!(
                        "Cache get failed for key {}: {}. Fetching from DB.",
                        cache_key, e
                    );
                }
            }
        }

        let db_result = db_fetch().await?;
        if let Some(ref value) = db_result {
            if let Some(cache) = self.get_cache() {
                match serde_json::to_string(value) {
                    Ok(serialized_value) => {
                        if let Err(e) = cache.set(cache_key, &serialized_value, None).await {
                            // Use cache.set()
                            warn!(
                                "Failed to cache data for key {} after DB fetch: {}",
                                cache_key, e
                            );
                        }
                    }
                    Err(e) => {
                        warn!(
                            "Failed to serialize value for caching key {}: {}",
                            cache_key, e
                        );
                    }
                }
            }
        }
        Ok(db_result)
    }

    // Correct implementation for store_in_cache_and_db using set and serde, matching trait generics
    async fn store_in_cache_and_db<T, F, Fut>(
        &self,
        cache_key: &str,
        value: &T,
        db_store: F,
    ) -> Result<()>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<()>> + Send,
        T: serde::Serialize + Sync + Send,
    {
        trace!("Storing in cache and DB for key: {}", cache_key);
        let db_store_result = db_store().await;

        if db_store_result.is_ok() {
            if let Some(cache) = self.get_cache() {
                match serde_json::to_string(value) {
                    Ok(serialized_value) => {
                        if let Err(e) = cache.set(cache_key, &serialized_value, None).await {
                            // Use cache.set()
                            warn!("Failed to cache data for key {}: {}", cache_key, e);
                        }
                    }
                    Err(e) => {
                        warn!(
                            "Failed to serialize value for caching key {}: {}",
                            cache_key, e
                        );
                    }
                }
            }
        }
        db_store_result
    }

    fn is_cache_enabled(&self) -> bool {
        self.cache.is_some()
    }

    fn get_cache(&self) -> Option<Arc<Cache>> {
        self.cache.clone()
    }

    async fn invalidate_cache(&self, cache_key: &str) -> Result<()> {
        if let Some(cache) = &self.cache {
            cache.delete(cache_key).await.map_err(|e| {
                Error::Other(format!(
                    "Failed to invalidate cache key {}: {}",
                    cache_key, e
                ))
            })
        } else {
            Ok(())
        }
    }

    async fn prioritize_for_flush(&self, entity_id: &str) -> Result<()> {
        if let Some(cache) = &self.cache {
            cache
                .prioritize_token_flush(entity_id)
                .await
                .map_err(|e| Error::Other(format!("Failed to prioritize token for flush: {}", e)))
        } else {
            Ok(())
        }
    }
}

/// Implement the generic Repository trait for TokenData
#[async_trait::async_trait]
impl Repository<TokenData, String> for TokenRepository {
    async fn find_by_id(&self, id: String) -> crate::core::error::Result<Option<TokenData>> {
        match self.get_token_price_stats(&id).await {
            Ok(token) => Ok(Some(token)),
            Err(Error::NotFound(_)) => Ok(None),
            Err(e) if e.to_string().contains("relation \"tokens\" does not exist") => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn find_all(&self) -> crate::core::error::Result<Vec<TokenData>> {
        self.get_latest_market_data().await
    }

    async fn save(&self, entity: &TokenData) -> crate::core::error::Result<String> {
        self.update_token_metadata_full(
            &entity.id,
            &TokenMetadata {
                name: entity.name.clone(),
                symbol: entity.symbol.clone(),
                decimals: 18,
                updated_at: Utc::now(),
            },
        )
        .await?;
        Ok(entity.id.clone())
    }

    async fn delete(&self, id: String) -> crate::core::error::Result<()> {
        log::warn!("Token deletion not supported: {}", id);
        Ok(())
    }
}
