use crate::infrastructure::database::queries::transaction::{builders, create, select, update};
use crate::infrastructure::database::Database;
use crate::infrastructure::dex::{SwapDirection, TransactionDetails, TransactionStatus};
use crate::infrastructure::errors::{Error, Result};
use chrono::{DateTime, Utc};
use log::{debug, info, warn};
use serde_json::json;
use std::sync::Arc;

/// Return type for extract_status_data helper
type StatusData = (
    String,            // status_str
    serde_json::Value, // status_data
    Option<String>,    // details
    Option<String>,    // error_message
    Option<String>,    // error_code
    Option<String>,    // revert_reason
    Option<String>,    // recovery_suggestion
    Option<u64>,       // confirmations
    Option<u64>,       // required_confirmations
    Option<f64>,       // finality_probability
    Option<f64>,       // gas_efficiency
    Option<u32>,       // retry_count
);

#[derive(Clone)]
pub struct TransactionRepository {
    db: Arc<Database>,
}

impl TransactionRepository {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Create a new transaction record from TransactionDetails
    pub async fn create_transaction(
        &self,
        tx_details: &TransactionDetails,
        swap_direction: SwapDirection,
        is_paper_trade: bool,
        slippage_tolerance: Option<f64>,
        price_limit: Option<f64>,
    ) -> Result<()> {
        let query = create::INSERT_TRANSACTION;

        let client = self.db.get_connection().await.map_err(|e| {
            Error::Database(format!(
                "Failed to get DB client for transaction creation: {}",
                e
            ))
        })?;

        // Extract priority from TransactionStatus if available
        let priority_str = self.extract_priority_from_status(&tx_details.status);
        let status_str = self.extract_status_string(&tx_details.status);

        client
            .execute(
                query,
                &[
                    &tx_details.transaction_hash,               // tx_hash
                    &tx_details.tx_id,                          // tx_id
                    &tx_details.block_number.map(|b| b as i64), // block_number
                    &tx_details.timestamp, // block_timestamp (using tx timestamp)
                    &tx_details.gas_used.map(|g| g as i64), // gas_used
                    &tx_details.gas_price, // gas_price
                    &None::<i64>,          // gas_limit (not in TransactionDetails)
                    &tx_details.network_fee_eth, // network_fee_eth
                    &tx_details.network_fee_usd, // network_fee_usd
                    &tx_details.fees_paid, // fees_paid
                    &tx_details.fee_currency, // fee_currency
                    &tx_details.token_in_address, // token_in_address
                    &tx_details.token_out_address, // token_out_address
                    &tx_details.amount_in, // amount_in
                    &tx_details.amount_out, // amount_out
                    &tx_details.actual_price, // actual_price
                    &format!("{:?}", swap_direction), // swap_direction
                    &priority_str,         // priority
                    &status_str,           // current_status
                    &is_paper_trade,       // is_paper_trade
                    &slippage_tolerance,   // slippage_tolerance
                    &price_limit,          // price_limit
                    &Utc::now(),           // created_at
                    &Utc::now(),           // updated_at
                ],
            )
            .await
            .map_err(|e| Error::Database(format!("Failed to create transaction: {}", e)))?;

        // Create initial status history entry
        self.add_status_history(&tx_details.transaction_hash, &tx_details.status)
            .await?;

        info!(
            "Created transaction record: {} ({:?} -> {:?})",
            tx_details.transaction_hash, swap_direction, status_str
        );

        Ok(())
    }

    /// Update transaction from status event with optional complete details
    ///
    /// This is the main entry point for transaction updates from events.
    /// Updates status, blockchain data (if available), and history in a single call.
    pub async fn update_from_transaction_event(
        &self,
        status: &TransactionStatus,
        details: Option<&TransactionDetails>,
    ) -> Result<()> {
        let tx_id = self.extract_tx_id(status)?;

        // 1. Update transaction status
        self.update_transaction_status(tx_id, status).await?;

        // 2. Update blockchain data
        if let Some(tx_details) = details {
            // We have complete transaction details from receipt
            self.update_complete_transaction_data(tx_id, tx_details)
                .await?;
        } else {
            // Fallback: extract what we can from status enum
            self.update_blockchain_data(tx_id, status).await?;
        }

        // 3. Status history already added by update_transaction_status

        Ok(())
    }

