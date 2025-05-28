use crate::core::error::{Error, Result};
use chrono::{DateTime, Utc};
use log::{debug, error, info, trace, warn};
use redis::{Client, Connection};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::marker::PhantomData;
use uuid;

/// Maximum number of retry attempts for a failed operation
const MAX_RETRY_ATTEMPTS: usize = 5;

/// Maximum batch size for processing
const DEFAULT_BATCH_SIZE: usize = 50;

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

impl QueueItem for PositionUpdateOperation {
    fn operation_id(&self) -> String {
        self.operation_id.clone()
    }

    fn token_id(&self) -> &str {
        &self.token_id
    }
}

/// Trade operation for the queue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeOperation {
    pub canonical_token_id: String,
    pub provider_token_id: String,
    pub price: f64,
    pub size: f64,
    pub is_buy: bool,
    pub timestamp: DateTime<Utc>,
    pub position_id: Option<i64>,
    pub is_position_close: bool,
    pub entry_price: Option<f64>,
    pub entry_time: Option<DateTime<Utc>>,
    pub delete_position: bool,
    pub operation_id: String,
    pub attempts: usize,
}

impl QueueItem for TradeOperation {
    fn operation_id(&self) -> String {
        self.operation_id.clone()
    }

    fn token_id(&self) -> &str {
        &self.canonical_token_id
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

    /// Get an async connection to Redis
    async fn get_async_connection(&self) -> Result<redis::aio::MultiplexedConnection> {
        // Always create a fresh connection instead of trying to clone
        self.redis_client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| {
                error!("Failed to get Redis async connection: {}", e);
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

        // Set expiration for metadata (24 hours)
        pipe.cmd("EXPIRE").arg(&metadata_key).arg(86400);

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

    /// Enqueue an operation asynchronously
    pub async fn enqueue_async(&self, item: T) -> Result<String> {
        let operation_id = item.operation_id();
        let serialized =
            serde_json::to_string(&item).map_err(|e| Error::Serialization(e.to_string()))?;

        let mut conn = self.get_async_connection().await?;
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

        // Set expiration for metadata (24 hours)
        pipe.cmd("EXPIRE").arg(&metadata_key).arg(86400);

        // Execute the pipeline
        let _: () = pipe.query_async(&mut conn).await.map_err(|e| {
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

    /// Claim a batch of operations for processing
    pub fn claim_batch(&self, batch_size: usize) -> Result<Vec<T>> {
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
                let attempts: usize = match redis::cmd("HGET")
                    .arg(&metadata_key)
                    .arg("attempts")
                    .query(&mut conn)
                {
                    Ok(a) => a,
                    Err(_) => 0, // Default to 0 if not found
                };

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

        // Only log when actually claiming items
        if !result.is_empty() {
            info!("Claimed {} items for processing", result.len());
        } else {
            trace!("Queue poll: no items found to process"); // Lower level log for empty polls
        }

        Ok(result)
    }

    /// Claim a batch of operations for processing asynchronously
    pub async fn claim_batch_async(&self, batch_size: usize) -> Result<Vec<T>> {
        let mut conn = self.get_async_connection().await?;
        let now = Utc::now();
        let mut result = Vec::new();

        for _ in 0..batch_size {
            // Move one item from main queue to processing queue
            let pop_result: Option<String> = redis::cmd("RPOPLPUSH")
                .arg(&self.main_queue_key)
                .arg(&self.processing_queue_key)
                .query_async(&mut conn)
                .await
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
                let attempts: usize = match redis::cmd("HGET")
                    .arg(&metadata_key)
                    .arg("attempts")
                    .query_async(&mut conn)
                    .await
                {
                    Ok(a) => a,
                    Err(_) => 0, // Default to 0 if not found
                };

                let mut pipe = redis::pipe();
                pipe.cmd("HSET")
                    .arg(&metadata_key)
                    .arg("status")
                    .arg("processing")
                    .arg("processing_started")
                    .arg(now.to_rfc3339())
                    .arg("attempts")
                    .arg(attempts + 1);

                let _: () = pipe.query_async(&mut conn).await.map_err(|e| {
                    error!("Failed to update metadata for {}: {}", operation_id, e);
                    Error::Redis(format!("Redis operation error: {}", e))
                })?;

                result.push(item);
            } else {
                // No more items in the queue
                break;
            }
        }

        // Only log when actually claiming items
        if !result.is_empty() {
            info!("Claimed {} items for processing", result.len());
        } else {
            trace!("Queue poll: no items found to process"); // Lower level log for empty polls
        }

        Ok(result)
    }

    /// Mark an operation as completed
    pub fn mark_completed(&self, item: &T) -> Result<()> {
        let operation_id = item.operation_id();
        let mut conn = self.get_connection()?;

        // First, check if the item is in the processing queue
        let serialized_item =
            serde_json::to_string(item).map_err(|e| Error::Serialization(e.to_string()))?;

        // Remove from processing queue
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

        // Update metadata
        let metadata_key = format!("{}:{}", self.metadata_prefix, operation_id);
        let mut pipe = redis::pipe();
        pipe.cmd("HSET")
            .arg(&metadata_key)
            .arg("status")
            .arg("completed")
            .arg("completed_at")
            .arg(Utc::now().to_rfc3339());

        // Set shorter expiration for completed items (1 hour)
        pipe.cmd("EXPIRE").arg(&metadata_key).arg(3600);

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

    /// Mark an operation as completed asynchronously
    pub async fn mark_completed_async(&self, item: &T) -> Result<()> {
        let operation_id = item.operation_id();
        let mut conn = self.get_async_connection().await?;

        // First, check if the item is in the processing queue
        let serialized_item =
            serde_json::to_string(item).map_err(|e| Error::Serialization(e.to_string()))?;

        // Remove from processing queue
        let removed: i32 = redis::cmd("LREM")
            .arg(&self.processing_queue_key)
            .arg(1) // Remove only one occurrence
            .arg(&serialized_item)
            .query_async(&mut conn)
            .await
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

        // Update metadata
        let metadata_key = format!("{}:{}", self.metadata_prefix, operation_id);
        let mut pipe = redis::pipe();
        pipe.cmd("HSET")
            .arg(&metadata_key)
            .arg("status")
            .arg("completed")
            .arg("completed_at")
            .arg(Utc::now().to_rfc3339());

        pipe.cmd("EXPIRE").arg(&metadata_key).arg(86400 * 7); // Keep failed metadata for 7 days

        let _: () = pipe.query_async(&mut conn).await.map_err(|e| {
            error!("Failed to update metadata for {}: {}", operation_id, e);
            Error::Redis(format!("Redis operation error: {}", e))
        })?;

        debug!("Marked item {} as completed", operation_id);
        Ok(())
    }

    /// Mark an operation as failed
    pub fn mark_failed(&self, item: &T, error_message: &str) -> Result<()> {
        let operation_id = item.operation_id();
        let mut conn = self.get_connection()?;

        // First, remove from processing queue
        let serialized_item =
            serde_json::to_string(item).map_err(|e| Error::Serialization(e.to_string()))?;

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

            // If we've already failed 3+ times on this token, move to failed queue immediately
            if token_failures >= 2 {
                warn!(
                    "Token {} has failed {} times, abandoning retry",
                    token_id,
                    token_failures + 1
                );

                // Update token failures count
                let _: () = redis::cmd("SET")
                    .arg(&token_failure_key)
                    .arg(token_failures + 1)
                    .arg("EX")
                    .arg(3600) // 1 hour expiration
                    .query(&mut conn)
                    .unwrap_or(());

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
            let _: () = redis::cmd("SET")
                .arg(&token_failure_key)
                .arg(token_failures + 1)
                .arg("EX")
                .arg(3600) // 1 hour expiration
                .query(&mut conn)
                .unwrap_or(());
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
            let backoff_seconds = (2_u32.pow(attempts as u32)).min(300) as usize; // Max 5 minutes

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

    /// Mark an operation as failed asynchronously
    pub async fn mark_failed_async(&self, item: &T, error_message: &str) -> Result<()> {
        let operation_id = item.operation_id();
        let mut conn = self.get_async_connection().await?;

        // First, remove from processing queue
        let serialized_item =
            serde_json::to_string(item).map_err(|e| Error::Serialization(e.to_string()))?;

        let removed: i32 = redis::cmd("LREM")
            .arg(&self.processing_queue_key)
            .arg(1) // Remove only one occurrence
            .arg(&serialized_item)
            .query_async(&mut conn)
            .await
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

        // Check current attempts
        let metadata_key = format!("{}:{}", self.metadata_prefix, operation_id);
        let attempts: usize = match redis::cmd("HGET")
            .arg(&metadata_key)
            .arg("attempts")
            .query_async(&mut conn)
            .await
        {
            Ok(a) => a,
            Err(_) => 1, // Default to 1 if not found
        };

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
                .arg(error_message)
                .arg("attempts")
                .arg(attempts);

            error!(
                "Item {} permanently failed after {} attempts: {}",
                operation_id, attempts, error_message
            );
        } else {
            // If we haven't reached max retries, put back in main queue
            pipe.cmd("LPUSH")
                .arg(&self.main_queue_key)
                .arg(&serialized_item);

            // Update metadata for retry
            pipe.cmd("HSET")
                .arg(&metadata_key)
                .arg("status")
                .arg("pending")
                .arg("last_error")
                .arg(error_message)
                .arg("attempts")
                .arg(attempts)
                .arg("retry_at")
                .arg(Utc::now().to_rfc3339());

            warn!(
                "Item {} failed, will retry (attempt {}/{}): {}",
                operation_id, attempts, MAX_RETRY_ATTEMPTS, error_message
            );
        }

        // Execute the pipeline
        let _: () = pipe.query_async(&mut conn).await.map_err(|e| {
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

    /// Get queue statistics asynchronously
    pub async fn get_metrics_async(&self) -> Result<QueueStats> {
        // Use tokio's spawn_blocking to run the synchronous operation in a thread pool
        let queue_clone = self.clone();
        let stats = tokio::task::spawn_blocking(move || queue_clone.get_queue_stats())
            .await
            .map_err(|e| {
                error!("Failed to get queue metrics in background task: {}", e);
                Error::Internal(format!("Task join error: {}", e))
            })??;

        Ok(stats)
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

/// Trade Operation Queue
pub type TradeOperationQueue = RedisQueue<TradeOperation>;

pub const POSITION_QUEUE_KEY: &str = "honeybadger:queue:positions";
pub const TRADE_QUEUE_KEY: &str = "honeybadger:queue:trades";

/// Configuration for persistent Redis queue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisQueueConfig {
    pub redis_url: String,
    pub position_batch_size: usize,
    pub trade_batch_size: usize,
    pub flush_interval_ms: u64,
    pub use_async: bool,
}

impl Default for RedisQueueConfig {
    fn default() -> Self {
        Self {
            redis_url: "redis://localhost:6379".to_string(),
            position_batch_size: DEFAULT_BATCH_SIZE,
            trade_batch_size: DEFAULT_BATCH_SIZE,
            flush_interval_ms: 1000, // 1 second
            use_async: true,
        }
    }
}
