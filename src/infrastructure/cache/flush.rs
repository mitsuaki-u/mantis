use crate::infrastructure::database::repositories::TokenRepository;
use crate::infrastructure::errors::{Error, Result};
use log::{debug, error, info, warn};
use redis::{AsyncCommands, RedisResult};
use std::sync::Arc;
use std::time::Duration;

use super::connection::ConnectionManager;
use super::types::{CachedMarketData, CachedTokenMetadata};

/// Database flush operations for cache
pub struct FlushManager {
    connection_manager: ConnectionManager,
    token_repo: Option<Arc<TokenRepository>>,
    batch_interval: Duration,
}

impl FlushManager {
    pub fn new(
        connection_manager: ConnectionManager,
        token_repo: Option<Arc<TokenRepository>>,
        batch_interval: Duration,
    ) -> Self {
        Self {
            connection_manager,
            token_repo,
            batch_interval,
        }
    }

    /// Start background flush task
    pub async fn start_flush_task(self) -> Result<()> {
        if !self.connection_manager.is_enabled() {
            info!("Cache is disabled, not starting flush task");
            return Ok(());
        }

        info!(
            "Starting cache flush task with interval: {:?}",
            self.batch_interval
        );

        // Move self into the async task
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(self.batch_interval);

            loop {
                // Check global shutdown flag first
                if crate::application::app::is_forced_shutdown() {
                    info!("Cache flush task: Global shutdown detected, exiting");
                    break;
                }

                interval.tick().await;

                // Check again after tick in case shutdown happened during wait
                if crate::application::app::is_forced_shutdown() {
                    info!("Cache flush task: Global shutdown detected after tick, exiting");
                    break;
                }

                debug!("Running scheduled cache flush");

                if let Err(e) = self.flush_to_database().await {
                    error!("Error flushing cache to database: {}", e);
                }
            }
            info!("Cache flush task terminated gracefully");
        });

        Ok(())
    }

    /// Flush cached data to database
    pub async fn flush_to_database(&self) -> Result<()> {
        if !self.connection_manager.is_enabled() || self.token_repo.is_none() {
            debug!("Cache or TokenRepository not available, skipping flush to database.");
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
        let mut conn = self.connection_manager.get_connection().await?;

        // Get TokenRepository reference
        let token_repo = match &self.token_repo {
            Some(repo) => repo.clone(),
            None => {
                return Err(Error::Config(
                    "TokenRepository not available in Cache".to_string(),
                ))
            }
        };

        // Collect price updates from cache
        let mut price_updates: Vec<(String, f64, f64)> = Vec::new();

        for token_id in &all_tokens {
            let price_key = format!("token:price:{}", token_id);

            let price_result: RedisResult<Option<String>> = conn.get(&price_key).await;

            let market_data_opt: Option<CachedMarketData> = price_result
                .ok()
                .flatten()
                .and_then(|s| serde_json::from_str(&s).ok());

            if let Some(market_data) = market_data_opt {
                price_updates.push((token_id.clone(), market_data.price, market_data.volume));
            } else {
                warn!(
                    "No price data found in cache for token {} during flush.",
                    token_id
                );
            }
        }

        // Store updates in the database using TokenRepository
        if !price_updates.is_empty() {
            for (token_id, price, volume) in price_updates {
                // Re-fetch metadata for this specific token_id
                let metadata_key = format!("token:metadata:{}", token_id);
                let metadata_result: RedisResult<Option<String>> = conn.get(&metadata_key).await;
                let current_metadata_opt: Option<CachedTokenMetadata> = metadata_result
                    .ok()
                    .flatten()
                    .and_then(|s| serde_json::from_str(&s).ok());

                let (symbol_to_use, name_to_use, decimals_to_use) = match current_metadata_opt {
                    Some(meta) => (meta.symbol, meta.name, meta.decimals as u8),
                    None => (token_id.clone(), token_id.clone(), 18), // Fallback to token_id and 18 decimals
                };

                // Update token metadata with price
                if let Err(e) = token_repo
                    .update_token_metadata_with_price(
                        &token_id,
                        &symbol_to_use,
                        &name_to_use,
                        decimals_to_use,
                        price,
                        volume,
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

    /// Prioritize a token for flushing
    pub async fn prioritize_token_flush(&self, token_id: &str) -> Result<()> {
        if !self.connection_manager.is_enabled() {
            return Ok(());
        }
        let mut conn = self.connection_manager.get_connection().await?;
        let _: () = conn
            .sadd("priority_flush", token_id)
            .await
            .map_err(|e| Error::Cache(format!("Redis SADD error: {}", e)))?;
        debug!("Prioritized token {} for flushing", token_id);
        Ok(())
    }

    /// Get tokens pending flush
    async fn get_pending_flush_tokens(&self) -> Result<Vec<String>> {
        if !self.connection_manager.is_enabled() {
            return Ok(Vec::new());
        }
        let mut conn = self.connection_manager.get_connection().await?;
        let tokens: Vec<String> = conn
            .smembers("pending_flush")
            .await
            .map_err(|e| Error::Cache(format!("Redis SMEMBERS error: {}", e)))?;
        Ok(tokens)
    }

    /// Clear pending flush tokens
    async fn clear_pending_flush(&self) -> Result<()> {
        if !self.connection_manager.is_enabled() {
            return Ok(());
        }
        let mut conn = self.connection_manager.get_connection().await?;
        let _: () = conn
            .del("pending_flush")
            .await
            .map_err(|e| Error::Cache(format!("Redis DEL error: {}", e)))?;
        Ok(())
    }

    /// Get priority flush tokens
    async fn get_priority_flush_tokens(&self) -> Result<Vec<String>> {
        if !self.connection_manager.is_enabled() {
            return Ok(Vec::new());
        }
        let mut conn = self.connection_manager.get_connection().await?;
        let tokens: Vec<String> = conn
            .smembers("priority_flush")
            .await
            .map_err(|e| Error::Cache(format!("Redis SMEMBERS error: {}", e)))?;
        Ok(tokens)
    }

    /// Clear priority flush tokens
    async fn clear_priority_flush(&self) -> Result<()> {
        if !self.connection_manager.is_enabled() {
            return Ok(());
        }
        let mut conn = self.connection_manager.get_connection().await?;
        let _: () = conn
            .del("priority_flush")
            .await
            .map_err(|e| Error::Cache(format!("Redis DEL error: {}", e)))?;
        Ok(())
    }
}
