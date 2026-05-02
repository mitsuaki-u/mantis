//! Redis queue management for DatabaseActor operations.
//!
//! Position updates (price/PnL changes) are queued for batch processing since they're
//! high-frequency telemetry data. BUY/SELL operations use ExecutionEvent::OrderExecuted
//! with immediate writes for consistency and proper event-driven architecture.

use super::DatabaseActor;
use crate::application::errors::{Error, Result};
use crate::config::Config;
use crate::infrastructure::database::queue::{PositionUpdateOperation, PositionUpdateQueue};
use chrono::{DateTime, Utc};
use log::{debug, error, info, warn};
use std::sync::Arc;

/// Create Redis queue for position update batching
pub fn create_queue(config: &Arc<Config>) -> Option<PositionUpdateQueue> {
    // Check if cache is enabled first
    if !config.cache.enabled {
        info!("⚠️ Redis cache disabled in config, position updates will be processed directly");
        return None;
    }

    if let Some(redis_url) = &config.cache.redis_url {
        let app_name = "mantis";

        match PositionUpdateQueue::new(redis_url, "position_updates", app_name) {
            Ok(queue) => {
                info!(
                    "✅ Position update queue initialized with Redis at {}",
                    redis_url
                );
                Some(queue)
            }
            Err(e) => {
                error!("❌ Failed to initialize position update queue: {}", e);
                None
            }
        }
    } else {
        warn!("⚠️ Redis URL not configured, position updates will be processed directly");
        None
    }
}

pub async fn queue_position_update(
    actor: &DatabaseActor,
    token_id: String,
    price: f64,
    pnl: f64,
    timestamp: DateTime<Utc>,
) -> Result<()> {
    if let Some(ref queue) = actor.position_queue {
        // Redis enabled: Queue for batch processing
        let operation = PositionUpdateOperation::new(&token_id, price, pnl, timestamp);

        match queue.enqueue(operation.clone()) {
            Ok(_) => {
                debug!("Enqueued position update for {} in Redis queue", token_id);
                Ok(())
            }
            Err(e) => {
                error!("Failed to queue position update for {}: {}", token_id, e);
                Err(Error::QueueOperation(format!(
                    "Failed to enqueue position update for {}: {}",
                    token_id, e
                )))
            }
        }
    } else {
        // Redis disabled: Write directly to database
        debug!(
            "Redis disabled, writing position update for {} directly to database",
            token_id
        );

        let repo = actor.repo_factory.position_repository();

        // Fetch existing position to get highest_price (same logic as batch processor)
        let highest_price = match repo.get_position_by_token_id(&token_id).await {
            Ok(Some((_id, pos))) => pos.highest_price.max(price),
            Ok(None) => {
                warn!(
                    "Position not found for update: {}. Using current price as highest.",
                    token_id
                );
                price
            }
            Err(e) => {
                error!(
                    "Error fetching position for update {}: {}. Using current price.",
                    token_id, e
                );
                price
            }
        };

        repo.update_position(&token_id, price, highest_price).await
    }
}
