//! Database table definitions
//!
//! This module contains the SQL definitions for all database tables,
//! views, and indexes used in the application.

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
            decimals INTEGER NOT NULL DEFAULT 18,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL,
            price_usd DOUBLE PRECISION,
            price_change_24h DOUBLE PRECISION,
            volume_24h DOUBLE PRECISION,
            chain TEXT,
            address TEXT,
            latest_news TEXT,
            price_ath DOUBLE PRECISION,
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
            price DOUBLE PRECISION NOT NULL,
            volume DOUBLE PRECISION,
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
            price DOUBLE PRECISION NOT NULL,
            size DOUBLE PRECISION NOT NULL,
            is_buy BOOLEAN NOT NULL,
            timestamp TIMESTAMPTZ NOT NULL,
            is_paper BOOLEAN NOT NULL DEFAULT TRUE,
            position_id BIGINT,
            close_position BOOLEAN DEFAULT FALSE,
            FOREIGN KEY (token_id) REFERENCES tokens(id),
            FOREIGN KEY (position_id) REFERENCES positions(id)
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
            entry_price DOUBLE PRECISION NOT NULL,
            current_price DOUBLE PRECISION NOT NULL,
            highest_price DOUBLE PRECISION NOT NULL,
            size DOUBLE PRECISION NOT NULL,
            entry_time TIMESTAMPTZ NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            is_paper BOOLEAN NOT NULL DEFAULT TRUE,
            unrealized_pnl DOUBLE PRECISION DEFAULT 0.0,
            sell_trade_id BIGINT,
            closed BOOLEAN DEFAULT FALSE,
            close_price DOUBLE PRECISION,
            profit DOUBLE PRECISION DEFAULT 0.0,
            fees_paid DOUBLE PRECISION DEFAULT 0.0,
            exit_time TIMESTAMPTZ,
            FOREIGN KEY (token_id) REFERENCES tokens(id)
        )
    ",
};

/// Position reservations table - prevents race conditions in position limit enforcement
///
/// This table implements atomic slot reservation to prevent concurrent signals from
/// exceeding max_positions limit. Each reservation is short-lived (5 minutes) and
/// automatically cleaned up via periodic maintenance or on position creation/error.
pub const TABLE_POSITION_RESERVATIONS: SchemaDefinition = SchemaDefinition {
    name: "position_reservations",
    sql: "
        CREATE TABLE IF NOT EXISTS position_reservations (
            id BIGSERIAL PRIMARY KEY,
            correlation_id TEXT NOT NULL UNIQUE,
            is_paper BOOLEAN NOT NULL DEFAULT TRUE,
            reserved_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            expires_at TIMESTAMPTZ NOT NULL
        )
    ",
};

/// DEX transactions table - stores blockchain transaction data
pub const TABLE_DEX_TRANSACTIONS: SchemaDefinition = SchemaDefinition {
    name: "dex_transactions",
    sql: "
        CREATE TABLE IF NOT EXISTS dex_transactions (
            -- Primary identifiers
            tx_hash TEXT PRIMARY KEY,
            tx_id TEXT NOT NULL,
            
            -- Blockchain data
            block_number BIGINT,
            block_timestamp TIMESTAMPTZ,
            gas_used BIGINT,
            gas_price TEXT,
            gas_limit BIGINT,
            
            -- Network fees
            network_fee_eth DOUBLE PRECISION,
            network_fee_usd DOUBLE PRECISION,
            fees_paid DOUBLE PRECISION,
            fee_currency TEXT DEFAULT 'ETH',
            
            -- Swap details
            token_in_address TEXT NOT NULL,
            token_out_address TEXT NOT NULL,
            amount_in DOUBLE PRECISION NOT NULL,
            amount_out DOUBLE PRECISION,
            actual_price DOUBLE PRECISION,
            swap_direction TEXT CHECK (swap_direction IN ('Buy', 'Sell')),
            
            -- Transaction metadata
            priority TEXT DEFAULT 'Standard' CHECK (priority IN ('Low', 'Medium', 'Standard', 'High', 'Urgent')),
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            
            -- Status tracking
            current_status TEXT NOT NULL DEFAULT 'Queued' CHECK (
                current_status IN ('Queued', 'Pending', 'Confirmed', 'Success', 'Failed', 'Cancelled', 'Dropped', 'Unknown')
            ),
            
            -- Additional context
            is_paper_trade BOOLEAN NOT NULL DEFAULT TRUE,
            slippage_tolerance DOUBLE PRECISION,
            price_limit DOUBLE PRECISION
        )
    ",
};

