use crate::infrastructure::errors::{Error, Result};
use redis::AsyncCommands;
use serde::Serialize;

use super::connection::ConnectionManager;

/// Basic cache operations (get, set, delete)
pub struct CacheOperations {
    connection_manager: ConnectionManager,
    prefix: String,
}

impl CacheOperations {
    pub fn new(connection_manager: ConnectionManager, prefix: String) -> Self {
        Self {
            connection_manager,
            prefix,
        }
    }

    /// Get a value from cache by key
    pub async fn get(&self, key: &str) -> Result<String> {
        if !self.connection_manager.is_enabled() {
            return Err(Error::Cache("Cache is disabled".to_string()));
        }
        let mut conn = self.connection_manager.get_connection().await?;
        let value: String = conn
            .get(key)
            .await
            .map_err(|e| Error::Cache(format!("Redis GET error for key {}: {}", key, e)))?;
        Ok(value)
    }

    /// Set a value in cache with optional TTL
    pub async fn set<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl_seconds: Option<usize>,
    ) -> Result<()> {
        if !self.connection_manager.is_enabled() {
            return Ok(()); // Silently ignore if cache is disabled
        }
        let mut conn = self.connection_manager.get_connection().await?;
        let serialized_value = serde_json::to_string(value).map_err(Error::from)?;
        let key = format!("{}:{}", self.prefix, key);

        if let Some(ttl) = ttl_seconds {
            let _: () = conn
                .set_ex(&key, serialized_value, ttl as u64)
                .await
                .map_err(|e| Error::Cache(format!("Redis SETEX error for key {}: {}", key, e)))?;
        } else {
            let _: () = conn
                .set(&key, serialized_value)
                .await
                .map_err(|e| Error::Cache(format!("Redis SET error for key {}: {}", key, e)))?;
        }
        Ok(())
    }

    /// Delete a value from cache
    pub async fn delete(&self, key: &str) -> Result<()> {
        if !self.connection_manager.is_enabled() {
            return Ok(());
        }
        let full_key = format!("{}:{}", self.prefix, key);
        let mut conn = self.connection_manager.get_connection().await?;
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
}
