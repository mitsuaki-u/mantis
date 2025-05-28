use crate::core::config::Config;
use crate::core::error::{Error, Result};
use chrono::Utc;
use log::{debug, info};
use tokio_postgres::{Client, Config as PgConfig, Error as PgError, NoTls};

/// Force initialize the database with all required tables using a direct PostgreSQL connection.
pub async fn force_initialize_database(config: &Config) -> Result<()> {
    info!(
        "Forcing direct PostgreSQL database initialization for database: {}",
        config.database.dbname
    );

    // Create a direct connection to PostgreSQL
    let mut client = create_direct_pg_connection(config).await?;

    // Create all required tables
    // It's generally better to run DDL in a transaction if supported and makes sense
    // For CREATE TABLE IF NOT EXISTS, individual execution is also fine.
    // Using batch_execute for simplicity here if no parameters are needed per statement.
    create_tables(&mut client).await?;

    // Create the necessary views
    create_views(&mut client).await?;

    // Set database version
    update_db_version(&mut client).await?;

    // Create indexes for better performance
    create_indexes(&mut client).await?;

    // Verify database is usable by performing a simple query
    verify_database(&mut client).await?;

    info!("PostgreSQL database successfully initialized with all required tables and views.");
    Ok(())
}

/// Create a direct connection to PostgreSQL
async fn create_direct_pg_connection(config: &Config) -> Result<Client> {
    let mut pg_config = PgConfig::new();
    pg_config.host(&config.database.host);
    pg_config.port(config.database.port);
    pg_config.user(&config.database.user);
    if let Some(password) = &config.database.password {
        pg_config.password(password);
    }
    pg_config.dbname(&config.database.dbname);

    let (client, connection) = pg_config
        .connect(NoTls)
        .await
        .map_err(|e| Error::Database(format!("Failed to connect to PostgreSQL: {}", e)))?;

    // The connection object performs the actual I/O, so spawn it off to run on its own.
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            // Check if this is due to shutdown
            if crate::domain::trading::execution::bot::is_forced_shutdown() {
                info!("PostgreSQL connection task: Global shutdown detected, connection closed gracefully");
            } else {
                eprintln!("PostgreSQL connection error: {}", e);
            }
        }
    });

    Ok(client)
}

/// Create all required tables for PostgreSQL
async fn create_tables(client: &mut Client) -> Result<()> {
    debug!("Creating tables for PostgreSQL...");

    let ddl_statements = "
        CREATE TABLE IF NOT EXISTS tokens (
            id TEXT PRIMARY KEY,
            symbol TEXT NOT NULL,
            name TEXT NOT NULL,
            last_updated TIMESTAMPTZ NOT NULL,
            price_usd DOUBLE PRECISION,
            price_change_24h DOUBLE PRECISION,
            volume_24h DOUBLE PRECISION,
            market_cap DOUBLE PRECISION,
            market_cap_rank INTEGER,
            chain TEXT,
            address TEXT,
            latest_news TEXT,
            price_ath DOUBLE PRECISION,
            last_updated_price TIMESTAMPTZ,
            is_tracked BOOLEAN NOT NULL DEFAULT FALSE,
            has_price_data BOOLEAN NOT NULL DEFAULT FALSE
        );

        CREATE TABLE IF NOT EXISTS price_history (
            id BIGSERIAL PRIMARY KEY,
            token_id TEXT NOT NULL REFERENCES tokens(id) ON DELETE CASCADE,
            price DOUBLE PRECISION NOT NULL,
            volume DOUBLE PRECISION NOT NULL,
            timestamp TIMESTAMPTZ NOT NULL
        );

        CREATE TABLE IF NOT EXISTS positions (
            id BIGSERIAL PRIMARY KEY,
            token_id TEXT NOT NULL REFERENCES tokens(id) ON DELETE CASCADE,
            provider_id TEXT NOT NULL, -- Consider if this should also be a FK
            entry_price DOUBLE PRECISION NOT NULL,
            current_price DOUBLE PRECISION NOT NULL,
            highest_price DOUBLE PRECISION NOT NULL,
            size DOUBLE PRECISION NOT NULL,
            entry_time TIMESTAMPTZ NOT NULL,
            is_paper BOOLEAN NOT NULL DEFAULT TRUE,
            unrealized_pnl DOUBLE PRECISION DEFAULT 0.0,
            closed BOOLEAN DEFAULT FALSE,
            updated_at TIMESTAMPTZ,
            profit_loss DOUBLE PRECISION DEFAULT 0.0,
            created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(token_id, is_paper)
        );

        CREATE TABLE IF NOT EXISTS trades (
            id BIGSERIAL PRIMARY KEY,
            token_id TEXT NOT NULL REFERENCES tokens(id) ON DELETE CASCADE,
            price DOUBLE PRECISION NOT NULL,
            size DOUBLE PRECISION NOT NULL,
            timestamp TIMESTAMPTZ NOT NULL,
            is_buy BOOLEAN NOT NULL,
            is_paper BOOLEAN NOT NULL DEFAULT TRUE,
            position_id BIGINT REFERENCES positions(id) ON DELETE SET NULL -- Or CASCADE
        );

        CREATE TABLE IF NOT EXISTS db_version (
            version INTEGER PRIMARY KEY,
            initialized_at TIMESTAMPTZ NOT NULL
        );

        CREATE TABLE IF NOT EXISTS dex_transaction_logs (
            tx_id TEXT NOT NULL,
            status TEXT NOT NULL,
            event_timestamp TIMESTAMPTZ NOT NULL,
            details JSONB,
            PRIMARY KEY (tx_id, event_timestamp)
        );
    ";

    client
        .batch_execute(ddl_statements)
        .await
        .map_err(|e| Error::Database(format!("Failed to create tables: {}", e)))?;

    debug!("Tables created successfully.");
    Ok(())
}

