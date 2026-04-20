use crate::infrastructure::constants::{
    COMPLETED_TTL_SECS, MAX_BACKOFF_SECS, MAX_RETRY_ATTEMPTS, METADATA_TTL_SECS,
    TOKEN_FAILURE_THRESHOLD,
};
use crate::infrastructure::errors::{Error, Result};
use chrono::{DateTime, Utc};
use log::{debug, error, info, trace, warn};
use redis::{Client, Connection};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::marker::PhantomData;
use uuid;

/// Trait for queue operations
pub trait QueueItem: Serialize + for<'de> Deserialize<'de> + Clone + Debug + Send + Sync {
    /// Get a unique identifier for this operation (used for tracing)
    fn operation_id(&self) -> String;

    /// Get the token ID associated with this operation
    fn token_id(&self) -> &str;
}

/// Position update operation for the queue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionUpdateOperation {
    /// Secure token identifier: "chain_id:contract_address"
    pub token_id: String,
    pub price: f64,
    pub pnl: f64,
    pub timestamp: DateTime<Utc>,
    #[serde(alias = "id", default = "generate_uuid")]
    pub operation_id: String,
    #[serde(default = "default_attempts")]
    pub attempts: usize,
}

fn generate_uuid() -> String {
    format!("pos_update_{}", uuid::Uuid::new_v4())
}

fn default_attempts() -> usize {
    0
}

impl PositionUpdateOperation {
    /// Create a new position update operation with only essential fields
    pub fn new(token_id: &str, price: f64, pnl: f64, timestamp: DateTime<Utc>) -> Self {
        Self {
            token_id: token_id.to_string(),
            price,
            pnl,
            timestamp,
            operation_id: generate_uuid(),
            attempts: 0,
        }
    }

    /// Get log-friendly name (shortened contract address)
    pub fn log_name(&self) -> String {
        // Extract contract address from token_id and show shortened version
        self.token_id
            .split(':')
            .nth(1)
            .map(|addr| {
                if addr.len() > 10 {
                    format!("{}...{}", &addr[..8], &addr[addr.len() - 6..])
                } else {
                    addr.to_string()
                }
            })
            .unwrap_or_else(|| self.token_id.clone())
    }
}

impl QueueItem for PositionUpdateOperation {
    fn operation_id(&self) -> String {
        self.operation_id.clone()
    }

    fn token_id(&self) -> &str {
        &self.token_id
    }
}

/// Redis-based persistent queue implementation
#[derive(Clone)]
pub struct RedisQueue<T: QueueItem> {
    redis_client: Client,
    main_queue_key: String,
    processing_queue_key: String,
    failed_queue_key: String,
    metadata_prefix: String,
    app_name: String,
    _phantom: PhantomData<T>,
}

