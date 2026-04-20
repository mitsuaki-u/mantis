//! Transaction-related SQL queries
//!
//! This module contains all SQL queries for transaction operations,
//! organized by functionality for better maintainability.

/// Transaction creation queries
pub mod create {
    /// Insert a new transaction record
    pub const INSERT_TRANSACTION: &str = r#"
        INSERT INTO dex_transactions (
            tx_hash, tx_id, block_number, block_timestamp, gas_used, gas_price, gas_limit,
            network_fee_eth, network_fee_usd, fees_paid, fee_currency,
            token_in_address, token_out_address, amount_in, amount_out, actual_price,
            swap_direction, priority, current_status, is_paper_trade,
            slippage_tolerance, price_limit, created_at, updated_at
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24
        )
    "#;

    /// Insert a new status history entry
    pub const INSERT_STATUS_HISTORY: &str = r#"
        INSERT INTO dex_transaction_status_history (
            tx_hash, status, timestamp, status_data, details, error_message, 
            error_code, revert_reason, recovery_suggestion, confirmations, 
            required_confirmations, finality_probability, gas_efficiency, retry_count
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
    "#;
}

/// Transaction update queries
pub mod update {
    /// Update transaction status
    pub const UPDATE_TRANSACTION_STATUS: &str = r#"
        UPDATE dex_transactions
        SET current_status = $2, updated_at = NOW()
        WHERE tx_hash = $1
    "#;

    /// Update blockchain data when transaction is confirmed
    pub const UPDATE_TRANSACTION_BLOCKCHAIN_DATA: &str = r#"
        UPDATE dex_transactions
        SET
            block_number = COALESCE($2, block_number),
            gas_used = COALESCE($3, gas_used),
            network_fee_eth = COALESCE($4, network_fee_eth),
            network_fee_usd = COALESCE($5, network_fee_usd),
            updated_at = NOW()
        WHERE tx_hash = $1
    "#;

    /// Update complete transaction data from TransactionDetails
    /// Used when we have full blockchain data from receipt
    pub const UPDATE_COMPLETE_TRANSACTION_DATA: &str = r#"
        UPDATE dex_transactions
        SET
            block_number = COALESCE($2, block_number),
            gas_used = COALESCE($3, gas_used),
            gas_price = COALESCE($4, gas_price),
            network_fee_eth = COALESCE($5, network_fee_eth),
            network_fee_usd = COALESCE($6, network_fee_usd),
            fees_paid = COALESCE($7, fees_paid),
            amount_out = COALESCE($8, amount_out),
            actual_price = COALESCE($9, actual_price),
            block_timestamp = COALESCE($10, block_timestamp),
            updated_at = NOW()
        WHERE tx_hash = $1
    "#;
}

/// Transaction retrieval queries
pub mod select {
    /// Get transaction with full details
    pub const SELECT_TRANSACTION_FULL: &str = r#"
        SELECT tx_hash, tx_id, block_number, block_timestamp, gas_used, gas_price, gas_limit,
               network_fee_eth, network_fee_usd, fees_paid, fee_currency,
               token_in_address, token_out_address, amount_in, amount_out, actual_price,
               swap_direction, priority, current_status, is_paper_trade,
               slippage_tolerance, price_limit, created_at, updated_at
        FROM dex_transactions
        WHERE tx_hash = $1
    "#;

    /// Get transaction status history
    pub const SELECT_STATUS_HISTORY: &str = r#"
        SELECT status, timestamp, status_data, details, error_message, error_code,
               revert_reason, recovery_suggestion, confirmations, required_confirmations,
               finality_probability, gas_efficiency, retry_count
        FROM dex_transaction_status_history
        WHERE tx_hash = $1
        ORDER BY timestamp ASC
    "#;

