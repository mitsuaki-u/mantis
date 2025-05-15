use crate::core::config::Config;
use crate::core::error::Error;
use crate::domain::dex::DexClient;
use crate::domain::trading::indicators::IndicatorWeights;
use crate::domain::trading::strategy::{MomentumStrategy, Position, Strategy};
use crate::infra::actors::{
    Actor, ActorRef, Command, DatabaseActor, ExecutionActor, MarketDataActor, Message, MessageBus,
    RiskManagerActor, StrategyActor, SupervisorActor,
};
use crate::infra::actors::{Event, RiskEvent};
use crate::infra::api::market::{create_market_api, MarketApi, MarketDataProvider};
use crate::infra::db::repositories::RepositoryFactory;
use crate::infra::db::Database;
use chrono::Utc;
use dirs;
use futures::pin_mut;
use log::{debug, error, info, trace, warn};
use serde_json::{self, json};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::mpsc;
use tokio::time::{interval, timeout, Duration};

// Global shutdown flag to ensure all tasks exit
static FORCE_SHUTDOWN: AtomicBool = AtomicBool::new(false);

// Check if we're in forced shutdown mode
pub fn is_forced_shutdown() -> bool {
    FORCE_SHUTDOWN.load(Ordering::SeqCst)
}

// Set force shutdown flag to ensure all tasks exit
pub fn set_forced_shutdown() {
    FORCE_SHUTDOWN.store(true, Ordering::SeqCst);
}

/// TradingBotSystem manages the actor-based trading system
pub struct TradingBotSystem {
    /// Has the bot been started?
    running: bool,
    /// The message bus for actor communication
    message_bus: Arc<MessageBus>,
    /// Actor references
    supervisor: Option<SupervisorActor>,
    market_actor_ref: Option<crate::actors::ActorRef>,
    strategy_actor_ref: Option<crate::actors::ActorRef>,
    risk_actor_ref: Option<crate::actors::ActorRef>,
    execution_actor_ref: Option<crate::actors::ActorRef>,
    database_actor_ref: Option<crate::actors::ActorRef>,
    /// Configuration
    config: Arc<Config>,
    /// Database instance
    db: Database, // Store Database object directly
    shutdown_flag: Arc<AtomicBool>,
}

