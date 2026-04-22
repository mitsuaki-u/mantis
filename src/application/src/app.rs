use crate::application::actors::execution::ExecutionActor;
use crate::application::actors::market::MarketDataActor;
use crate::application::actors::risk_manager::RiskManagerActor;
use crate::application::actors::strategy::StrategyActor;
use crate::application::actors::supervisor::SupervisorActor;
use crate::application::actors::DatabaseActor;
use crate::application::errors::Error;
use crate::config::Config;
use crate::core::strategies::{factory, traits::Strategy};
use crate::infrastructure::database::repositories::RepositoryFactory;
use crate::infrastructure::database::Database;
use crate::infrastructure::dex::DexClient;
use crate::infrastructure::market::providers::{create_market_api, MarketDataProvider};
use crate::EventRouter;
// apply_defaults is defined in this file, no need to import it
use chrono::{DateTime, Utc};
use futures::pin_mut;
use log::{debug, error, info, warn};
use serde_json::{self, json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::time::Duration;

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
    event_router: Arc<EventRouter>,
    /// Actor references
    supervisor: Option<SupervisorActor>,
    market_actor_ref: Option<crate::application::actors::ActorRef>,
    strategy_actor_ref: Option<crate::application::actors::ActorRef>,
    risk_actor_ref: Option<crate::application::actors::ActorRef>,
    execution_actor_ref: Option<crate::application::actors::ActorRef>,
    database_actor_ref: Option<crate::application::actors::ActorRef>,
    /// Configuration
    config: Arc<Config>,
    /// Database instance
    db: Database, // Store Database object directly
    /// Session start time for exit summary
    start_time: Option<DateTime<Utc>>,
}

pub fn apply_defaults(config: &mut Value, defaults: &[(&str, Value)]) {
    for (key, default_value) in defaults {
        if config.get(key).is_none() {
            config[key] = default_value.clone();
        }
    }
}

impl TradingBotSystem {
    /// Create a new trading bot system
    pub fn new(db: Database, config: Config, event_router: Arc<EventRouter>) -> Self {
        Self {
            running: false,
            event_router,
            supervisor: None,
            market_actor_ref: None,
            strategy_actor_ref: None,
            risk_actor_ref: None,
            execution_actor_ref: None,
            database_actor_ref: None,
            config: Arc::new(config),
            db,
            start_time: None,
        }
    }

    /// Start the trading bot system
    pub async fn start(
        &mut self,
        strategy_name: &str,
        params: &serde_json::Value,
    ) -> Result<(), Error> {
        if self.running {
            return Err(Error::InvalidInput(
                "Trading bot is already running".to_string(),
            ));
        }

        info!(
            "Starting trading bot system with {} strategy",
            strategy_name
        );

        // Create the strategy instance using type-safe config structs
        let strategy = match strategy_name {
            "momentum" => {
                // Create base config from params with config defaults as fallback
                let mut config_value = params.clone();

                // Apply config defaults for missing values
                apply_defaults(
                    &mut config_value,
                    &[
                        (
                            "momentum_entry_threshold",
                            self.config.trading.signal_confidence_threshold.into(),
                        ),
                        ("min_volume", self.config.trading.min_volume.into()),
                        ("stop_loss_pct", self.config.trading.stop_loss.into()),
                        (
                            "max_volatility_24h",
                            self.config.trading.max_volatility_24h.into(),
                        ),
                        (
                            "indicator_profile",
                            self.config.trading.indicator_profile.clone().into(),
                        ),
                        ("rsi_weight", self.config.trading.rsi_weight.into()),
                        ("macd_weight", self.config.trading.macd_weight.into()),
                        (
                            "bollinger_weight",
                            self.config.trading.bollinger_weight.into(),
                        ),
                        ("volume_weight", self.config.trading.volume_weight.into()),
                    ],
                );

                // Deserialize into type-safe config struct
                let config: factory::MomentumConfig = serde_json::from_value(config_value)
                    .map_err(|e| {
                        Error::Trading(format!("Invalid momentum strategy config: {}", e))
                    })?;

                info!(
                    "Creating momentum strategy (threshold={:.3}, volume=${:.0}, stop_loss={:.1}%, paper={})",
                    config.momentum_entry_threshold,
                    config.min_volume,
                    config.stop_loss_pct,
                    !self.config.trading.live_trading
                );
                debug!("Strategy config: rsi_weight={:.2}, macd_weight={:.2}, bollinger_weight={:.2}, volume_weight={:.2}",
                    config.rsi_weight.unwrap_or(self.config.trading.rsi_weight),
                    config.macd_weight.unwrap_or(self.config.trading.macd_weight),
                    config.bollinger_weight.unwrap_or(self.config.trading.bollinger_weight),
                    config.volume_weight.unwrap_or(self.config.trading.volume_weight)
                );

                // Use our type-safe factory function
                factory::create_strategy(config)
                    .map_err(|e| Error::Trading(format!("Strategy creation failed: {}", e)))?
            }
            "rsi" => {
                // Create base config from params with config defaults as fallback
                let mut config_value = params.clone();

                // Apply config defaults for missing values
                apply_defaults(
                    &mut config_value,
                    &[
                        ("oversold_threshold", 30.0.into()),   // RSI oversold level
                        ("overbought_threshold", 70.0.into()), // RSI overbought level
                        ("min_volume", self.config.trading.min_volume.into()),
                        ("stop_loss_pct", self.config.trading.stop_loss.into()),
                        (
                            "max_volatility_24h",
                            self.config.trading.max_volatility_24h.into(),
                        ),
                    ],
                );

                // Deserialize into type-safe config struct
                let config: factory::RsiConfig = serde_json::from_value(config_value)
                    .map_err(|e| Error::Trading(format!("Invalid RSI strategy config: {}", e)))?;

                info!(
                    "Creating RSI strategy (oversold={:.1}, overbought={:.1}, volume=${:.0}, stop_loss={:.1}%, paper={})",
                    config.oversold_threshold,
                    config.overbought_threshold,
                    config.min_volume,
                    config.stop_loss_pct,
                    !self.config.trading.live_trading
                );

                // Use our type-safe factory function
                factory::create_strategy(config)
                    .map_err(|e| Error::Trading(format!("Strategy creation failed: {}", e)))?
            }
            _ => {
                return Err(Error::InvalidInput(format!(
                    "Unsupported strategy: {}",
                    strategy_name
                )));
            }
        };

        // Start the actor system
        self.start_actor_system(strategy, params).await?;

        Ok(())
    }