impl<T: QueueItem + 'static> RedisQueue<T> {
    /// Create a new Redis queue
    pub fn new(redis_url: &str, queue_name: &str, app_name: &str) -> Result<Self> {
        let redis_client = Client::open(redis_url).map_err(|e| {
            error!("Failed to connect to Redis at {}: {}", redis_url, e);
            Error::Redis(format!("Redis connection error: {}", e))
        })?;

        let prefix = format!("{}:queue", app_name);

        Ok(Self {
            redis_client,
            main_queue_key: format!("{}:{}:main", prefix, queue_name),
            processing_queue_key: format!("{}:{}:processing", prefix, queue_name),
            failed_queue_key: format!("{}:{}:failed", prefix, queue_name),
            metadata_prefix: format!("{}:metadata", prefix),
            app_name: app_name.to_string(),
            _phantom: PhantomData,
        })
    }

    /// Get a connection to Redis
    fn get_connection(&self) -> Result<Connection> {
        self.redis_client.get_connection().map_err(|e| {
            error!("Failed to get Redis connection: {}", e);
            Error::Redis(format!("Redis connection error: {}", e))
        })
    }

    /// Enqueue an operation
    pub fn enqueue(&self, item: T) -> Result<String> {
        let operation_id = item.operation_id();
        let serialized =
            serde_json::to_string(&item).map_err(|e| Error::Serialization(e.to_string()))?;

        let mut conn = self.get_connection()?;
        let mut pipe = redis::pipe();

        // Add to the main queue
        pipe.cmd("LPUSH").arg(&self.main_queue_key).arg(&serialized);

        // Store metadata
        let metadata_key = format!("{}:{}", self.metadata_prefix, operation_id);
        pipe.cmd("HSET")
            .arg(&metadata_key)
            .arg("status")
            .arg("pending")
            .arg("token_id")
            .arg(item.token_id())
            .arg("created_at")
            .arg(Utc::now().to_rfc3339())
            .arg("attempts")
            .arg(0);

        // Set expiration for metadata
        pipe.cmd("EXPIRE").arg(&metadata_key).arg(METADATA_TTL_SECS);

        // Execute the pipeline
        let _: () = pipe.query(&mut conn).map_err(|e| {
            error!("Failed to enqueue item {}: {}", operation_id, e);
            Error::Redis(format!("Redis operation error: {}", e))
        })?;

        debug!(
            "Enqueued item {} for token {}",
            operation_id,
            item.token_id()
        );
        Ok(operation_id)
    }

    /// Helper method to remove an item from the processing queue
    fn remove_from_processing_queue(&self, item: &T) -> Result<bool> {
        let operation_id = item.operation_id();
        let serialized_item =
            serde_json::to_string(item).map_err(|e| Error::Serialization(e.to_string()))?;

        let mut conn = self.get_connection()?;
        let removed: i32 = redis::cmd("LREM")
            .arg(&self.processing_queue_key)
            .arg(1) // Remove only one occurrence
            .arg(&serialized_item)
            .query(&mut conn)
            .map_err(|e| {
                error!(
                    "Failed to remove item {} from processing queue: {}",
                    operation_id, e
                );
                Error::Redis(format!("Redis operation error: {}", e))
            })?;

        if removed == 0 {
            warn!("Item {} was not found in processing queue", operation_id);
        }

        Ok(removed > 0)
    }

    /// Dequeue a batch of operations for processing
    pub fn dequeue_batch(&self, batch_size: usize) -> Result<Vec<T>> {
        let mut conn = self.get_connection()?;
        let now = Utc::now();
        let mut result = Vec::new();

        for _ in 0..batch_size {
            // Move one item from main queue to processing queue
            let pop_result: Option<String> = redis::cmd("RPOPLPUSH")
                .arg(&self.main_queue_key)
                .arg(&self.processing_queue_key)
                .query(&mut conn)
                .map_err(|e| {
                    error!("Failed to move item from main to processing queue: {}", e);
                    Error::Redis(format!("Redis operation error: {}", e))
                })?;

            if let Some(serialized) = pop_result {
                // Parse the item
                let item: T = match serde_json::from_str(&serialized) {
                    Ok(item) => item,
                    Err(e) => {
                        error!("Failed to deserialize queue item: {}", e);
                        // Skip this item and continue
                        continue;
                    }
                };

                let operation_id = item.operation_id();

                // Update metadata
                let metadata_key = format!("{}:{}", self.metadata_prefix, operation_id);

                // Get current attempts
                let attempts: usize = redis::cmd("HGET")
                    .arg(&metadata_key)
                    .arg("attempts")
                    .query(&mut conn)
                    .unwrap_or(0);

                let mut pipe = redis::pipe();
                pipe.cmd("HSET")
                    .arg(&metadata_key)
                    .arg("status")
                    .arg("processing")
                    .arg("processing_started")
                    .arg(now.to_rfc3339())
                    .arg("attempts")
                    .arg(attempts + 1);

                let _: () = pipe.query(&mut conn).map_err(|e| {
                    error!("Failed to update metadata for {}: {}", operation_id, e);
                    Error::Redis(format!("Redis operation error: {}", e))
                })?;

                result.push(item);
            } else {
                // No more items in the queue
                break;
            }
        }

        // Only log when actually dequeuing items
        if !result.is_empty() {
            info!("Dequeued {} items for processing", result.len());
        } else {
            trace!("Queue poll: no items found to process"); // Lower level log for empty polls
        }

        Ok(result)
    }

    /// Mark an operation as completed
    pub fn mark_completed(&self, item: &T) -> Result<()> {
        let operation_id = item.operation_id();

        // Remove from processing queue
        self.remove_from_processing_queue(item)?;

        // Update metadata
        let metadata_key = format!("{}:{}", self.metadata_prefix, operation_id);
        let mut conn = self.get_connection()?;
        let mut pipe = redis::pipe();
        pipe.cmd("HSET")
            .arg(&metadata_key)
            .arg("status")
            .arg("completed")
            .arg("completed_at")
            .arg(Utc::now().to_rfc3339());

        // Set shorter expiration for completed items
        pipe.cmd("EXPIRE")
            .arg(&metadata_key)
            .arg(COMPLETED_TTL_SECS);

        let _: () = pipe.query(&mut conn).map_err(|e| {
            error!(
                "Failed to update metadata for completed item {}: {}",
                operation_id, e
            );
            Error::Redis(format!("Redis operation error: {}", e))
        })?;

        debug!("Marked item {} as completed", operation_id);
        Ok(())
    }

    /// Mark an operation as failed
    pub fn mark_failed(&self, item: &T, error_message: &str) -> Result<()> {
        let operation_id = item.operation_id();

        // Remove from processing queue
        self.remove_from_processing_queue(item)?;

        let mut conn = self.get_connection()?;
        let serialized_item =
            serde_json::to_string(item).map_err(|e| Error::Serialization(e.to_string()))?;

        // Check current attempts
        let metadata_key = format!("{}:{}", self.metadata_prefix, operation_id);
        let attempts: usize = redis::cmd("HGET")
            .arg(&metadata_key)
            .arg("attempts")
            .query(&mut conn)
            .unwrap_or(0);

        // Check if this is a foreign key constraint error, which indicates a missing token
        let is_foreign_key_error = error_message.contains("FOREIGN KEY constraint failed");
        let token_id = item.token_id();

        // If this is a FK error, check if we've already failed on this token multiple times
        if is_foreign_key_error {
            // Use Redis to track token-specific failures
            let token_failure_key = format!("{}:token_failures:{}", self.app_name, token_id);
            let token_failures: usize = redis::cmd("GET")
                .arg(&token_failure_key)
                .query(&mut conn)
                .unwrap_or(0);

            // If we've already failed multiple times on this token, move to failed queue immediately
            if token_failures >= TOKEN_FAILURE_THRESHOLD {
                warn!(
                    "Token {} has failed {} times, abandoning retry",
                    token_id,
                    token_failures + 1
                );

                // Update token failures count
                if let Err(e) = redis::cmd("SET")
                    .arg(&token_failure_key)
                    .arg(token_failures + 1)
                    .arg("EX")
                    .arg(METADATA_TTL_SECS)
                    .query::<()>(&mut conn)
                {
                    warn!(
                        "Failed to update token failure counter for {}: {}",
                        token_id, e
                    );
                }

                // Put in failed queue
                let mut pipe = redis::pipe();
                pipe.cmd("LPUSH")
                    .arg(&self.failed_queue_key)
                    .arg(&serialized_item);

                // Update metadata
                pipe.cmd("HSET")
                    .arg(&metadata_key)
                    .arg("status")
                    .arg("failed")
                    .arg("failed_at")
                    .arg(Utc::now().to_rfc3339())
                    .arg("error")
                    .arg(format!(
                        "Abandoned after multiple FK failures: {}",
                        error_message
                    ));

                let _: () = pipe.query(&mut conn).map_err(|e| {
                    error!("Failed to update failed item {}: {}", operation_id, e);
                    Error::Redis(format!("Redis operation error: {}", e))
                })?;

                error!(
                    "Item {} abandoned due to repeated foreign key failures for token {}",
                    operation_id, token_id
                );

                return Ok(());
            }

            // Increment token failure count
            if let Err(e) = redis::cmd("SET")
                .arg(&token_failure_key)
                .arg(token_failures + 1)
                .arg("EX")
                .arg(METADATA_TTL_SECS)
                .query::<()>(&mut conn)
            {
                warn!(
                    "Failed to update token failure counter for {}: {}",
                    token_id, e
                );
            }
        }

        let mut pipe = redis::pipe();

        if attempts >= MAX_RETRY_ATTEMPTS {
            // If we've exceeded max retries, move to failed queue
            pipe.cmd("LPUSH")
                .arg(&self.failed_queue_key)
                .arg(&serialized_item);

            // Update metadata to show permanently failed
            pipe.cmd("HSET")
                .arg(&metadata_key)
                .arg("status")
                .arg("failed")
                .arg("failed_at")
                .arg(Utc::now().to_rfc3339())
                .arg("error")
                .arg(error_message);

            error!(
                "Item {} permanently failed after {} attempts: {}",
                operation_id, attempts, error_message
            );
        } else {
            // If we haven't reached max retries, put back in main queue
            // Calculate backoff time: 2^attempts seconds (exponential backoff)
            let backoff_seconds =
                (2_u32.pow(attempts as u32)).min(MAX_BACKOFF_SECS as u32) as usize;

            // Use a delayed queue using a sorted set with score = current time + backoff
            let now = Utc::now().timestamp() as f64;
            let score = now + backoff_seconds as f64;

            // Use a delayed queue (sorted set)
            let delayed_queue = format!("{}:delayed", self.main_queue_key);

            pipe.cmd("ZADD")
                .arg(&delayed_queue)
                .arg(score)
                .arg(&serialized_item);

            // Update metadata
            pipe.cmd("HSET")
                .arg(&metadata_key)
                .arg("status")
                .arg("delayed")
                .arg("retry_after")
                .arg(Utc::now().to_rfc3339())
                .arg("error")
                .arg(error_message);

            debug!(
                "Item {} scheduled for retry in {} seconds: {}",
                operation_id, backoff_seconds, error_message
            );
        }

        // Execute the pipeline
        let _: () = pipe.query(&mut conn).map_err(|e| {
            error!("Failed to update failed item {}: {}", operation_id, e);
            Error::Redis(format!("Redis operation error: {}", e))
        })?;

        Ok(())
    }

    /// Process delayed items that are ready
    pub fn process_delayed_items(&self) -> Result<usize> {
        let mut conn = self.get_connection()?;
        let now = Utc::now().timestamp() as f64;
        let delayed_queue = format!("{}:delayed", self.main_queue_key);

        // Get items that are ready to be processed
        let ready_items: Vec<String> = redis::cmd("ZRANGEBYSCORE")
            .arg(&delayed_queue)
            .arg(0)
            .arg(now)
            .query(&mut conn)
            .map_err(|e| {
                error!("Failed to get ready items from delayed queue: {}", e);
                Error::Redis(format!("Redis operation error: {}", e))
            })?;

        if ready_items.is_empty() {
            return Ok(0);
        }

        let mut pipe = redis::pipe();

        // Move items from delayed queue to main queue
        for item in &ready_items {
            // Add to main queue
            pipe.cmd("LPUSH").arg(&self.main_queue_key).arg(item);

            // Remove from delayed queue
            pipe.cmd("ZREM").arg(&delayed_queue).arg(item);

            // Parse the item to get operation_id
            if let Ok(parsed_item) = serde_json::from_str::<T>(item) {
                let operation_id = parsed_item.operation_id();
                let metadata_key = format!("{}:{}", self.metadata_prefix, operation_id);

                // Update metadata
                pipe.cmd("HSET")
                    .arg(&metadata_key)
                    .arg("status")
                    .arg("pending");
            }
        }

        let _: () = pipe.query(&mut conn).map_err(|e| {
            error!("Failed to move delayed items to main queue: {}", e);
            Error::Redis(format!("Redis operation error: {}", e))
        })?;

        info!(
            "Moved {} items from delayed queue to main queue",
            ready_items.len()
        );
        Ok(ready_items.len())
    }

    /// Handle stuck operations (operations that have been processing for too long)
    pub fn handle_stuck_operations(&self, stuck_timeout_seconds: u64) -> Result<usize> {
        let mut conn = self.get_connection()?;
        let threshold =
            (Utc::now() - chrono::Duration::seconds(stuck_timeout_seconds as i64)).to_rfc3339();

        // Get all items in processing queue
        let processing_items: Vec<String> = redis::cmd("LRANGE")
            .arg(&self.processing_queue_key)
            .arg(0)
            .arg(-1)
            .query(&mut conn)
            .map_err(|e| {
                error!("Failed to get items from processing queue: {}", e);
                Error::Redis(format!("Redis operation error: {}", e))
            })?;

        let mut stuck_count = 0;

        for serialized in processing_items {
            // Parse the item
            let item: T = match serde_json::from_str(&serialized) {
                Ok(item) => item,
                Err(e) => {
                    error!("Failed to deserialize queue item: {}", e);
                    continue;
                }
            };

            let operation_id = item.operation_id();
            let metadata_key = format!("{}:{}", self.metadata_prefix, operation_id);

            // Check if item is stuck
            let processing_started: Option<String> = redis::cmd("HGET")
                .arg(&metadata_key)
                .arg("processing_started")
                .query(&mut conn)
                .ok();

            if let Some(started) = processing_started {
                if started < threshold {
                    // This item has been processing for too long
                    warn!(
                        "Found stuck item {}, processing since {}",
                        operation_id, started
                    );

                    // Mark as failed and move back to main queue
                    if let Err(e) = self.mark_failed(&item, "Operation timed out") {
                        error!(
                            "Failed to mark stuck item {} as failed: {}",
                            operation_id, e
                        );
                        continue;
                    }

                    stuck_count += 1;
                }
            }
        }

        if stuck_count > 0 {
            info!("Recovered {} stuck operations", stuck_count);
        }

        Ok(stuck_count)
    }

    /// Get queue statistics
    pub fn get_queue_stats(&self) -> Result<QueueStats> {
        let mut conn = self.get_connection()?;

        let main_queue_length: usize = redis::cmd("LLEN")
            .arg(&self.main_queue_key)
            .query(&mut conn)
            .map_err(|e| {
                error!("Failed to get main queue length: {}", e);
                Error::Redis(format!("Redis operation error: {}", e))
            })?;

        let processing_queue_length: usize = redis::cmd("LLEN")
            .arg(&self.processing_queue_key)
            .query(&mut conn)
            .map_err(|e| {
                error!("Failed to get processing queue length: {}", e);
                Error::Redis(format!("Redis operation error: {}", e))
            })?;

        let failed_queue_length: usize = redis::cmd("LLEN")
            .arg(&self.failed_queue_key)
            .query(&mut conn)
            .map_err(|e| {
                error!("Failed to get failed queue length: {}", e);
                Error::Redis(format!("Redis operation error: {}", e))
            })?;

        let delayed_queue_length: usize = redis::cmd("ZCARD")
            .arg(format!("{}:delayed", self.main_queue_key))
            .query(&mut conn)
            .map_err(|e| {
                error!("Failed to get delayed queue length: {}", e);
                Error::Redis(format!("Redis operation error: {}", e))
            })?;

        Ok(QueueStats {
            queue_name: self.main_queue_key.clone(),
            pending: main_queue_length,
            processing: processing_queue_length,
            failed: failed_queue_length,
            delayed: delayed_queue_length,
        })
    }
}

