use crate::core::domain::token::TokenData;
use crate::infrastructure::cache::Cache;
use crate::infrastructure::cache::CachingRepository;
use crate::infrastructure::database::pool::{Database, TokenMetadata};
use crate::infrastructure::database::queries;
use crate::infrastructure::errors::{Error, Result};
use chrono::{DateTime, Utc};
use log::{debug, error, info, trace, warn};
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use serde_json;
use std::convert::TryFrom;
use std::future::Future;
use std::sync::Arc;
use tokio_postgres::types::ToSql;

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

    /// Get access to the underlying database for creating other repositories
    pub fn get_database(&self) -> Database {
        self.db.clone()
    }

    /// Update token metadata and optionally the latest price (async)
    pub async fn update_token_metadata_with_price(
        &self,
        token_id: &str,
        symbol: &str,
        name: &str,
        decimals: u8,
        price: f64,
        volume: f64,
    ) -> Result<()> {
        debug!(
            "Updating token metadata and price for {} ({})",
            token_id, symbol
        );

        let mut client = self.db.get_connection().await?;
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
                    &(decimals as i32), // Use actual token decimals from Alchemy
                    &now,
                    &true, // is_tracked
                    &true, // has_price_data
                ],
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
        // CRITICAL: Normalize to lowercase for consistency
        let token_id = token_id.to_lowercase();

        let cached_data_opt = if let Some(cache) = &self.cache {
            cache.get_price_data(&token_id).await?
        } else {
            None
        };

        let has_price_data = if cached_data_opt.is_some() {
            true
        } else {
            let history = self.get_price_history(&token_id, 1).await?;
            !history.is_empty()
        };

        if !has_price_data {
            info!(
                "Token {} exists but has no price data, adding default price data",
                token_id
            );

            // Use ERC-20 standard defaults when adding initial price data.
            // At this point, the token typically has minimal metadata (created via INSERT_TOKEN_SIMPLE).
            // Full metadata (symbol, name, decimals) should be populated when the token is
            // discovered via DEX pool data. Using 18 decimals is appropriate because:
            // 1. 18 is the ERC-20 standard and covers most tokens
            // 2. Tokens without price data usually lack metadata entirely
            // 3. This is temporary until proper metadata is fetched
            self.update_token_metadata_with_price(&token_id, &token_id, &token_id, 18, 0.0, 0.0)
                .await?;
            info!(
                "Successfully added default price data for token {} (using ERC-20 standard: 18 decimals)",
                token_id
            );
        } else {
            debug!("Token {} already has price data", token_id);
        }
        Ok(())
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
        // CRITICAL: Normalize to lowercase for consistency with tokens table
        let normalized_token_id = token_id.to_lowercase();

        debug!(
            "Storing price data for {}: ${:.4}, vol: ${:.2}",
            normalized_token_id, price, volume
        );

        // Store in database first
        let client = self.db.get_connection().await?;
        let now = Utc::now();
        client
            .execute(
                queries::price::INSERT_PRICE,
                &[
                    &normalized_token_id as &(dyn ToSql + Sync),
                    &price,
                    &volume,
                    &now,
                ],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        // Cache the price data using the cache's dedicated method
        if let Some(cache) = &self.cache {
            if let Err(e) = cache
                .cache_price_data(&normalized_token_id, price, volume)
                .await
            {
                warn!(
                    "Failed to cache price data for {}: {}",
                    normalized_token_id, e
                );
            }
        }

        Ok(())
    }

    /// Batch store price data for multiple tokens using a single transaction
    pub async fn batch_store_price_data(&self, price_data: &[(String, f64, f64)]) -> Result<()> {
        if price_data.is_empty() {
            return Ok(());
        }

        // CRITICAL: Normalize all token IDs to lowercase for consistency
        let normalized_data: Vec<(String, f64, f64)> = price_data
            .iter()
            .map(|(token_id, price, volume)| (token_id.to_lowercase(), *price, *volume))
            .collect();

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
        for (token_id, price, volume) in &normalized_data {
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

        // Cache all the price data using the cache's dedicated method (use normalized IDs)
        if let Some(cache) = &self.cache {
            for (token_id, price, volume) in &normalized_data {
                if let Err(e) = cache.cache_price_data(token_id, *price, *volume).await {
                    warn!(
                        "Failed to cache price data for {} in batch: {}",
                        token_id, e
                    );
                }
            }
        }

        info!("Batch stored {} price data entries", normalized_data.len());
        Ok(())
    }

    /// Get token price and metadata (now async)
    pub async fn get_token_price_stats(&self, token_id: &str) -> Result<TokenData> {
        // CRITICAL: Normalize to lowercase to match database and ensure cache key consistency
        let token_id_string = token_id.to_lowercase();

        // 1. Check cache first (if enabled)
        if let Some(cache) = &self.cache {
            let cache_key = format!("token_stats:{}", token_id_string);
            if let Ok(cached_data_str) = cache.get(&cache_key).await {
                if let Ok(cached_data) = serde_json::from_str::<TokenData>(&cached_data_str) {
                    // CRITICAL: Validate cached data matches requested token_id
                    if cached_data.id.to_lowercase() == token_id.to_lowercase() {
                        debug!("Cache hit for token stats: {}", token_id);
                        return Ok(cached_data);
                    } else {
                        warn!(
                            "🚨 CACHE CORRUPTION DETECTED: Requested token {}, but cache returned {}. Invalidating cache entry.",
                            token_id, cached_data.id
                        );
                        // Invalidate corrupt cache entry
                        let _ = cache.delete(&cache_key).await;
                    }
                }
            }
        }

        // 2. Ensure token exists
        self.ensure_token_exists(&token_id_string).await?;

        // 3. Get latest price from price_history
        let latest_price_result = self
            .db
            .get_connection()
            .await?
            .query_opt(
                queries::price::GET_LATEST_PRICE,
                &[&token_id_string as &(dyn ToSql + Sync)],
            )
            .await;

        let latest_price = match latest_price_result {
            Ok(Some(row)) => {
                let price_value: f64 = row.get(0);
                Decimal::from_f64(price_value).ok_or_else(|| {
                    Error::NotFound(format!(
                        "Invalid price data for token: {}. Price value {} cannot be converted to Decimal",
                        token_id_string, price_value
                    ))
                })?
            }
            Ok(None) => {
                error!(
                    "No price data found for token: {}. Cannot provide price stats without valid price data.",
                    token_id_string
                );
                return Err(Error::NotFound(format!(
                    "No price data available for token: {}",
                    token_id_string
                )));
            }
            Err(e) => {
                error!(
                    "Failed to fetch latest price for {}: {}",
                    token_id_string, e
                );
                return Err(Error::Database(e.to_string()));
            }
        };

        // CRITICAL: Validate the price using our enhanced validation
        crate::core::utils::validation::price::validate_price(
            latest_price,
            &format!("Latest price for {}", token_id_string),
        )?;

        // 4. Get 24h price stats (GET_TOKEN_PRICE_STATS)
        let stats_result = self
            .db
            .get_connection()
            .await?
            .query_one(queries::price::GET_TOKEN_PRICE_STATS, &[&token_id_string])
            .await;

        let (price_change_24h, volume_24h) = match stats_result {
            Ok(row) => {
                let price_change_24h_value: Option<f64> = row.get(0);
                let volume_24h_value: Option<f64> = row.get(1);

                let price_change_24h = match price_change_24h_value {
                    Some(v) => match crate::core::utils::f64_to_decimal(v, "price_change_24h") {
                        Ok(val) => val,
                        Err(e) => {
                            warn!(
                                "Failed to convert price_change_24h {} to Decimal for {}: {}, using 0",
                                v, token_id_string, e
                            );
                            Decimal::ZERO
                        }
                    },
                    None => Decimal::ZERO,
                };

                let volume_24h = match volume_24h_value {
                    Some(v) => match crate::core::utils::f64_to_decimal(v, "volume_24h") {
                        Ok(val) => val,
                        Err(e) => {
                            warn!(
                                "Failed to convert volume_24h {} to Decimal for {}: {}, using 0",
                                v, token_id_string, e
                            );
                            Decimal::ZERO
                        }
                    },
                    None => Decimal::ZERO,
                };

                (price_change_24h, volume_24h)
            }
            Err(e) => {
                error!("Failed to fetch price stats for {}: {}", token_id_string, e);
                (Decimal::ZERO, Decimal::ZERO)
            }
        };

        // 5. Get basic token info
        let token_info_result = self
            .db
            .get_connection()
            .await?
            .query_one(
                queries::token::GET_TOKEN_INFO,
                &[&token_id_string as &(dyn ToSql + Sync)],
            )
            .await;

        let (symbol, name, decimals) = match token_info_result {
            Ok(row) => (
                row.get::<_, String>(0),
                row.get::<_, String>(1),
                row.get::<_, i32>(2),
            ),
            Err(e) => {
                warn!(
                    "Failed to get basic token info for {}: {}. Using token_id as fallback with 18 decimals (ERC-20 standard).",
                    token_id_string, e
                );
                // Note: 18 decimals is the ERC-20 standard and most common value
                // but USDC/USDT use 6, WBTC uses 8. This fallback may cause issues
                // for non-standard tokens if token metadata is missing from database.
                (token_id_string.clone(), token_id_string.clone(), 18)
            }
        };

        let token_data = TokenData {
            id: token_id_string.clone(),
            symbol,
            name,
            price_usd: latest_price,
            price_change_24h,
            volume_24h,
            decimals,
            chain: String::new(),
            address: None,
            latest_news: None,
            last_updated: Some(Utc::now()),
        };

        // 6. Cache the result (using normalized token_id_string for consistency)
        if let Some(cache) = &self.cache {
            if let Ok(serialized) = serde_json::to_string(&token_data) {
                if let Err(e) = cache
                    .set(
                        &format!("token_stats:{}", token_id_string),
                        &serialized,
                        None,
                    )
                    .await
                {
                    warn!("Failed to cache token stats for {}: {}", token_id_string, e);
                }
            }
        }

        Ok(token_data)
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

    /// Attempt to get token price stats with fallback mechanisms
    pub async fn get_token_price_stats_with_fallback(&self, token_id: &str) -> Result<TokenData> {
        // CRITICAL: Normalize to lowercase for consistency
        let token_id_normalized = token_id.to_lowercase();

        // Try primary method first (already normalizes internally)
        match self.get_token_price_stats(&token_id_normalized).await {
            Ok(data) => Ok(data),
            Err(Error::NotFound(_)) => {
                warn!(
                    "No price data found for {} in primary lookup, attempting fallbacks...",
                    token_id_normalized
                );

                // Fallback 1: Try to get cached price and volume data first
                if let Some(cache) = &self.cache {
                    if let Ok(Some(cached_price)) = cache.get_price_data(&token_id_normalized).await
                    {
                        if cached_price.price > 0.0 {
                            warn!(
                                "Using cached fallback data for {}: ${:.8}, vol: ${:.2}",
                                token_id_normalized, cached_price.price, cached_price.volume
                            );

                            return Ok(TokenData {
                                id: token_id_normalized.clone(),
                                symbol: token_id_normalized.to_uppercase(),
                                name: token_id_normalized.clone(),
                                price_usd: Decimal::from_f64(cached_price.price).ok_or_else(
                                    || {
                                        Error::InvalidInput(format!(
                                            "Cannot convert cached price {} to Decimal for {}",
                                            cached_price.price, token_id_normalized
                                        ))
                                    },
                                )?,
                                price_change_24h: Decimal::ZERO,
                                volume_24h: Decimal::from_f64(cached_price.volume)
                                    .unwrap_or(Decimal::ZERO),
                                decimals: 18, // Default for cached fallback data
                                chain: String::new(),
                                address: None,
                                latest_news: None,
                                last_updated: Some(Utc::now()),
                            });
                        }
                    }
                }

                // Fallback 2: Try to get just the latest price from database without 24h stats
                if let Ok(Some(latest_price)) = self.get_latest_price(&token_id_normalized).await {
                    if latest_price > 0.0 {
                        warn!(
                            "Using database fallback price data for {}: ${:.8} (no 24h stats available)",
                            token_id_normalized, latest_price
                        );

                        return Ok(TokenData {
                            id: token_id_normalized.clone(),
                            symbol: token_id_normalized.to_uppercase(),
                            name: token_id_normalized.clone(),
                            price_usd: Decimal::from_f64(latest_price).ok_or_else(|| {
                                Error::InvalidInput(format!(
                                    "Cannot convert fallback price {} to Decimal for {}",
                                    latest_price, token_id_normalized
                                ))
                            })?,
                            price_change_24h: Decimal::ZERO,
                            volume_24h: Decimal::ZERO,
                            decimals: 18, // Default for database fallback
                            chain: String::new(),
                            address: None,
                            latest_news: None,
                            last_updated: Some(Utc::now()),
                        });
                    }
                }

                // Fallback 3: Check if token exists but has no price data
                if let Ok(true) = self.token_exists(&token_id_normalized).await {
                    error!(
                        "Token {} exists in database but has no valid price data. This indicates a data quality issue.",
                        token_id_normalized
                    );
                    return Err(Error::NotFound(format!(
                        "Token {} exists but has no valid price data - possible data corruption",
                        token_id_normalized
                    )));
                }

                // No fallbacks worked
                Err(Error::NotFound(format!(
                    "No price data available for token {} from any source",
                    token_id_normalized
                )))
            }
            Err(other_error) => Err(other_error),
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
            let token_id: String = row.get(0);
            let price_usd: f64 = row.get(3);
            let price_change_24h: Option<f64> = row.get(4);
            let volume_24h: Option<f64> = row.get(5);

            // Validate critical price data - skip tokens with invalid prices to prevent corruption
            let price_usd_decimal = match Decimal::from_f64(price_usd) {
                Some(price) => price,
                None => {
                    error!("Failed to convert price_usd {} to Decimal for token {} in get_latest_market_data. Skipping to prevent corrupted data.", price_usd, token_id);
                    continue; // Skip this token entirely to prevent corrupted data
                }
            };

            // Handle optional 24h data safely - these can default to zero as they're supplementary
            let price_change_24h_decimal = price_change_24h
                .and_then(Decimal::from_f64)
                .unwrap_or(Decimal::ZERO); // Safe default for supplementary data

            let volume_24h_decimal = volume_24h
                .and_then(Decimal::from_f64)
                .unwrap_or(Decimal::ZERO); // Safe default for supplementary data

            tokens.push(TokenData {
                id: token_id,
                symbol: row.get(1),
                name: row.get(2),
                price_usd: price_usd_decimal,
                price_change_24h: price_change_24h_decimal,
                volume_24h: volume_24h_decimal,
                decimals: 18, // Default for batch query
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

    /// Ensure a token exists in the database (async)
    pub async fn ensure_token_exists(&self, token_id: &str) -> Result<()> {
        if self.token_exists(token_id).await? {
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
            .query_one("SELECT COUNT(*) FROM trades WHERE is_paper = TRUE", &[])
            .await;
        let live_row = client
            .query_one("SELECT COUNT(*) FROM trades WHERE is_paper = FALSE", &[])
            .await;

        let paper_count = paper_row.map(|r| r.get::<_, i64>(0)).unwrap_or(0);
        let live_count = live_row.map(|r| r.get::<_, i64>(0)).unwrap_or(0);

        Ok((paper_count + live_count) as usize)
    }

    /// Get the latest price for a token (async)
    pub async fn get_latest_price(&self, token_id: &str) -> Result<Option<f64>> {
        // CRITICAL: Normalize to lowercase for consistency
        let token_id_normalized = token_id.to_lowercase();

        // First try to get from cache
        if let Some(cache) = &self.cache {
            if let Ok(Some(cached_price)) = cache.get_price_data(&token_id_normalized).await {
                debug!(
                    "Using cached price for {}: ${:.4}",
                    token_id_normalized, cached_price.price
                );
                return Ok(Some(cached_price.price));
            }
        }

        // Fallback to database query
        let client = self.db.get_connection().await?;
        let result = client
            .query_opt(
                queries::price::GET_LATEST_PRICE,
                &[&token_id_normalized as &(dyn ToSql + Sync)],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        match result {
            Some(row) => {
                let price: f64 = row.get(0);
                debug!(
                    "Retrieved latest price from DB for {}: ${:.4}",
                    token_id, price
                );
                Ok(Some(price))
            }
            None => {
                debug!("No price data found for {} in cache or database", token_id);
                Ok(None)
            }
        }
    }

    pub async fn get_token_metrics(
        &self,
        token_id: &str,
    ) -> Result<Option<crate::core::domain::market::TokenMetrics>> {
        debug!("Getting token metrics for {}", token_id);
        match self.get_token_price_stats(token_id).await {
            Ok(token_data) => {
                // Use TryFrom to avoid silent data corruption
                match crate::core::domain::market::TokenMetrics::try_from(&token_data) {
                    Ok(metrics) => Ok(Some(metrics)),
                    Err(e) => {
                        error!(
                            "Failed to convert token data to metrics for {}: {}",
                            token_id, e
                        );
                        Err(e)
                    }
                }
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
        // CRITICAL: Normalize to lowercase for consistency
        let token_id_normalized = token_id.to_lowercase();

        let client = self.db.get_connection().await?;
        let row = client
            .query_one(
                "SELECT COUNT(*) FROM price_history WHERE token_id = $1",
                &[&token_id_normalized as &(dyn ToSql + Sync)],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;
        let count: i64 = row.get(0);
        Ok(count > 0)
    }

    /// Health check to verify database connectivity
    pub async fn health_check(&self) -> Result<()> {
        debug!("Performing database health check");
        let client = self.db.get_connection().await?;

        // Simple query to test database connectivity
        let _row = client
            .query_one("SELECT 1", &[])
            .await
            .map_err(|e| Error::Database(format!("Health check failed: {}", e)))?;

        debug!("Database health check passed");
        Ok(())
    }

    /// Get price history for a token (async)
    pub async fn get_price_history(
        &self,
        token_id: &str,
        limit: usize,
    ) -> Result<Vec<(f64, f64, chrono::DateTime<Utc>)>> {
        // CRITICAL: Normalize to lowercase for consistency
        let token_id_normalized = token_id.to_lowercase();

        debug!(
            "Getting price history for token {} with limit {}",
            token_id_normalized, limit
        );
        let client = self.db.get_connection().await?;

        let rows = client
            .query(
                queries::price::GET_PRICE_HISTORY,
                &[&token_id_normalized as &(dyn ToSql + Sync), &(limit as i64)],
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
                    &metadata.name,
                    &metadata.symbol,
                    &metadata.decimals,
                    &metadata.updated_at,
                    &true, // is_tracked
                    &true, // has_price_data
                ],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        debug!(
            "Upserted token metadata for {} with name {}, symbol {}, decimals {}",
            token_id, metadata.name, metadata.symbol, metadata.decimals
        );
        Ok(())
    }

    /// Get just the token symbol for a given token ID
    /// Returns the token_id as fallback if symbol is not found or on error
    pub async fn get_token_symbol(&self, token_id: &str) -> String {
        match self.get_token_price_stats_with_fallback(token_id).await {
            Ok(token_data) => {
                if !token_data.symbol.is_empty() && token_data.symbol != token_id {
                    token_data.symbol
                } else {
                    token_id.to_string()
                }
            }
            Err(e) => {
                debug!(
                    "Failed to get token symbol for {}: {}, using token_id as fallback",
                    token_id, e
                );
                token_id.to_string()
            }
        }
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