    /// Base query for transactions with filters (to be extended with WHERE clauses)
    pub const SELECT_TRANSACTIONS_BASE: &str = r#"
        SELECT tx_hash, tx_id, block_number, block_timestamp, gas_used, gas_price, gas_limit,
               network_fee_eth, network_fee_usd, fees_paid, fee_currency,
               token_in_address, token_out_address, amount_in, amount_out, actual_price,
               swap_direction, priority, current_status, is_paper_trade,
               slippage_tolerance, price_limit, created_at, updated_at
        FROM dex_transactions
    "#;

    /// Get transactions by status
    pub const SELECT_TRANSACTIONS_BY_STATUS: &str = r#"
        SELECT tx_hash, tx_id, block_number, block_timestamp, gas_used, gas_price, gas_limit,
               network_fee_eth, network_fee_usd, fees_paid, fee_currency,
               token_in_address, token_out_address, amount_in, amount_out, actual_price,
               swap_direction, priority, current_status, is_paper_trade,
               slippage_tolerance, price_limit, created_at, updated_at
        FROM dex_transactions
        WHERE current_status = $1
    "#;

    /// Get recent transactions (base query, parameters added dynamically)
    pub const SELECT_RECENT_TRANSACTIONS: &str = r#"
        SELECT tx_hash, tx_id, block_number, block_timestamp, gas_used, gas_price, gas_limit,
               network_fee_eth, network_fee_usd, fees_paid, fee_currency,
               token_in_address, token_out_address, amount_in, amount_out, actual_price,
               swap_direction, priority, current_status, is_paper_trade,
               slippage_tolerance, price_limit, created_at, updated_at
        FROM dex_transactions
    "#;
}

/// Query building helpers
pub mod builders {
    use super::select;

    /// Build a query for transactions by status with optional paper trade filter
    pub fn build_transactions_by_status_query(with_paper_filter: bool, with_limit: bool) -> String {
        let mut query = String::from(select::SELECT_TRANSACTIONS_BY_STATUS);

        let mut param_count = 1;
        if with_paper_filter {
            param_count += 1;
            query.push_str(&format!(" AND is_paper_trade = ${}", param_count));
        }

        query.push_str(" ORDER BY created_at DESC");

        if with_limit {
            param_count += 1;
            query.push_str(&format!(" LIMIT ${}", param_count));
        }

        query
    }

    /// Build a query for recent transactions with optional paper trade filter
    pub fn build_recent_transactions_query(with_paper_filter: bool) -> String {
        let mut query = String::from(select::SELECT_RECENT_TRANSACTIONS);

        if with_paper_filter {
            query.push_str(" WHERE is_paper_trade = $1 ORDER BY created_at DESC LIMIT $2");
        } else {
            query.push_str(" ORDER BY created_at DESC LIMIT $1");
        }

        query
    }
}

/// Common query fragments for reuse
pub mod fragments {
    /// Standard transaction fields for SELECT queries
    pub const TRANSACTION_FIELDS: &str = r#"
        tx_hash, tx_id, block_number, block_timestamp, gas_used, gas_price, gas_limit,
        network_fee_eth, network_fee_usd, fees_paid, fee_currency,
        token_in_address, token_out_address, amount_in, amount_out, actual_price,
        swap_direction, priority, current_status, is_paper_trade,
        slippage_tolerance, price_limit, created_at, updated_at
    "#;

    /// Standard status history fields for SELECT queries
    pub const STATUS_HISTORY_FIELDS: &str = r#"
        status, timestamp, status_data, details, error_message, error_code,
        revert_reason, recovery_suggestion, confirmations, required_confirmations,
        finality_probability, gas_efficiency, retry_count
    "#;

    /// Common WHERE clauses
    pub const WHERE_TX_HASH: &str = "WHERE tx_hash = $1";
    pub const WHERE_STATUS: &str = "WHERE current_status = $1";
    pub const WHERE_PAPER_TRADE: &str = "WHERE is_paper_trade = $1";

    /// Common ORDER BY clauses
    pub const ORDER_BY_CREATED_DESC: &str = "ORDER BY created_at DESC";
    pub const ORDER_BY_TIMESTAMP_ASC: &str = "ORDER BY timestamp ASC";
}
