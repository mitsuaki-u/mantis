// Remove SQLite imports
// use rusqlite::{Connection, params, Transaction};
// use rusqlite::OptionalExtension;

// Add PostgreSQL imports
use deadpool_postgres::{
    Config as DeadpoolConfig, ManagerConfig, PoolError, RecyclingMethod, Runtime,
};
use tokio_postgres::{Error as PgError, NoTls};

use crate::core::config::Config;
use crate::core::error::{Error, Result};
use chrono::Duration as ChronoDuration;
use chrono::{DateTime, Utc};
// use crate::core::error::{self, Error}; // Use explicit path for Error
// pub use error::Result; // Explicitly re-export Result
// use crate::domain::trading::strategy::Position; // Likely unused directly here now
use std::sync::Arc;
// use std::time::{Duration, Instant}; // Removed unused Duration, Instant
use crate::infra::db::schema;
use log::{error, info, warn}; // Removed unused debug, trace
use serde::{Deserialize, Serialize}; // Added import
                                     // use crate::infra::db::transaction::QueryableWrapper; // Removed, PG handles transactions differently
                                     // use crate::infra::db::transaction::execute_sql_safely; // Removed

// Import sibling modules
// use super::schema; // Removed unused schema
// use super::queries; // Queries might be refactored or removed
// use super::task_handler; // Might be needed later
// use super::repositories; // Might be needed later
// use super::transaction; // Removed

/// Enum representing trade side (buy/sell)
#[derive(Debug, Clone, PartialEq)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub fn from_bool(is_buy: bool) -> Self {
        if is_buy {
            Side::Buy
        } else {
            Side::Sell
        }
    }
}

/// Defines the time period for metrics analysis
#[derive(Debug, Clone, Copy)]
pub enum MetricsPeriod {
    Day,
    Week,
    Month,
    Year,
    AllTime,
}

impl MetricsPeriod {
    /// Get the date range for this period
    pub fn date_range(&self) -> (String, String) {
        let now = Utc::now();
        let end_date = now.to_rfc3339();

        let start_date = match self {
            MetricsPeriod::Day => (now - ChronoDuration::days(1)).to_rfc3339(),
            MetricsPeriod::Week => (now - ChronoDuration::days(7)).to_rfc3339(),
            MetricsPeriod::Month => (now - ChronoDuration::days(30)).to_rfc3339(),
            MetricsPeriod::Year => (now - ChronoDuration::days(365)).to_rfc3339(),
            MetricsPeriod::AllTime => "1970-01-01T00:00:00+00:00".to_string(),
        };

        (start_date, end_date)
    }
}

/// Represents generic token metadata (potentially useful outside DB context)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenMetadata {
    pub name: String,
    pub symbol: String,
    pub decimals: i32,
    pub updated_at: DateTime<Utc>,
}

/// Represents generic price data (potentially useful outside DB context)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceData {
    pub price: f64,
    pub timestamp: DateTime<Utc>,
}

// Function to convert PostgreSQL errors
fn convert_pg_error(e: PgError) -> Error {
    Error::Database(e.to_string())
}

fn convert_pool_error(e: PoolError) -> Error {
    Error::Database(format!("Connection pool error: {}", e))
}

// Define connection pool types for PostgreSQL
pub type Pool = deadpool_postgres::Pool;
pub type Client = deadpool_postgres::Client;

/// Database interface for storing trading data (now using PostgreSQL)
#[derive(Clone)]
pub struct Database {
    pool: Arc<Pool>,
    query_logging: bool,
}

