use crate::core::error::{Error, Result};
use crate::infra::db::Database;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone)]
pub struct DexTransactionLogRepository {
    db: Arc<Database>,
}

impl DexTransactionLogRepository {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Logs a DEX transaction event to the database.
    ///
    /// # Arguments
    /// * `tx_id` - The unique transaction identifier.
    /// * `status_text` - A textual representation of the transaction's current status (e.g., "Pending", "Success", "Failed").
    /// * `event_timestamp` - The timestamp when this status event occurred or was observed.
    /// * `details_json` - Optional JSONB value containing status-specific details.
    ///   - For Success: could include TransactionDetails.
    ///   - For Failed: could include reason, error_code, gas_used.
    ///   - For others: relevant contextual information.
    pub async fn log_event(
        &self,
        tx_id: &str,
        status_text: &str,
        event_timestamp: DateTime<Utc>,
        details_json: Option<Value>,
    ) -> Result<()> {
        let query = r#"
            INSERT INTO dex_transaction_logs (tx_id, status, event_timestamp, details)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (tx_id, event_timestamp) DO UPDATE SET 
                status = EXCLUDED.status,
                details = EXCLUDED.details;
        "#;

        let client =
            self.db.get_connection().await.map_err(|e| {
                Error::Database(format!("Failed to get DB client for logging: {}", e))
            })?;

        client
            .execute(
                query,
                &[&tx_id, &status_text, &event_timestamp, &details_json],
            )
            .await
            .map_err(|e| Error::Database(format!("Failed to log DEX event to DB: {}", e)))?;
        Ok(())
    }
}
