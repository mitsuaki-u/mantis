//! Main module for database interactions, defining the schema and repositories.

pub mod database;
pub mod queries;
// pub mod transaction; // Removed - Not used with PostgreSQL
pub mod connection_monitor;
pub mod queue;
pub mod repositories;
pub mod schema;
pub mod task_handler;

// Re-export key components
pub use database::Database;
pub use repositories::RepositoryFactory;

// Removed old SQLite-specific re-exports
/*
// Re-export core types (potentially moved/refactored)
// TODO: Verify these re-exports are still correct after PG migration
pub use database::{
    TokenMetrics,
    CompletedTrade,
    TradeRecord,
    MetricsPeriod,
    Side,
    TradingStats,
    SqlitePool,
    PooledConnection,
    static_configure_connection,
    initialize_connection,
    from_config,
};
*/