    /// Extract transaction ID from TransactionStatus
    fn extract_tx_id<'a>(&self, status: &'a TransactionStatus) -> Result<&'a str> {
        match status {
            TransactionStatus::Queued { tx_id, .. } => Ok(tx_id),
            TransactionStatus::Pending { tx_id, .. } => Ok(tx_id),
            TransactionStatus::Confirmed { tx_id, .. } => Ok(tx_id),
            TransactionStatus::Success { tx_id, .. } => Ok(tx_id),
            TransactionStatus::Failed { tx_id, .. } => Ok(tx_id),
            TransactionStatus::Dropped { tx_id } => Ok(tx_id),
            TransactionStatus::Cancelled | TransactionStatus::Unknown => Err(Error::Database(
                "Cannot extract tx_id from Cancelled/Unknown status".to_string(),
            )),
        }
    }

    /// Update transaction status and add to history
    pub async fn update_transaction_status(
        &self,
        tx_hash: &str,
        new_status: &TransactionStatus,
    ) -> Result<()> {
        let status_str = self.extract_status_string(new_status);

        // Update main transaction record
        let update_query = update::UPDATE_TRANSACTION_STATUS;

        let client = self.db.get_connection().await.map_err(|e| {
            Error::Database(format!("Failed to get DB client for status update: {}", e))
        })?;

        let rows_affected = client
            .execute(update_query, &[&tx_hash, &status_str])
            .await
            .map_err(|e| Error::Database(format!("Failed to update transaction status: {}", e)))?;

        if rows_affected == 0 {
            warn!("No transaction found with hash {} to update", tx_hash);
            return Err(Error::Database(format!(
                "Transaction {} not found",
                tx_hash
            )));
        }

        // Add status history entry
        self.add_status_history(tx_hash, new_status).await?;

        debug!("Updated transaction {} status to {}", tx_hash, status_str);
        Ok(())
    }

    /// Update blockchain data from TransactionStatus when transaction confirms/fails
    ///
    /// Extracts gas_used, block_number, and calculates fees from the status event
    pub async fn update_blockchain_data(
        &self,
        tx_hash: &str,
        status: &TransactionStatus,
    ) -> Result<()> {
        let query = update::UPDATE_TRANSACTION_BLOCKCHAIN_DATA;

        // Extract blockchain data from status
        let (block_number, gas_used, network_fee_eth, network_fee_usd): (
            Option<i64>,
            Option<i64>,
            Option<f64>,
            Option<f64>,
        ) = match status {
            TransactionStatus::Success { .. } => {
                // Success status doesn't contain block/gas data in current enum
                // Would need to be fetched separately or added to Success variant
                (None, None, None, None)
            }
            TransactionStatus::Failed { gas_used, .. } => {
                // Failed status has gas_used
                let gas_used_i64 = gas_used.map(|g| g as i64);
                // Could calculate fees if we had gas_price
                (None, gas_used_i64, None, None)
            }
            TransactionStatus::Confirmed { .. } => {
                // Confirmed doesn't have blockchain data either
                (None, None, None, None)
            }
            _ => (None, None, None, None),
        };

        let client = self.db.get_connection().await.map_err(|e| {
            Error::Database(format!(
                "Failed to get DB client for blockchain data update: {}",
                e
            ))
        })?;

        let rows_affected = client
            .execute(
                query,
                &[
                    &tx_hash,
                    &block_number,
                    &gas_used,
                    &network_fee_eth,
                    &network_fee_usd,
                ],
            )
            .await
            .map_err(|e| Error::Database(format!("Failed to update blockchain data: {}", e)))?;

        if rows_affected > 0 {
            debug!(
                "Updated blockchain data for transaction {} (gas_used: {:?})",
                tx_hash, gas_used
            );
        }

        Ok(())
    }

    /// Update complete transaction data from TransactionDetails
    ///
    /// Used when full blockchain data is available from transaction receipt
    pub async fn update_complete_transaction_data(
        &self,
        tx_hash: &str,
        details: &TransactionDetails,
    ) -> Result<()> {
        let query = update::UPDATE_COMPLETE_TRANSACTION_DATA;

        let client = self.db.get_connection().await.map_err(|e| {
            Error::Database(format!(
                "Failed to get DB client for complete data update: {}",
                e
            ))
        })?;

        let rows_affected = client
            .execute(
                query,
                &[
                    &tx_hash,
                    &details.block_number.map(|b| b as i64),
                    &details.gas_used.map(|g| g as i64),
                    &details.gas_price,
                    &details.network_fee_eth,
                    &details.network_fee_usd,
                    &details.fees_paid,
                    &details.amount_out,
                    &details.actual_price,
                    &details.timestamp,
                ],
            )
            .await
            .map_err(|e| {
                Error::Database(format!("Failed to update complete transaction data: {}", e))
            })?;

        if rows_affected > 0 {
            info!(
                "Updated complete transaction data for {} (block: {:?}, gas: {:?}, fees: {:.6} ETH)",
                tx_hash, details.block_number, details.gas_used, details.fees_paid
            );
        }

        Ok(())
    }

    /// Add a status history entry
    async fn add_status_history(&self, tx_hash: &str, status: &TransactionStatus) -> Result<()> {
        let query = create::INSERT_STATUS_HISTORY;

        let client = self.db.get_connection().await.map_err(|e| {
            Error::Database(format!("Failed to get DB client for status history: {}", e))
        })?;

        let (
            status_str,
            status_data,
            details,
            error_message,
            error_code,
            revert_reason,
            recovery_suggestion,
            confirmations,
            required_confirmations,
            finality_probability,
            gas_efficiency,
            retry_count,
        ) = self.extract_status_data(status);

        client
            .execute(
                query,
                &[
                    &tx_hash,
                    &status_str,
                    &Utc::now(),
                    &status_data,
                    &details,
                    &error_message,
                    &error_code,
                    &revert_reason,
                    &recovery_suggestion,
                    &confirmations.map(|c| c as i64),
                    &required_confirmations.map(|c| c as i64),
                    &finality_probability,
                    &gas_efficiency,
                    &retry_count.map(|c| c as i32),
                ],
            )
            .await
            .map_err(|e| Error::Database(format!("Failed to add status history: {}", e)))?;

        Ok(())
    }

    /// Get transaction with full history
    pub async fn get_transaction_with_history(
        &self,
        tx_hash: &str,
    ) -> Result<Option<TransactionWithHistory>> {
        let tx_query = select::SELECT_TRANSACTION_FULL;

        let history_query = select::SELECT_STATUS_HISTORY;

        let client = self
            .db
            .get_connection()
            .await
            .map_err(|e| Error::Database(format!("Failed to get DB client: {}", e)))?;

        // Get main transaction
        let tx_rows = client
            .query(tx_query, &[&tx_hash])
            .await
            .map_err(|e| Error::Database(format!("Failed to query transaction: {}", e)))?;

        if tx_rows.is_empty() {
            return Ok(None);
        }

        let tx_row = &tx_rows[0];
        let transaction = Transaction {
            tx_hash: tx_row.get("tx_hash"),
            tx_id: tx_row.get("tx_id"),
            block_number: tx_row
                .get::<_, Option<i64>>("block_number")
                .map(|b| b as u64),
            block_timestamp: tx_row.get("block_timestamp"),
            gas_used: tx_row.get::<_, Option<i64>>("gas_used").map(|g| g as u64),
            gas_price: tx_row.get("gas_price"),
            gas_limit: tx_row.get::<_, Option<i64>>("gas_limit").map(|g| g as u64),
            network_fee_eth: tx_row.get("network_fee_eth"),
            network_fee_usd: tx_row.get("network_fee_usd"),
            fees_paid: tx_row.get("fees_paid"),
            fee_currency: tx_row.get("fee_currency"),
            token_in_address: tx_row.get("token_in_address"),
            token_out_address: tx_row.get("token_out_address"),
            amount_in: tx_row.get("amount_in"),
            amount_out: tx_row.get("amount_out"),
            actual_price: tx_row.get("actual_price"),
            swap_direction: tx_row.get("swap_direction"),
            priority: tx_row.get("priority"),
            current_status: tx_row.get("current_status"),
            is_paper_trade: tx_row.get("is_paper_trade"),
            slippage_tolerance: tx_row.get("slippage_tolerance"),
            price_limit: tx_row.get("price_limit"),
            created_at: tx_row.get("created_at"),
            updated_at: tx_row.get("updated_at"),
        };

        // Get status history
        let history_rows = client
            .query(history_query, &[&tx_hash])
            .await
            .map_err(|e| Error::Database(format!("Failed to query transaction history: {}", e)))?;

        let mut history = Vec::new();
        for row in history_rows {
            history.push(TransactionStatusHistoryEntry {
                status: row.get("status"),
                timestamp: row.get("timestamp"),
                status_data: row.get("status_data"),
                details: row.get("details"),
                error_message: row.get("error_message"),
                error_code: row.get("error_code"),
                revert_reason: row.get("revert_reason"),
                recovery_suggestion: row.get("recovery_suggestion"),
                confirmations: row.get::<_, Option<i64>>("confirmations").map(|c| c as u64),
                required_confirmations: row
                    .get::<_, Option<i64>>("required_confirmations")
                    .map(|c| c as u64),
                finality_probability: row.get("finality_probability"),
                gas_efficiency: row.get("gas_efficiency"),
                retry_count: row.get::<_, Option<i32>>("retry_count").map(|c| c as u32),
            });
        }

        Ok(Some(TransactionWithHistory {
            transaction,
            status_history: history,
        }))
    }

    /// Get transactions by status with optional filters
    pub async fn get_transactions_by_status(
        &self,
        status: &str,
        is_paper_trade: Option<bool>,
        limit: Option<i64>,
    ) -> Result<Vec<Transaction>> {
        let query =
            builders::build_transactions_by_status_query(is_paper_trade.is_some(), limit.is_some());

        let client = self
            .db
            .get_connection()
            .await
            .map_err(|e| Error::Database(format!("Failed to get DB client: {}", e)))?;

        let rows = match (is_paper_trade, limit) {
            (Some(paper_flag), Some(limit_val)) => {
                client
                    .query(&query, &[&status, &paper_flag, &limit_val])
                    .await
            }
            (Some(paper_flag), None) => client.query(&query, &[&status, &paper_flag]).await,
            (None, Some(limit_val)) => client.query(&query, &[&status, &limit_val]).await,
            (None, None) => client.query(&query, &[&status]).await,
        }
        .map_err(|e| Error::Database(format!("Failed to query transactions by status: {}", e)))?;

        let mut transactions = Vec::new();
        for row in rows {
            transactions.push(Transaction {
                tx_hash: row.get("tx_hash"),
                tx_id: row.get("tx_id"),
                block_number: row.get::<_, Option<i64>>("block_number").map(|b| b as u64),
                block_timestamp: row.get("block_timestamp"),
                gas_used: row.get::<_, Option<i64>>("gas_used").map(|g| g as u64),
                gas_price: row.get("gas_price"),
                gas_limit: row.get::<_, Option<i64>>("gas_limit").map(|g| g as u64),
                network_fee_eth: row.get("network_fee_eth"),
                network_fee_usd: row.get("network_fee_usd"),
                fees_paid: row.get("fees_paid"),
                fee_currency: row.get("fee_currency"),
                token_in_address: row.get("token_in_address"),
                token_out_address: row.get("token_out_address"),
                amount_in: row.get("amount_in"),
                amount_out: row.get("amount_out"),
                actual_price: row.get("actual_price"),
                swap_direction: row.get("swap_direction"),
                priority: row.get("priority"),
                current_status: row.get("current_status"),
                is_paper_trade: row.get("is_paper_trade"),
                slippage_tolerance: row.get("slippage_tolerance"),
                price_limit: row.get("price_limit"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            });
        }

        Ok(transactions)
    }

    /// Get recent transactions for analysis
    pub async fn get_recent_transactions(
        &self,
        limit: i64,
        is_paper_trade: Option<bool>,
    ) -> Result<Vec<Transaction>> {
        let query = builders::build_recent_transactions_query(is_paper_trade.is_some());

        let client = self
            .db
            .get_connection()
            .await
            .map_err(|e| Error::Database(format!("Failed to get DB client: {}", e)))?;

        let rows = match is_paper_trade {
            Some(paper_flag) => client.query(&query, &[&paper_flag, &limit]).await,
            None => client.query(&query, &[&limit]).await,
        }
        .map_err(|e| Error::Database(format!("Failed to query recent transactions: {}", e)))?;

        let mut transactions = Vec::new();
        for row in rows {
            transactions.push(Transaction {
                tx_hash: row.get("tx_hash"),
                tx_id: row.get("tx_id"),
                block_number: row.get::<_, Option<i64>>("block_number").map(|b| b as u64),
                block_timestamp: row.get("block_timestamp"),
                gas_used: row.get::<_, Option<i64>>("gas_used").map(|g| g as u64),
                gas_price: row.get("gas_price"),
                gas_limit: row.get::<_, Option<i64>>("gas_limit").map(|g| g as u64),
                network_fee_eth: row.get("network_fee_eth"),
                network_fee_usd: row.get("network_fee_usd"),
                fees_paid: row.get("fees_paid"),
                fee_currency: row.get("fee_currency"),
                token_in_address: row.get("token_in_address"),
                token_out_address: row.get("token_out_address"),
                amount_in: row.get("amount_in"),
                amount_out: row.get("amount_out"),
                actual_price: row.get("actual_price"),
                swap_direction: row.get("swap_direction"),
                priority: row.get("priority"),
                current_status: row.get("current_status"),
                is_paper_trade: row.get("is_paper_trade"),
                slippage_tolerance: row.get("slippage_tolerance"),
                price_limit: row.get("price_limit"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            });
        }

        Ok(transactions)
    }

    /// Helper method to extract priority from TransactionStatus
    fn extract_priority_from_status(&self, status: &TransactionStatus) -> String {
        match status {
            TransactionStatus::Queued { priority, .. } => format!("{:?}", priority),
            _ => "Standard".to_string(),
        }
    }

    /// Helper method to extract status string from TransactionStatus
    fn extract_status_string(&self, status: &TransactionStatus) -> String {
        match status {
            TransactionStatus::Queued { .. } => "Queued".to_string(),
            TransactionStatus::Pending { .. } => "Pending".to_string(),
            TransactionStatus::Confirmed { .. } => "Confirmed".to_string(),
            TransactionStatus::Success { .. } => "Success".to_string(),
            TransactionStatus::Failed { .. } => "Failed".to_string(),
            TransactionStatus::Cancelled => "Cancelled".to_string(),
            TransactionStatus::Dropped { .. } => "Dropped".to_string(),
            TransactionStatus::Unknown => "Unknown".to_string(),
        }
    }

    /// Helper method to extract all status data for history
    fn extract_status_data(&self, status: &TransactionStatus) -> StatusData {
        match status {
            TransactionStatus::Queued {
                tx_id,
                submission_time,
                priority,
            } => (
                "Queued".to_string(),
                json!({
                    "tx_id": tx_id,
                    "submission_time": submission_time,
                    "priority": format!("{:?}", priority)
                }),
                Some(format!("Transaction queued with {:?} priority", priority)),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ),
            TransactionStatus::Pending {
                tx_id,
                submission_time,
                last_checked,
                block_height,
                retry_count,
            } => (
                "Pending".to_string(),
                json!({
                    "tx_id": tx_id,
                    "submission_time": submission_time,
                    "last_checked": last_checked,
                    "block_height": block_height
                }),
                Some("Transaction pending confirmation".to_string()),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(*retry_count),
            ),
            TransactionStatus::Confirmed {
                tx_id,
                details,
                confirmations,
                required_confirmations,
                finality_probability,
            } => (
                "Confirmed".to_string(),
                json!({
                    "tx_id": tx_id,
                    "confirmations": confirmations,
                    "required_confirmations": required_confirmations,
                    "finality_probability": finality_probability
                }),
                Some(details.clone()),
                None,
                None,
                None,
                None,
                Some(*confirmations),
                Some(*required_confirmations),
                Some(*finality_probability),
                None,
                None,
            ),
            TransactionStatus::Success {
                tx_id,
                gas_efficiency,
                details,
            } => (
                "Success".to_string(),
                json!({
                    "tx_id": tx_id,
                    "gas_efficiency": gas_efficiency
                }),
                Some(details.clone()),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(*gas_efficiency),
                None,
            ),
            TransactionStatus::Failed {
                tx_id,
                reason,
                error_code,
                gas_used,
                revert_reason,
                recovery_suggestion,
            } => (
                "Failed".to_string(),
                json!({
                    "tx_id": tx_id,
                    "reason": reason,
                    "gas_used": gas_used
                }),
                None,
                Some(reason.clone()),
                error_code.clone(),
                revert_reason.clone(),
                recovery_suggestion.clone(),
                None,
                None,
                None,
                None,
                None,
            ),
            TransactionStatus::Cancelled => (
                "Cancelled".to_string(),
                json!({}),
                Some("Transaction cancelled".to_string()),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ),
            TransactionStatus::Dropped { tx_id } => (
                "Dropped".to_string(),
                json!({ "tx_id": tx_id }),
                Some("Transaction dropped from mempool".to_string()),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ),
            TransactionStatus::Unknown => (
                "Unknown".to_string(),
                json!({}),
                Some("Transaction status unknown".to_string()),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ),
        }
    }
}

/// Main transaction record
#[derive(Debug, Clone)]
pub struct Transaction {
    pub tx_hash: String,
    pub tx_id: String,
    pub block_number: Option<u64>,
    pub block_timestamp: Option<DateTime<Utc>>,
    pub gas_used: Option<u64>,
    pub gas_price: Option<String>,
    pub gas_limit: Option<u64>,
    pub network_fee_eth: Option<f64>,
    pub network_fee_usd: Option<f64>,
    pub fees_paid: Option<f64>,
    pub fee_currency: String,
    pub token_in_address: String,
    pub token_out_address: String,
    pub amount_in: f64,
    pub amount_out: Option<f64>,
    pub actual_price: Option<f64>,
    pub swap_direction: String,
    pub priority: String,
    pub current_status: String,
    pub is_paper_trade: bool,
    pub slippage_tolerance: Option<f64>,
    pub price_limit: Option<f64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Status history entry
#[derive(Debug, Clone)]
pub struct TransactionStatusHistoryEntry {
    pub status: String,
    pub timestamp: DateTime<Utc>,
    pub status_data: Option<serde_json::Value>,
    pub details: Option<String>,
    pub error_message: Option<String>,
    pub error_code: Option<String>,
    pub revert_reason: Option<String>,
    pub recovery_suggestion: Option<String>,
    pub confirmations: Option<u64>,
    pub required_confirmations: Option<u64>,
    pub finality_probability: Option<f64>,
    pub gas_efficiency: Option<f64>,
    pub retry_count: Option<u32>,
}

/// Transaction with full history
#[derive(Debug, Clone)]
pub struct TransactionWithHistory {
    pub transaction: Transaction,
    pub status_history: Vec<TransactionStatusHistoryEntry>,
}