impl Database {
    /// Create a new database connection pool using configuration
    pub async fn new(config: &Config) -> Result<Self> {
        info!("Initializing PostgreSQL connection pool...");
        let db_config = &config.database;

        // Explicitly create tokio_postgres::Config for connection parameters
        let mut pg_connect_config = tokio_postgres::Config::new();
        pg_connect_config.host(&db_config.host);
        pg_connect_config.port(db_config.port);
        pg_connect_config.user(&db_config.user); // This should be 'admin'
        if let Some(password) = &db_config.password {
            pg_connect_config.password(password); // Password for 'admin'
        }
        pg_connect_config.dbname(&db_config.dbname);
        // Add other necessary pg_connect_config settings here if needed
        // e.g. pg_connect_config.connect_timeout(std::time::Duration::from_secs(5));

        // Create the deadpool manager with the tokio_postgres::Config
        let manager = deadpool_postgres::Manager::new(pg_connect_config, NoTls);

        // Create the pool using Pool::builder
        let pool = deadpool_postgres::Pool::builder(manager)
            .max_size(db_config.pool_max_size) // Configure pool size
            .runtime(Runtime::Tokio1) // Specify runtime
            // .timeouts(deadpool_postgres::Timeouts::default()) // Example for timeouts
            .build()
            .map_err(|e| Error::Database(format!("Failed to build PostgreSQL pool: {}", e)))?;

        info!(
            "✅ PostgreSQL connection pool created for database '{}' on {}:{}",
            db_config.dbname, db_config.host, db_config.port
        );

        let db = Self {
            pool: Arc::new(pool),
            query_logging: db_config.query_logging,
        };

        // Perform initial connection test and schema initialization
        // Run schema initialization asynchronously
        if let Err(e) = db.initialize_db().await {
            error!("Failed to initialize database schema: {}", e);
            return Err(e);
        }

        Ok(db)
    }

    /// Get a database client from the pool
    pub async fn get_connection(&self) -> Result<Client> {
        // Add retry logic if needed, similar to the SQLite version
        self.pool.get().await.map_err(convert_pool_error)
    }

    /// Initialize the database schema (async)
    pub async fn initialize_db(&self) -> Result<()> {
        info!("Initializing PostgreSQL database schema...");
        let mut client = self.get_connection().await?;

        // Call the actual schema initialization function
        schema::initialize_schema(&mut client).await?;

        info!("PostgreSQL database schema initialization complete.");
        Ok(())
    }

