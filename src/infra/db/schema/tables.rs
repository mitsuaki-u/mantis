//! Database table definitions
//!
//! This module contains the SQL definitions for all database tables,
//! views, and indexes used in the application.

use crate::core::error::Result;
use log::{debug, error, info};

/// A schema definition consisting of a name and SQL statement
#[derive(Debug, Clone)]
pub struct SchemaDefinition {
    pub name: &'static str,
    pub sql: &'static str,
}

// =========== Table Definitions ===========

/// Tokens table - stores token metadata
pub const TABLE_TOKENS: SchemaDefinition = SchemaDefinition {
    name: "tokens",
    sql: "
        CREATE TABLE IF NOT EXISTS tokens (
            id TEXT PRIMARY KEY,
            symbol TEXT NOT NULL DEFAULT 'UNKNOWN',
            name TEXT NOT NULL DEFAULT 'UNKNOWN',
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL,
            price_usd FLOAT8,
            price_change_24h FLOAT8,
            volume_24h FLOAT8,
            market_cap FLOAT8,
            market_cap_rank INTEGER,
            chain TEXT,
            address TEXT,
            latest_news TEXT,
            price_ath FLOAT8,
            last_updated_price TIMESTAMPTZ,
            is_tracked BOOLEAN NOT NULL DEFAULT FALSE,
            has_price_data BOOLEAN NOT NULL DEFAULT FALSE
        )
    ",
};

/// Price history table - stores historical price data
pub const TABLE_PRICE_HISTORY: SchemaDefinition = SchemaDefinition {
    name: "price_history",
    sql: "
        CREATE TABLE IF NOT EXISTS price_history (
            id BIGSERIAL PRIMARY KEY,
            token_id TEXT NOT NULL,
            price FLOAT8 NOT NULL,
            volume FLOAT8,
            timestamp TIMESTAMPTZ NOT NULL,
            FOREIGN KEY (token_id) REFERENCES tokens(id) ON DELETE CASCADE,
            CONSTRAINT price_history_token_id_timestamp_unique UNIQUE (token_id, timestamp)
        )
    ",
};

/// Trades table - stores executed trades
pub const TABLE_TRADES: SchemaDefinition = SchemaDefinition {
    name: "trades",
    sql: "
        CREATE TABLE IF NOT EXISTS trades (
            id BIGSERIAL PRIMARY KEY,
            token_id TEXT NOT NULL,
            provider_id TEXT NOT NULL,
            price FLOAT8 NOT NULL,
            size FLOAT8 NOT NULL,
            is_buy BOOLEAN NOT NULL,
            timestamp TIMESTAMPTZ NOT NULL,
            is_paper BOOLEAN NOT NULL DEFAULT TRUE,
            position_id BIGINT,
            close_position BOOLEAN DEFAULT FALSE,
            FOREIGN KEY (token_id) REFERENCES tokens(id)
        )
    ",
};
/// Positions table - stores open trading positions
pub const TABLE_POSITIONS: SchemaDefinition = SchemaDefinition {
    name: "positions",
    sql: "
        CREATE TABLE IF NOT EXISTS positions (
            id BIGSERIAL PRIMARY KEY,
            token_id TEXT NOT NULL,
            provider_id TEXT NOT NULL,
            entry_price FLOAT8 NOT NULL,
            current_price FLOAT8 NOT NULL,
            highest_price FLOAT8 NOT NULL,
            size FLOAT8 NOT NULL,
            entry_time TIMESTAMPTZ NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            is_paper BOOLEAN NOT NULL DEFAULT TRUE,
            unrealized_pnl FLOAT8 DEFAULT 0.0,
            buy_trade_id BIGINT,
            sell_trade_id BIGINT,
            closed BOOLEAN DEFAULT FALSE,
            close_price FLOAT8,
            profit FLOAT8 DEFAULT 0.0,
            exit_time TIMESTAMPTZ,
            FOREIGN KEY (token_id) REFERENCES tokens(id),
            CONSTRAINT positions_token_id_is_paper_unique UNIQUE (token_id, is_paper)
        )
    ",
};

