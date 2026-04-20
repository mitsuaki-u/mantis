use crate::infrastructure::errors::{Error, Result};
use deadpool_redis::{
    Config as RedisConfig, Connection, Pool as RedisPool, Runtime as RedisRuntime,
};
use log::{error, info, warn};

/// Connection pool management for Redis cache
#[derive(Clone)]
pub struct ConnectionManager {
    pool: Option<RedisPool>,
}

impl ConnectionManager {
    /// Create a new connection manager with Redis pool
    pub async fn new(redis_url: &str) -> Self {
        let mut pool = None;
        let mut cfg = RedisConfig::from_url(redis_url);

        match cfg.create_pool(Some(RedisRuntime::Tokio1)) {
            Ok(p) => {
                // Test connection from pool
                match p.get().await {
                    Ok(_) => {
                        info!("✅ Redis connection pool initialized successfully for {}. Cache enabled.", redis_url);
                        pool = Some(p);
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
        if pool.is_none() && redis_url.contains("localhost") {
            let ip_url = redis_url.replace("localhost", "127.0.0.1");
            warn!(
                "Initial Redis connection failed. Trying fallback URL: {}",
                ip_url
            );
            cfg = RedisConfig::from_url(&ip_url);
            match cfg.create_pool(Some(RedisRuntime::Tokio1)) {
                Ok(p) => match p.get().await {
                    Ok(_) => {
                        info!("✅ Redis connection pool initialized successfully for {}. Cache enabled.", ip_url);
                        pool = Some(p);
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

        if pool.is_none() {
            warn!("❌ All Redis connection attempts failed. Cache will be disabled.");
        }

        Self { pool }
    }

    /// Check if the connection pool is available and cache is enabled
    pub fn is_enabled(&self) -> bool {
        self.pool.is_some()
    }

    /// Get a connection from the pool
    pub async fn get_connection(&self) -> Result<Connection> {
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

    /// Initialize and validate the connection pool
    pub fn initialize(&self) -> Result<()> {
        if self.is_enabled() {
            info!("Cache is enabled and pool available.");
            Ok(())
        } else {
            info!("Cache is disabled.");
            Ok(())
        }
    }
}