    /// Reset the database (PostgreSQL specific)
    pub async fn reset_database(config: &Config) -> Result<()> {
        info!(
            "Resetting PostgreSQL database - THIS WILL DELETE ALL DATA in database '{}'",
            config.database.dbname
        );
        // Connect directly (not using pool) to drop/create DB
        let db_config = &config.database;
        let mut pg_base_config = tokio_postgres::Config::new();
        pg_base_config.host(&db_config.host);
        pg_base_config.port(db_config.port);
        pg_base_config.user(&db_config.user);
        if let Some(password) = &db_config.password {
            pg_base_config.password(password);
        }
        // Connect to the default 'postgres' database to drop the target one
        pg_base_config.dbname("postgres");

        let (client, connection) = pg_base_config.connect(NoTls).await.map_err(|e| {
            Error::Database(format!(
                "Failed to connect to 'postgres' DB for reset: {}",
                e
            ))
        })?;

        // Spawn the connection task
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                error!("PostgreSQL connection error during reset: {}", e);
            }
        });

        info!("Dropping database: {}", db_config.dbname);
        client
            .execute(
                &format!("DROP DATABASE IF EXISTS \"{}\"", db_config.dbname),
                &[],
            )
            .await
            .map_err(|e| Error::Database(format!("Failed to drop database: {}", e)))?;

        info!("Creating database: {}", db_config.dbname);
        client
            .execute(&format!("CREATE DATABASE \"{}\"", db_config.dbname), &[])
            .await
            .map_err(|e| Error::Database(format!("Failed to create database: {}", e)))?;

        // Close the direct connection
        // client is dropped here

        // Re-initialize using the normal pool mechanism to run schema creation
        info!("Re-initializing database connection and schema...");
        let db = Database::new(config).await?;
        // initialize_db is called within new(), so schema should be created.

        info!("PostgreSQL database reset completed successfully");
        Ok(())
    }

    /// Perform maintenance tasks on the database (PostgreSQL specific)
    pub async fn perform_maintenance(&self) -> Result<()> {
        info!("Running PostgreSQL database maintenance tasks (VACUUM ANALYZE)");
        let client = self.get_connection().await?;

        // Execute VACUUM and ANALYZE
        // Note: VACUUM FULL requires exclusive lock, standard VACUUM is usually preferred.
        client
            .batch_execute("VACUUM ANALYZE;")
            .await
            .map_err(|e| Error::Database(format!("PostgreSQL maintenance failed: {}", e)))?;

        info!("PostgreSQL maintenance tasks completed.");
        Ok(())
    }

    /// Get a clone of the connection pool
    pub fn get_pool(&self) -> Arc<Pool> {
        self.pool.clone()
    }

    /// Check the health of the connection pool (PostgreSQL specific)
    pub async fn check_pool_health(&self) -> (bool, String) {
        let state = self.pool.status();
        let mut is_healthy = true;
        let mut message = format!(
            "Pool health: Size={}, Available={}, Waiting={}",
            state.size, state.available, state.waiting
        );

        // Check if we can get a connection
        match self.get_connection().await {
            Ok(client) => {
                // Optionally run a quick query like "SELECT 1"
                if let Err(e) = client.query_one("SELECT 1", &[]).await {
                    is_healthy = false;
                    message = format!("{}. Failed test query: {}", message, e);
                    warn!("Pool health check: Failed test query: {}", e);
                }
            }
            Err(e) => {
                is_healthy = false;
                message = format!("{}. Failed to get connection: {}", message, e);
                warn!("Pool health check: Failed to get connection: {}", e);
            }
        }

        if !is_healthy {
            message = format!("Warning: {}", message);
        }

        (is_healthy, message)
    }

    /// Test the connection by executing a simple query
    pub async fn test_connection(&self) -> Result<()> {
        let client = self.get_connection().await?;
        client
            .query_one("SELECT 1", &[])
            .await
            .map(|_| ()) // Discard the row result
            .map_err(|e| Error::Database(format!("Database connection test failed: {}", e)))
    }

    /// Test write permission (PostgreSQL specific)
    pub async fn test_write_permission(&self) -> Result<()> {
        let client = self.get_connection().await?;
        // Try creating and dropping a temporary table
        client
            .batch_execute(
                "
            CREATE TEMP TABLE __test_write__ (id INT);
            DROP TABLE __test_write__;
        ",
            )
            .await
            .map_err(|e| Error::Database(format!("Database write permission test failed: {}", e)))
    }

    // Removed SQLite specific methods:
    // - new_with_path
    // - initialize_pool
    // - configure_pooled_connection
    // - get_raw_connection
    // - ensure_initialized
    // - configure_connection
    // - execute_pragma
    // - check_database_structure
    // - with_token_operation (needs PG transaction handling)
    // - verify_foreign_keys
    // - check_file_limits
    // - create_schema (called within initialize_db)
    // - handle_connection_limit_reached
    // - static_configure_connection
    // - initialize_connection
    // - from_config (replaced by async new)
}

// Removed TradingStats struct if only used here
// Removed static_configure_connection function
// Removed initialize_connection function
// Removed from_config function (replaced by Database::new)

// Removed check_file_limits function block
/*
pub fn check_file_limits() -> Result<()> {
    // ... implementation ...
}
*/

// Removed handle_connection_limit_reached function block
/*
pub fn handle_connection_limit_reached() -> Result<()> {
    // ... implementation ...
}
*/

pub fn is_paper_trading() -> bool {
    match Config::load() {
        Ok(config) => {
            // Assuming the structure is config.trading.paper_trading
            // Adjust if the actual structure is different
            config.trading.paper_trading
        }
        Err(e) => {
            warn!(
                "Failed to load config for is_paper_trading check: {}. Defaulting to false (live trading).",
                e
            );
            false // Default to false (live) on error for safety
        }
    }
}
