use crate::core::config::Config;
use crate::core::error::{Error, Result};
use chrono::Utc;
use log::{info, debug};
use rusqlite::Connection;
use std::path::PathBuf;
use rusqlite::types::ToSql;

/// Force initialize the database with all required tables
/// This function bypasses the connection pool and transaction system
/// to directly create the tables using SQLite
pub async fn force_initialize_database(config: &Config) -> Result<()> {
    // Get the database path
    let db_path = config.db_path()?;
    info!("Forcing direct database initialization at: {:?}", &db_path);
    
    // Ensure parent directory exists
    ensure_directory_exists(&db_path)?;
    
    // Create a direct connection to SQLite without using the connection pool
    let conn = create_direct_connection(&db_path)?;
    
    // Configure SQLite for optimal performance
    configure_pragmas(&conn)?;
    
    // Create all required tables
    create_tables(&conn)?;
    
    // Create the necessary views
    create_views(&conn)?;
    
    // Set database version
    update_db_version(&conn)?;
    
    // Create indexes for better performance
    create_indexes(&conn)?;
    
    // Verify database is usable by performing a simple query
    verify_database(&conn)?;
    
    info!("Database successfully initialized with all required tables and views");
    Ok(())
}

/// Ensure the database directory exists
fn ensure_directory_exists(db_path: &PathBuf) -> Result<()> {
    if let Some(parent) = db_path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::Io(format!("Failed to create database directory: {}", e)))?;
            info!("Created database directory: {:?}", parent);
        }
    }
    Ok(())
}

/// Create a direct connection to SQLite
fn create_direct_connection(db_path: &PathBuf) -> Result<Connection> {
    Connection::open(db_path)
        .map_err(|e| Error::Database(e))
}

/// Configure SQLite pragmas for optimal performance
fn configure_pragmas(conn: &Connection) -> Result<()> {
    // Enable WAL mode for better concurrency
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| Error::Database(e))?;
    
    // Set synchronous mode to NORMAL for better performance
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| Error::Database(e))?;
    
    // Enable foreign keys
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|e| Error::Database(e))?;
    
    // Set a larger cache size for better performance
    conn.pragma_update(None, "cache_size", -50000)
        .map_err(|e| Error::Database(e))?;
    
    // Store temp tables in memory
    conn.pragma_update(None, "temp_store", "MEMORY")
        .map_err(|e| Error::Database(e))?;
    
    // Enable memory-mapped I/O
    conn.pragma_update(None, "mmap_size", 268435456) // 256MB
        .map_err(|e| Error::Database(e))?;
    
    // Set page size
    conn.pragma_update(None, "page_size", 8192)
        .map_err(|e| Error::Database(e))?;
    
    // Set a longer busy timeout to reduce "database is locked" errors (5 seconds)
    conn.pragma_update(None, "busy_timeout", 5000)
        .map_err(|e| Error::Database(e))?;

    // Set normal locking mode (not exclusive)
    conn.pragma_update(None, "locking_mode", "NORMAL")
        .map_err(|e| Error::Database(e))?;
    
    Ok(())
}