/// Schema version table - tracks database schema version
pub const TABLE_DB_VERSION: SchemaDefinition = SchemaDefinition {
    name: "db_version",
    sql: "
        CREATE TABLE IF NOT EXISTS db_version (
            version INTEGER PRIMARY KEY,
            initialized_at TIMESTAMPTZ NOT NULL,
            description TEXT NOT NULL DEFAULT ''
        )
    ",
};

// =========== View Definitions ===========

/// Paper positions view - filters positions table for paper trades
pub const VIEW_PAPER_POSITIONS: SchemaDefinition = SchemaDefinition {
    name: "paper_positions",
    sql: "
        CREATE OR REPLACE VIEW paper_positions AS 
        SELECT * FROM positions WHERE is_paper = TRUE
    ",
};

/// Live positions view - filters positions table for live trades
pub const VIEW_LIVE_POSITIONS: SchemaDefinition = SchemaDefinition {
    name: "live_positions",
    sql: "
        CREATE OR REPLACE VIEW live_positions AS 
        SELECT * FROM positions WHERE is_paper = FALSE
    ",
};

/// Paper trades view - filters trades table for paper trades
pub const VIEW_PAPER_TRADES: SchemaDefinition = SchemaDefinition {
    name: "paper_trades",
    sql: "
        CREATE OR REPLACE VIEW paper_trades AS 
        SELECT * FROM trades WHERE is_paper = TRUE
    ",
};

/// Live trades view - filters trades table for live trades
pub const VIEW_LIVE_TRADES: SchemaDefinition = SchemaDefinition {
    name: "live_trades",
    sql: "
        CREATE OR REPLACE VIEW live_trades AS 
        SELECT * FROM trades WHERE is_paper = FALSE
    ",
};

// =========== Index Definitions ===========

/// Index on price_history for token_id and timestamp
pub const INDEX_PRICE_HISTORY_TOKEN_TIMESTAMP: SchemaDefinition = SchemaDefinition {
    name: "idx_price_history_token_id_timestamp",
    sql: "
        CREATE INDEX IF NOT EXISTS idx_price_history_token_id_timestamp 
        ON price_history(token_id, timestamp)
    ",
};

/// Index on price_history for timestamp
pub const INDEX_PRICE_HISTORY_TIMESTAMP: SchemaDefinition = SchemaDefinition {
    name: "idx_price_history_timestamp",
    sql: "
        CREATE INDEX IF NOT EXISTS idx_price_history_timestamp 
        ON price_history(timestamp)
    ",
};

/// Index on positions for token_id
pub const INDEX_POSITIONS_TOKEN: SchemaDefinition = SchemaDefinition {
    name: "idx_positions_token_id",
    sql: "
        CREATE INDEX IF NOT EXISTS idx_positions_token_id 
        ON positions(token_id)
    ",
};

/// Index on positions for entry_time
pub const INDEX_POSITIONS_ENTRY_TIME: SchemaDefinition = SchemaDefinition {
    name: "idx_positions_entry_time",
    sql: "
        CREATE INDEX IF NOT EXISTS idx_positions_entry_time 
        ON positions(entry_time)
    ",
};

/// Index on trades for token_id and timestamp
pub const INDEX_TRADES_TOKEN_TIMESTAMP: SchemaDefinition = SchemaDefinition {
    name: "idx_trades_token_id_timestamp",
    sql: "
        CREATE INDEX IF NOT EXISTS idx_trades_token_id_timestamp 
        ON trades(token_id, timestamp)
    ",
};

/// Index on trades to prevent duplicate trades
pub const INDEX_TRADES_UNIQUE: SchemaDefinition = SchemaDefinition {
    name: "idx_trades_unique",
    sql: "
        CREATE UNIQUE INDEX IF NOT EXISTS idx_trades_unique 
        ON trades(token_id, price, size, timestamp, is_buy, is_paper)
    ",
};