/// Create the necessary views for PostgreSQL
async fn create_views(client: &mut Client) -> Result<()> {
    debug!("Creating views for PostgreSQL...");
    let view_statements = "
        CREATE OR REPLACE VIEW paper_positions AS
            SELECT * FROM positions WHERE is_paper = TRUE;

        CREATE OR REPLACE VIEW live_positions AS
            SELECT * FROM positions WHERE is_paper = FALSE;

        CREATE OR REPLACE VIEW paper_trades AS
            SELECT * FROM trades WHERE is_paper = TRUE;

        CREATE OR REPLACE VIEW live_trades AS
            SELECT * FROM trades WHERE is_paper = FALSE;
    ";
    // Note: CREATE VIEW IF NOT EXISTS is not standard in older PostgreSQL.
    // Using CREATE OR REPLACE VIEW is generally safer for idempotent view creation.

    client
        .batch_execute(view_statements)
        .await
        .map_err(|e| Error::Database(format!("Failed to create views: {}", e)))?;
    debug!("Views created successfully.");
    Ok(())
}

/// Update the database version for PostgreSQL
async fn update_db_version(client: &mut Client) -> Result<()> {
    debug!("Updating database version for PostgreSQL...");
    client
        .execute(
            "INSERT INTO db_version (version, initialized_at) VALUES (1, $1)
             ON CONFLICT (version) DO UPDATE SET initialized_at = EXCLUDED.initialized_at",
            &[&Utc::now()],
        )
        .await
        .map_err(|e| Error::Database(format!("Failed to update db_version: {}", e)))?;
    debug!("Database version updated.");
    Ok(())
}

/// Create indexes for better performance in PostgreSQL
async fn create_indexes(client: &mut Client) -> Result<()> {
    debug!("Creating indexes for PostgreSQL...");
    let index_statements = "
        CREATE INDEX IF NOT EXISTS idx_price_history_token_id_timestamp
            ON price_history(token_id, timestamp);

        CREATE INDEX IF NOT EXISTS idx_price_history_timestamp
            ON price_history(timestamp);

        CREATE INDEX IF NOT EXISTS idx_positions_token_id_is_paper
            ON positions(token_id, is_paper); -- Combined for uniqueness constraint

        CREATE INDEX IF NOT EXISTS idx_positions_entry_time
            ON positions(entry_time);

        CREATE INDEX IF NOT EXISTS idx_trades_token_id_timestamp
            ON trades(token_id, timestamp);

        CREATE INDEX IF NOT EXISTS idx_dex_transaction_logs_tx_id
            ON dex_transaction_logs(tx_id);
        
        CREATE INDEX IF NOT EXISTS idx_dex_transaction_logs_event_timestamp
            ON dex_transaction_logs(event_timestamp);
    ";
    // Note: UNIQUE constraint on positions(token_id, is_paper) already creates an index.
    // Explicitly creating idx_positions_token_id_is_paper might be redundant or beneficial
    // depending on query patterns beyond just uniqueness checks.

    client
        .batch_execute(index_statements)
        .await
        .map_err(|e| Error::Database(format!("Failed to create indexes: {}", e)))?;
    debug!("Indexes created successfully.");
    Ok(())
}

/// Verify the PostgreSQL database is usable
async fn verify_database(client: &mut Client) -> Result<()> {
    debug!("Verifying PostgreSQL database connectivity...");
    let row = client
        .query_one("SELECT COUNT(*) FROM tokens", &[])
        .await
        .map_err(|e| Error::Database(format!("Database verification query failed: {}", e)))?;

    let count: i64 = row.get(0);
    info!(
        "PostgreSQL database verification complete. Tokens table contains {} entries.",
        count
    );
    Ok(())
}