    /// Stop the trading bot system
    pub async fn stop(&mut self) -> Result<(), Error> {
        info!("Stopping trading bot system");

        if !self.running {
            debug!("Trading bot system is not running, nothing to stop");
            return Ok(());
        }

        // Display exit summary before shutdown
        let start_time = self.start_time.unwrap_or_else(Utc::now);
        let is_paper = !self.config.trading.live_trading;
        crate::core::utils::display::display_exit_summary(start_time, &self.db, is_paper).await;

        // Mark the system as not running first to prevent new operations
        self.running = false;

        // Use the supervisor to stop all actors
        if let Some(supervisor) = &self.supervisor {
            debug!("Stopping all actors via supervisor");
            supervisor.stop_all_actors().await?;
            info!("All actors stopped successfully");
        } else {
            warn!("No supervisor available, actors may not have been stopped cleanly");
        }

        Ok(())
    }

    /// Check if the trading bot is running
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Get the status of the trading bot system
    pub async fn get_status(&self) -> Result<serde_json::Value, Error> {
        let mut status = json!({
            "running": self.running,
            "paper_trading": !self.config.trading.live_trading,
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
            "paper_trading": !self.config.trading.live_trading,
            "market": null,
            "strategy": null,
            "risk": null,
            "execution": null,
            "database": null,
        });

        // Get status from each actor
        if let Some(ref market_ref) = self.market_actor_ref {
            if let Ok(status) = self.get_actor_status(market_ref).await {
                metrics["market"] = json!({"status": status});
            }
        }

        if let Some(ref strategy_ref) = self.strategy_actor_ref {
            if let Ok(status) = self.get_actor_status(strategy_ref).await {
                metrics["strategy"] = json!({"status": status});
            }
        }

        if let Some(ref risk_ref) = self.risk_actor_ref {
            if let Ok(status) = self.get_actor_status(risk_ref).await {
                metrics["risk"] = json!({"status": status});
            }
        }

        if let Some(ref execution_ref) = self.execution_actor_ref {
            if let Ok(status) = self.get_actor_status(execution_ref).await {
                metrics["execution"] = json!({"status": status});
            }
        }

        if let Some(ref database_ref) = self.database_actor_ref {
            if let Ok(status) = self.get_actor_status(database_ref).await {
                metrics["database"] = json!({"status": status});
            }
        }

        Ok(metrics)
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
                if let Err(e) = market_ref.send(crate::application::actors::Message::Command(
                    crate::application::actors::Command::UpdateConfig(market_config.clone()),
                )) {
                    warn!("Failed to send config update to market actor: {}", e);
                }
            }
        }

        if let Some(ref strategy_ref) = self.strategy_actor_ref {
            if let Some(strategy_config) = config.get("strategy") {
                // Strategy config is simplified - no confidence threshold needed
                let modified_config = strategy_config.clone();

                if let Err(e) = strategy_ref.send(crate::application::actors::Message::Command(
                    crate::application::actors::Command::UpdateConfig(modified_config),
                )) {
                    warn!("Failed to send config update to strategy actor: {}", e);
                }
            }
        }

        if let Some(ref risk_ref) = self.risk_actor_ref {
            if let Some(risk_config) = config.get("risk") {
                if let Err(e) = risk_ref.send(crate::application::actors::Message::Command(
                    crate::application::actors::Command::UpdateConfig(risk_config.clone()),
                )) {
                    warn!("Failed to send config update to risk actor: {}", e);
                }
            }
        }

        if let Some(ref execution_ref) = self.execution_actor_ref {
            if let Some(execution_config) = config.get("execution") {
                if let Err(e) = execution_ref.send(crate::application::actors::Message::Command(
                    crate::application::actors::Command::UpdateConfig(execution_config.clone()),
                )) {
                    warn!("Failed to send config update to execution actor: {}", e);
                }
            }
        }

        if let Some(ref database_ref) = self.database_actor_ref {
            if let Some(database_config) = config.get("database") {
                if let Err(e) = database_ref.send(crate::application::actors::Message::Command(
                    crate::application::actors::Command::UpdateConfig(database_config.clone()),
                )) {
                    warn!("Failed to send config update to database actor: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Run the trading bot in the foreground until stopped
    pub async fn run_foreground(&self, state_file_path: &std::path::Path) -> Result<(), Error> {
        info!("Running trading bot in foreground mode");
        info!("Watching state file at: {:?}", state_file_path);

        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(2));
        let mut tick_count = 0;

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
                // Check state file periodically
                _ = interval.tick() => {
                    tick_count += 1;
                    if tick_count % 5 == 0 {
                        debug!("Trading bot still running (tick {})", tick_count);
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

                            // Display exit summary as the last output before exiting
                            let start_time = self.start_time.unwrap_or_else(Utc::now);
                            let is_paper = !self.config.trading.live_trading;
                            crate::core::utils::display::display_exit_summary(start_time, &self.db, is_paper).await;

                            break;
                        },
                        Err(err) => {
                            error!("Error waiting for Ctrl+C: {}", err);
                        }
                    }
                }
            }
        }

        debug!("Trading bot foreground process exiting");

        // Set force shutdown one final time to ensure all background tasks exit
        set_forced_shutdown();

        // Short sleep to allow final cleanup (100ms)
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        Ok(())
    }

    /// Stop all actors with a timeout to prevent hanging
    async fn stop_all_actors_with_timeout(&self) {
        info!("Stopping all actors with timeout protection");

        // Send stop commands to all actors synchronously
        let mut stop_errors = Vec::new();

        if let Some(ref market_ref) = self.market_actor_ref {
            if let Err(e) = market_ref.send(crate::application::actors::Message::Command(
                crate::application::actors::Command::Stop,
            )) {
                stop_errors.push(format!("market: {}", e));
            }
        }
        if let Some(ref strategy_ref) = self.strategy_actor_ref {
            if let Err(e) = strategy_ref.send(crate::application::actors::Message::Command(
                crate::application::actors::Command::Stop,
            )) {
                stop_errors.push(format!("strategy: {}", e));
            }
        }
        if let Some(ref risk_ref) = self.risk_actor_ref {
            if let Err(e) = risk_ref.send(crate::application::actors::Message::Command(
                crate::application::actors::Command::Stop,
            )) {
                stop_errors.push(format!("risk: {}", e));
            }
        }
        if let Some(ref execution_ref) = self.execution_actor_ref {
            if let Err(e) = execution_ref.send(crate::application::actors::Message::Command(
                crate::application::actors::Command::Stop,
            )) {
                stop_errors.push(format!("execution: {}", e));
            }
        }
        if let Some(ref database_ref) = self.database_actor_ref {
            if let Err(e) = database_ref.send(crate::application::actors::Message::Command(
                crate::application::actors::Command::Stop,
            )) {
                stop_errors.push(format!("database: {}", e));
            }
        }

        // Give actors time to stop gracefully
        tokio::time::sleep(tokio::time::Duration::from_secs(
            crate::application::constants::GRACEFUL_SHUTDOWN_SECS,
        ))
        .await;

        // Report results
        if stop_errors.is_empty() {
            info!("All actor stop commands sent successfully");
        } else {
            let error_msg = format!(
                "Failed to send stop commands to: {}",
                stop_errors.join(", ")
            );
            error!("{}", error_msg);
        }

        // Fix the mutable borrow issue - just log without trying to modify supervisor
        if self.supervisor.is_some() {
            info!("Cleaning up supervisor and message bus");
        }

        info!("Actors shutdown process complete");
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
        let tokens_to_track = &self.config.trading.tokens_to_track;

        // Return the configured tokens to track
        // If empty, the MarketDataActor will discover tokens automatically up to max_tokens_to_scan limit
        tokens_to_track.clone()
    }

    /// Create the appropriate MarketApi instance based on configuration
    async fn create_market_api(&self) -> Result<Box<dyn MarketDataProvider>, Error> {
        // Use the factory function directly
        Ok(create_market_api(&self.config).await)
    }

    /// Get status from an actor
    async fn get_actor_status(
        &self,
        actor_ref: &crate::application::actors::ActorRef,
    ) -> Result<String, Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        if let Err(e) = actor_ref.send(crate::application::actors::Message::Query(
            crate::application::actors::Query::GetStatus,
            tx,
        )) {
            return Err(Error::InvalidInput(format!("Failed to send query: {}", e)));
        }

        match tokio::time::timeout(Duration::from_secs(1), rx).await {
            Ok(Ok(Ok(crate::application::actors::QueryResult::Status(status)))) => Ok(status),
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

    /// Start the actor system with the given strategy
    async fn start_actor_system(
        &mut self,
        strategy: Strategy,
        _params: &serde_json::Value,
    ) -> Result<(), Error> {
        debug!("Starting actor system with strategy: {:?}", strategy);

        // Create the supervisor first - it will manage all other actors
        // Changed health check interval from 30 seconds to 300 seconds (5 minutes)
        let supervisor_obj = SupervisorActor::new().with_health_check_interval(300);

        // Create an Arc for the global registry
        let supervisor_arc = Arc::new(supervisor_obj.clone());

        // Store a local reference
        self.supervisor = Some(supervisor_obj);

        // Also register it globally so it can be found from anywhere during recovery
        SupervisorActor::register_as_global(supervisor_arc);

        // Create repositories using the stored db instance
        let repo_factory =
            RepositoryFactory::new(self.db.clone(), !self.config.trading.live_trading);

        let token_repo = repo_factory.token_repository();
        let pos_repo = repo_factory.position_repository();

        // Get the tokens to track
        let tokens_to_track = self.get_tokens_to_track();
        if !tokens_to_track.is_empty() {
            debug!(
                "Will track the following tokens: {}",
                tokens_to_track.join(", ")
            );
        }

        // Create the market data actor
        let market_api = self.create_market_api().await?;
        let market_actor =
            MarketDataActor::new(self.config.clone(), market_api, self.event_router.clone());

        let market_ref =
            crate::application::actors::spawn_actor(market_actor, "market".to_string()).await?;
        self.market_actor_ref = Some(market_ref.clone());

        // Register with EventRouter for EventRouter
        if let Err(e) = self
            .event_router
            .register_actor("market".to_string(), market_ref.clone())
            .await
        {
            warn!("Failed to register market actor with EventRouter: {}", e);
        }

        // Create the dex client first for strategy actor
        let dex_client = if !self.config.trading.live_trading {
            // Paper trading if NOT live
            DexClient::new_paper_trading(&self.config)?
        } else {
            DexClient::new_live(&self.config, self.event_router.clone()).await?
        };
        let dex_client_arc = Arc::new(dex_client);

        // Create and start the strategy actor
        debug!("Creating strategy actor...");
        let strategy_actor = StrategyActor::new(
            token_repo.clone(),
            pos_repo.clone(),
            strategy.clone(),
            self.event_router.clone(),
            dex_client_arc.clone(),
            self.config.trading.take_profit,
            self.config.trading.stop_loss,
        );

        debug!(
            "StrategyActor initialized: Take Profit={:.1}%, Stop Loss={:.1}%, Price validation threshold={:.1}%",
            self.config.trading.take_profit,
            self.config.trading.stop_loss,
            crate::core::constants::MAX_PRICE_DISCREPANCY_THRESHOLD * 100.0
        );

        let strategy_ref =
            crate::application::actors::spawn_actor(strategy_actor, "strategy".to_string()).await?;
        self.strategy_actor_ref = Some(strategy_ref.clone());

        // Register with EventRouter for EventRouter
        if let Err(e) = self
            .event_router
            .register_actor("strategy".to_string(), strategy_ref.clone())
            .await
        {
            warn!("Failed to register strategy actor with EventRouter: {}", e);
        }

        // Create the AI Advisor actor — sits between StrategyActor and RiskManagerActor.
        // Intercepts BUY signals, runs them through Claude, and re-emits approved ones.
        let ai_advisor_actor = crate::application::actors::AIAdvisorActor::new(
            self.config.anthropic_api_key.clone(),
            self.event_router.clone(),
            self.config.trading.max_positions,
        );
        let ai_advisor_ref =
            crate::application::actors::spawn_actor(ai_advisor_actor, "ai_advisor".to_string()).await?;

        if let Err(e) = self
            .event_router
            .register_actor("ai_advisor".to_string(), ai_advisor_ref.clone())
            .await
        {
            warn!("Failed to register AI advisor actor: {}", e);
        }

        // Create the execution actor first, as its ref is needed by RiskManagerActor
        let execution_actor = ExecutionActor::new(
            token_repo.clone(),
            pos_repo.clone(),
            dex_client_arc.clone(),
            self.event_router.clone(),
            self.config.clone(),
        );
        debug!("Spawning execution actor");
        let execution_ref =
            crate::application::actors::spawn_actor(execution_actor, "execution".to_string())
                .await?;
        self.execution_actor_ref = Some(execution_ref.clone());

        // Register with EventRouter for EventRouter
        if let Err(e) = self
            .event_router
            .register_actor("execution".to_string(), execution_ref.clone())
            .await
        {
            warn!("Failed to register execution actor with EventRouter: {}", e);
        }
        debug!("Execution actor spawned and registered");

        debug!("Creating risk actor");
        let risk_actor = RiskManagerActor::new(
            token_repo.clone(),
            pos_repo.clone(),
            self.event_router.clone(),
            self.config.clone(),
        );
        let risk_ref =
            crate::application::actors::spawn_actor(risk_actor, "risk".to_string()).await?;
        self.risk_actor_ref = Some(risk_ref.clone());

        // Register with EventRouter for EventRouter
        if let Err(e) = self
            .event_router
            .register_actor("risk".to_string(), risk_ref.clone())
            .await
        {
            warn!("Failed to register risk actor with EventRouter: {}", e);
        }

        // Create the database actor
        // Pass the repo_factory directly
        let database_actor = DatabaseActor::new(
            repo_factory.clone(), // Pass factory
            self.event_router.clone(),
            self.config.clone(),
        );
        let database_ref =
            crate::application::actors::spawn_actor(database_actor, "database".to_string()).await?;
        self.database_actor_ref = Some(database_ref.clone());

        // Register with EventRouter for EventRouter
        if let Err(e) = self
            .event_router
            .register_actor("database".to_string(), database_ref.clone())
            .await
        {
            warn!("Failed to register database actor with EventRouter: {}", e);
        }

        // Register all actors with the supervisor for health monitoring and error recovery
        if let Some(supervisor) = &self.supervisor {
            supervisor
                .register_actor("market".to_string(), market_ref.clone())
                .await?;
            supervisor
                .register_actor("strategy".to_string(), strategy_ref.clone())
                .await?;
            supervisor
                .register_actor("execution".to_string(), execution_ref.clone())
                .await?;
            supervisor
                .register_actor("risk".to_string(), risk_ref.clone())
                .await?;
            supervisor
                .register_actor("database".to_string(), database_ref.clone())
                .await?;

            debug!("All actors registered with supervisor");

            // Start all actors through the supervisor
            supervisor.start_all_actors().await?;

            info!("All actors started through supervisor");

            // Enable health monitoring with basic configuration
            if let Err(e) = supervisor.watch_actors().await {
                warn!("Failed to start supervisor health monitoring: {}", e);
            } else {
                debug!("Supervisor health monitoring started");
            }
        } else {
            error!("Supervisor is not available, cannot register actors");
            return Err(Error::Internal(
                "Failed to initialize supervisor".to_string(),
            ));
        }

        // Wait for actors to initialize
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Setup event subscriptions between actors
        self.setup_event_subscriptions().await?;

        // Mark the system as running and record start time
        self.running = true;
        self.start_time = Some(Utc::now());

        Ok(())
    }

    /// Set up event subscriptions between actors for proper event flow
    async fn setup_event_subscriptions(&mut self) -> Result<(), Error> {
        debug!("Setting up event routing between actors");

        // Verify the routing setup
        self.event_router.setup_actor_routing().await?;

        // Wait a moment for routing to be established
        tokio::time::sleep(Duration::from_millis(
            crate::application::constants::ACTOR_SHUTDOWN_WAIT_MS,
        ))
        .await;

        info!("Event routing established successfully");
        Ok(())
    }
}
