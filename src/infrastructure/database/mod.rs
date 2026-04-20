//! Main module for database interactions, defining the schema and repositories.

pub mod models;
pub mod pool;
pub mod queries;
pub mod queue;
pub mod repositories;
pub mod schema;
pub mod task_handler;

// Re-export key components
pub use pool::Database;
pub use repositories::RepositoryFactory;

// Re-export database models
pub use models::{ClosedPositionSummary, DbPosition, DbPriceHistory, DbTrade, PositionSummary};
