use super::{Actor, Command, Event, Message, Query, QueryResult};
use super::{ExecutionEvent, MarketEvent, RiskEvent};
use crate::config::Config;
use crate::core::error::{Error, Result};
use crate::core::models::market::TokenMetrics as DbTokenMetrics;
use crate::domain::trading::execution::bot::is_forced_shutdown;
use crate::infra::db::queries;
use crate::infra::db::queue::{
    PositionUpdateOperation, PositionUpdateQueue, TradeOperation, TradeOperationQueue,
};
use crate::infra::db::repositories::RepositoryFactory;
use tokio_postgres::types::ToSql;

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use log::{debug, error, info, trace, warn};
use tokio;
use uuid::Uuid;

pub struct DatabaseActor {
    repo_factory: RepositoryFactory,
    message_bus: Arc<super::MessageBus>,
    config: Arc<crate::config::Config>,

    position_queue: Option<Arc<PositionUpdateQueue>>,
    trade_queue: Option<Arc<TradeOperationQueue>>,
    task_handle: Option<tokio::task::JoinHandle<()>>,

    last_activity_time: Arc<std::sync::Mutex<Instant>>,
    last_maintenance_time: Arc<std::sync::Mutex<Option<DateTime<Utc>>>>,
}

impl Clone for DatabaseActor {
    fn clone(&self) -> Self {
        Self {
            repo_factory: self.repo_factory.clone(),
            message_bus: self.message_bus.clone(),
            config: self.config.clone(),
            position_queue: self.position_queue.clone(),
            trade_queue: self.trade_queue.clone(),
            task_handle: None,
            last_activity_time: self.last_activity_time.clone(),
            last_maintenance_time: self.last_maintenance_time.clone(),
        }
    }
}

static mut SHUTDOWN_TX: Option<tokio::sync::mpsc::Sender<()>> = None;
static SHUTDOWN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

