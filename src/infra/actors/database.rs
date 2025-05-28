use super::{Actor, Command, Event, Message, Query, QueryResult};
use super::{
    DexTransactionEvent, EventType, ExecutionEvent, MarketEvent, RiskEvent, StrategyEvent,
};
use crate::config::Config;
use crate::core::error::{Error, Result};
use crate::domain::dex::TransactionStatus;
use crate::infra::db::queue::{
    PositionUpdateOperation, PositionUpdateQueue, TradeOperation, TradeOperationQueue,
};
use crate::infra::db::repositories::RepositoryFactory;

use crate::core::models::token::TokenData;

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use log::{debug, error, info, trace, warn};
use tokio;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::domain::trading::strategy::{Position as StrategyPosition, Signal};

use std::sync::atomic::{AtomicBool, Ordering};

pub struct DatabaseActor {
    repo_factory: RepositoryFactory,
    message_bus: Arc<super::MessageBus>,
    config: Arc<crate::config::Config>,

    position_queue: Option<Arc<PositionUpdateQueue>>,
    trade_queue: Option<Arc<TradeOperationQueue>>,
    task_handle: Option<tokio::task::JoinHandle<()>>,

    last_activity_time: Arc<std::sync::Mutex<Instant>>,
    last_maintenance_time: Arc<std::sync::Mutex<Option<DateTime<Utc>>>>,

    running: bool,
    shutdown_flag: Arc<AtomicBool>,
    event_sender: mpsc::Sender<Event>,
    event_receiver: Option<mpsc::Receiver<Event>>,
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
            running: self.running,
            shutdown_flag: self.shutdown_flag.clone(),
            event_sender: self.event_sender.clone(),
            event_receiver: None,
        }
    }
}

