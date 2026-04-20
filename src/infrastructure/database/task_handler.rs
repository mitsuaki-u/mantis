use crate::infrastructure::database::queue::{PositionUpdateOperation, PositionUpdateQueue};
use crate::infrastructure::database::repositories::PositionRepository;
use crate::infrastructure::errors::Result;
use log::{debug, error, warn};

/// Handler for processing batched database operations from Redis queues
pub struct DatabaseTaskHandler;

impl DatabaseTaskHandler {
    /// Process a batch of position updates from the queue
    pub async fn process_position_batch(
        position_repo: PositionRepository,
        queue: PositionUpdateQueue,
        batch_size: usize,
    ) -> Result<usize> {
        debug!("Processing position batch with size {}", batch_size);

        let batch = match queue.dequeue_batch(batch_size) {
            Ok(b) => b,
            Err(e) => {
                error!("Failed to dequeue position batch: {}", e);
                return Err(e);
            }
        };

        if batch.is_empty() {
            return Ok(0);
        }

        debug!("Dequeued {} position updates to process", batch.len());
        let mut processed = 0;

        // Process each position update
        for operation in batch {
            let token_id = operation.token_id.clone();
            if let Err(e) =
                Self::process_single_position_update(&position_repo, &token_id, &operation, &queue)
                    .await
            {
                error!("Failed to process position update for {}: {}", token_id, e);
                continue;
            }
            processed += 1;
        }

        debug!("Successfully processed {} position updates", processed);
        Ok(processed)
    }

    /// Process a single position update
    async fn process_single_position_update(
        position_repo: &PositionRepository,
        token_id: &str,
        position_update: &PositionUpdateOperation,
        queue: &PositionUpdateQueue,
    ) -> Result<()> {
        debug!(
            "Processing position update for {} at price ${:.4}",
            token_id, position_update.price
        );

        // Fetch existing position to get highest_price
        let highest_price = match position_repo.get_position_by_token_id(token_id).await {
            Ok(Some((_id, pos))) => pos.highest_price.max(position_update.price),
            Ok(None) => {
                warn!(
                    "Position not found for update: {}. Using current price as highest.",
                    token_id
                );
                position_update.price
            }
            Err(e) => {
                error!(
                    "Error fetching position for update {}: {}. Using current price.",
                    token_id, e
                );
                position_update.price
            }
        };

        if let Err(e) = position_repo
            .update_position(token_id, position_update.price, highest_price)
            .await
        {
            error!("Failed to store position update for {}: {}", token_id, e);
            if let Err(mark_err) = queue.mark_failed(position_update, &e.to_string()) {
                error!("Failed to mark position update as failed: {}", mark_err);
            }
            return Err(e);
        }
        if let Err(e) = queue.mark_completed(position_update) {
            warn!(
                "Failed to mark position update as completed for {}: {}",
                token_id, e
            );
        }
        Ok(())
    }
}