impl DatabaseActor {
    pub fn new(
        repo_factory: RepositoryFactory,
        message_bus: Arc<super::MessageBus>,
        config: Arc<crate::config::Config>,
    ) -> Self {
        let (position_queue, trade_queue) = Self::create_queues(&config);

        Self {
            repo_factory,
            message_bus,
            config,
            position_queue: position_queue.map(Arc::new),
            trade_queue: trade_queue.map(Arc::new),
            task_handle: None,
            last_activity_time: Arc::new(std::sync::Mutex::new(Instant::now())),
            last_maintenance_time: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Create Redis queues for persistent operation batching
    fn create_queues(
        config: &Arc<Config>,
    ) -> (Option<PositionUpdateQueue>, Option<TradeOperationQueue>) {
        // Check if Redis is configured and the URL is present
        if let Some(redis_url) = &config.cache.redis_url {
            // Pattern should bind redis_url to &String
            let app_name = "honeybadger";

            // Create position update queue
            // Pass redis_url (&String), deref coercion should handle it as &str for the function
            let position_queue =
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
                };

            // Create trade operation queue
            // Pass redis_url (&String), deref coercion should handle it as &str for the function
            let trade_queue =
                match TradeOperationQueue::new(redis_url, "trade_operations", app_name) {
                    Ok(queue) => {
                        info!(
                            "✅ Trade operation queue initialized with Redis at {}",
                            redis_url
                        );
                        Some(queue)
                    }
                    Err(e) => {
                        error!("❌ Failed to initialize trade operation queue: {}", e);
                        None
                    }
                };

            (position_queue, trade_queue)
        } else {
            warn!("⚠️ Redis URL not configured, database operations will be processed directly (less resilient)");
            (None, None)
        }
    }

    // Add methods to queue operations
    fn queue_position_update(
        &self,
        token_id: String,
        price: f64,
        pnl: f64,
        timestamp: chrono::DateTime<Utc>,
    ) {
        if let Some(ref queue) = self.position_queue {
            info!(
                "Position queue available, attempting to enqueue update for {}",
                token_id
            );

            let operation = PositionUpdateOperation {
                token_id: token_id.clone(),
                price,
                pnl,
                timestamp,
                operation_id: format!("pos_update_{}", Uuid::new_v4()),
                attempts: 0,
            };

            match queue.enqueue(operation.clone()) {
                Ok(_) => {
                    debug!("Enqueued position update for {} in Redis queue", token_id);
                    return;
                }
                Err(e) => {
                    error!("Failed to queue position update for {}: {} - Redis enqueue failed, using fallback", token_id, e);
                    self.queue_position_update_fallback(operation);
                }
            }
        } else {
            warn!(
                "Position update queue not available, using fallback for token {}",
                token_id
            );
            self.queue_position_update_fallback(PositionUpdateOperation {
                token_id: token_id.clone(),
                price,
                pnl,
                timestamp,
                operation_id: format!("pos_update_{}", Uuid::new_v4()),
                attempts: 0,
            });
        }
    }

    fn queue_trade_operation(
        &self,
        token_id: String,
        price: f64,
        size: f64,
        is_buy: bool,
        timestamp: chrono::DateTime<Utc>,
    ) {
        // Validate that we don't process zero-size or zero-price trades
        if size <= 0.0 || price <= 0.0 {
            info!("🚫 Skipping trade operation for {} with zero or negative values: price=${:.4}, size=${:.2}", 
                 token_id, price, size);
            return;
        }

        if let Some(ref queue) = self.trade_queue {
            info!(
                "Trade queue available, attempting to enqueue operation for {}",
                token_id
            );

            let operation = TradeOperation {
                token_id: token_id.clone(),
                price,
                size,
                is_buy,
                timestamp,
                position_id: None,
                is_position_close: false,
                entry_price: None,
                entry_time: None,
                delete_position: false,
                operation_id: format!("trade_{}", Uuid::new_v4()),
                attempts: 0,
            };

            match queue.enqueue(operation.clone()) {
                Ok(_) => {
                    debug!("Enqueued trade operation for {} in Redis queue", token_id);
                    return;
                }
                Err(e) => {
                    error!("Failed to queue trade operation for {}: {} - Redis enqueue failed, using fallback", token_id, e);
                    self.queue_trade_operation_fallback(operation);
                }
            }
        } else {
            warn!(
                "Trade operation queue not available, using fallback for token {}",
                token_id
            );
            self.queue_trade_operation_fallback(TradeOperation {
                token_id: token_id.clone(),
                price,
                size,
                is_buy,
                timestamp,
                position_id: None,
                is_position_close: false,
                entry_price: None,
                entry_time: None,
                delete_position: false,
                operation_id: format!("trade_{}", Uuid::new_v4()),
                attempts: 0,
            });
        }
    }

    fn queue_position_close(
        &self,
        token_id: String,
        price: f64,
        size: f64,
        entry_price: f64,
        entry_time: chrono::DateTime<Utc>,
        delete_position: bool,
        timestamp: chrono::DateTime<Utc>,
    ) {
        // Validate that we don't process zero-size or zero-price trades
        if size <= 0.0 || price <= 0.0 {
            info!("🚫 Skipping position close for {} with zero or negative values: price=${:.4}, size=${:.2}", 
                 token_id, price, size);
            return;
        }

        if let Some(ref queue) = self.trade_queue {
            info!(
                "Trade queue available, attempting to enqueue position close for {}",
                token_id
            );

            let operation = TradeOperation {
                token_id: token_id.clone(),
                price,
                size,
                is_buy: false, // Position close is always a SELL
                timestamp,
                position_id: None,
                is_position_close: true,
                entry_price: Some(entry_price),
                entry_time: Some(entry_time),
                delete_position,
                operation_id: format!("close_{}", Uuid::new_v4()),
                attempts: 0,
            };

            match queue.enqueue(operation.clone()) {
                Ok(_) => {
                    debug!("Enqueued position close for {} in Redis queue", token_id);
                    return;
                }
                Err(e) => {
                    error!("Failed to queue position close for {}: {} - Redis enqueue failed, using fallback", token_id, e);
                    self.queue_position_close_fallback(operation);
                }
            }
        } else {
            warn!(
                "Trade queue not available, using fallback for position close trade {}",
                token_id
            );
            self.queue_position_close_fallback(TradeOperation {
                token_id: token_id.clone(),
                price,
                size,
                is_buy: false,
                timestamp,
                position_id: None,
                is_position_close: true,
                entry_price: Some(entry_price),
                entry_time: Some(entry_time),
                delete_position,
                operation_id: format!("close_{}", Uuid::new_v4()),
                attempts: 0,
            });
        }
    }

    /// Process a trade operation (Now async)
    async fn process_trade(&self, trade: TradeOperation) -> Result<()> {
        let trade_repo = self.repo_factory.trade_repository();
        let token_repo = self.repo_factory.token_repository();
        let db = trade_repo.get_db();
        let token_id = trade.token_id.to_lowercase();

        debug!(
            "Processing trade for token {}: price=${:.4}, size={}, is_buy={}",
            token_id, trade.price, trade.size, trade.is_buy
        );

        // Validate trade parameters
        if trade.price <= 0.0 || trade.size <= 0.0 {
            error!(
                "❌ Rejecting trade for {} with invalid price ${:.4} or size ${:.2}",
                token_id, trade.price, trade.size
            );
            return Err(Error::InvalidInput(format!(
                "Invalid trade values: price={}, size={}",
                trade.price, trade.size
            )));
        }

        // First ensure the token exists
        if let Err(e) = token_repo.ensure_token_exists(&token_id).await {
            error!(
                "DB Error: Failed to ensure token {} exists: {}. Aborting trade processing.",
                token_id, e
            );
            return Err(e);
        }

        // If this is a position close, handle it differently
        if trade.is_position_close {
            warn!("process_position_close needs to be async. Skipping call for now.");
            return Ok(()); // Placeholder
                           // return self.process_position_close(trade).await;
        }

        // Record the trade using the pooled transaction system
        // Replace retry_pooled_transaction with direct client transaction
        let result = async {
            let mut client = db.get_connection().await?;
            let tx = client
                .transaction()
                .await
                .map_err(|e| Error::Database(e.to_string()))?;

            let normalized_token_id = token_id.to_lowercase();
            let timestamp_str = trade.timestamp.to_rfc3339();
            let is_buy_int = if trade.is_buy { 1 } else { 0 };
            let is_paper_int = if self.config.trading.paper_trading {
                1
            } else {
                0
            };

            if let Some(pos_id) = trade.position_id {
                // Use explicit Vec type AND cast each element
                let params: Vec<&(dyn ToSql + Sync)> = vec![
                    &normalized_token_id as &(dyn ToSql + Sync),
                    &trade.price as &(dyn ToSql + Sync),
                    &trade.size as &(dyn ToSql + Sync),
                    &timestamp_str as &(dyn ToSql + Sync),
                    &is_buy_int as &(dyn ToSql + Sync),
                    &is_paper_int as &(dyn ToSql + Sync),
                    &pos_id as &(dyn ToSql + Sync),
                ];
                tx.execute(
                    queries::trade::INSERT_TRADE_WITH_POSITION_ID,
                    &params, // Pass slice of the Vec
                )
                .await
                .map_err(|e| Error::Database(e.to_string()))?;
            } else {
                // Use explicit Vec type AND cast each element
                let params: Vec<&(dyn ToSql + Sync)> = vec![
                    &normalized_token_id as &(dyn ToSql + Sync),
                    &trade.price as &(dyn ToSql + Sync),
                    &trade.size as &(dyn ToSql + Sync),
                    &timestamp_str as &(dyn ToSql + Sync),
                    &is_buy_int as &(dyn ToSql + Sync),
                    &is_paper_int as &(dyn ToSql + Sync),
                ];
                tx.execute(
                    queries::trade::INSERT_TRADE_SELL,
                    &params, // Pass slice of the Vec
                )
                .await
                .map_err(|e| Error::Database(e.to_string()))?;
            }

            info!(
                "✅ Recorded {} trade for {} at ${:.4}",
                if trade.is_buy { "BUY" } else { "SELL" },
                normalized_token_id,
                trade.price
            );

            tx.commit()
                .await
                .map_err(|e| Error::Database(e.to_string()))
        }
        .await; // Execute the async block

        match result {
            Ok(_) => Ok(()),
            Err(e) => {
                error!(
                    "❌ Failed to record trade for {} after maximum retries: {}",
                    token_id, e
                );
                Err(e)
            }
        }
    }

    /// Process a position close operation (Needs to be async)
    async fn process_position_close(&self, trade: TradeOperation) -> Result<()> {
        let trade_repo = self.repo_factory.trade_repository();
        let db = trade_repo.get_db();
        let token_id = trade.token_id.to_lowercase();

        debug!("Processing position close for token {}", token_id);

        // Calculate profit/loss (optional, can be done elsewhere)
        let (profit, profit_pct) =
            if let (Some(entry_price), Some(_entry_time)) = (trade.entry_price, trade.entry_time) {
                let profit = trade.size * (trade.price - entry_price);
                let profit_pct = if entry_price != 0.0 {
                    (trade.price - entry_price) / entry_price * 100.0
                } else {
                    0.0
                };
                (profit, profit_pct)
            } else {
                warn!(
                    "Missing entry price/time for P/L calculation on position close for {}",
                    token_id
                );
                (0.0, 0.0)
            };

        // Record the trade and delete the position using a transaction
        let result = async {
            let mut client = db.get_connection().await?;
            let tx = client
                .transaction()
                .await
                .map_err(|e| Error::Database(e.to_string()))?;

            let normalized_token_id = token_id.to_lowercase();
            let timestamp_str = trade.timestamp.to_rfc3339();
            let is_paper_int = if self.config.trading.paper_trading {
                1
            } else {
                0
            };

            // Insert the closing trade
            // Use explicit Vec type AND cast each element
            let close_params: Vec<&(dyn ToSql + Sync)> = vec![
                &normalized_token_id as &(dyn ToSql + Sync),
                &trade.price as &(dyn ToSql + Sync),
                &trade.size as &(dyn ToSql + Sync),
                &timestamp_str as &(dyn ToSql + Sync),
                &is_paper_int as &(dyn ToSql + Sync),
            ];
            tx.execute(
                queries::trade::INSERT_TRADE_SELL,
                &close_params, // Pass slice of the Vec
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

            // Delete the position if requested
            if trade.delete_position {
                // Use explicit Vec type AND cast each element
                let delete_params: Vec<&(dyn ToSql + Sync)> = vec![
                    &normalized_token_id as &(dyn ToSql + Sync),
                    &is_paper_int as &(dyn ToSql + Sync),
                ];
                let rows_deleted = tx
                    .execute(
                        "DELETE FROM positions WHERE token_id = $1 AND is_paper = $2",
                        &delete_params, // Pass slice of the Vec
                    )
                    .await
                    .map_err(|e| Error::Database(e.to_string()))?;
                if rows_deleted == 0 {
                    warn!(
                        "Tried to delete position {} during close, but it was not found.",
                        normalized_token_id
                    );
                }
            }

            info!(
                "✅ Recorded position close for {} at ${:.4} with P/L ${:.2} ({:.1}%)",
                normalized_token_id, trade.price, profit, profit_pct
            );

            tx.commit()
                .await
                .map_err(|e| Error::Database(e.to_string()))
        }
        .await; // Execute the async block

        match result {
            Ok(_) => Ok(()),
            Err(e) => {
                error!(
                    "❌ Failed to process position close for {}: {}",
                    token_id, e
                );
                Err(e)
            }
        }
    }

    /// Check if it's a good time to run maintenance tasks
    ///
    /// Returns true if:
    /// 1. It's been at least 24 hours since the last maintenance
    /// 2. The system is currently idle (no recent activity)
    async fn should_run_maintenance(&self) -> bool {
        // Check last maintenance time
        let maintenance_interval = chrono::Duration::hours(24);
        let now = Utc::now();

        // Check when maintenance was last performed
        let should_run_by_time = {
            let last_maintenance = self.last_maintenance_time.lock().unwrap();
            match *last_maintenance {
                None => true, // Never run before, so yes, run it
                Some(last_time) => now.signed_duration_since(last_time) >= maintenance_interval,
            }
        };

        // If it's not time yet by schedule, don't run
        if !should_run_by_time {
            return false;
        }

        // Check if the system is idle
        let is_idle = {
            let last_activity = self.last_activity_time.lock().unwrap();
            // Consider the system idle if no activity in the last 5 minutes
            let idle_threshold = std::time::Duration::from_secs(5 * 60);
            last_activity.elapsed() >= idle_threshold
        };

        should_run_by_time && is_idle
    }

    /// Schedule maintenance to run during quiet periods
    async fn schedule_maintenance(&self) {
        let this = self.clone();
        info!("Maintenance scheduler check on startup.");
        tokio::spawn(async move {
            if this.should_run_maintenance().await {
                info!("Running scheduled maintenance (startup check)...",);
                if let Err(e) = this.run_maintenance_now().await {
                    error!("Sched startup maint FAIL: {}", e);
                }
            }
            let mut check_interval = tokio::time::interval(tokio::time::Duration::from_secs(3600)); // Hourly check
            info!("Maintenance scheduler started (hourly check).");
            loop {
                tokio::select! {
                    _ = check_interval.tick() => {
                        // Check shutdown signal FIRST
                        if is_forced_shutdown() {
                            info!("Maintenance scheduler stopping due to shutdown signal.");
                            break; // Exit the loop
                        }

                        trace!("Checking if scheduled maintenance needed...");
                        if this.should_run_maintenance().await {
                            info!("Running scheduled maintenance (periodic check)...",);
                            if let Err(e) = this.run_maintenance_now().await { error!("Sched periodic maint FAIL: {}", e); }
                        }
                    }
                    // Can add a dedicated shutdown channel receiver here if needed later
                }
            }
            info!("Maintenance scheduler task finished."); // Log when loop exits
        });
    }

    /// Get maintenance status information including last run time and next scheduled time
    fn get_maintenance_status(&self) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
        let last_run = *self.last_maintenance_time.lock().unwrap();
        let next_sched = last_run.map(|last| {
            last + chrono::Duration::hours(self.config.database.maintenance_interval_hours as i64)
        });
        (last_run, next_sched)
    }

    fn queue_position_update_fallback(&self, operation: PositionUpdateOperation) {
        // ... implementation ...
    }

    fn queue_trade_operation_fallback(&self, operation: TradeOperation) {
        // ... implementation ...
    }

    fn queue_position_close_fallback(&self, operation: TradeOperation) {
        // ... implementation ...
    }

    async fn gather_metrics(&self) -> serde_json::Value {
        let mut metrics = serde_json::json!({
            "paper_trading": self.config.trading.paper_trading,
        });
        let (last_maint, next_maint) = self.get_maintenance_status();
        let maint_info = serde_json::json!({
            "last_run": last_maint.map(|dt| dt.to_rfc3339()),
            "next_scheduled": next_maint.map(|dt| dt.to_rfc3339()),
            "auto_enabled": true
        });
        metrics
            .as_object_mut()
            .unwrap()
            .insert("maintenance".to_string(), maint_info);
        let mut q_metrics = serde_json::Map::new();
        if let Some(q) = &self.position_queue {
            if let Ok(stats) = q.get_metrics_async().await {
                q_metrics.insert("position_queue".to_string(), serde_json::json!(stats));
            } else {
                error!("Failed position Q metrics")
            }
        }
        if let Some(q) = &self.trade_queue {
            if let Ok(stats) = q.get_metrics_async().await {
                q_metrics.insert("trade_queue".to_string(), serde_json::json!(stats));
            } else {
                error!("Failed trade Q metrics")
            }
        }
        if !q_metrics.is_empty() {
            metrics
                .as_object_mut()
                .unwrap()
                .insert("queues".to_string(), serde_json::Value::Object(q_metrics));
        }
        // TODO: Add pool health check metrics
        metrics
    }

    // Placeholder method for running maintenance now
    async fn run_maintenance_now(&self) -> Result<()> {
        info!("Manually triggering database maintenance...");
        match self.repo_factory.get_db().perform_maintenance().await {
            Ok(_) => {
                info!("✅ Manual database maintenance completed successfully.");
                Ok(())
            }
            Err(e) => {
                error!("❌ Manual database maintenance failed: {}", e);
                Err(e)
            }
        }
    }

    // Placeholder associated function for processing batch
    async fn process_batch(
        repo_factory: &RepositoryFactory,
        position_queue: Option<&PositionUpdateQueue>,
        trade_queue: Option<&TradeOperationQueue>,
        config: &Config,
    ) -> Result<()> {
        trace!("Processing database batch...");
        let batch_size = config.database.batch_size;
        let mut total_processed = 0;

        // Process Position Updates
        if let Some(queue) = position_queue {
            let position_repo = repo_factory.position_repository(); // Create repo instance
            match crate::infra::db::task_handler::DatabaseTaskHandler::process_position_batch(
                position_repo.as_ref().clone(),
                queue.clone(),
                batch_size,
            )
            .await
            {
                Ok(count) => {
                    if count > 0 {
                        debug!("Processed {} position updates from queue.", count);
                        total_processed += count;
                    }
                }
                Err(e) => {
                    error!("Error processing position batch: {}", e);
                    // Decide if we should stop or continue with trade batch
                }
            }
        }

        // Process Trade Operations
        if let Some(queue) = trade_queue {
            let trade_repo = repo_factory.trade_repository();
            let position_repo = repo_factory.position_repository(); // Get position repo
            match crate::infra::db::task_handler::DatabaseTaskHandler::process_trade_batch(
                trade_repo.as_ref().clone(),
                queue.clone(),
                Some(position_repo.as_ref().clone()),
                batch_size,
            )
            .await
            {
                Ok(count) => {
                    if count > 0 {
                        debug!("Processed {} trade operations from queue.", count);
                        total_processed += count;
                    }
                }
                Err(e) => {
                    error!("Error processing trade batch: {}", e);
                }
            }
        }

        if total_processed > 0 {
            trace!(
                "Finished processing database batch ({} items total).",
                total_processed
            );
        } else {
            trace!("Database batch processing cycle complete (no items).",);
        }
        Ok(())
    }

    // Placeholder method for handling events
    async fn handle_event(&self, event: Event) -> Result<()> {
        trace!("Handling event: {:?}", event);
        match event {
            Event::Market(market_event) => match market_event {
                MarketEvent::PriceUpdate {
                    token_id,
                    price,
                    volume,
                    timestamp,
                } => {
                    let token_repo = self.repo_factory.token_repository();
                    // Use store_price_data, assuming token metadata is handled elsewhere or by ensure_token_exists
                    // Ensure token exists before storing price data might be prudent
                    match token_repo.ensure_token_exists(&token_id).await {
                        Ok(_) => {
                            match token_repo
                                .store_price_data(&token_id, price, volume.unwrap_or(0.0))
                                .await
                            {
                                Ok(_) => {
                                    debug!("Successfully stored price data for {}", token_id);
                                }
                                Err(e) => {
                                    error!("Failed to store price data for {}: {}", token_id, e);
                                    // Decide if this error should be propagated
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to ensure token {} exists before storing price: {}. Price data not stored.", token_id, e);
                        }
                    }
                }
                MarketEvent::MarketDataError(err) => {
                    error!("Received MarketDataError event: {}", err);
                    // Potentially log this error to a different system/table
                }
                MarketEvent::SupervisorRecoveryRequest(msg) => {
                    info!("Received SupervisorRecoveryRequest: {}", msg);
                    // Log recovery attempts
                }
                MarketEvent::StatusCheck => {
                    // Explicitly handle StatusCheck
                    trace!("DatabaseActor ignoring StatusCheck market event.");
                }
                _ => {
                    trace!("DatabaseActor ignoring unhandled MarketEvent variant.");
                }
            },
            Event::Execution(ExecutionEvent::OrderExecuted {
                token_id,
                signal,
                size,
                price,
                timestamp,
            }) => {
                // TRADE RECORDING REMOVED HERE:
                // The trade associated with opening a position is already recorded atomically
                // within PositionRepository::record_position_with_trade by the ExecutionActor.
                // Recording it again here based on the OrderExecuted event causes duplicates.
                // If standalone trades (not linked to positions) need recording based on this event,
                // specific logic would be required to differentiate.
                debug!(
                    "DatabaseActor received OrderExecuted event for {} - Trade recording is handled elsewhere.",
                    token_id
                );
            }
            Event::Risk(RiskEvent::PositionClosed {
                token_id,
                pnl,
                timestamp,
                entry_price,
                exit_price,
                size,
                entry_time,
                delete_position,
            }) => {
                let position_repo = self.repo_factory.position_repository();
                warn!("Handling PositionClosed event in DatabaseActor - needs logic to find position ID before recording close trade.");
                // Fetch position_id based on token_id and is_paper
                match position_repo.get_position_by_token_id(&token_id).await {
                    Ok(Some((position_id, _pos))) => {
                        // Corrected call with 7 arguments (timestamp is used for exit_time)
                        match position_repo
                            .record_position_close_with_trade(
                                position_id,
                                &token_id,
                                exit_price,
                                size,
                                entry_price,
                                entry_time,
                                timestamp,
                            )
                            .await
                        {
                            Ok(_) => info!(
                                "Successfully recorded position close for token_id: {}",
                                token_id
                            ),
                            Err(e) => error!(
                                "Failed to record position close for token_id {}: {}",
                                token_id, e
                            ),
                        }
                        if delete_position {
                            // Use delete_open_position_by_id
                            if let Err(e) =
                                position_repo.delete_open_position_by_id(position_id).await
                            {
                                error!(
                                    "Failed to delete position {} after closing: {}",
                                    position_id, e
                                );
                            }
                        }
                    }
                    Ok(None) => {
                        error!(
                            "Could not find open position for token_id {} to record closure.",
                            token_id
                        );
                    }
                    Err(e) => {
                        error!("Failed to query position_id for token {}: {}", token_id, e);
                    }
                }
            }
            // Add handlers for other event types (Strategy, Risk, Database) as needed
            _ => {
                trace!("DatabaseActor ignoring event type: {:?}", event);
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl Actor for DatabaseActor {
    fn start(
        &mut self,
    ) -> impl std::future::Future<Output = crate::core::error::Result<()>> + Send {
        info!("Starting DatabaseActor...");
        // Clone needed parts for the background task
        let repo_factory = self.repo_factory.clone();
        let position_queue = self.position_queue.clone(); // Clone Option<Arc<Queue>>
        let trade_queue = self.trade_queue.clone(); // Clone Option<Arc<Queue>>
        let batch_interval_secs = self.config.database.batch_interval_secs; // Get interval from config
        let config_clone = self.config.clone(); // Clone config for process_batch

        // Spawn the background task
        let handle = tokio::spawn(async move {
            info!(
                "DatabaseActor background task started (interval: {}s)",
                batch_interval_secs
            );
            let interval = Duration::from_secs(batch_interval_secs);
            let mut interval_timer = tokio::time::interval(interval);

            loop {
                tokio::select! {
                    _ = interval_timer.tick() => {
                        if is_forced_shutdown() {
                            info!("DatabaseActor task detected forced shutdown.");
                            break;
                        }
                        // Process batches asynchronously
                        // Pass Option<&Queue> by borrowing from the cloned Option<Arc<Queue>>
                        if let Err(e) = DatabaseActor::process_batch(
                            &repo_factory,
                            position_queue.as_deref(), // Pass Option<&PositionUpdateQueue>
                            trade_queue.as_deref(),   // Pass Option<&TradeOperationQueue>
                            &config_clone, // Pass reference to cloned Config
                        ).await {
                            error!("Error processing database batch: {}", e);
                        }
                    }
                    // TODO: Add shutdown signal channel if needed
                }
            }
            info!("DatabaseActor background task finished.");
        });

        self.task_handle = Some(handle);
        info!("DatabaseActor started and background task spawned.");
        // Since start is not async, return an immediate future
        async { Ok(()) }
    }

    fn stop(&mut self) -> Result<()> {
        info!("Stopping DatabaseActor...");
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
            info!("DatabaseActor background task signalled to abort.");
            // Note: We don't await the handle here as stop is synchronous.
        } else {
            warn!("DatabaseActor stop called but no task handle found.");
        }
        Ok(())
    }

    fn handle_message(
        &mut self,
        msg: Message,
    ) -> impl std::future::Future<Output = crate::core::error::Result<()>> + Send {
        trace!("DatabaseActor received message: {:?}", msg);
        // Clone self FOR the async block
        let self_clone = self.clone();
        // Wrap the message handling logic in an async block
        async move {
            match msg {
                Message::Event(event) => {
                    if let Err(e) = self_clone.handle_event(event).await {
                        error!("Error handling event in DatabaseActor: {}", e);
                    }
                }
                Message::Query(Query::GetStatus, responder) => {
                    let status = QueryResult::Status("Running".to_string());
                    let _ = responder
                        .send(Ok(status))
                        .map_err(|_| error!("Failed to send response for GetStatus query"));
                }
                Message::Query(Query::GetMetrics, responder) => {
                    let metrics = self_clone.gather_metrics().await;
                    let _ = responder
                        .send(Ok(QueryResult::Metrics(metrics)))
                        .map_err(|_| error!("Failed to send response for GetMetrics query"));
                }
                Message::Query(Query::GetMaintenanceStatus, responder) => {
                    let (last_run, next_run) = self_clone.get_maintenance_status();
                    let result = Ok(QueryResult::MaintenanceStatus { last_run, next_run });
                    let _ = responder.send(result).map_err(|_| {
                        error!("Failed to send response for GetMaintenanceStatus query")
                    });
                }
                Message::Query(query, responder) => {
                    warn!("DatabaseActor received unhandled query: {:?}", query);
                    let _ = responder.send(Err(Error::NotImplemented(format!(
                        "Query {:?} not handled",
                        query
                    ))));
                }
                Message::Command(Command::MaintenanceDb) => {
                    info!("Received MaintenanceDb command.");
                    let maint_self = self_clone.clone();
                    tokio::spawn(async move {
                        if let Err(e) = maint_self.run_maintenance_now().await {
                            error!("Manual database maintenance failed: {}", e);
                        }
                    });
                }
                Message::Command(cmd) => {
                    warn!("DatabaseActor received unhandled command: {:?}", cmd);
                }
            }
            Ok(())
        }
    }
}