// =========== Alter Table Definitions (for deferred foreign keys) ===========

pub const ALTER_TABLE_TRADES_ADD_FK_POSITION: SchemaDefinition = SchemaDefinition {
    name: "alter_trades_add_fk_position",
    sql: "
        ALTER TABLE trades
        ADD CONSTRAINT fk_trades_position_id
        FOREIGN KEY (position_id) REFERENCES positions(id)
    ",
};

pub const ALTER_TABLE_POSITIONS_ADD_FK_BUY_TRADE: SchemaDefinition = SchemaDefinition {
    name: "alter_positions_add_fk_buy_trade",
    sql: "
        ALTER TABLE positions
        ADD CONSTRAINT fk_positions_buy_trade_id
        FOREIGN KEY (buy_trade_id) REFERENCES trades(id)
    ",
};

pub const ALTER_TABLE_POSITIONS_ADD_FK_SELL_TRADE: SchemaDefinition = SchemaDefinition {
    name: "alter_positions_add_fk_sell_trade",
    sql: "
        ALTER TABLE positions
        ADD CONSTRAINT fk_positions_sell_trade_id
        FOREIGN KEY (sell_trade_id) REFERENCES trades(id)
    ",
};

/// Get all alter table definitions for deferred constraints
pub fn get_alter_table_definitions() -> Vec<&'static SchemaDefinition> {
    vec![
        &ALTER_TABLE_TRADES_ADD_FK_POSITION,
        &ALTER_TABLE_POSITIONS_ADD_FK_BUY_TRADE,
        &ALTER_TABLE_POSITIONS_ADD_FK_SELL_TRADE,
    ]
}

// =========== Table Collections ===========

/// Get all table definitions
pub fn get_table_definitions() -> Vec<&'static SchemaDefinition> {
    vec![
        &TABLE_TOKENS,
        &TABLE_PRICE_HISTORY,
        &TABLE_TRADES,
        &TABLE_POSITIONS,
        &TABLE_DB_VERSION,
    ]
}

/// Get all view definitions
pub fn get_view_definitions() -> Vec<&'static SchemaDefinition> {
    vec![
        &VIEW_PAPER_POSITIONS,
        &VIEW_LIVE_POSITIONS,
        &VIEW_PAPER_TRADES,
        &VIEW_LIVE_TRADES,
    ]
}

/// Get all index definitions
pub fn get_index_definitions() -> Vec<&'static SchemaDefinition> {
    vec![
        &INDEX_PRICE_HISTORY_TOKEN_TIMESTAMP,
        &INDEX_PRICE_HISTORY_TIMESTAMP,
        &INDEX_POSITIONS_TOKEN,
        &INDEX_POSITIONS_ENTRY_TIME,
        &INDEX_TRADES_TOKEN_TIMESTAMP,
        &INDEX_TRADES_UNIQUE,
    ]
}

/// Get names of all required tables
pub fn get_required_tables() -> Vec<&'static str> {
    get_table_definitions().iter().map(|def| def.name).collect()
}

/// Get names of all required views
pub fn get_required_views() -> Vec<&'static str> {
    get_view_definitions().iter().map(|def| def.name).collect()
}

/// Get names of all tables
pub fn get_table_names() -> Vec<&'static str> {
    vec![
        "tokens",
        "price_history",
        "positions",
        "trades",
        "db_version",
    ]
}

/// Get all table, view, and index SQL definitions as a single string
pub fn get_tables_sql() -> String {
    let mut sql = String::new();

    // Add table definitions
    for table in get_table_definitions() {
        sql.push_str(&format!("{};\n\n", table.sql.trim()));
    }

    // Add view definitions
    for view in get_view_definitions() {
        sql.push_str(&format!("{};\n\n", view.sql.trim()));
    }

    // Add index definitions
    for index in get_index_definitions() {
        sql.push_str(&format!("{};\n\n", index.sql.trim()));
    }

    sql
}
