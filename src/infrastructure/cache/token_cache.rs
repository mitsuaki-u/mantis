use crate::infrastructure::database::pool::TokenMetadata;
use crate::infrastructure::errors::{Error, Result};
use chrono::Utc;
use log::{debug, error, warn};
use redis::{AsyncCommands, RedisResult};

use super::config::CacheConfig;
use super::connection::ConnectionManager;
use super::types::{CachedMarketData, CachedTokenMetadata};

/// Token-specific cache operations
pub struct TokenCache {
    connection_manager: ConnectionManager,
}

impl TokenCache {
    pub fn new(connection_manager: ConnectionManager) -> Self {
        Self { connection_manager }
    }

    /// Cache token metadata
    pub async fn cache_token_metadata(
        &self,
        token_id: &str,
        symbol: &str,
        decimals: u8,
    ) -> Result<()> {
        if !self.connection_manager.is_enabled() {
            debug!("Cache disabled, skipping metadata caching for {}", token_id);
            return Ok(());
        }

        let token_ttl = CacheConfig::get_token_cache_ttl();
        let key = format!("token:metadata:{}", token_id);
        let metadata = CachedTokenMetadata {
            name: token_id.to_string(),
            symbol: symbol.to_string(),
            decimals: decimals as i32,
            timestamp: Utc::now(),
        };

        let serialized =
            serde_json::to_string(&metadata).map_err(|e| Error::Serialization(e.to_string()))?;

        let mut conn = self.connection_manager.get_connection().await?;

        let _: () = conn
            .set_ex(&key, serialized, token_ttl as u64)
            .await
            .map_err(|e| Error::Cache(format!("Redis SETEX error: {}", e)))?;

        debug!(
            "Cached metadata for token {} (TTL: {} seconds)",
            token_id, token_ttl
        );
        Ok(())
    }

    /// Get cached token metadata
    pub async fn get_token_metadata(&self, token_id: &str) -> Result<Option<CachedTokenMetadata>> {
        if !self.connection_manager.is_enabled() {
            debug!("Cache disabled, not checking metadata for {}", token_id);
            return Ok(None);
        }

        let key = format!("token:metadata:{}", token_id);

        let mut conn = self.connection_manager.get_connection().await?;
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

    /// Cache price and volume data for a token
    pub async fn cache_price_data(&self, token_id: &str, price: f64, volume: f64) -> Result<()> {
        if !self.connection_manager.is_enabled() {
            debug!("Cache disabled, skipping price caching for {}", token_id);
            return Ok(());
        }

        let key = format!("token:price:{}", token_id);
        let market_data = CachedMarketData {
            price,
            volume,
            timestamp: Utc::now(),
        };

        let serialized =
            serde_json::to_string(&market_data).map_err(|e| Error::Serialization(e.to_string()))?;

        let mut conn = self.connection_manager.get_connection().await?;
        let _: () = conn
            .set(&key, serialized)
            .await
            .map_err(|e| Error::Cache(format!("Redis SET error: {}", e)))?;

        debug!(
            "Cached price data for token {}: ${:.4} vol ${:.2} (indefinite storage)",
            token_id, price, volume
        );
        Ok(())
    }

    /// Get cached price and volume data for a token
    pub async fn get_price_data(&self, token_id: &str) -> Result<Option<CachedMarketData>> {
        if !self.connection_manager.is_enabled() {
            debug!("Cache disabled, not checking price data for {}", token_id);
            return Ok(None);
        }

        let key = format!("token:price:{}", token_id);
        let mut conn = self.connection_manager.get_connection().await?;

        let result: RedisResult<Option<String>> = conn.get(&key).await;

        match result {
            Ok(Some(value)) => match serde_json::from_str::<CachedMarketData>(&value) {
                Ok(market_data) => {
                    debug!("💾 Cache HIT for price data: {}", token_id);
                    Ok(Some(market_data))
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

    /// Set metadata for a specific token using TokenMetadata from database
    pub async fn set_token_metadata(&self, token_id: &str, metadata: &TokenMetadata) -> Result<()> {
        let key = format!("token:meta:{}", token_id);
        let cache_data = CachedTokenMetadata {
            name: metadata.name.clone(),
            symbol: metadata.symbol.clone(),
            decimals: metadata.decimals,
            timestamp: Utc::now(),
        };
        let token_ttl = CacheConfig::get_token_cache_ttl();

        let serialized =
            serde_json::to_string(&cache_data).map_err(|e| Error::Serialization(e.to_string()))?;

        let mut conn = self.connection_manager.get_connection().await?;
        let _: () = conn
            .set_ex(&key, serialized, token_ttl as u64)
            .await
            .map_err(|e| Error::Cache(format!("Redis SETEX error: {}", e)))?;

        Ok(())
    }
}