/// Queue statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStats {
    pub queue_name: String,
    pub pending: usize,
    pub processing: usize,
    pub failed: usize,
    pub delayed: usize,
}

/// Position Update Queue
pub type PositionUpdateQueue = RedisQueue<PositionUpdateOperation>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::utils::normalization;
    use chrono::Utc;

    #[test]
    fn test_token_id_utils() {
        // Test create_token_id
        let token_id =
            normalization::create_token_id(1, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48")
                .unwrap();
        assert_eq!(token_id, "1:0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48");

        // Test parse_token_id
        let (chain_id, address) = normalization::parse_token_id(&token_id).unwrap();
        assert_eq!(chain_id, 1);
        assert_eq!(address, "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48");

        // Test token_id validation via parse_token_id
        assert!(normalization::parse_token_id(&token_id).is_ok());
        assert!(normalization::parse_token_id("invalid").is_err());

        // Test get_chain_name
        assert_eq!(normalization::get_chain_name(1), "ethereum");
        assert_eq!(normalization::get_chain_name(137), "polygon");
        assert_eq!(normalization::get_chain_name(999), "unknown");

        // Test format_token_display
        let display = normalization::format_token_display(&token_id, "USDC").unwrap();
        assert_eq!(display, "USDC (ethereum)");
    }

    #[test]
    fn test_position_update_operation() {
        let token_id = "1:0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
        let op = PositionUpdateOperation::new(token_id, 1.0, 100.0, Utc::now());

        assert_eq!(op.token_id, token_id);
        assert_eq!(op.price, 1.0);
        assert_eq!(op.pnl, 100.0);

        // Test log_name returns shortened address (preserves original case)
        let log_name = op.log_name();
        // Should return "0xA0b869...06eB48" from "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
        assert_eq!(log_name, "0xA0b869...06eB48");
    }

    #[test]
    fn test_validation_failures() {
        // Test invalid contract address
        assert!(normalization::create_token_id(1, "invalid_address").is_err());

        // Test invalid token_id format
        assert!(normalization::parse_token_id("invalid").is_err());
        assert!(normalization::parse_token_id("1:2:3").is_err());
    }
}