impl DatabaseActor {
    pub fn new(
        repo_factory: RepositoryFactory,
        message_bus: Arc<super::MessageBus>,
        config: Arc<crate::config::Config>,
    ) -> Self {
        let (position_queue, trade_queue) = Self::create_queues(&config);
        let (event_sender, event_receiver) = mpsc::channel(100);

        Self {
            repo_factory,
            message_bus,
            config,
            position_queue: position_queue.map(Arc::new),
            trade_queue: trade_queue.map(Arc::new),
            task_handle: None,
            last_activity_time: Arc::new(std::sync::Mutex::new(Instant::now())),
            last_maintenance_time: Arc::new(std::sync::Mutex::new(None)),
            running: false,
            shutdown_flag: Arc::new(AtomicBool::new(false)),
            event_sender,
            event_receiver: Some(event_receiver),
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
        canonical_token_id: String,
        provider_token_id: String,
        price: f64,
        size: f64,
        is_buy: bool,
        timestamp: chrono::DateTime<Utc>,
    ) {
        // Validate that we don't process zero-size or zero-price trades
        if size <= 0.0 || price <= 0.0 {
            info!("🚫 Skipping trade operation for {} with zero or negative values: price=${:.4}, size=${:.2}", 
                 canonical_token_id, price, size);
            return;
        }

        if let Some(ref queue) = self.trade_queue {
            info!(
                "Trade queue available, attempting to enqueue operation for {}",
                canonical_token_id
            );

            let operation = TradeOperation {
                canonical_token_id: canonical_token_id.clone(),
                provider_token_id: provider_token_id.clone(),
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
                    debug!(
                        "Enqueued trade operation for {} in Redis queue",
                        canonical_token_id
                    );
                    return;
                }
                Err(e) => {
                    error!("Failed to queue trade operation for {}: {} - Redis enqueue failed, using fallback", canonical_token_id, e);
                    self.queue_trade_operation_fallback(operation);
                }
            }
        } else {
            warn!(
                "Trade operation queue not available, using fallback for token {}",
                canonical_token_id
            );
            self.queue_trade_operation_fallback(TradeOperation {
                canonical_token_id: canonical_token_id.clone(),
                provider_token_id: provider_token_id.clone(),
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
        canonical_token_id: String,
        provider_token_id: String,
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
                 canonical_token_id, price, size);
            return;
        }

        if let Some(ref queue) = self.trade_queue {
            info!(
                "Trade queue available, attempting to enqueue position close for {}",
                canonical_token_id
            );

            let operation = TradeOperation {
                canonical_token_id: canonical_token_id.clone(),
                provider_token_id: provider_token_id.clone(),
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
            };

            match queue.enqueue(operation.clone()) {
                Ok(_) => {
                    debug!(
                        "Enqueued position close for {} in Redis queue",
                        canonical_token_id
                    );
                    return;
                }
                Err(e) => {
                    error!("Failed to queue position close for {}: {} - Redis enqueue failed, using fallback", canonical_token_id, e);
                    self.queue_position_close_fallback(operation);
                }
            }
        } else {
            warn!(
                "Trade queue not available, using fallback for position close trade {}",
                canonical_token_id
            );
            self.queue_position_close_fallback(TradeOperation {
                canonical_token_id: canonical_token_id.clone(),
                provider_token_id: provider_token_id.clone(),
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

    /// Get maintenance status information including last run time and next scheduled time
    fn get_maintenance_status(&self) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
        let last_run = *self.last_maintenance_time.lock().unwrap();
        let next_sched = last_run.map(|last| {
            last + chrono::Duration::hours(self.config.database.maintenance_interval_hours as i64)
        });
        (last_run, next_sched)
    }

    // Fallback methods for when direct DB operations fail
    fn queue_position_update_fallback(&self, _operation: PositionUpdateOperation) {
        // Prefixed
        // TODO: Implement fallback queueing logic (e.g., to an in-memory queue or a file)
        error!("Fallback: Failed to queue position update directly. This operation is lost if not handled by a persistent queue.");
    }
    fn queue_trade_operation_fallback(&self, _operation: TradeOperation) {
        // Prefixed
        // TODO: Implement fallback queueing logic
        error!("Fallback: Failed to queue trade operation directly. This operation is lost if not handled by a persistent queue.");
    }
    fn queue_position_close_fallback(&self, _operation: TradeOperation) {
        // Prefixed
        // TODO: Implement fallback queueing logic
        error!("Fallback: Failed to queue position close directly. This operation is lost if not handled by a persistent queue.");
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

    async fn log_dex_transaction_status(&self, status_event: &TransactionStatus) -> Result<()> {
        let repo = self.repo_factory.dex_transaction_log_repository();
        let (tx_id, status_text, event_time, details_json) = match status_event {
            TransactionStatus::Queued {
                tx_id,
                submission_time,
                priority,
            } => (
                tx_id.clone(),
                "Queued".to_string(),
                *submission_time,
                Some(serde_json::json!({ "priority": priority })),
            ),
            TransactionStatus::Pending {
                tx_id,
                submission_time,
                last_checked,
                block_height,
                retry_count,
            } => (
                tx_id.clone(),
                "Pending".to_string(),
                *last_checked,
                Some(serde_json::json!({
                    "submission_time": submission_time,
                    "block_height": block_height,
                    "retry_count": retry_count
                })),
            ),
            TransactionStatus::Confirmed {
                details,
                confirmations,
                required_confirmations,
                finality_probability,
            } => {
                (
                    details.tx_id.clone(),
                    "Confirmed".to_string(),
                    Utc::now(), // Or use details.confirmation_time if available and more suitable
                    Some(serde_json::json!({
                        "details": details, // TransactionDetails itself should be Serialize
                        "confirmations": confirmations,
                        "required_confirmations": required_confirmations,
                        "finality_probability": finality_probability
                    })),
                )
            }
            TransactionStatus::Success {
                details,
                execution_time,
                gas_efficiency,
            } => {
                (
                    details.tx_id.clone(),
                    "Success".to_string(),
                    details.confirmation_time.unwrap_or_else(Utc::now),
                    Some(serde_json::json!({
                        "details": details, // TransactionDetails
                        "execution_time_ms": execution_time.num_milliseconds(),
                        "gas_efficiency": gas_efficiency
                    })),
                )
            }
            TransactionStatus::Failed {
                tx_id,
                reason,
                error_code,
                gas_used,
                revert_reason,
                recovery_suggestion,
            } => (
                tx_id.clone(),
                "Failed".to_string(),
                Utc::now(),
                Some(serde_json::json!({
                    "reason": reason,
                    "error_code": error_code,
                    "gas_used": gas_used,
                    "revert_reason": revert_reason,
                    "recovery_suggestion": recovery_suggestion
                })),
            ),
            TransactionStatus::Dropped {
                tx_id,
                reason,
                replacement_tx,
                gas_price_delta,
                network_congestion,
            } => (
                tx_id.clone(),
                "Dropped".to_string(),
                Utc::now(),
                Some(serde_json::json!({
                    "reason": reason,
                    "replacement_tx": replacement_tx,
                    "gas_price_delta": gas_price_delta,
                    "network_congestion": network_congestion
                })),
            ),
        };

        info!(
            "DatabaseActor: Logging DEX event - TxID: {}, Status: {}, Time: {}",
            tx_id, status_text, event_time
        );

        match repo
            .log_event(&tx_id, &status_text, event_time, details_json)
            .await
        {
            Ok(_) => debug!(
                "DatabaseActor: Successfully logged dex_transaction_event for {}",
                tx_id
            ),
            Err(e) => error!(
                "DatabaseActor: Failed to log dex_transaction_event for {}: {}",
                tx_id, e
            ),
        }
        Ok(())
    }

    async fn handle_event(&self, event: Event) -> Result<()> {
        trace!("DatabaseActor received event: {:?}", event);
        *self.last_activity_time.lock().unwrap() = Instant::now();

        match event {
            Event::Market(market_event) => match market_event {
                MarketEvent::PriceUpdate {
                    token_id,
                    price,
                    volume,
                    timestamp,
                    ..
                } => {
                    info!(
                        "DBActor: MarketPriceUpdate - Token: {}, Price: {:.4}, Volume: {:?}, Timestamp: {}",
                        token_id, price, volume, timestamp
                    );
                    // TODO: If direct storage of every price tick is needed, implement here.
                    // For now, relying on other actors (e.g., StrategyActor) to process and
                    // potentially trigger DB updates for aggregated data or trades.
                }
                MarketEvent::VolumeUpdate {
                    token_id,
                    volume,
                    timestamp,
                    ..
                } => {
                    info!(
                        "DBActor: MarketVolumeUpdate - Token: {}, Volume: {:.2}, Timestamp: {}",
                        token_id, volume, timestamp
                    );
                    // TODO: Similar to PriceUpdate, direct storage of every volume tick might be excessive.
                    // Relying on other actors for processing.
                }
                MarketEvent::MarketDataError(e) => {
                    warn!("DBActor: MarketDataError received: {}", e);
                }
                MarketEvent::StatusCheck => {
                    trace!("DBActor: Market StatusCheck ignored.");
                }
                MarketEvent::SupervisorRecoveryRequest(req) => {
                    info!(
                        "DBActor: SupervisorRecoveryRequest: {}. (Not directly handled by DB)",
                        req
                    );
                }
                MarketEvent::NewTokenDiscovered {
                    token_id,
                    name,
                    symbol,
                    price,
                    source,
                    timestamp,
                    ..
                } => {
                    info!(
                        "DBActor: NewTokenDiscovered - Token ID: {}, Name: {}, Symbol: {}, Price: {:.4}, Source: {}, Timestamp: {}",
                        token_id, name, symbol, price, source, timestamp
                    );
                    let token_repo = self.repo_factory.token_repository();
                    let token_id_norm = TokenData::normalize_token_id(&token_id); // Ensure consistent casing

                    // 1. Ensure the token record exists
                    if let Err(e) = token_repo.ensure_token_exists(&token_id_norm).await {
                        error!("DBActor: Failed to ensure token {} exists: {}. Skipping further processing for this token discovery.", token_id_norm, e);
                        // Optionally, you could return here or decide how critical this failure is.
                        // For now, we'll log and attempt other operations.
                    }

                    // 2. Update metadata (name, symbol)
                    if let Err(e) = token_repo
                        .update_token_metadata(&token_id_norm, &symbol, &name)
                        .await
                    {
                        error!(
                            "DBActor: Failed to update metadata for new token {}: {}",
                            token_id_norm, e
                        );
                    }

                    // 3. Store initial price (volume is not in NewTokenDiscovered, so use 0.0)
                    if let Err(e) = token_repo
                        .store_price_data(&token_id_norm, price, 0.0)
                        .await
                    {
                        error!(
                            "DBActor: Failed to store initial price for new token {}: {}",
                            token_id_norm, e
                        );
                    }

                    // Logging for source and discovery timestamp (as persistence is deferred)
                    debug!(
                        "DBActor: NewTokenDiscovered - Source: '{}', Discovery Timestamp: '{}' for token {} (logged, not persisted to dedicated columns yet).",
                        source, timestamp, token_id_norm
                    );
                }
                MarketEvent::MarketAnomalyDetected {
                    token_id,
                    anomaly_type,
                    description,
                    severity,
                    timestamp,
                    ..
                } => {
                    // Use WARN or ERROR based on severity if possible, default to WARN
                    warn!(
                        "DBActor: MarketAnomalyDetected - Token: {}, Type: {}, Severity: {}, Description: '{}', Timestamp: {}",
                        token_id, anomaly_type, severity, description, timestamp
                    );
                    // TODO: Future: Log to a dedicated 'market_anomalies' table with all details.
                    // For now, relies on structured logging.
                }
            },
            Event::Execution(execution_event) => match execution_event {
                ExecutionEvent::OrderExecuted {
                    canonical_token_id,
                    provider_token_id,
                    signal,
                    executed_value_usd,
                    token_quantity,
                    price_per_token,
                    timestamp,
                } => {
                    info!(
                        "DBActor: OrderExecuted Event for {} (provider ID: {}) - Signal: {:?}, Value: {}, Quantity: {}, Price: {}, Timestamp: {}",
                        canonical_token_id, provider_token_id, signal, executed_value_usd, token_quantity, price_per_token, timestamp
                    );
                    if signal == Signal::Buy {
                        if token_quantity <= 0.0 {
                            error!("DatabaseActor: Calculated position size is zero or negative for OrderExecuted event for {}. Cannot record position.", canonical_token_id);
                            return Ok(());
                        }

                        let final_strategy_position_data = StrategyPosition {
                            token_id: canonical_token_id.clone(),
                            provider_id: provider_token_id.clone(),
                            entry_price: price_per_token,
                            current_price: price_per_token,
                            highest_price: price_per_token,
                            size: token_quantity, // Use token_quantity for position size
                            entry_time: timestamp,
                            unrealized_pnl: 0.0,
                        };

                        match self
                            .repo_factory
                            .position_repository()
                            .record_position_with_trade(
                                &final_strategy_position_data,
                                price_per_token, // Trade price
                                token_quantity,  // Trade size (amount of token)
                                timestamp,
                            )
                            .await
                        {
                            Ok(db_position_id) => {
                                info!("DatabaseActor: Recorded new position (DB ID: {}) and BUY trade for token {} from OrderExecuted event", db_position_id, canonical_token_id);
                            }
                            Err(e) => {
                                error!("DatabaseActor: Failed to record position/trade for token {} from OrderExecuted event: {}", canonical_token_id, e);
                            }
                        }
                    } else {
                        // For SELL signals, we might also queue a trade operation if needed,
                        // though typically position closure is handled by RiskEvent::PositionClosed.
                        // If OrderExecuted for a SELL is meant to *only* log the trade and not affect position state here:
                        self.queue_trade_operation(
                            canonical_token_id.clone(),
                            provider_token_id.clone(), // Pass provider_token_id
                            price_per_token,
                            token_quantity,
                            false, // is_buy is false for a sell
                            timestamp,
                        );
                    }
                }
                ExecutionEvent::PositionUpdate {
                    token_id,
                    current_price,
                    pnl,
                    timestamp,
                    ..
                } => {
                    info!(
                        "DBActor: PositionUpdate - Token: {}, Price: {:.4}, PNL: {:.2}, Timestamp: {}",
                        token_id, current_price, pnl, timestamp
                    );
                    self.queue_position_update(token_id.to_string(), current_price, pnl, timestamp);
                }
                ExecutionEvent::StatusCheck => trace!("DBActor: Execution StatusCheck ignored."),
                ExecutionEvent::OrderFailed {
                    token_id,
                    order_id,
                    reason,
                    timestamp,
                    ..
                } => {
                    error!(
                        "DBActor: OrderFailed - Token: {}, OrderID: {:?}, Reason: '{}', Timestamp: {}",
                        token_id, order_id, reason, timestamp
                    );
                    // TODO: Future: Log to a dedicated 'order_failures' table or use dex_transaction_logs
                    // if there's a corresponding blockchain tx_id that failed.
                    // For now, relies on structured error logging.
                }
            },
            Event::Risk(risk_event) => match risk_event {
                RiskEvent::RiskAssessment {
                    token_id, .. // Capture other fields with ..
                } => {
                    trace!(
                        "DBActor: RiskAssessment for {} - Not directly handled by DB.",
                    token_id
                );
            }
                RiskEvent::PositionOpened {
                    token_id,
                    position_id,
                    amount,
                    price,
                    timestamp,
                    ..
                } => {
                    info!(
                        "DBActor: PositionOpened Event - Token ID: {}, External Position ID: {}, Amount: {}, Price: {}, Timestamp: {}",
                        token_id, position_id, amount, price, timestamp
                    );
                    let position_repo = self.repo_factory.position_repository();

                    // Construct the domain::trading::strategy::Position object
                    let strategy_position_data = StrategyPosition {
                        token_id: token_id.clone(),
                        provider_id: token_id.clone(), // Assuming provider_id is token_id for now, or derive appropriately
                        entry_price: price,
                        current_price: price,    // Initial current_price is entry_price
                        highest_price: price,    // Initial highest_price is entry_price
                        size: amount,
                        entry_time: timestamp,
                        unrealized_pnl: 0.0,     // Initial P&L is 0
                    };

                    match position_repo.record_position_with_trade(
                        &strategy_position_data, // Pass the constructed position
                        price,                   // Price for the trade
                        amount,                  // Size for the trade
                        timestamp                // Timestamp for the trade
                    ).await {
                        Ok(db_position_id) => {
                            info!(
                                "DBActor: Successfully recorded position (DB ID: {}) and trade for token {} (External Event ID: {})",
                                db_position_id, token_id, position_id
                            );
                        }
                        Err(e) => {
                            error!(
                                "DBActor: Failed to record position and trade for token {} (External Event ID: {}): {}",
                                token_id, position_id, e
                            );
                        }
                    }
                }
                RiskEvent::PositionClosed {
                token_id, // This is canonical
                pnl,
                timestamp,
                entry_price,
                exit_price,
                size,
                entry_time,
                delete_position,
                } => {
                    info!(
                        "DBActor: RiskPositionClosed Event - Token ID: {}, PNL: {}, ExitPrice: {}, Size: {}, Delete: {}",
                        token_id, pnl, exit_price, size, delete_position
                    );
                    // We need provider_token_id here. 
                    // The RiskEvent::PositionClosed event currently only has `token_id` (canonical).
                    // This is a gap. For now, we might have to use token_id for both, or fetch it.
                    // Fetching provider_id from the position being closed:
                    let position_repo = self.repo_factory.position_repository();
                    let provider_id_for_close = match tokio::runtime::Handle::current().block_on(position_repo.get_provider_id_for_token(&token_id)) {
                        Ok(Some(pid)) => pid,
                        Ok(None) => {
                            error!("DBActor: Could not find provider_id for position {} to close. Using canonical_id as fallback.", token_id);
                            token_id.clone() // Fallback
                        },
                        Err(e) => {
                             error!("DBActor: Error fetching provider_id for {}: {}. Using canonical_id as fallback.", token_id, e);
                             token_id.clone() // Fallback
                        }
                    };

                    self.queue_position_close(
                        token_id.to_string(),
                        provider_id_for_close, // Pass fetched or fallback provider_id
                        exit_price,
                        size,
                        entry_price,
                        entry_time,
                        delete_position,
                        timestamp
                    );
                }
                RiskEvent::RiskLimitExceeded {
                    limit_type,
                    current,
                    max,
                    .. // Ensure timestamp is captured by ..
                } => {
                    info!(
                        "DBActor: RiskLimitExceeded: Type {}, Current {}, Max {}",
                        limit_type, current, max
                    );
                }
                RiskEvent::RiskMetricsUpdate { .. } => { // Capture fields with ..
                    trace!("DBActor: RiskMetricsUpdate - Not directly handled by DB.");
                }
                RiskEvent::InvalidSignalReceived {
                    token_id, .. // Capture fields with ..
                } => {
                    trace!(
                        "DBActor: InvalidSignalReceived for {} - Not directly handled by DB.",
                        token_id
                    );
                }
                RiskEvent::StatusCheck => trace!("DBActor: Risk StatusCheck ignored."),
                RiskEvent::InsufficientFunds {
                    token_id, .. // Capture fields with ..
                } => {
                    trace!(
                        "DBActor: InsufficientFunds for {} - Not directly handled by DB.",
                        token_id
                    );
                }
                RiskEvent::TradeSizeAdjusted {
                    token_id, .. // Capture fields with ..
                } => {
                    trace!(
                        "DBActor: TradeSizeAdjusted for {} - Not directly handled by DB.",
                        token_id
                    );
                }
                RiskEvent::TradingHalted { token_id, reason, timestamp } => {
                        error!(
                        "DBActor: TradingHalted Event - Token ID: {}, Reason: '{}', Timestamp: {}",
                        token_id, reason, timestamp
                    );
                    // TODO: Future: Consider if this needs to be persisted in a dedicated table or if logging is sufficient.
                }
            },
            Event::Strategy(strategy_event) => match strategy_event {
                StrategyEvent::Signal {
                    token_id, .. // Capture other fields with ..
                } => {
                    trace!(
                        "DBActor: StrategySignal for {} - Not directly handled by DB.",
                            token_id
                        );
                    }
                StrategyEvent::StatusCheck => trace!("DBActor: Strategy StatusCheck ignored."),
            },
            Event::DexTransaction(dex_tx_event) => {
                match dex_tx_event {
                    DexTransactionEvent::StatusUpdated { status } => {
                        if let Err(e) = self.log_dex_transaction_status(&status).await {
                            error!("DatabaseActor: Failed to log DEX transaction status for tx {:?}: {}", status, e);
                        }
                    }
                    DexTransactionEvent::Submitted { tx_id, .. } => {
                        trace!("DatabaseActor: Received DexTransactionEvent::Submitted for {}, no action taken.", tx_id);
                    }
                }
            }
            Event::Database(db_event) => {
                trace!(
                    "DatabaseActor received self-generated DatabaseEvent: {:?}, ignoring.",
                    db_event
                );
            }
        }
        Ok(())
    }

    async fn touch_last_activity(&mut self) {
        *self.last_activity_time.lock().unwrap() = Instant::now();
    }

    async fn process_events_internal(&mut self, mut event_rx: mpsc::Receiver<Event>) {
        info!("DatabaseActor internal event processing loop starting.");
        while let Some(event) = event_rx.recv().await {
            if !self.running || self.shutdown_flag.load(Ordering::Relaxed) {
                info!("DatabaseActor: Shutting down internal event processor.");
                break;
            }
            if let Err(e) = self.handle_event(event).await {
                error!(
                    "DatabaseActor: Error handling event in internal loop: {}",
                    e
                );
            }
        }
        info!("DatabaseActor internal event processing loop finished.");
    }
}

#[async_trait::async_trait]
impl Actor for DatabaseActor {
    fn start(
        &mut self,
    ) -> impl std::future::Future<Output = crate::core::error::Result<()>> + Send {
        self.running = true;
        info!("🚀 DatabaseActor starting...");
        *self.last_activity_time.lock().unwrap() = Instant::now();

        // Start the batch processing task if queues are available
        if self.position_queue.is_some() || self.trade_queue.is_some() {
            let repo_factory_clone = self.repo_factory.clone();
            let position_queue_clone = self.position_queue.clone();
            let trade_queue_clone = self.trade_queue.clone();
            let config_clone = self.config.clone();
            let shutdown_flag_clone = self.shutdown_flag.clone(); // Clone shutdown_flag

            self.task_handle = Some(tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(
                    config_clone.database.batch_interval_secs,
                ));
                while !shutdown_flag_clone.load(Ordering::Relaxed) {
                    // Check shutdown_flag
                    interval.tick().await;
                    if crate::domain::trading::execution::bot::is_forced_shutdown()
                        || shutdown_flag_clone.load(Ordering::Relaxed)
                    {
                        info!("DatabaseActor batch processing task shutting down due to signal.");
                        break;
                    }

                    debug!("DatabaseActor: Running batch processing task...");
                    match Self::process_batch(
                        &repo_factory_clone,
                        position_queue_clone.as_deref(),
                        trade_queue_clone.as_deref(),
                        &config_clone,
                    )
                    .await
                    {
                        Ok(_) => debug!("Database batch processed successfully."),
                        Err(e) => error!("Error processing database batch: {}", e),
                    }
                }
                info!("DatabaseActor batch processing task stopped.");
            }));
        }

        let mut self_clone = self.clone();
        let event_tx_for_subscription = self.event_sender.clone(); // Use the actor's own sender

        async move {
            // Subscribe to DexTransactionEvents
            if let Err(e) = self_clone
                .message_bus
                .subscribe(
                    format!("{:?}", EventType::DexTransaction), // Ensure EventType can be formatted
                    event_tx_for_subscription.clone(),
                )
                .await
            {
                error!(
                    "DatabaseActor failed to subscribe to DexTransaction events: {}",
                    e
                );
                // Depending on policy, may want to return Err(e) here
            } else {
                info!("DatabaseActor subscribed to DexTransaction events.");
            }

            // Start the main event processing loop using the actor's own receiver
            if let Some(event_rx_for_loop) = self_clone.event_receiver.take() {
                self_clone.process_events_internal(event_rx_for_loop).await;
            } else {
                error!(
                    "DatabaseActor event receiver was already taken. Cannot start main event loop."
                );
                return Err(Error::Internal("Event receiver unavailable".to_string()));
            }

            info!("DatabaseActor started successfully.");
            Ok(())
        }
    }

    fn stop(&mut self) -> crate::core::error::Result<()> {
        info!("Stopping DatabaseActor...");
        self.running = false;
        self.shutdown_flag.store(true, Ordering::Relaxed); // Signal background tasks to stop
        if let Some(_handle) = self.task_handle.take() {
            // Don't abort directly, let the task observe shutdown_flag
            info!("DatabaseActor background task was running, should stop soon.");
            // Optionally, can add a timed join here if synchronous stop is critical
            // tokio::spawn(async move { handle.await });
        }
        info!("DatabaseActor stopped.");
        Ok(())
    }

    fn handle_message(
        &mut self,
        msg: Message,
    ) -> impl std::future::Future<Output = crate::core::error::Result<()>> + Send {
        let mut self_clone = self.clone();
        async move {
            self_clone.touch_last_activity().await;
            trace!("DatabaseActor received message: {:?}", msg);
            match msg {
                Message::Event(event) => {
                    // Send event to the internal mpsc channel for processing by process_events_internal
                    if let Err(e) = self_clone.event_sender.send(event).await {
                        error!(
                            "Failed to send event to internal DatabaseActor channel: {}",
                            e
                        );
                    }
                }
                Message::Query(query, responder) => match query {
                    Query::GetStatus => {
                        let status_msg = format!(
                            "Running: {}, Last Activity: {:?}s ago, Pending Pos: {}, Pending Trades: {}",
                            self_clone.running,
                            self_clone.last_activity_time.lock().unwrap().elapsed().as_secs(),
                            "N/A", // Placeholder for position queue length
                            "N/A"  // Placeholder for trade queue length
                        );
                        let status = QueryResult::Status(status_msg);
                        if responder.send(Ok(status)).is_err() {
                            error!("Failed to send response for GetStatus query");
                        }
                    }
                    Query::GetMetrics => {
                        let metrics = self_clone.gather_metrics().await;
                        if responder.send(Ok(QueryResult::Metrics(metrics))).is_err() {
                            error!("Failed to send response for GetMetrics query");
                        }
                    }
                    Query::GetMaintenanceStatus => {
                        let (last_run, next_run) = self_clone.get_maintenance_status();
                        let result = Ok(QueryResult::MaintenanceStatus { last_run, next_run });
                        if responder.send(result).is_err() {
                            error!("Failed to send response for GetMaintenanceStatus query");
                        }
                    }
                    _ => {
                        // Catch-all for other unhandled queries
                        warn!("DatabaseActor received unhandled query: {:?}", query);
                        if responder
                            .send(Err(Error::NotImplemented(format!(
                                "Query {:?} not handled by DatabaseActor",
                                query
                            ))))
                            .is_err()
                        {
                            error!("Failed to send error for unhandled query");
                        }
                    }
                },
                Message::Command(Command::MaintenanceDb) => {
                    info!("Received MaintenanceDb command. Scheduling maintenance.");
                    let maint_self = self_clone.clone();
                    tokio::spawn(async move {
                        if let Err(e) = maint_self.run_maintenance_now().await {
                            error!("Manual database maintenance failed: {}", e);
                        }
                    });
                }
                Message::Command(Command::StartMaintenanceScheduler) => {
                    info!("Received StartMaintenanceScheduler command.");
                    // if self_clone.maintenance_task_handle.is_none() { // Temporarily comment out if problematic
                    //     self_clone.start_maintenance_scheduler_task().await;
                    // } else {
                    //     info!("Maintenance scheduler already running.");
                    // }
                    warn!("StartMaintenanceScheduler command is currently a no-op pending review of maintenance_task_handle.");
                }
                Message::Command(cmd) => {
                    warn!("DatabaseActor received unhandled command: {:?}", cmd);
                }
            }
            Ok(())
        }
    }
}
