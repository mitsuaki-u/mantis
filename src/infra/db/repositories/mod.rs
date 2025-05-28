pub mod dex_transaction_log;
pub mod position;
pub mod token;
pub mod trade;

pub use dex_transaction_log::DexTransactionLogRepository;
pub use position::CompletedPosition;
pub use position::PositionRepository;
pub use position::RecordCloseArgs;
pub use token::TokenRepository;
pub use trade::TradeRepository;

use crate::core::config::Config;
use crate::core::error::Result;
use crate::infra::cache::Cache;
use crate::infra::db::Database;
use std::future::Future;
use std::sync::Arc;

/// General repository trait
#[async_trait::async_trait]
pub trait Repository<T, ID> {
    /// Find an entity by its ID
    async fn find_by_id(&self, id: ID) -> crate::core::error::Result<Option<T>>;

    /// Find all entities
    async fn find_all(&self) -> crate::core::error::Result<Vec<T>>;

    /// Save an entity
    async fn save(&self, entity: &T) -> crate::core::error::Result<ID>;

    /// Delete an entity by its ID
    async fn delete(&self, id: ID) -> crate::core::error::Result<()>;
}

/// Trait for repositories that support caching
#[async_trait::async_trait]
pub trait CachingRepository {
    /// Get a value from cache, falling back to the database fetch function
    async fn get_from_cache_or_db<T, F, FutDb>(
        &self,
        cache_key: &str,
        db_fetch: F,
    ) -> Result<Option<T>>
    where
        F: FnOnce() -> FutDb + Send + 'static,
        FutDb: Future<Output = Result<Option<T>>> + Send,
        T: serde::de::DeserializeOwned + serde::Serialize + Send + 'static;

    /// Store a value in both the cache and database
    async fn store_in_cache_and_db<T, F, Fut>(
        &self,
        cache_key: &str,
        value: &T,
        db_store: F,
    ) -> Result<()>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<()>> + Send,
        T: serde::Serialize + Sync + Send;

    /// Check if the cache is enabled
    fn is_cache_enabled(&self) -> bool;

    /// Get the cache reference if available
    fn get_cache(&self) -> Option<Arc<Cache>>;

    /// Invalidate a specific cache entry
    async fn invalidate_cache(&self, cache_key: &str) -> Result<()>;

    /// Prioritize a specific entity for cache flushing
    async fn prioritize_for_flush(&self, entity_id: &str) -> Result<()>;
}

/// Repository factory to create and provide instances of all repositories
#[derive(Clone)]
pub struct RepositoryFactory {
    db: Database,
    config: Arc<Config>,
}

impl RepositoryFactory {
    /// Create a new repository factory
    pub fn new(db: Database, config: Arc<Config>) -> Self {
        Self { db, config }
    }

    /// Get the token repository
    pub fn token_repository(&self) -> Arc<TokenRepository> {
        Arc::new(TokenRepository::new(
            self.db.clone(),
            self.config.trading.paper_trading,
        ))
    }

    /// Get the position repository
    pub fn position_repository(&self) -> Arc<PositionRepository> {
        Arc::new(PositionRepository::new(
            self.db.clone(),
            self.config.trading.paper_trading,
        ))
    }

    /// Get the trade repository
    pub fn trade_repository(&self) -> Arc<TradeRepository> {
        Arc::new(TradeRepository::new(
            self.db.clone(),
            self.config.trading.paper_trading,
        ))
    }

    /// Get the DexTransactionLog repository
    pub fn dex_transaction_log_repository(&self) -> DexTransactionLogRepository {
        DexTransactionLogRepository::new(Arc::new(self.db.clone()))
    }

    /// Get a clone of the underlying database connection pool provider
    pub fn get_db(&self) -> Database {
        self.db.clone()
    }

    /// Get a reference to the config
    pub fn get_config(&self) -> &Config {
        &self.config
    }
}