impl TradingBotSystem {
    /// Create a new trading bot system
    pub fn new(db: Database, config: Config) -> Self {
        // Accept Database object
        let message_bus = MessageBus::instance();
        let bus_id = format!("{:p}", Arc::as_ptr(&message_bus));
        debug!(
            "Creating TradingBotSystem with global MessageBus instance [id: {}]",
            bus_id
        );
        trace!("This ensures all components share the same MessageBus instance for proper event routing");

        Self {
            running: false,
            message_bus,
            supervisor: None,
            market_actor_ref: None,
            strategy_actor_ref: None,
            risk_actor_ref: None,
            execution_actor_ref: None,
            database_actor_ref: None,
            config: Arc::new(config),
            db, // Store the db object
            shutdown_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Configure wide scan mode for token discovery
    pub fn with_wide_scan_mode(mut self, enabled: bool) -> Self {
        if enabled {
            info!("Enabling wide scan mode - the trading bot will process all available tokens");
        } else {
            debug!("Wide scan mode disabled - the trading bot will only process explicitly tracked tokens");
        }

        // Store in config for later use when creating market actor
        Arc::make_mut(&mut self.config).trading.wide_scan_mode = enabled;
        self
    }

    /// Create a new trading bot system with an existing MessageBus
    pub fn with_message_bus(mut self, bus: Arc<MessageBus>) -> Self {
        self.message_bus = bus;
        self
    }

    /// Start the trading bot system
    pub async fn start(
        &mut self,
        strategy_name: &str,
        params: &serde_json::Value,
    ) -> Result<(), Error> {
        info!(
            "Starting trading bot system with {} strategy",
            strategy_name
        );

        // Create the strategy instance
        let strategy = match strategy_name {
            "momentum" => {
                let threshold = params["threshold"].as_f64().unwrap_or(5.0);
                let min_volume = params["min_volume"].as_f64().unwrap_or(100000.0);
                let stop_loss = params["stop_loss"].as_f64().unwrap_or(5.0);
                let min_data_points = params["min_data_points"].as_u64().map(|p| p as usize);
                let risk_tolerance = params["risk_tolerance"].as_u64().map(|r| r as u8);
                let testing_mode = params["testing_mode"].as_str();

                // Get indicator weights from params
                let indicator_weights = if let Some(weights) = params.get("indicator_weights") {
                    Some(IndicatorWeights::new(
                        weights["rsi"].as_f64().unwrap_or(0.3),
                        weights["macd"].as_f64().unwrap_or(0.3),
                        weights["bollinger_bands"].as_f64().unwrap_or(0.2),
                        weights["volume"].as_f64().unwrap_or(0.2),
                    ))
                } else {
                    None
                };

                let mut strategy = MomentumStrategy::new(threshold, min_volume, stop_loss);

                // Then set the weights using the builder method
                if let Some(weights) = indicator_weights {
                    strategy = strategy.with_indicator_weights(weights);
                }

                if let Some(points) = min_data_points {
                    strategy = strategy.with_min_data_points(points);
                }

                if let Some(level) = risk_tolerance {
                    strategy = strategy.with_risk_tolerance(level as f64);
                }

                // Apply testing mode if provided
                if let Some(mode) = testing_mode {
                    let trading_mode = match mode.to_lowercase().as_str() {
                        "fast" => crate::trading::indicators::TradingMode::FastTest,
                        "ultra" => crate::trading::indicators::TradingMode::UltraFast,
                        _ => crate::trading::indicators::TradingMode::Production,
                    };
                    strategy = strategy.with_trading_mode(trading_mode);
                    info!("🔧 Setting trading mode to: {:?}", trading_mode);
                }

                crate::trading::strategy::Strategy::new(Box::new(strategy))
            }
            "mock" => {
                let threshold = params["threshold"].as_f64().unwrap_or(5.0);
                let min_volume = params["min_volume"].as_f64().unwrap_or(100000.0);
                let stop_loss = params["stop_loss"].as_f64().unwrap_or(5.0);
                let risk_tolerance = params["risk_tolerance"].as_u64().map(|r| r as u8);

                // Create base mock strategy
                let mut mock_strategy =
                    crate::trading::strategy::MockStrategy::new(threshold, min_volume, stop_loss);

                // Configure mock strategy based on risk tolerance
                if let Some(level) = risk_tolerance {
                    let hold_duration = match level {
                        0 => 300, // Conservative: 5 minutes
                        1 => 240, // Conservative-Moderate: 4 minutes
                        2 => 180, // Moderate: 3 minutes
                        3 => 120, // Moderate-Aggressive: 2 minutes
                        4 => 60,  // Aggressive: 1 minute
                        _ => 30,  // Very Aggressive: 30 seconds
                    };
                    mock_strategy = mock_strategy.with_hold_duration(hold_duration);

                    let success_rate = match level {
                        0 => 0.7,  // Conservative: 70% success
                        1 => 0.65, // Conservative-Moderate: 65% success
                        2 => 0.6,  // Moderate: 60% success
                        3 => 0.55, // Moderate-Aggressive: 55% success
                        4 => 0.5,  // Aggressive: 50% success
                        _ => 0.45, // Very Aggressive: 45% success
                    };
                    mock_strategy = mock_strategy.with_success_rate(success_rate);
                }

                // Set signal interval based on market scan interval if available
                let interval = params["interval"].as_u64().unwrap_or(60);
                mock_strategy = mock_strategy.with_signal_interval(interval);

                info!(
                    "🧪 Created mock strategy for testing with signal interval: {}s",
                    interval
                );
                crate::trading::strategy::Strategy::new(Box::new(mock_strategy))
            }
            _ => {
                error!("Unsupported strategy: {}", strategy_name);
                return Err(Error::InvalidInput(format!(
                    "Unsupported strategy: {}",
                    strategy_name
                )));
            }
        };

        // Start the actor system
        self.start_actor_system(strategy).await?;

        Ok(())
    }

    /// Stop the trading bot system
    pub async fn stop(&mut self) -> Result<(), Error> {
        info!("Stopping trading bot system");

        if !self.running {
            debug!("Trading bot system is not running, nothing to stop");
            return Ok(());
        }

        // Mark the system as not running first to prevent new operations
        self.running = false;

        // Use the supervisor to stop all actors if it exists
        if let Some(supervisor) = &self.supervisor {
            info!("Using supervisor to stop all actors");
            if let Err(e) = supervisor.stop_all_actors().await {
                error!("Error stopping actors through supervisor: {}", e);
                // Continue with direct shutdown as fallback
            } else {
                info!("All actors stopped successfully through supervisor");
                return Ok(());
            }
        }

        // Fallback: Direct stop of each actor if supervisor is not working
        info!("Fallback: Stopping each actor directly");

        if let Some(ref market_ref) = self.market_actor_ref {
            debug!("Stopping market actor");
            if let Err(e) = market_ref
                .send(crate::actors::Message::Command(
                    crate::actors::Command::Stop,
                ))
                .await
            {
                error!("Error stopping market actor: {}", e);
            }
        }

        if let Some(ref strategy_ref) = self.strategy_actor_ref {
            debug!("Stopping strategy actor");
            if let Err(e) = strategy_ref
                .send(crate::actors::Message::Command(
                    crate::actors::Command::Stop,
                ))
                .await
            {
                error!("Error stopping strategy actor: {}", e);
            }
        }

        if let Some(ref risk_ref) = self.risk_actor_ref {
            debug!("Stopping risk actor");
            if let Err(e) = risk_ref
                .send(crate::actors::Message::Command(
                    crate::actors::Command::Stop,
                ))
                .await
            {
                error!("Error stopping risk actor: {}", e);
            }
        }

        if let Some(ref execution_ref) = self.execution_actor_ref {
            debug!("Stopping execution actor");
            if let Err(e) = execution_ref
                .send(crate::actors::Message::Command(
                    crate::actors::Command::Stop,
                ))
                .await
            {
                error!("Error stopping execution actor: {}", e);
            }
        }

        if let Some(ref database_ref) = self.database_actor_ref {
            debug!("Stopping database actor");
            if let Err(e) = database_ref
                .send(crate::actors::Message::Command(
                    crate::actors::Command::Stop,
                ))
                .await
            {
                error!("Error stopping database actor: {}", e);
            }
        }

        // Wait a bit for actors to finish cleanup
        tokio::time::sleep(Duration::from_millis(500)).await;

        info!("Trading bot system stopped");
        Ok(())
    }

    /// Check if the trading bot is running
    pub fn is_running(&self) -> bool {
        self.running
    }

    // TODO: Add status checks for Actor subscriptions
    /// Get the status of the trading bot system
    pub async fn get_status(&self) -> Result<serde_json::Value, Error> {
        let mut status = json!({
            "running": self.running,
            "paper_trading": self.config.trading.paper_trading,
            "actors": {
                "market": false,
                "strategy": false,
                "risk": false,
                "execution": false,
                "database": false
            },
            "strategy_type": "",
            "tokens_tracked": self.get_tokens_to_track(),
            "started_at": "",
        });

        // Get status from each actor
        if let Some(ref market_ref) = self.market_actor_ref {
            if let Ok(actor_status) = self.get_actor_status(market_ref).await {
                status["actors"]["market"] = json!(actor_status.contains("running: true"));
            }
        }

        if let Some(ref strategy_ref) = self.strategy_actor_ref {
            if let Ok(actor_status) = self.get_actor_status(strategy_ref).await {
                status["actors"]["strategy"] = json!(actor_status.contains("running: true"));

                // Try to extract strategy type
                if let Some(start_idx) = actor_status.find("strategy: ") {
                    if let Some(end_idx) = actor_status[start_idx..].find(',') {
                        let strategy_type = &actor_status[start_idx + 10..start_idx + end_idx];
                        status["strategy_type"] = json!(strategy_type);
                    }
                }
            }
        }

        if let Some(ref risk_ref) = self.risk_actor_ref {
            if let Ok(actor_status) = self.get_actor_status(risk_ref).await {
                status["actors"]["risk"] = json!(actor_status.contains("running: true"));
            }
        }

        if let Some(ref execution_ref) = self.execution_actor_ref {
            if let Ok(actor_status) = self.get_actor_status(execution_ref).await {
                status["actors"]["execution"] = json!(actor_status.contains("running: true"));
            }
        }

        if let Some(ref database_ref) = self.database_actor_ref {
            if let Ok(actor_status) = self.get_actor_status(database_ref).await {
                status["actors"]["database"] = json!(actor_status.contains("running: true"));
            }
        }

        Ok(status)
    }

    /// Get metrics from the trading bot system
    pub async fn get_metrics(&self) -> Result<serde_json::Value, Error> {
        let mut metrics = json!({
            "running": self.running,
            "paper_trading": self.config.trading.paper_trading,
            "market": null,
            "strategy": null,
            "risk": null,
            "execution": null,
            "database": null,
        });

        // Get metrics from each actor
        if let Some(ref market_ref) = self.market_actor_ref {
            if let Ok(actor_metrics) = self.get_actor_metrics(market_ref).await {
                metrics["market"] = actor_metrics;
            }
        }

        if let Some(ref strategy_ref) = self.strategy_actor_ref {
            if let Ok(actor_metrics) = self.get_actor_metrics(strategy_ref).await {
                metrics["strategy"] = actor_metrics;
            }
        }

        if let Some(ref risk_ref) = self.risk_actor_ref {
            if let Ok(actor_metrics) = self.get_actor_metrics(risk_ref).await {
                metrics["risk"] = actor_metrics;
            }
        }

        if let Some(ref execution_ref) = self.execution_actor_ref {
            if let Ok(actor_metrics) = self.get_actor_metrics(execution_ref).await {
                metrics["execution"] = actor_metrics;
            }
        }

        if let Some(ref database_ref) = self.database_actor_ref {
            if let Ok(actor_metrics) = self.get_actor_metrics(database_ref).await {
                metrics["database"] = actor_metrics;
            }
        }

        Ok(metrics)
    }

    // TODO: This is not even being used, remove?
    /// Get the current positions from the execution actor
    pub async fn get_positions(&self) -> Result<Vec<Position>, Error> {
        if !self.running {
            return Err(Error::InvalidInput(
                "Trading bot is not running".to_string(),
            ));
        }

        if let Some(ref execution_ref) = self.execution_actor_ref {
            let (tx, rx) = tokio::sync::oneshot::channel();
            execution_ref
                .send(crate::actors::Message::Query(
                    crate::actors::Query::GetMetrics,
                    tx,
                ))
                .await?;

            match rx.await {
                Ok(Ok(crate::actors::QueryResult::Metrics(metrics))) => {
                    if let Some(positions_json) = metrics.get("positions") {
                        if let Some(positions_array) = positions_json.as_array() {
                            let mut positions = Vec::new();
                            for pos in positions_array {
                                if let (Some(token_id), Some(entry_price), Some(size)) = (
                                    pos.get("token_id").and_then(|v| v.as_str()),
                                    pos.get("entry_price").and_then(|v| v.as_f64()),
                                    pos.get("size").and_then(|v| v.as_f64()),
                                ) {
                                    positions.push(Position {
                                        token_id: token_id.to_string(),
                                        provider_id: token_id.to_string(), // Use token_id as provider_id
                                        coingecko_id: token_id.to_string(), // Also use token_id for coingecko_id
                                        entry_price,
                                        current_price: entry_price,
                                        highest_price: entry_price,
                                        size,
                                        unrealized_pnl: 0.0,
                                        entry_time: Utc::now(),
                                    });
                                }
                            }
                            return Ok(positions);
                        }
                    }
                    Err(Error::Parse(
                        "Failed to parse positions from metrics".to_string(),
                    ))
                }
                Ok(Ok(_)) => Err(Error::Parse("Unexpected query result type".to_string())),
                Ok(Err(e)) => Err(e),
                Err(e) => Err(Error::Task(format!(
                    "Failed to receive query response: {}",
                    e
                ))),
            }
        } else {
            Err(Error::InvalidInput(
                "Execution actor not initialized".to_string(),
            ))
        }
    }

    /// Update configuration for all actors
    pub async fn update_config(&self, config: &serde_json::Value) -> Result<(), Error> {
        if !self.running {
            return Err(Error::InvalidInput(
                "Trading bot is not running".to_string(),
            ));
        }

        info!("Updating configuration for all actors");

        // Update each actor with relevant config
        if let Some(ref market_ref) = self.market_actor_ref {
            if let Some(market_config) = config.get("market") {
                market_ref
                    .send(crate::actors::Message::Command(
                        crate::actors::Command::UpdateConfig(market_config.clone()),
                    ))
                    .await?;
            }
        }

        if let Some(ref strategy_ref) = self.strategy_actor_ref {
            if let Some(strategy_config) = config.get("strategy") {
                // Set confidence_threshold directly from config or use default
                let mut modified_config = strategy_config.clone();

                // If no confidence_threshold is specified, use the default from config
                if !modified_config.get("confidence_threshold").is_some() {
                    if let Ok(mut obj) = serde_json::from_value::<
                        serde_json::Map<String, serde_json::Value>,
                    >(modified_config.clone())
                    {
                        obj.insert(
                            "confidence_threshold".to_string(),
                            serde_json::json!(self.config.trading.threshold),
                        );
                        modified_config = serde_json::Value::Object(obj);
                    }
                }

                strategy_ref
                    .send(crate::actors::Message::Command(
                        crate::actors::Command::UpdateConfig(modified_config),
                    ))
                    .await?;
            }
        }

        if let Some(ref risk_ref) = self.risk_actor_ref {
            if let Some(risk_config) = config.get("risk") {
                risk_ref
                    .send(crate::actors::Message::Command(
                        crate::actors::Command::UpdateConfig(risk_config.clone()),
                    ))
                    .await?;
            }
        }

        if let Some(ref execution_ref) = self.execution_actor_ref {
            if let Some(execution_config) = config.get("execution") {
                execution_ref
                    .send(crate::actors::Message::Command(
                        crate::actors::Command::UpdateConfig(execution_config.clone()),
                    ))
                    .await?;
            }
        }

        if let Some(ref database_ref) = self.database_actor_ref {
            if let Some(database_config) = config.get("database") {
                database_ref
                    .send(crate::actors::Message::Command(
                        crate::actors::Command::UpdateConfig(database_config.clone()),
                    ))
                    .await?;
            }
        }

        Ok(())
    }

    /// Run the trading bot in the foreground until stopped
    pub async fn run_foreground(&self, state_file_path: &std::path::Path) -> Result<(), Error> {
        use futures::pin_mut;

        info!("Running trading bot in foreground mode");
        info!("Watching state file at: {:?}", state_file_path);

        let mut checkpoint_count = 0;

        // Create a future that will complete when Ctrl+C is pressed
        let ctrl_c = tokio::signal::ctrl_c();
        pin_mut!(ctrl_c);

        // Run until the state file is removed, the bot is stopped, or Ctrl+C is received
        loop {
            // Check if we should continue running
            if !self.running || !state_file_path.exists() {
                info!("State file removed or bot stopped, stopping trading bot");
                break;
            }

            // Use tokio::select! to wait for either a timeout or Ctrl+C
            tokio::select! {
                // Sleep for a few seconds before checking again
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(2)) => {
                    // Checkpoint state
                    if let Err(e) = self.checkpoint_state().await {
                        warn!("Failed to checkpoint trading bot state: {}", e);
                    } else {
                        checkpoint_count += 1;
                        if checkpoint_count % 5 == 0 {
                            debug!("Trading bot still running, checkpoint count: {}", checkpoint_count);
                        }
                    }
                },
                // Handle Ctrl+C signal
                ctrl_c_result = &mut ctrl_c => {
                    match ctrl_c_result {
                        Ok(()) => {
                            info!("Received Ctrl+C signal, gracefully stopping trading bot");

                            // We can't call self.stop() directly because run_foreground takes &self
                            // and stop() requires &mut self, so use stop_all_actors_with_timeout instead
                            info!("Sending stop commands to all actors...");
                            self.stop_all_actors_with_timeout().await;

                            // Clean up the state file
                            if let Err(e) = std::fs::remove_file(state_file_path) {
                                warn!("Failed to remove state file: {}", e);
                            }

                            // Set the global shutdown flag to make sure background tasks exit
                            set_forced_shutdown();

                            info!("Trading bot stopped, exiting now");
                            break;
                        },
                        Err(err) => {
                            error!("Error waiting for Ctrl+C: {}", err);
                        }
                    }
                }
            }
        }

        info!(
            "Trading bot foreground process exiting after {} checkpoints",
            checkpoint_count
        );

        // Set force shutdown one final time to ensure all background tasks exit
        set_forced_shutdown();

        // Short sleep to allow final cleanup
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        Ok(())
    }

    /// Stop all actors with a timeout to prevent hanging
    async fn stop_all_actors_with_timeout(&self) {
        info!("Stopping all actors with timeout protection");

        // Create a vector of stop futures
        let mut stop_futures = Vec::new();

        // Add stop commands for each actor
        if let Some(ref market_ref) = self.market_actor_ref {
            stop_futures.push(market_ref.send(crate::actors::Message::Command(
                crate::actors::Command::Stop,
            )));
        }
        if let Some(ref strategy_ref) = self.strategy_actor_ref {
            stop_futures.push(strategy_ref.send(crate::actors::Message::Command(
                crate::actors::Command::Stop,
            )));
        }
        if let Some(ref risk_ref) = self.risk_actor_ref {
            stop_futures.push(risk_ref.send(crate::actors::Message::Command(
                crate::actors::Command::Stop,
            )));
        }
        if let Some(ref execution_ref) = self.execution_actor_ref {
            stop_futures.push(execution_ref.send(crate::actors::Message::Command(
                crate::actors::Command::Stop,
            )));
        }
        if let Some(ref database_ref) = self.database_actor_ref {
            stop_futures.push(database_ref.send(crate::actors::Message::Command(
                crate::actors::Command::Stop,
            )));
        }

        // Wait for all actors to stop or timeout after 2 seconds
        match timeout(
            std::time::Duration::from_secs(2),
            futures::future::join_all(stop_futures),
        )
        .await
        {
            Ok(results) => {
                let success_count = results.iter().filter(|r| r.is_ok()).count();
                info!(
                    "Successfully stopped {}/{} actors",
                    success_count,
                    results.len()
                );

                if success_count < results.len() {
                    warn!("Some actors failed to stop gracefully, forcing shutdown");
                }
            }
            Err(_) => {
                warn!("Timeout waiting for actors to stop, forcing shutdown");
            }
        }

        // Fix the mutable borrow issue - just log without trying to modify supervisor
        if self.supervisor.is_some() {
            info!("Cleaning up supervisor and message bus");
        }

        info!("Actors shutdown process complete");
    }

    /// Checkpoint the bot's state to ensure it can recover if needed
    async fn checkpoint_state(&self) -> Result<(), Error> {
        if !self.running {
            return Err(Error::InvalidInput(
                "Trading bot is not running".to_string(),
            ));
        }

        // Get the current status
        let status = match self.get_status().await {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to get status for checkpoint: {}", e);
                return Err(e);
            }
        };

        // Get metrics if possible
        let metrics = self.get_metrics().await.unwrap_or_else(|e| {
            warn!("Failed to get metrics for checkpoint: {}", e);
            json!({
                "error": format!("Failed to get metrics: {}", e)
            })
        });

        // Create a combined state object
        let checkpoint = json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "status": status,
            "metrics": metrics,
            "config": {
                "paper_trading": self.config.trading.paper_trading,
                "scan_interval": self.config.data_collection.interval,
                "max_position_size": self.config.trading.max_position_size,
                "risk": {
                    "stop_loss_pct": self.config.trading.stop_loss,
                    "take_profit_pct": self.config.trading.take_profit
                }
            }
        });

        // Write the checkpoint to a file - use a temp file first and then rename
        let checkpoint_dir = dirs::config_dir()
            .ok_or_else(|| {
                Error::Config("Could not determine configuration directory".to_string())
            })?
            .join("honeybadger");

        // Create directory if it doesn't exist
        if !checkpoint_dir.exists() {
            info!("Creating checkpoint directory at: {:?}", checkpoint_dir);
            std::fs::create_dir_all(&checkpoint_dir).map_err(|e| {
                Error::Config(format!("Failed to create checkpoint directory: {}", e))
            })?;
        }

        let checkpoint_file = checkpoint_dir.join("trading_checkpoint.json");
        let temp_file = checkpoint_dir.join("trading_checkpoint.tmp.json");

        // Write to temp file first
        std::fs::write(&temp_file, serde_json::to_string_pretty(&checkpoint)?)
            .map_err(|e| Error::Io(format!("Failed to write checkpoint file: {}", e)))?;

        // Rename temp file to checkpoint file (atomic operation on most filesystems)
        std::fs::rename(&temp_file, &checkpoint_file)
            .map_err(|e| Error::Io(format!("Failed to finalize checkpoint file: {}", e)))?;

        Ok(())
    }

    /// Get health report from the supervisor
    pub async fn get_health_report(&self) -> Result<serde_json::Value, Error> {
        if !self.running {
            return Err(Error::InvalidInput(
                "Trading bot is not running".to_string(),
            ));
        }

        if let Some(supervisor) = &self.supervisor {
            info!("Retrieving actor health report from supervisor");
            supervisor.get_health_report().await
        } else {
            Err(Error::InvalidInput(
                "Supervisor is not available".to_string(),
            ))
        }
    }

    /// Restart a specific actor
    pub async fn restart_actor(&self, id: &str) -> Result<(), Error> {
        if !self.running {
            return Err(Error::InvalidInput(
                "Trading bot is not running".to_string(),
            ));
        }

        if let Some(supervisor) = &self.supervisor {
            info!("Requesting restart of actor '{}' via supervisor", id);
            supervisor.restart_actor(id).await
        } else {
            Err(Error::InvalidInput(
                "Supervisor is not available".to_string(),
            ))
        }
    }

    /// Get the list of tokens to track based on configuration
    fn get_tokens_to_track(&self) -> Vec<String> {
        // Define default tokens to track
        let default_tokens = vec![
            "bitcoin".to_string(),
            "ethereum".to_string(),
            "binancecoin".to_string(),
            "solana".to_string(),
            "cardano".to_string(),
        ];

        // Log whether we're in wide scan mode, as this affects how tokens_to_track is used
        if self.config.trading.wide_scan_mode {
            debug!("Wide scan mode is enabled - tokens_to_track will be used as a priority list, but all tokens will be processed");
        } else {
            debug!("Wide scan mode is disabled - only tokens in tokens_to_track will be processed");
        }

        // In the future, if tokens_to_track is added to config, this can be updated
        // For now, just return the default tokens
        default_tokens
    }

    /// Create the appropriate MarketApi instance based on configuration
    fn create_market_api(&self) -> Result<Box<dyn MarketDataProvider>, Error> {
        // Use the factory function directly
        Ok(create_market_api(&self.config))
    }

    /// Get status from an actor
    async fn get_actor_status(&self, actor_ref: &crate::actors::ActorRef) -> Result<String, Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        actor_ref
            .send(crate::actors::Message::Query(
                crate::actors::Query::GetStatus,
                tx,
            ))
            .await?;

        match tokio::time::timeout(Duration::from_secs(5), rx).await {
            Ok(Ok(Ok(crate::actors::QueryResult::Status(status)))) => Ok(status),
            Ok(Ok(Ok(_))) => Err(Error::Parse(
                "Unexpected query result type for status".to_string(),
            )),
            Ok(Ok(Err(e))) => Err(e),
            Ok(Err(e)) => Err(Error::Task(format!(
                "Failed to receive status response: {}",
                e
            ))),
            Err(_) => Err(Error::Task(
                "Timeout waiting for actor status response".to_string(),
            )),
        }
    }

    /// Get metrics from an actor
    async fn get_actor_metrics(
        &self,
        actor_ref: &crate::actors::ActorRef,
    ) -> Result<serde_json::Value, Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        actor_ref
            .send(crate::actors::Message::Query(
                crate::actors::Query::GetMetrics,
                tx,
            ))
            .await?;

        match tokio::time::timeout(Duration::from_secs(5), rx).await {
            Ok(Ok(Ok(crate::actors::QueryResult::Metrics(metrics)))) => Ok(metrics),
            Ok(Ok(Ok(_))) => Err(Error::Parse(
                "Unexpected query result type for metrics".to_string(),
            )),
            Ok(Ok(Err(e))) => Err(e),
            Ok(Err(e)) => Err(Error::Task(format!(
                "Failed to receive metrics response: {}",
                e
            ))),
            Err(_) => Err(Error::Task(
                "Timeout waiting for actor metrics response".to_string(),
            )),
        }
    }

    /// Start the actor system with the given strategy
    async fn start_actor_system(&mut self, strategy: Strategy) -> Result<(), Error> {
        info!("Starting actor system with strategy: {:?}", strategy);

        // Log wide scan mode status early
        if self.config.trading.wide_scan_mode {
            info!("🔍 Wide scan mode is ENABLED - the system will process ALL available tokens from API");
        } else {
            info!("🔍 Wide scan mode is DISABLED - the system will ONLY process specifically tracked tokens");
        }

        // Create the supervisor first - it will manage all other actors
        let supervisor_obj =
            SupervisorActor::new(self.message_bus.clone()).with_health_check_interval(30); // Check health every 30 seconds

        // Create an Arc for the global registry
        let supervisor_arc = Arc::new(supervisor_obj.clone());

        // Store a local reference
        self.supervisor = Some(supervisor_obj);

        // Also register it globally so it can be found from anywhere during recovery
        info!("Registering supervisor in global registry for recovery operations");
        SupervisorActor::register_as_global(supervisor_arc);

        // Create repositories using the stored db instance
        let repo_factory = RepositoryFactory::new(self.db.clone(), self.config.clone());

        // NO NEED to create Database instance here anymore
        // let db = crate::db::Database::new()?;
        // let repo_factory = crate::repositories::RepositoryFactory::new(db) ...

        let token_repo = repo_factory.token_repository();
        let pos_repo = repo_factory.position_repository();

        // Get the tokens to track
        let tokens_to_track = self.get_tokens_to_track();
        info!(
            "🔍 Will track the following tokens: {}",
            tokens_to_track.join(", ")
        );

        // Create the market data actor
        let market_api = self.create_market_api()?;
        let market_actor = MarketDataActor::new(
            self.config.clone(),
            market_api,
            token_repo.clone(),
            self.message_bus.clone(),
        );

        let market_ref = crate::actors::spawn_actor(market_actor, "market".to_string()).await?;
        self.market_actor_ref = Some(market_ref.clone());

        // Create the strategy actor
        let strategy_actor = StrategyActor::new(
            token_repo.clone(),
            strategy,
            self.message_bus.clone(),
            self.config.clone(),
        );
        let strategy_ref =
            crate::actors::spawn_actor(strategy_actor, "strategy".to_string()).await?;
        self.strategy_actor_ref = Some(strategy_ref.clone());

        // Create the risk manager actor
        let risk_config = &self.config.trading;
        let risk_actor = RiskManagerActor::new(
            token_repo.clone(),
            pos_repo.clone(),
            self.message_bus.clone(),
            risk_config.max_position_size,
            risk_config.stop_loss,
            risk_config.take_profit,
            self.config.clone(),
        );
        let risk_ref = crate::actors::spawn_actor(risk_actor, "risk".to_string()).await?;
        self.risk_actor_ref = Some(risk_ref.clone());

        // Create the execution actor
        let dex_client = if self.config.trading.paper_trading {
            // Always use paper trading client if paper trading is enabled, regardless of testnet setting
            info!("Paper trading enabled, using paper trading DEX client");
            DexClient::new_paper_trading()
        } else if self.config.dex.testnet {
            // Only use testnet client when paper trading is false but testnet is true
            info!("Testnet trading enabled, using testnet DEX client");
            DexClient::new_testnet(&self.config)?
        } else {
            // Live trading
            info!("Live trading enabled, using live DEX client");
            DexClient::new_live()
        };
        let execution_actor = ExecutionActor::new(
            token_repo.clone(),
            pos_repo.clone(),
            dex_client,
            self.message_bus.clone(),
            self.config.clone(),
        );
        let execution_ref =
            crate::actors::spawn_actor(execution_actor, "execution".to_string()).await?;
        self.execution_actor_ref = Some(execution_ref.clone());

        // Create the database actor
        // Pass the repo_factory directly
        let database_actor = DatabaseActor::new(
            repo_factory.clone(), // Pass factory
            self.message_bus.clone(),
            self.config.clone(),
        );
        let database_ref =
            crate::actors::spawn_actor(database_actor, "database".to_string()).await?;
        self.database_actor_ref = Some(database_ref.clone());

        // Register all actors with the supervisor
        if let Some(supervisor) = &self.supervisor {
            debug!("Registering actors with supervisor...");

            if let Some(ref market_ref) = self.market_actor_ref {
                supervisor
                    .register_actor("market".to_string(), market_ref.clone())
                    .await?;
                debug!("Market actor registered with supervisor");
            }

            if let Some(ref strategy_ref) = self.strategy_actor_ref {
                supervisor
                    .register_actor("strategy".to_string(), strategy_ref.clone())
                    .await?;
                debug!("Strategy actor registered with supervisor");
            }

            if let Some(ref risk_ref) = self.risk_actor_ref {
                supervisor
                    .register_actor("risk".to_string(), risk_ref.clone())
                    .await?;
                debug!("Risk actor registered with supervisor");
            }

            if let Some(ref execution_ref) = self.execution_actor_ref {
                supervisor
                    .register_actor("execution".to_string(), execution_ref.clone())
                    .await?;
                debug!("Execution actor registered with supervisor");
            }

            if let Some(ref database_ref) = self.database_actor_ref {
                supervisor
                    .register_actor("database".to_string(), database_ref.clone())
                    .await?;
                debug!("Database actor registered with supervisor");
            }

            // Start all actors through the supervisor
            debug!("Starting all actors through supervisor...");
            supervisor.start_all_actors().await?;

            // Start health monitoring through supervisor
            debug!("Starting actor health monitoring...");
            supervisor.watch_actors().await?;

            // Database maintenance is now handled directly by the database actor
            debug!("Database actor will handle its own maintenance");

            info!("All actors started successfully through supervisor");
        } else {
            // If supervisor creation failed
            return Err(Error::Internal("Failed to create supervisor".to_string()));
        }

        // A small delay to ensure all actors are fully initialized
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Set up event subscriptions after all actors are started
        self.setup_event_subscriptions().await?;

        // Mark the system as running
        self.running = true;

        info!("Actor system started successfully");
        Ok(())
    }

    /// Set up event subscriptions between actors for proper event flow
    async fn setup_event_subscriptions(&mut self) -> Result<(), Error> {
        debug!("Setting up event subscriptions between actors");

        // Get supervisor reference - we need this to establish connections
        let supervisor = match &self.supervisor {
            Some(supervisor) => supervisor,
            None => {
                error!("Supervisor not initialized - cannot set up event subscriptions");
                return Err(Error::Internal("Supervisor not initialized".to_string()));
            }
        };

        // Define the main event flow connections between actors
        // (source event type, target actor ID)
        let main_connections = [
            ("market", "strategy"),
            ("strategy", "risk"),
            ("risk", "execution"),
            // Also send database events to multiple actors for position tracking
            ("database", "strategy"),
            ("database", "risk"),
            ("database", "execution"),
        ];

        // Set up the main connections through the supervisor
        let connection_results = supervisor
            .establish_actor_connections(&main_connections)
            .await?;

        // Set up database subscriptions separately as they have special handling
        // Subscribe database actor to all other event types
        if self.database_actor_ref.is_some() {
            match supervisor.establish_database_connections("database").await {
                Ok(result) => {
                    // Critical check for risk events - needed for position tracking
                    if !result.get("risk").unwrap_or(&false) {
                        error!("CRITICAL: Database actor failed to subscribe to risk events - trades and positions may not be stored");
                    }
                }
                Err(e) => {
                    // Don't fail the whole setup if database subscriptions fail
                    error!("Failed to set up database subscriptions: {}", e);
                }
            }
        }

        // Check if any crucial connections failed
        let crucial_connections = ["market→strategy", "strategy→risk", "risk→execution"];
        for conn in crucial_connections.iter() {
            if !connection_results.get(*conn).unwrap_or(&false) {
                error!(
                    "Critical connection '{}' failed - system may not function correctly",
                    conn
                );
                return Err(Error::Task(format!(
                    "Failed to establish critical connection: {}",
                    conn
                )));
            }
        }

        // Log subscriber counts for verification
        let message_bus = self.message_bus.clone();
        let market_count = message_bus.get_subscriber_count("market").await;
        let strategy_count = message_bus.get_subscriber_count("strategy").await;
        let risk_count = message_bus.get_subscriber_count("risk").await;
        let execution_count = message_bus.get_subscriber_count("execution").await;
        let database_count = message_bus.get_subscriber_count("database").await;

        info!("Event subscriptions established: market({}), strategy({}), risk({}), execution({}), database({})",
              market_count, strategy_count, risk_count, execution_count, database_count);

        if database_count < 1 {
            warn!(
                "⚠️ No subscribers found for database events - sell orders may not work properly!"
            );
        }

        Ok(())
    }
}
