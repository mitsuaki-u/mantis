//! Database schema management module
//!
//! This module provides centralized database schema management, including:
//! - Table definitions
//! - Schema version tracking
//! - Database initialization

// Remove SQLite imports
// use rusqlite::Connection;
// use rusqlite::Result as RusqliteResult;
use crate::core::error::{Error, Result};
use log::{debug, error, info, warn}; // Add error and warn
use tokio_postgres::Client as PgClient; // Use PgClient
                                        // Remove unused imports
                                        // use std::collections::HashSet;
                                        // use crate::infra::db::queries;

// Submodules
pub mod tables;
pub mod version;

// Re-export useful items
pub use tables::SchemaDefinition;

/// Current schema version - increment when making schema changes
pub const CURRENT_SCHEMA_VERSION: i32 = 1;

/// Initialize the database schema (async for PostgreSQL)
///
/// This function creates all the necessary tables and indices
/// for the application to function properly.
pub async fn initialize_schema(client: &mut PgClient) -> Result<()> {
    // Changed signature to async + PgClient
    info!("Initializing PostgreSQL database schema");

    // Foreign keys are typically handled differently in PG (constraints)
    // No direct equivalent PRAGMA needed here.

    // Use a transaction for schema initialization
    let transaction = client
        .transaction()
        .await
        .map_err(|e| Error::Database(e.to_string()))?; // Use map_err

    // Create tables using batch execution
    debug!("Creating database schema object by object...");
    // let tables_sql = tables::get_tables_sql();
    // transaction.batch_execute(&tables_sql).await.map_err(|e| {
    //     error!(
    //         "Failed to execute schema batch (tables, views, indexes): {}",
    //         e
    //     );
    //     // It's important to propagate this error, as IF NOT EXISTS should handle most benign cases.
    //     // Any error here is likely a more serious problem (syntax, permissions, etc.)
    //     Error::Database(format!("Failed to execute schema batch: {}", e))
    // })?;
    // debug!("Database tables, views, and indexes created or verified via batch.");

    for def in tables::get_table_definitions() {
        info!("Applying DDL for table: {}", def.name);
        transaction.execute(def.sql, &[]).await.map_err(|e| {
            error!(
                "Failed to apply DDL for table {}: {}\nSQL: {}\n",
                def.name, e, def.sql
            );
            Error::Database(format!("Failed DDL for table {}: {}", def.name, e))
        })?;
    }
    info!("Tables created or verified.");

    for def in tables::get_view_definitions() {
        info!("Applying DDL for view: {}", def.name);
        transaction.execute(def.sql, &[]).await.map_err(|e| {
            error!(
                "Failed to apply DDL for view {}: {}\nSQL: {}\n",
                def.name, e, def.sql
            );
            Error::Database(format!("Failed DDL for view {}: {}", def.name, e))
        })?;
    }
    info!("Views created or verified.");

    for def in tables::get_index_definitions() {
        info!("Applying DDL for index: {}", def.name);
        transaction.execute(def.sql, &[]).await.map_err(|e| {
            error!(
                "Failed to apply DDL for index {}: {}\nSQL: {}\n",
                def.name, e, def.sql
            );
            Error::Database(format!("Failed DDL for index {}: {}", def.name, e))
        })?;
    }
    info!("Indexes created or verified.");

    info!("Applying ALTER TABLE statements for deferred constraints...");
    for def in tables::get_alter_table_definitions() {
        let savepoint_name = format!("sp_{}", def.name.replace("-", "_").replace(":", "_")); // Sanitize name further
        info!(
            "Applying DDL for alter: {} with savepoint {}",
            def.name, savepoint_name
        );

        // Create a savepoint
        if let Err(e) = transaction
            .execute(format!("SAVEPOINT {};", savepoint_name).as_str(), &[])
            .await
        {
            error!("Failed to create savepoint {}: {}", savepoint_name, e);
            return Err(Error::Database(format!(
                "Failed to create savepoint {}: {}",
                savepoint_name, e
            )));
        }

        match transaction.execute(def.sql, &[]).await {
            Ok(_) => {
                debug!("Successfully applied DDL for alter: {}", def.name);
                // Release the savepoint
                if let Err(e) = transaction
                    .execute(
                        format!("RELEASE SAVEPOINT {};", savepoint_name).as_str(),
                        &[],
                    )
                    .await
                {
                    error!("Failed to release savepoint {}: {}", savepoint_name, e);
                    // This might not be fatal enough to stop everything, but log it as a warning.
                    warn!(
                        "Failed to release savepoint {}, continuing...",
                        savepoint_name
                    );
                }
            }
            Err(e) => {
                if let Some(db_err) = e.as_db_error() {
                    let msg = db_err.message().to_lowercase();
                    // Check for common "already exists" type errors for constraints, relations, types etc.
                    if msg.contains("already exists")
                        || db_err.code() == &tokio_postgres::error::SqlState::DUPLICATE_OBJECT
                        || db_err.code() == &tokio_postgres::error::SqlState::DUPLICATE_TABLE
                    {
                        debug!(
                            "Object in DDL for alter {} likely already exists, rolling back to savepoint {}: {}",
                            def.name, savepoint_name, e
                        );
                        if let Err(e) = transaction
                            .execute(
                                format!("ROLLBACK TO SAVEPOINT {};", savepoint_name).as_str(),
                                &[],
                            )
                            .await
                        {
                            error!("Failed to rollback to savepoint {}: {}", savepoint_name, e);
                            return Err(Error::Database(format!(
                                "Failed to rollback to savepoint {}: {}",
                                savepoint_name, e
                            )));
                        }
                        // Also release the savepoint after rolling back to it, to clean it up.
                        // Some databases automatically discard savepoints on rollback to, others might not.
                        // It's safer to explicitly release it if no longer needed.
                        if let Err(e) = transaction
                            .execute(
                                format!("RELEASE SAVEPOINT {};", savepoint_name).as_str(),
                                &[],
                            )
                            .await
                        {
                            warn!(
                                "Failed to release savepoint {} after rollback: {}, continuing...",
                                savepoint_name, e
                            );
                        }
                    } else {
                        error!(
                            "Failed to apply DDL for alter {}: {}\nSQL: {}\n",
                            def.name, e, def.sql
                        );
                        // The main transaction will be rolled back by the caller or on drop due to this error.
                        return Err(Error::Database(format!(
                            "Failed DDL for alter {}: {}",
                            def.name, e
                        )));
                    }
                } else {
                    // Non-DB error
                    error!(
                        "Non-DB error when applying DDL for alter {}: {}\nSQL: {}\n",
                        def.name, e, def.sql
                    );
                    return Err(Error::Database(format!(
                        "Failed DDL for alter {} (non-DB error): {}",
                        def.name, e
                    )));
                }
            }
        }
    }
    info!("ALTER TABLE statements applied or verified.");

    // ensure_token_columns_exist is removed - schema should be complete in tables.rs
    // If ALTER TABLE is needed later, implement async version using client.execute

    // Create indexes using definitions from tables module (This loop is now redundant as indexes are created above)
    // info!("Creating database indexes...");
    // for index_def in tables::get_index_definitions() {
    //     debug!("Applying index: {}", index_def.name);
    //     if let Err(e) = transaction.execute(index_def.sql, &[]).await {
    //         if let Some(db_err) = e.as_db_error() {
    //             if db_err.code().code() == "42P07" { // duplicate_table (covers indexes too)
    //                 debug!("Index {} already exists, skipping.", index_def.name);
    //             } else {
    //                 error!("Failed to create index {}: {}", index_def.name, e);
    //                 warn!(
    //                     "Non-fatal error creating index {}, continuing...",
    //                     index_def.name
    //                 );
    //                 // Potentially return Err(Error::Database(e.to_string()));
    //             }
    //         } else {
    //             error!(
    //                 "Failed to create index {} (non-DB error): {}",
    //                 index_def.name, e
    //             );
    //             warn!(
    //                 "Non-fatal error creating index {}, continuing...",
    //                 index_def.name
    //             );
    //             // Potentially return Err(Error::Database(e.to_string()));
    //         }
    //     }
    // }
    // info!("Database indexes created or verified.");

    // Set the schema version (Needs async adaptation)
    // Assuming version::set_schema_version is updated or replaced
    // version::set_schema_version_async(&transaction, CURRENT_SCHEMA_VERSION, "Initial schema").await?;
    warn!("Schema versioning needs async implementation."); // Placeholder

    // Commit the transaction
    transaction
        .commit()
        .await
        .map_err(|e| Error::Database(e.to_string()))?; // Use map_err

    info!(
        "Database schema initialized to version {}",
        CURRENT_SCHEMA_VERSION
    );
    Ok(())
}

// Removed ensure_token_columns_exist (relied on SQLite PRAGMA)

// Removed create_tables (merged into initialize_schema with batch_execute)

// Removed export_schema (relied on sqlite_master)
// To export schema in PG, query information_schema.tables, information_schema.columns etc.
// This requires a different implementation.
/*
pub async fn export_schema_pg(client: &mut PgClient) -> Result<String> {
    warn!("Schema export for PostgreSQL not implemented.");
    Ok("-- PostgreSQL schema export not implemented --".to_string())
}
*/