/// DEX transaction status history table - tracks status changes over time
pub const TABLE_DEX_TRANSACTION_STATUS_HISTORY: SchemaDefinition = SchemaDefinition {
    name: "dex_transaction_status_history",
    sql: "
        CREATE TABLE IF NOT EXISTS dex_transaction_status_history (
            id BIGSERIAL PRIMARY KEY,
            tx_hash TEXT NOT NULL,
            status TEXT NOT NULL,
            timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            
            -- Status-specific data
            status_data JSONB DEFAULT '{}',
            
            -- Common status fields
            details TEXT,
            error_message TEXT,
            error_code TEXT,
            revert_reason TEXT,
            recovery_suggestion TEXT,
            
            -- Confirmation tracking
            confirmations BIGINT,
            required_confirmations BIGINT DEFAULT 12,
            finality_probability DOUBLE PRECISION,
            
            -- Performance metrics
            gas_efficiency DOUBLE PRECISION,
            retry_count INTEGER DEFAULT 0,
            
            FOREIGN KEY (tx_hash) REFERENCES dex_transactions(tx_hash) ON DELETE CASCADE
        )
    ",
};

// =========== View Definitions ===========

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

/// Partial unique index on positions - prevents duplicate open positions for same token
/// Only enforces uniqueness on open positions (closed = FALSE)
/// This allows the same token to have multiple closed positions in history
pub const INDEX_POSITIONS_UNIQUE_OPEN: SchemaDefinition = SchemaDefinition {
    name: "idx_positions_token_id_is_paper_unique_open",
    sql: "
        CREATE UNIQUE INDEX IF NOT EXISTS idx_positions_token_id_is_paper_unique_open
        ON positions(token_id, is_paper)
        WHERE closed = FALSE
    ",
};