/// Create all required tables
fn create_tables(conn: &Connection) -> Result<()> {
    debug!("Creating tokens table...");
    // Create the tokens table
    conn.execute_returning(
        "CREATE TABLE IF NOT EXISTS tokens (
            id TEXT PRIMARY KEY,
            symbol TEXT NOT NULL,
            name TEXT NOT NULL,
            last_updated TEXT NOT NULL,
            price_usd REAL,
            price_change_24h REAL,
            volume_24h REAL,
            market_cap REAL,
            market_cap_rank INTEGER,
            chain TEXT,
            address TEXT,
            latest_news TEXT,
            price_ath REAL,
            last_updated_price TEXT,
            is_tracked BOOLEAN NOT NULL DEFAULT 0,
            has_price_data BOOLEAN NOT NULL DEFAULT 0,
         )",
        &[] as &[&dyn ToSql]
    ).map_err(|e| Error::from(e))?;
    
    debug!("Creating price_history table...");
    // Create the price_history table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS price_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            token_id TEXT NOT NULL,
            price REAL NOT NULL,
            volume REAL NOT NULL,
            timestamp TEXT NOT NULL,
            FOREIGN KEY(token_id) REFERENCES tokens(id)
         )",
        [],
    ).map_err(|e| Error::Database(e))?;
    
    debug!("Creating positions table...");
    // Create the positions table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS positions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            token_id TEXT NOT NULL,
            provider_id TEXT NOT NULL,
            entry_price REAL NOT NULL,
            current_price REAL NOT NULL,
            highest_price REAL NOT NULL,
            size REAL NOT NULL,
            entry_time TEXT NOT NULL,
            is_paper INTEGER NOT NULL DEFAULT 1,
            unrealized_pnl REAL DEFAULT 0.0,
            closed INTEGER DEFAULT 0,
            updated_at TEXT,
            profit_loss REAL DEFAULT 0.0,
            created_at TEXT,
            UNIQUE(token_id, is_paper),
            FOREIGN KEY(token_id) REFERENCES tokens(id)
         )",
        [],
    ).map_err(|e| Error::Database(e))?;
    
    debug!("Creating trades table...");
    // Create the trades table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS trades (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            token_id TEXT NOT NULL,
            price REAL NOT NULL,
            size REAL NOT NULL,
            timestamp TEXT NOT NULL,
            is_buy INTEGER NOT NULL,
            is_paper INTEGER NOT NULL DEFAULT 1,
            position_id INTEGER,
            FOREIGN KEY(token_id) REFERENCES tokens(id),
            FOREIGN KEY(position_id) REFERENCES positions(id)
         )",
        [],
    ).map_err(|e| Error::Database(e))?;
    
    debug!("Creating db_version table...");
    // Create the db_version table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS db_version (
            version INTEGER PRIMARY KEY,
            initialized_at TEXT NOT NULL
        )",
        [],
    ).map_err(|e| Error::Database(e))?;
    
    Ok(())
}

/// Create the necessary views
fn create_views(conn: &Connection) -> Result<()> {
    debug!("Creating paper_positions view...");
    // Create paper_positions view
    conn.execute(
        "CREATE VIEW IF NOT EXISTS paper_positions AS 
         SELECT * FROM positions WHERE is_paper = 1",
        [],
    ).map_err(|e| Error::Database(e))?;
    
    debug!("Creating live_positions view...");
    // Create live_positions view
    conn.execute(
        "CREATE VIEW IF NOT EXISTS live_positions AS 
         SELECT * FROM positions WHERE is_paper = 0",
        [],
    ).map_err(|e| Error::Database(e))?;
    
    debug!("Creating paper_trades view...");
    // Create paper_trades view
    conn.execute(
        "CREATE VIEW IF NOT EXISTS paper_trades AS 
         SELECT * FROM trades WHERE is_paper = 1",
        [],
    ).map_err(|e| Error::Database(e))?;
    
    debug!("Creating live_trades view...");
    // Create live_trades view
    conn.execute(
        "CREATE VIEW IF NOT EXISTS live_trades AS 
         SELECT * FROM trades WHERE is_paper = 0",
        [],
    ).map_err(|e| Error::Database(e))?;
    
    Ok(())
}

/// Update the database version
fn update_db_version(conn: &Connection) -> Result<()> {
    // Insert or update the version record
    conn.execute(
        "INSERT OR REPLACE INTO db_version (version, initialized_at) VALUES (1, ?)",
        [Utc::now().to_rfc3339()],
    ).map_err(|e| Error::Database(e))?;
    
    Ok(())
}

/// Create indexes for better performance
fn create_indexes(conn: &Connection) -> Result<()> {
    debug!("Creating price_history indexes...");
    // Price history indexes
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_price_history_token_id_timestamp 
         ON price_history(token_id, timestamp)",
        [],
    ).map_err(|e| Error::Database(e))?;
    
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_price_history_timestamp 
         ON price_history(timestamp)",
        [],
    ).map_err(|e| Error::Database(e))?;
    
    debug!("Creating positions indexes...");
    // Positions indexes
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_positions_token_id 
         ON positions(token_id)",
        [],
    ).map_err(|e| Error::Database(e))?;
    
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_positions_entry_time 
         ON positions(entry_time)",
        [],
    ).map_err(|e| Error::Database(e))?;
    
    debug!("Creating trades indexes...");
    // Trades indexes
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_trades_token_id_timestamp 
         ON trades(token_id, timestamp)",
        [],
    ).map_err(|e| Error::Database(e))?;
    
    Ok(())
}

/// Verify the database is usable
fn verify_database(conn: &Connection) -> Result<()> {
    // Verify we can query the tokens table
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tokens",
        [],
        |row| row.get(0)
    ).map_err(|e| Error::Database(e))?;
    
    info!("Database verification complete. Tokens table contains {} entries.", count);
    
    Ok(())
} 