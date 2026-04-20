//! Application bootstrap and initialization logic

use crate::application::app::is_forced_shutdown;
use crate::config::Config;
use crate::error::Error;
use crate::infrastructure::cache::Cache;
use crate::infrastructure::database::repositories::TokenRepository;
use crate::infrastructure::database::Database;
use log::{debug, error, info, warn};
use std::sync::Arc;
use std::time::Duration;

/// Initialize the database connection pool
pub async fn init_database(config: &Config) -> Result<Database, Error> {
    debug!("Initializing database...");
    // Use the async new method
    Database::new(config).await.map_err(|e| {
        error!("Fatal: Failed to initialize database: {}", e);
        // Provide more context for common connection errors
        if e.to_string().contains("connection refused") {
            eprintln!("Error: Could not connect to the PostgreSQL database.");
            eprintln!("Please ensure PostgreSQL is running and accessible at {}:{}.", config.database.host, config.database.port);
            eprintln!("Check database credentials in config.json or environment variables.");
        } else if e.to_string().contains("password authentication failed") {
            eprintln!("Error: PostgreSQL password authentication failed for user '{}'.", config.database.user);
             eprintln!("Check database credentials in config.json or environment variables (MANTIS_DB_PASSWORD).");
        } else if e.to_string().contains("database") && e.to_string().contains("does not exist") {
             eprintln!("Error: PostgreSQL database '{}' does not exist.", config.database.dbname);
             eprintln!("Please create the database or check the dbname setting in config.json.");
    }
        e
    })
}

/// Initialize the Redis cache connection pool
pub async fn init_cache(config: &Config, token_repo: Arc<TokenRepository>) -> Option<Cache> {
    if !config.cache.enabled {
        info!("Redis cache is disabled in configuration.");
        return None;
    }

    let redis_url_str = match &config.cache.redis_url {
        Some(url) => {
            info!("Initializing Redis cache at {}...", url);
            url.as_str()
        }
        None => {
            warn!(
                "Redis cache is enabled but URL is not configured. Cache will not be initialized."
            );
            return None;
        }
    };

    let mut cache = Cache::new(
        redis_url_str,
        crate::infrastructure::constants::CACHE_FLUSH_INTERVAL_SECS,
    )
    .await;

    if !cache.is_enabled() {
        warn!("Cache initialization failed or cache is disabled. Cache features inactive.");
        return None;
    }

    cache = cache.with_token_repository(token_repo.clone());

    info!("✅ Redis cache initialized successfully.");
    let cache_clone = Arc::new(cache.clone());
    let flush_interval_secs = crate::infrastructure::constants::CACHE_FLUSH_INTERVAL_SECS;

    tokio::spawn(async move {
        info!(
            "Starting background cache flush task (interval: {}s)",
            flush_interval_secs
        );
        let mut interval = tokio::time::interval(Duration::from_secs(flush_interval_secs));
        loop {
            // Check global shutdown flag first
            if is_forced_shutdown() {
                info!("Cache flush task: Global shutdown detected, exiting");
                break;
            }

            interval.tick().await;

            // Check again after tick in case shutdown happened during wait
            if is_forced_shutdown() {
                info!("Cache flush task: Global shutdown detected after tick, exiting");
                break;
            }

            debug!("Running periodic cache flush...");
            if let Err(e) = cache_clone.manual_flush().await {
                error!("Error during background cache flush: {}", e);
            }
        }
        info!("Cache flush task terminated gracefully");
    });
    Some(cache)
}
