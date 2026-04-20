//! Database schema management module
//!
//! This module provides simple database schema initialization.

use crate::infrastructure::errors::{Error, Result};
use log::debug;
use tokio_postgres::Client;

// Submodules
pub mod tables;

// Re-export useful items
pub use tables::SchemaDefinition;

/// Initialize the database schema (async for PostgreSQL)
///
/// This function creates all the necessary tables and indices
/// for the application to function properly.
pub async fn initialize_schema(client: &mut Client) -> Result<()> {
    debug!("Initializing PostgreSQL database schema...");

    let transaction = client
        .transaction()
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

    // Create tables
    for def in tables::get_table_definitions() {
        debug!("Creating table: {}", def.name);
        transaction
            .execute(def.sql, &[])
            .await
            .map_err(|e| Error::Database(format!("Failed to create table {}: {}", def.name, e)))?;
    }

    // Create indexes
    for def in tables::get_index_definitions() {
        debug!("Creating index: {}", def.name);
        transaction
            .execute(def.sql, &[])
            .await
            .map_err(|e| Error::Database(format!("Failed to create index {}: {}", def.name, e)))?;
    }

    transaction
        .commit()
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

    debug!("Database schema initialized successfully");
    Ok(())
}
