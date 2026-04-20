use crate::infrastructure::database::repositories::TokenRepository;
use crate::infrastructure::errors::{Error, Result};
use chrono::Utc;
use log::{debug, info, warn};
use rust_decimal::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;

use super::config::CacheConfig;
use super::connection::ConnectionManager;
use super::types::{CachedMarketData, CachedTokenMetadata};

/// Batch operations for cache
pub struct BatchOperations {
    connection_manager: ConnectionManager,
    token_repo: Option<Arc<TokenRepository>>,
}

impl BatchOperations {
    pub fn new(
        connection_manager: ConnectionManager,
        token_repo: Option<Arc<TokenRepository>>,
    ) -> Self {
        Self {
            connection_manager,
            token_repo,
        }
    }

    /// Batch get token data (metadata and price) for multiple tokens
    pub async fn batch_get_token_data(
        &self,
        token_ids: &[String],
    ) -> Result<HashMap<String, (Option<(String, String)>, Option<(f64, f64)>)>> {
        if !self.connection_manager.is_enabled() || token_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut conn = self.connection_manager.get_connection().await?;
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
                serde_json::from_str::<CachedMarketData>(&s)
                    .map_err(|e| warn!("Failed to parse price data for {}: {}", token_id, e))
                    .ok()
                    .map(|p| (p.price, p.volume))
            });

            result_map.insert(token_id.clone(), (metadata, price_data));
        }

        Ok(result_map)
    }

    /// Preload common token data into cache
    pub async fn preload_common_data(&self, token_ids: &[String]) -> Result<()> {
        if !self.connection_manager.is_enabled() || self.token_repo.is_none() {
            debug!("Cache or TokenRepository not available, skipping preload.");
            return Ok(());
        }

        info!(
            "Preloading cache with data for {} common tokens",
            token_ids.len()
        );

        let mut conn = self.connection_manager.get_connection().await?;
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

                            // Validate price conversion - skip caching if invalid to prevent corrupted cache data
                            let price_f64 = match token_data.price_usd.to_f64() {
                                Some(price) => price,
                                None => {
                                    warn!("Failed to convert price_usd to f64 for token {} during cache preload: {:?}. Skipping cache entry to prevent corrupted data.", token_id, token_data.price_usd);
                                    continue; // Skip this token to avoid caching invalid data
                                }
                            };

                            let price = CachedMarketData {
                                price: price_f64,
                                volume: 0.0, // Default volume for preloaded data
                                timestamp: Utc::now(),
                            };

                            if let (Ok(ser_meta), Ok(ser_price)) = (
                                serde_json::to_string(&metadata),
                                serde_json::to_string(&price),
                            ) {
                                let token_ttl = CacheConfig::get_token_cache_ttl();
                                pipe.set_ex(&metadata_key, ser_meta, token_ttl as u64)
                                    .set(&price_key, ser_price);
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
}
