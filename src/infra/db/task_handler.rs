use crate::core::error::{Error, Result};
use crate::infra::db::queue::{PositionUpdateOperation, TradeOperation};
use crate::infra::db::queue::{PositionUpdateQueue, TradeOperationQueue};
use crate::infra::db::repositories::position::RecordCloseArgs;
use crate::infra::db::repositories::{PositionRepository, TokenRepository, TradeRepository};
use crate::infra::db::Database;
use log::{debug, error, info, warn};
use std::sync::Arc;
use tokio;
// use tokio::task::JoinError;

/// Handler for database operations that should be run in a blocking context
/// This struct contains only synchronous methods for database operations
/// that will be executed on a blocking thread pool
pub struct DatabaseTaskHandler;

impl DatabaseTaskHandler {
    /// Process a batch of position updates from the queue (Now requires PositionRepository)
    pub async fn process_position_batch(
        position_repo: PositionRepository,
        queue: PositionUpdateQueue,
        batch_size: usize,
    ) -> Result<usize> {
        debug!("Processing position batch with size {}", batch_size);

        let batch = match queue.claim_batch(batch_size) {
            Ok(b) => b,
            Err(e) => {
                error!("Failed to claim position batch: {}", e);
                return Err(e);
            }
        };

        if batch.is_empty() {
            return Ok(0);
        }

        debug!("Claimed {} position updates to process", batch.len());
        let mut processed = 0;

        // Process each position update
        for operation in batch {
            let token_id = operation.token_id.clone();
            // Call the async version
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

    /// Process a single position update (Now async)
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

        // Call the async update_position method
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

    /// Process a batch of trade operations from the queue (Now async)
    pub async fn process_trade_batch(
        trade_repo: TradeRepository,
        queue: TradeOperationQueue,
        position_repo: Option<PositionRepository>,
        batch_size: usize,
    ) -> Result<usize> {
        debug!("Processing trade batch with size {}", batch_size);

        let batch = match queue.claim_batch(batch_size) {
            Ok(b) => b,
            Err(e) => {
                error!("Failed to claim trade batch: {}", e);
                return Err(e);
            }
        };

        if batch.is_empty() {
            return Ok(0);
        }

        debug!("Claimed {} trade operations to process", batch.len());
        let mut processed = 0;

        // Process each trade operation
        for operation in batch {
            let token_id = operation.canonical_token_id.clone();
            // Call the async version
            if let Err(e) = Self::process_single_trade_operation(
                &trade_repo,
                &token_id,
                &operation,
                &queue,
                position_repo.as_ref(),
            )
            .await
            {
                error!("Failed to process trade operation for {}: {}", token_id, e);
                continue;
            }
            processed += 1;
        }

        debug!("Successfully processed {} trade operations", processed);
        Ok(processed)
    }

    /// Process a single trade operation (already async)
    async fn process_single_trade_operation(
        trade_repo: &TradeRepository,
        token_id: &str,
        trade_op: &TradeOperation,
        queue: &TradeOperationQueue,
        position_repo: Option<&PositionRepository>,
    ) -> Result<()> {
        match (trade_op.is_position_close, trade_op.position_id) {
            (true, _) => {
                debug!(
                    "Processing position close for {} at price ${:.4}",
                    token_id, trade_op.price
                );
                let position_repo = position_repo.ok_or_else(|| {
                    Error::Other("Position repository required for close op".to_string())
                })?;

                // Fetch position_id if not present in operation (might be needed)
                // For now, assume entry_price/entry_time are sufficient identifiers if passed
                let entry_price = trade_op.entry_price.ok_or_else(|| {
                    Error::InvalidInput("Missing entry_price for close op".to_string())
                })?;
                let entry_time = trade_op.entry_time.ok_or_else(|| {
                    Error::InvalidInput("Missing entry_time for close op".to_string())
                })?;

                // Fetch the position_id based on token_id
                let position_id = match position_repo.get_position_by_token_id(token_id).await {
                    Ok(Some((id, _pos))) => id,
                    Ok(None) => {
                        warn!("Could not find open position for {} to close.", token_id);
                        queue.mark_completed(trade_op)?;
                        return Ok(());
                    }
                    Err(e) => {
                        error!("DB error fetching position ID for {}: {}", token_id, e);
                        queue.mark_failed(trade_op, &e.to_string())?;
                        return Err(e);
                    }
                };

                // Replace record_position_close with record_position_close_with_trade
                if let Err(e) = position_repo
                    .record_position_close_with_trade(
                        position_id,
                        RecordCloseArgs {
                            token_id,
                            exit_price: trade_op.price,
                            size: trade_op.size,
                            entry_price,
                            entry_time,
                            exit_time: trade_op.timestamp,
                        },
                    )
                    .await
                {
                    error!("Failed to close position for {}: {}", token_id, e);
                    if let Err(mark_err) = queue.mark_failed(trade_op, &e.to_string()) {
                        error!("Failed to mark position close as failed: {}", mark_err);
                    }
                    return Err(e);
                }
            }
            (false, Some(pos_id)) => {
                debug!(
                    "Processing trade with position link for {} at price ${:.4}",
                    token_id, trade_op.price
                );
                if let Err(e) = trade_repo
                    .record_trade_with_position(
                        token_id,
                        trade_op.price,
                        trade_op.size,
                        trade_op.is_buy,
                        pos_id,
                    )
                    .await
                {
                    error!("Failed to record trade with position {}: {}", pos_id, e);
                    if let Err(mark_err) = queue.mark_failed(trade_op, &e.to_string()) {
                        error!("Failed to mark trade as failed: {}", mark_err);
                    }
                    return Err(e);
                }
            }
            (false, None) => {
                debug!(
                    "Processing standalone trade for {} at price ${:.4}",
                    trade_op.canonical_token_id, trade_op.price
                );
                if let Err(e) = trade_repo
                    .record_trade(
                        &trade_op.canonical_token_id,
                        &trade_op.provider_token_id,
                        trade_op.price,
                        trade_op.size,
                        trade_op.is_buy,
                        trade_op.timestamp,
                    )
                    .await
                {
                    error!(
                        "Failed to record standalone trade for {}: {}",
                        trade_op.canonical_token_id, e
                    );
                    if let Err(mark_err) = queue.mark_failed(trade_op, &e.to_string()) {
                        error!("Failed to mark trade as failed: {}", mark_err);
                    }
                    return Err(e);
                }
            }
        }

        // Mark operation as completed if successful
        if let Err(e) = queue.mark_completed(trade_op) {
            warn!(
                "Failed to mark trade op as completed for {}: {}",
                token_id, e
            );
        }
        Ok(())
    }

    // --- Async Wrappers for Blocking Operations ---
    // These methods remain synchronous but are intended to be called within spawn_blocking

    // Note: Original functions were synchronous. Keeping them sync for now,
    // but they call async repo methods. This is wrong.
    // The correct approach is to make the calling functions (process_batch) async
    // and call the async repo methods directly with .await.
    // For now, commenting out the wrappers as they are incorrect.

    /*
    pub fn store_price_data_sync(
        token_repo: TokenRepository,
        token_id: String,
        price: f64,
        volume: f64
    ) -> Result<()> {
        // This needs to be async or run in a runtime
        // tokio::runtime::Runtime::new().unwrap().block_on(async {
        //     token_repo.store_price_data(&token_id, price, volume).await
        // })
        Err(Error::NotImplemented("store_price_data_sync needs rework for async".to_string()))
    }
    */

    pub async fn batch_store_price_data(
        token_repo: TokenRepository,
        price_data: Vec<(String, f64, f64)>,
    ) -> Result<()> {
        // This wrapper IS async, so it can call the repo method directly.
        token_repo.batch_store_price_data(&price_data).await
        // Original spawn_blocking logic removed as it was incorrect for async repo method
        // tokio::task::spawn_blocking(move || {
        //     token_repo.batch_store_price_data(&price_data) // This called sync version
        // })
        // .await.map_err(convert_join_error)? // Handle JoinError
    }

    pub async fn update_token_metadata(
        token_repo: TokenRepository,
        token_id: String,
        symbol: String,
        name: String, // Added name
    ) -> Result<()> {
        // This wrapper IS async
        token_repo
            .update_token_metadata(&token_id, &symbol, &name)
            .await
        // tokio::task::spawn_blocking(move || {
        //     token_repo.update_token_metadata(&token_id, &symbol)
        // })
        // .await.map_err(convert_join_error)?
    }

    /// Synchronously perform scheduled database maintenance (VACUUM ANALYZE)
    pub fn perform_scheduled_maintenance(db: Arc<Database>) -> Result<()> {
        info!("Performing scheduled database maintenance...");
        let start_time = std::time::Instant::now();

        // Maintenance needs to run in a blocking context if DB calls are sync
        // But our perform_maintenance IS async. We need to block on it.
        // Best practice: Call this from an async context where possible.
        // If called from sync context, need a runtime.

        // Create a temporary runtime to block on the async maintenance task
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| Error::Task(format!("Failed to create runtime for maintenance: {}", e)))?;

        rt.block_on(async {
            match db.perform_maintenance().await {
                Ok(_) => {
                    let duration = start_time.elapsed();
                    info!(
                        "✅ Database maintenance completed successfully in {:.2?}",
                        duration
                    );
                    Ok(())
                }
                Err(e) => {
                    error!("❌ Database maintenance failed: {}", e);
                    Err(e)
                }
            }
        })
    }

    /// Asynchronously check connection pool health
    pub async fn check_connection_pool(db: Arc<Database>) -> Result<(bool, String)> {
        let pool_status = db.check_pool_health().await;
        if !pool_status.0 {
            warn!("Connection pool health check failed: {}", pool_status.1);
            return Err(Error::Database(format!(
                "Pool health check failed: {}",
                pool_status.1
            )));
        }
        debug!("Connection pool health check successful: {}", pool_status.1);
        Ok(pool_status)
    }

    /// Perform essential database health checks (called periodically)
    pub fn perform_health_checks(db: Arc<Database>) -> Result<()> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| {
                Error::Task(format!("Failed to create runtime for health checks: {}", e))
            })?;

        rt.block_on(async {
            info!("Performing database health checks...");
            // 1. Test basic connection
            if let Err(e) = db.test_connection().await {
                error!("❌ Health Check: Database connection test failed: {}", e);
                return Err(e);
            }
            debug!("✅ Health Check: Basic connection test passed.");

            // 2. Check pool health
            let pool_status = db.check_pool_health().await;
            if !pool_status.0 {
                warn!(
                    "⚠️ Health Check: Connection pool health check failed: {}",
                    pool_status.1
                );
                // Don't return error, just warn, as pool might recover
            } else {
                debug!(
                    "✅ Health Check: Pool health check passed: {}",
                    pool_status.1
                );
            }

            // 3. Test write permission
            if let Err(e) = db.test_write_permission().await {
                error!(
                    "❌ Health Check: Database write permission test failed: {}",
                    e
                );
                return Err(e);
            }
            debug!("✅ Health Check: Write permission test passed.");

            info!("✅ All database health checks passed successfully.");
            Ok(())
        })
    }
}