/// Composite index on position_reservations for efficient counting by mode and active status
/// Optimizes: SELECT COUNT(*) WHERE is_paper = ? AND expires_at > NOW()
pub const INDEX_POSITION_RESERVATIONS_IS_PAPER_EXPIRES: SchemaDefinition = SchemaDefinition {
    name: "idx_position_reservations_is_paper_expires_at",
    sql: "
        CREATE INDEX IF NOT EXISTS idx_position_reservations_is_paper_expires_at
        ON position_reservations(is_paper, expires_at)
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

/// Index on dex_transactions for block number
pub const INDEX_DEX_TRANSACTIONS_BLOCK_NUMBER: SchemaDefinition = SchemaDefinition {
    name: "idx_dex_transactions_block_number",
    sql: "
        CREATE INDEX IF NOT EXISTS idx_dex_transactions_block_number 
        ON dex_transactions(block_number)
    ",
};

/// Index on dex_transactions for block timestamp
pub const INDEX_DEX_TRANSACTIONS_BLOCK_TIMESTAMP: SchemaDefinition = SchemaDefinition {
    name: "idx_dex_transactions_block_timestamp",
    sql: "
        CREATE INDEX IF NOT EXISTS idx_dex_transactions_block_timestamp 
        ON dex_transactions(block_timestamp)
    ",
};

/// Index on dex_transactions for token addresses
pub const INDEX_DEX_TRANSACTIONS_TOKEN_ADDRESSES: SchemaDefinition = SchemaDefinition {
    name: "idx_dex_transactions_token_addresses",
    sql: "
        CREATE INDEX IF NOT EXISTS idx_dex_transactions_token_addresses 
        ON dex_transactions(token_in_address, token_out_address)
    ",
};

/// Index on dex_transactions for current status
pub const INDEX_DEX_TRANSACTIONS_CURRENT_STATUS: SchemaDefinition = SchemaDefinition {
    name: "idx_dex_transactions_current_status",
    sql: "
        CREATE INDEX IF NOT EXISTS idx_dex_transactions_current_status 
        ON dex_transactions(current_status)
    ",
};

/// Index on dex_transactions for created_at
pub const INDEX_DEX_TRANSACTIONS_CREATED_AT: SchemaDefinition = SchemaDefinition {
    name: "idx_dex_transactions_created_at",
    sql: "
        CREATE INDEX IF NOT EXISTS idx_dex_transactions_created_at 
        ON dex_transactions(created_at)
    ",
};

/// Index on dex_transactions for paper trading flag
pub const INDEX_DEX_TRANSACTIONS_IS_PAPER: SchemaDefinition = SchemaDefinition {
    name: "idx_dex_transactions_is_paper",
    sql: "
        CREATE INDEX IF NOT EXISTS idx_dex_transactions_is_paper 
        ON dex_transactions(is_paper_trade)
    ",
};

/// Index on dex_transaction_status_history for tx_hash and timestamp
pub const INDEX_DEX_TRANSACTION_STATUS_HISTORY_TX_TIMESTAMP: SchemaDefinition = SchemaDefinition {
    name: "idx_dex_transaction_status_history_tx_timestamp",
    sql: "
        CREATE INDEX IF NOT EXISTS idx_dex_transaction_status_history_tx_timestamp 
        ON dex_transaction_status_history(tx_hash, timestamp)
    ",
};

/// Index on dex_transaction_status_history for status and timestamp
pub const INDEX_DEX_TRANSACTION_STATUS_HISTORY_STATUS_TIMESTAMP: SchemaDefinition =
    SchemaDefinition {
        name: "idx_dex_transaction_status_history_status_timestamp",
        sql: "
        CREATE INDEX IF NOT EXISTS idx_dex_transaction_status_history_status_timestamp 
        ON dex_transaction_status_history(status, timestamp)
    ",
    };

// =========== Table Collections ===========

/// Get all table definitions in dependency order
/// Tables must be created before other tables that reference them via foreign keys
pub fn get_table_definitions() -> Vec<&'static SchemaDefinition> {
    vec![
        &TABLE_TOKENS,                         // No dependencies
        &TABLE_POSITIONS,                      // Depends on: tokens
        &TABLE_POSITION_RESERVATIONS,          // No dependencies (independent atomicity table)
        &TABLE_PRICE_HISTORY,                  // Depends on: tokens
        &TABLE_TRADES,                         // Depends on: tokens, positions
        &TABLE_DEX_TRANSACTIONS,               // No dependencies
        &TABLE_DEX_TRANSACTION_STATUS_HISTORY, // Depends on: dex_transactions
    ]
}

/// Get all index definitions
pub fn get_index_definitions() -> Vec<&'static SchemaDefinition> {
    vec![
        &INDEX_PRICE_HISTORY_TOKEN_TIMESTAMP,
        &INDEX_PRICE_HISTORY_TIMESTAMP,
        &INDEX_POSITIONS_TOKEN,
        &INDEX_POSITIONS_ENTRY_TIME,
        &INDEX_POSITIONS_UNIQUE_OPEN,
        &INDEX_POSITION_RESERVATIONS_IS_PAPER_EXPIRES,
        &INDEX_TRADES_TOKEN_TIMESTAMP,
        &INDEX_TRADES_UNIQUE,
        &INDEX_DEX_TRANSACTIONS_BLOCK_NUMBER,
        &INDEX_DEX_TRANSACTIONS_BLOCK_TIMESTAMP,
        &INDEX_DEX_TRANSACTIONS_TOKEN_ADDRESSES,
        &INDEX_DEX_TRANSACTIONS_CURRENT_STATUS,
        &INDEX_DEX_TRANSACTIONS_CREATED_AT,
        &INDEX_DEX_TRANSACTIONS_IS_PAPER,
        &INDEX_DEX_TRANSACTION_STATUS_HISTORY_TX_TIMESTAMP,
        &INDEX_DEX_TRANSACTION_STATUS_HISTORY_STATUS_TIMESTAMP,
    ]
}

/// Get names of all required tables
pub fn get_required_tables() -> Vec<&'static str> {
    get_table_definitions().iter().map(|def| def.name).collect()
}
