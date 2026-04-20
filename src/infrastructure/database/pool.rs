use deadpool_postgres::{PoolError, Runtime};
use tokio_postgres::NoTls;

use crate::config::Config;
use crate::infrastructure::database::schema;
use crate::infrastructure::errors::{Error, Result};
use chrono::{DateTime, Utc};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Represents generic token metadata (potentially useful outside DB context)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenMetadata {
    pub name: String,
    pub symbol: String,
    pub decimals: i32,
    pub updated_at: DateTime<Utc>,
}

pub type Pool = deadpool_postgres::Pool;
pub type Client = deadpool_postgres::Client;

#[derive(Clone)]
pub struct Database {
    pool: Arc<Pool>,
}

impl Database {
    /// Create a new database connection pool using configuration
    pub async fn new(config: &Config) -> Result<Self> {
        debug!("Initializing PostgreSQL connection pool...");
        let db_config = &config.database;

        let mut pg_connect_config = tokio_postgres::Config::new();
        pg_connect_config.host(&db_config.host);
        pg_connect_config.port(db_config.port);
        pg_connect_config.user(&db_config.user);
        if let Some(password) = &db_config.password {
            pg_connect_config.password(password);
        }
        pg_connect_config.dbname(&db_config.dbname);
        // Suppress NOTICE messages (e.g. "table already exists") to reduce log noise
        pg_connect_config.options("--client_min_messages=warning");

        let manager = deadpool_postgres::Manager::new(pg_connect_config, NoTls);
        let pool = deadpool_postgres::Pool::builder(manager)
            .max_size(db_config.pool_max_size)
            .runtime(Runtime::Tokio1)
            .build()
            .map_err(|e| Error::Database(format!("Failed to build PostgreSQL pool: {}", e)))?;

        debug!(
            "PostgreSQL connection pool created for database '{}' (max_size: {})",
            db_config.dbname, db_config.pool_max_size
        );

        let db = Self {
            pool: Arc::new(pool),
        };

        if let Err(e) = db.initialize_db().await {
            error!("Failed to initialize database schema: {}", e);
            return Err(e);
        }

        debug!("Database initialization completed successfully");
        Ok(db)
    }

    /// Get a database client from the pool
    pub async fn get_connection(&self) -> Result<Client> {
        self.pool.get().await.map_err(convert_pool_error)
    }

    /// Initialize the database schema (async)
    pub async fn initialize_db(&self) -> Result<()> {
        debug!("Initializing PostgreSQL database schema...");
        let mut client = self.get_connection().await?;
        schema::initialize_schema(&mut client).await?;

        debug!("PostgreSQL database schema initialization complete.");
        Ok(())
    }

    /// Reset the database (PostgreSQL specific)
    pub async fn reset_database(config: &Config) -> Result<()> {
        info!(
            "Resetting PostgreSQL database - THIS WILL DELETE ALL DATA in database '{}'",
            config.database.dbname
        );
        // Connect to the 'postgres' maintenance DB to drop/recreate the target DB
        let db_config = &config.database;
        let mut pg_base_config = tokio_postgres::Config::new();
        pg_base_config.host(&db_config.host);
        pg_base_config.port(db_config.port);
        pg_base_config.user(&db_config.user);
        if let Some(password) = &db_config.password {
            pg_base_config.password(password);
        }
        pg_base_config.dbname("postgres");
        pg_base_config.options("--client_min_messages=warning");

        let (client, connection) = pg_base_config.connect(NoTls).await.map_err(|e| {
            Error::Database(format!(
                "Failed to connect to 'postgres' DB for reset: {}",
                e
            ))
        })?;

        tokio::spawn(async move {
            if let Err(e) = connection.await {
                // Check if this is due to shutdown
                if crate::application::app::is_forced_shutdown() {
                    info!("PostgreSQL connection task during reset: Global shutdown detected, connection closed gracefully");
                } else {
                    error!("PostgreSQL connection error during reset: {}", e);
                }
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

        // client drop closes the direct connection
        info!("Re-initializing database connection and schema...");
        Database::new(config).await?;

        info!("PostgreSQL database reset completed successfully");
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

    #[cfg(test)]
    pub async fn new_test_db() -> Result<Self> {
        let config = Config::default(); // Use default config for tests
        let db = Database::new(&config).await?;
        // For a test DB, we likely want to ensure it's clean
        // This might be handled by test setup/teardown, or explicitly here.
        // For now, new_test_db will just create/connect and init schema via Database::new.
        // If we want to return the db instance, we need to assign it without underscore.
        Ok(db)
    }
}

fn convert_pool_error(e: PoolError) -> Error {
    Error::Database(format!("Connection pool error: {}", e))
}
