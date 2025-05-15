use super::DatabaseEvent;
use super::{
    Actor, Command, Event, ExecutionEvent, MarketEvent, Message, Query, QueryResult, RiskEvent,
    StrategyEvent,
};
use crate::core::config::Config;
use crate::core::error::{Error, Result};
use crate::core::models::market::TokenMetrics;
use crate::core::models::token::TokenData;
use crate::domain::trading::risk::RiskManager;
use crate::domain::trading::strategy::Signal;
use crate::infra::actors::MessageBus;
use crate::infra::db::repositories::{PositionRepository, TokenRepository};
use chrono::Utc;
use log::{debug, error, info, trace, warn};
use serde_json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct RiskManagerActor {
    risk_manager: RiskManager,
    position_repo: Arc<PositionRepository>,
    token_repo: Arc<TokenRepository>,
    message_bus: Arc<MessageBus>,
    config: Arc<Config>,
    running: bool,
    max_position_size: f64,
    max_daily_loss: f64,
    max_drawdown: f64,
    current_daily_loss: f64,
    current_drawdown: f64,
    token_risks: HashMap<String, f64>,
    risk_scores: HashMap<String, f64>,
    positions: HashMap<String, Position>,
    daily_pnl: f64,
    last_activity: Arc<tokio::sync::Mutex<Instant>>,
}

#[derive(Clone, Debug)]
struct Position {
    symbol: String,
    size: f64,
    value: f64,
}

impl RiskManagerActor {
    pub fn new(
        token_repo: Arc<TokenRepository>,
        position_repo: Arc<PositionRepository>,
        message_bus: Arc<MessageBus>,
        max_position_size: f64,
        stop_loss_pct: f64,
        take_profit_pct: f64,
        config: Arc<Config>,
    ) -> Self {
        let risk_manager = RiskManager::new(max_position_size, max_position_size);

        // Read from 'config' before it is moved to 'self.config'
        let max_daily_loss_from_config = config.trading.max_daily_loss;
        let max_drawdown_from_config = config.trading.max_drawdown;

        Self {
            risk_manager,
            position_repo,
            token_repo,
            message_bus,
            config, // 'config' (the Arc) is moved here to self.config
            running: false,
            max_position_size,
            max_daily_loss: max_daily_loss_from_config, // Use the value read before the move
            max_drawdown: max_drawdown_from_config,     // Use the value read before the move
            current_daily_loss: 0.0,
            current_drawdown: 0.0,
            token_risks: HashMap::new(),
            risk_scores: HashMap::new(),
            positions: HashMap::new(),
            daily_pnl: 0.0,
            last_activity: Arc::new(tokio::sync::Mutex::new(Instant::now())),
        }
    }

    async fn handle_strategy_signal(
        &mut self,
        token_id: String,
        signal: Signal,
        confidence: f64,
    ) -> Result<()> {
        if !self.running {
            return Ok(());
        }

        // Normalize token_id to lowercase to match database storage format
        let normalized_id = TokenData::normalize_token_id(&token_id);

        // Log original and normalized token ID
        info!("🔍 Processing strategy signal for token {} (normalized to {}), Signal: {:?}, Confidence: {:.2}", 
             token_id, normalized_id, signal, confidence);

        // Check if the token already has price data / exists
        let token_actually_exists = match self.token_repo.token_exists(&normalized_id).await {
            Ok(exists) => exists,
            Err(e) => {
                error!("Failed to check if token '{}' (normalized from '{}') exists: {}. Discarding signal.", normalized_id, token_id, e);
                return Ok(()); // Don't proceed if DB check fails
            }
        };

        if !token_actually_exists {
            error!(
                "Token '{}' (normalized from '{}') does not exist in the database. Discarding {:?} signal.",
                normalized_id, token_id, signal
            );
            // Optionally, publish an event indicating a bad signal was received
            let bad_signal_event = Event::Risk(RiskEvent::InvalidSignalReceived {
                token_id: token_id.clone(), // original token_id
                reason: format!(
                    "Token '{}' (normalized to '{}') not found in database.",
                    token_id, normalized_id
                ),
                timestamp: Utc::now(),
            });
            if let Err(e) = self.message_bus.publish(bad_signal_event).await {
                warn!("Failed to publish InvalidSignalReceived event: {}", e);
            }
            return Ok(()); // Do not proceed for non-existent tokens
        }
        // If we reach here, token_actually_exists is true.
        debug!(
            "✅ Verified token '{}' (normalized from '{}') exists.",
            normalized_id, token_id
        );

        // For SELL signals, verify position exists before proceeding
        if signal == Signal::Sell {
            // Check if position exists in either paper trading or live trading mode
            let open_positions = match self.position_repo.get_open_positions().await {
                Ok(positions) => positions,
                Err(e) => {
                    error!("Failed to get open positions: {}", e);
                    Vec::new()
                }
            };

            info!(
                "📊 SELL signal check: Found {} open positions",
                open_positions.len()
            );

            for pos in &open_positions {
                debug!(
                    "Position: {}, Entry: ${:.4}, Current: ${:.4}",
                    pos.token_id, pos.entry_price, pos.current_price
                );
            }

            let exists = open_positions
                .iter()
                .any(|p| p.token_id.to_lowercase() == normalized_id.to_lowercase());

            if !exists {
                warn!(
                    "⚠️ Ignoring sell signal for token {} - no position exists",
                    normalized_id
                );
                return Ok(());
            } else {
                info!(
                    "✅ Position exists for {} - proceeding with sell signal",
                    normalized_id
                );
            }
        } else if signal == Signal::Buy {
            // For BUY signals, verify position doesn't already exist
            let open_positions = match self.position_repo.get_open_positions().await {
                Ok(positions) => positions,
                Err(e) => {
                    error!("Failed to get open positions: {}", e);
                    Vec::new()
                }
            };

            let exists = open_positions
                .iter()
                .any(|p| p.token_id.to_lowercase() == normalized_id.to_lowercase());

            if exists {
                warn!(
                    "⚠️ Ignoring buy signal for token {} - position already exists",
                    normalized_id
                );
                return Ok(());
            } else {
                info!(
                    "✅ No existing position for {} - proceeding with buy signal",
                    normalized_id
                );
            }
        }

        // Get token data and convert to TokenMetrics
        match self.token_repo.get_token_price_stats(&normalized_id).await {
            Ok(token_data) => {
                let token_metrics = crate::types::market::TokenMetrics::from(&token_data);

                // Check risk limits
                if self.current_daily_loss >= self.max_daily_loss {
                    info!(
                        "🚨 Daily loss limit exceeded: {:.2} > {:.2}",
                        self.current_daily_loss, self.max_daily_loss
                    );
                    let event = Event::Risk(RiskEvent::RiskLimitExceeded {
                        limit_type: "daily_loss".to_string(),
                        current: self.current_daily_loss,
                        max: self.max_daily_loss,
                        timestamp: Utc::now(),
                    });
                    self.message_bus.publish(event).await?;
                    return Ok(());
                }

                if self.current_drawdown >= self.max_drawdown {
                    info!(
                        "🚨 Drawdown limit exceeded: {:.2} > {:.2}",
                        self.current_drawdown, self.max_drawdown
                    );
                    let event = Event::Risk(RiskEvent::RiskLimitExceeded {
                        limit_type: "drawdown".to_string(),
                        current: self.current_drawdown,
                        max: self.max_drawdown,
                        timestamp: Utc::now(),
                    });
                    self.message_bus.publish(event).await?;
                    return Ok(());
                }

                // Calculate position size based on risk parameters
                let position_size = self.calculate_position_size(&token_metrics, confidence);

                // Double-check that position size is valid before publishing event
                if position_size <= 0.0 {
                    warn!("⚠️ Calculated position size for token {} is invalid: ${:.4} - skipping risk assessment", 
                          token_id, position_size);
                    return Ok(());
                }

                // Publish risk assessment event
                let event = Event::Risk(RiskEvent::RiskAssessment {
                    token_id: token_data.id.clone(),
                    signal,
                    confidence,
                    position_size,
                    timestamp: Utc::now(),
                });

                info!("📢 Publishing risk assessment event for token {}", token_id);

                self.message_bus.publish(event).await?;
                Ok(())
            }
            Err(e) => {
                error!(
                    "Error getting price data for token {}: {:?}",
                    normalized_id, e
                );
                Err(e)
            }
        }
    }

    fn calculate_position_size(&self, token: &TokenMetrics, confidence: f64) -> f64 {
        // Base position size on max position size and confidence
        let base_size = self.max_position_size * confidence;

        // Adjust for volatility
        let volatility_factor = 1.0 - (token.price_change_24h.abs() / 100.0);

        // Adjust for volume
        let volume_factor = (token.volume_24h / 1_000_000.0).min(1.0);

        // Calculate position size
        let position_size = base_size * volatility_factor * volume_factor;

        // Ensure we never return zero or negative position size
        // This prevents downstream issues with zero-size positions
        if position_size <= 0.01 {
            info!("⚠️ Calculated position size for {} was too small (${:.4}), using minimum size of $0.01", 
                  token.id, position_size);
            return 0.01;
        }

        position_size
    }

    async fn update_risk_metrics(&mut self, pnl: f64) -> Result<()> {
        // Update daily loss
        if pnl < 0.0 {
            self.current_daily_loss += pnl.abs();
        }

        // Update drawdown
        if pnl < 0.0 && pnl.abs() > self.current_drawdown {
            self.current_drawdown = pnl.abs();
        }

        // Publish risk metrics update
        let event = Event::Risk(RiskEvent::RiskMetricsUpdate {
            daily_loss: self.current_daily_loss,
            drawdown: self.current_drawdown,
            timestamp: Utc::now(),
        });

        self.message_bus.publish(event).await?;
        Ok(())
    }

    pub fn update_token_risk(&mut self, token_id: &str, risk_score: f64) {
        let normalized_id = TokenData::normalize_token_id(token_id);
        self.token_risks.insert(normalized_id, risk_score);
    }

    pub fn get_token_risk(&self, token_id: &str) -> Option<f64> {
        let normalized_id = TokenData::normalize_token_id(token_id);
        self.token_risks.get(&normalized_id).copied()
    }

    pub fn update_risk_score(&mut self, token_id: &str, score: f64) {
        let normalized_id = TokenData::normalize_token_id(token_id);
        self.risk_scores.insert(normalized_id, score);
    }

    pub fn get_risk_score(&self, token_id: &str) -> Option<f64> {
        let normalized_id = TokenData::normalize_token_id(token_id);
        self.risk_scores.get(&normalized_id).copied()
    }

    async fn handle_event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Market(market_event) => {
                match market_event {
                    MarketEvent::PriceUpdate {
                        token_id, price, ..
                    } => {
                        // Update position values based on price changes
                        if let Some(position) = self.positions.get_mut(&token_id) {
                            let old_value = position.value;
                            position.value = position.size * price;

                            // Update our daily P&L
                            self.daily_pnl += position.value - old_value;

                            // Check if we need to enforce risk limits
                            self.check_risk_limits(&token_id).await?;
                        }
                        Ok(())
                    }
                    _ => Ok(()),
                }
            }
            Event::Strategy(strategy_event) => match strategy_event {
                StrategyEvent::Signal {
                    token_id,
                    signal,
                    confidence,
                    ..
                } => {
                    let mut this = self.clone();
                    tokio::spawn(async move {
                        if let Err(e) = this
                            .handle_strategy_signal(token_id, signal, confidence)
                            .await
                        {
                            error!("Error handling strategy signal: {:?}", e);
                        }
                    });
                    Ok(())
                }
                _ => Ok(()),
            },
            Event::Execution(execution_event) => {
                match execution_event {
                    ExecutionEvent::PositionUpdate {
                        token_id,
                        current_price,
                        pnl,
                        timestamp,
                    } => {
                        debug!(
                            "RiskManager received PositionUpdate for {}: Price=${:.4}, Pnl=${:.2}",
                            token_id, current_price, pnl
                        );

                        // Fetch the full position details
                        match self.position_repo.get_position_by_token_id(&token_id).await {
                            Ok(Some((_position_id, position))) => {
                                let mut exit_reason: Option<String> = None;

                                // 1. Check Stop Loss
                                let stop_loss_price = position.entry_price
                                    * (1.0 - self.config.trading.stop_loss / 100.0);
                                if current_price <= stop_loss_price {
                                    exit_reason = Some(format!(
                                        "Stop Loss triggered: {:.4} <= {:.4}",
                                        current_price, stop_loss_price
                                    ));
                                }

                                // 2. Check Take Profit (only if not stopped out)
                                if exit_reason.is_none() {
                                    let take_profit_price = position.entry_price
                                        * (1.0 + self.config.trading.take_profit / 100.0);
                                    if current_price >= take_profit_price {
                                        exit_reason = Some(format!(
                                            "Take Profit triggered: {:.4} >= {:.4}",
                                            current_price, take_profit_price
                                        ));
                                    }
                                }

                                // 3. Check Trailing Stop (only if not stopped/profit taken and in profit)
                                if exit_reason.is_none() && current_price > position.entry_price {
                                    let trailing_stop_pct = self.config.trading.stop_loss / 2.0; // Example: 50% of stop loss
                                    let trailing_stop_price =
                                        position.highest_price * (1.0 - trailing_stop_pct / 100.0);
                                    if current_price <= trailing_stop_price {
                                        exit_reason = Some(format!("Trailing Stop triggered: {:.4} <= {:.4} (from high {:.4})", 
                                                                  current_price, trailing_stop_price, position.highest_price));
                                    }
                                }

                                // 4. Check MockStrategy Time-based Exit (Approximate check)
                                // Only apply if strategy is mock and no other exit triggered yet
                                if exit_reason.is_none() && self.config.trading.strategy == "mock" {
                                    // Use a fixed duration for now, adjusted for faster mock testing
                                    let mock_hold_duration = chrono::Duration::seconds(45); // Lowered from 5 minutes
                                    let current_hold_duration =
                                        Utc::now().signed_duration_since(position.entry_time); // Calculate duration
                                    debug!("Mock Check for {}: Current Hold = {}s, Required Hold = {}s",
                                           token_id, current_hold_duration.num_seconds(), mock_hold_duration.num_seconds()); // Log it
                                    if current_hold_duration >= mock_hold_duration {
                                        // Updated log message to reflect seconds
                                        exit_reason = Some(format!(
                                            "Mock strategy hold time exceeded (> {}s)",
                                            mock_hold_duration.num_seconds()
                                        ));
                                    }
                                }

                                // If an exit condition was met, publish RiskAssessment for SELL
                                if let Some(reason) = exit_reason {
                                    info!("🛑 Exit condition met for {}: {}. Publishing SELL RiskAssessment.", token_id, reason);
                                    let sell_event = Event::Risk(RiskEvent::RiskAssessment {
                                        token_id: token_id.clone(),
                                        signal: Signal::Sell,
                                        confidence: 1.0, // Rule-based exit
                                        position_size: position.size, // Use actual position size
                                        timestamp: Utc::now(),
                                    });
                                    if let Err(e) = self.message_bus.publish(sell_event).await {
                                        error!(
                                            "Failed to publish SELL RiskAssessment for {}: {}",
                                            token_id, e
                                        );
                                    }
                                } else {
                                    trace!(
                                        "No exit conditions met for {} based on PositionUpdate.",
                                        token_id
                                    );
                                }
                            }
                            Ok(None) => {
                                warn!("Received PositionUpdate for token '{}' but no corresponding open position found in DB.", token_id);
                            }
                            Err(e) => {
                                error!("Failed to fetch position details for '{}' during PositionUpdate handling: {}", token_id, e);
                            }
                        }
                        Ok(())
                    }
                    ExecutionEvent::OrderExecuted {
                        token_id,
                        size,
                        price,
                        ..
                    } => {
                        // Update position if we're tracking it
                        let position = Position {
                            symbol: token_id.clone(),
                            size,
                            value: size * price,
                        };

                        self.positions.insert(token_id.clone(), position);
                        self.update_exposure();

                        // Check risk limits after fill
                        self.check_risk_limits(&token_id).await?;

                        Ok(())
                    }
                    _ => Ok(()),
                }
            }
            Event::Risk(risk_event) => {
                match risk_event {
                    RiskEvent::PositionClosed {
                        token_id,
                        pnl,
                        exit_price,
                        size,
                        entry_price,
                        entry_time,
                        timestamp,
                        delete_position,
                        ..
                    } => {
                        // Remove the position and update P&L
                        self.positions.remove(&token_id);
                        self.daily_pnl += pnl;
                        self.update_exposure();

                        // Find position ID from database using token_id - use paper_trading from config
                        let is_paper = self.config.trading.paper_trading;

                        // Query the position ID directly with SQL
                        let mut conn =
                            match self.position_repo.get_database().get_connection().await {
                                Ok(conn) => conn,
                                Err(e) => {
                                    error!("Failed to get database connection: {}", e);
                                    return Ok(());
                                }
                            };

                        let position_id_result = conn.query_one(
                            "SELECT id FROM positions WHERE token_id = $1 AND is_paper = $2 LIMIT 1", 
                            &[&token_id, &is_paper]
                        ).await;

                        match position_id_result {
                            Ok(row) => {
                                let position_id: i64 = row.get(0);
                                info!(
                                    "Found existing position ID {} for token {}",
                                    position_id, token_id
                                );

                                // Now record the close using the found ID
                                if let Err(e) = self
                                    .position_repo
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
                                    error!(
                                        "Failed to record position close using fetched ID: {}",
                                        e
                                    );
                                }
                            }
                            Err(e) => {
                                error!("Failed to fetch position ID for token {}: {}", token_id, e);
                                // Optionally try fallback record_position_close if needed
                                /*
                                warn!("record_position_close_with_trade failed, attempting fallback record_position_close");
                                // Extract necessary arguments from the event
                                // ... (Need to ensure 'position' variable is accessible here or reconstructed)
                                let position_id = position.id; // Assuming Position struct has id field
                                let size = position.size; // Assuming Position struct has size field
                                let entry_price = position.entry_price; // Assuming Position struct has entry_price
                                let entry_time = position.entry_time; // Assuming Position struct has entry_time
                                let exit_time = timestamp; // Use the event timestamp as exit time

                                // Attempt to call record_position_close_with_trade again with full args
                                if let Err(e_fallback) = self.position_repo.record_position_close_with_trade(
                                    position_id,
                                    &token_id,
                                    exit_price,
                                    size,
                                    entry_price,
                                    entry_time,
                                    exit_time
                                ).await {
                                    error!("Failed to record position close in database (fallback call failed): {}", e_fallback);
                                } else {
                                    info!("Successfully recorded position close using fallback logic for {}", token_id);
                                }
                                */
                            }
                        }

                        // If delete_position flag is set, delete the position from database
                        if delete_position {
                            info!(
                                "Initiating position deletion for {} after risk-based closure",
                                token_id
                            );
                            // Replace delete_open_position with delete_position_by_token_id
                            if let Err(e) = self
                                .position_repo
                                .delete_position_by_token_id(&token_id)
                                .await
                            {
                                error!(
                                    "Failed to delete position {} from DB after closure: {}",
                                    token_id, e
                                );
                            }
                        }

                        Ok(())
                    }
                    _ => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }

    async fn check_risk_limits(&mut self, symbol: &str) -> Result<()> {
        // Check if we've exceeded max position size
        if let Some(position) = self.positions.get(symbol) {
            if position.size.abs() > self.max_position_size {
                warn!(
                    "Position size for {} exceeds maximum allowed: {} vs {}",
                    symbol, position.size, self.max_position_size
                );

                // Publish a risk event that we're over the maximum position size
                let risk_event = RiskEvent::RiskLimitExceeded {
                    limit_type: "max_position_size".to_string(),
                    current: position.size,
                    max: self.max_position_size,
                    timestamp: Utc::now(),
                };

                self.message_bus.publish(Event::Risk(risk_event)).await?;
            }
        }

        // Check if we've exceeded max daily loss
        if self.daily_pnl < -self.max_daily_loss {
            warn!(
                "Daily P&L exceeds maximum allowed loss: {} vs {}",
                self.daily_pnl, self.max_daily_loss
            );

            // Publish a risk event that we're over the maximum daily loss
            let risk_event = RiskEvent::RiskLimitExceeded {
                limit_type: "max_daily_loss".to_string(),
                current: self.daily_pnl,
                max: self.max_daily_loss,
                timestamp: Utc::now(),
            };

            self.message_bus.publish(Event::Risk(risk_event)).await?;
        }

        Ok(())
    }

    fn update_exposure(&mut self) {
        self.current_daily_loss = self
            .positions
            .values()
            .fold(0.0, |acc, pos| acc + pos.value.abs());
    }
}

impl Actor for RiskManagerActor {
    fn start(&mut self) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            info!("RiskManagerActor: Starting initialization");
            self.running = true;

            // Create channels for receiving events
            let (market_tx, mut market_rx) = tokio::sync::mpsc::channel(100);
            let (strategy_tx, mut strategy_rx) = tokio::sync::mpsc::channel(100);
            let (execution_tx, mut execution_rx) = tokio::sync::mpsc::channel(100);
            let (position_tx, mut position_rx) = tokio::sync::mpsc::channel(100);

            // Subscribe to market events
            info!("RiskManagerActor: Attempting to subscribe to market events...");
            self.message_bus
                .subscribe("market".to_string(), market_tx)
                .await?;
            info!("RiskManagerActor: Successfully subscribed to market events");

            // Subscribe to strategy events
            info!("RiskManagerActor: Attempting to subscribe to strategy events...");
            self.message_bus
                .subscribe("strategy".to_string(), strategy_tx)
                .await?;
            info!("RiskManagerActor: Successfully subscribed to strategy events");

            // Subscribe to execution events
            info!("RiskManagerActor: Attempting to subscribe to execution events...");
            self.message_bus
                .subscribe("execution".to_string(), execution_tx)
                .await?;
            info!("RiskManagerActor: Successfully subscribed to execution events");

            // Subscribe to position updates
            info!("RiskManagerActor: Attempting to subscribe to position updates...");
            self.message_bus
                .subscribe("position".to_string(), position_tx)
                .await?;
            info!("RiskManagerActor: Successfully subscribed to position updates");

            // Spawn market event handler
            info!("RiskManagerActor: Spawning market event handler task...");
            let mut this = self.clone();
            tokio::spawn(async move {
                info!("RiskManagerActor: Market event handler task started");
                while let Some(event) = market_rx.recv().await {
                    if let Err(e) = this.handle_event(event).await {
                        error!("Error handling market event: {}", e);
                    }
                }
                warn!("RiskManagerActor: Market event handler task exited unexpectedly!");
            });

            // Spawn strategy event handler
            info!("RiskManagerActor: Spawning strategy event handler task...");
            let mut this = self.clone();
            tokio::spawn(async move {
                info!("RiskManagerActor: Strategy event handler task started");
                while let Some(event) = strategy_rx.recv().await {
                    if let Err(e) = this.handle_event(event).await {
                        error!("Error handling strategy event: {}", e);
                    }
                }
                warn!("RiskManagerActor: Strategy event handler task exited unexpectedly!");
            });

            // Spawn execution event handler
            info!("RiskManagerActor: Spawning execution event handler task...");
            let mut this = self.clone();
            tokio::spawn(async move {
                info!("RiskManagerActor: Execution event handler task started");
                while let Some(event) = execution_rx.recv().await {
                    if let Err(e) = this.handle_event(event).await {
                        error!("Error handling execution event: {}", e);
                    }
                }
                warn!("RiskManagerActor: Execution event handler task exited unexpectedly!");
            });

            // Spawn position update handler
            info!("RiskManagerActor: Spawning position update handler task...");
            let mut this = self.clone();
            tokio::spawn(async move {
                info!("RiskManagerActor: Position update handler task started");
                while let Some(event) = position_rx.recv().await {
                    if let Err(e) = this.handle_event(event).await {
                        error!("Error handling position update event: {}", e);
                    }
                }
                warn!("RiskManagerActor: Position update handler task exited unexpectedly!");
            });

            info!("RiskManagerActor: All event handlers spawned successfully");
            info!("RiskManagerActor: Initialization complete - ready to process events");

            Ok(())
        }
    }

    fn stop(&mut self) -> Result<()> {
        self.running = false;
        info!("Stopping RiskManagerActor");
        Ok(())
    }

    fn handle_message(
        &mut self,
        msg: Message,
    ) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            match msg {
                Message::Event(event) => self.handle_event(event).await,
                Message::Command(cmd) => match cmd {
                    Command::Start => {
                        debug!("Starting RiskManagerActor");
                        self.start().await
                    }
                    Command::Stop => {
                        debug!("Stopping RiskManagerActor");
                        self.running = false;
                        Ok(())
                    }
                    Command::UpdateConfig(config_json) => {
                        // Just log config updates for now
                        debug!("Received config update: {:?}", config_json);
                        Ok(())
                    }
                    Command::MaintenanceDb => {
                        // Ignore maintenance command - not relevant for risk manager
                        debug!("Ignoring maintenance command - not relevant for RiskManagerActor");
                        Ok(())
                    }
                    Command::StartMaintenanceScheduler => {
                        // Ignore maintenance scheduler command - not relevant for risk manager
                        debug!("Ignoring maintenance scheduler command - not relevant for RiskManagerActor");
                        Ok(())
                    }
                    _ => {
                        // Ignore other commands
                        debug!("RiskManagerActor ignoring unsupported command");
                        Ok(())
                    }
                },
                Message::Query(query, responder) => match query {
                    Query::GetStatus => {
                        let status = format!(
                            "RiskManagerActor running: {}, Daily Loss: {:.2}, Drawdown: {:.2}",
                            self.running, self.current_daily_loss, self.current_drawdown
                        );
                        responder
                            .send(Ok(QueryResult::Status(status)))
                            .map_err(|e| {
                                Error::InvalidInput(format!(
                                    "Failed to send status response: {:?}",
                                    e
                                ))
                            })
                    }
                    Query::GetMetrics => {
                        let metrics = serde_json::json!({
                            "running": self.running,
                            "max_position_size": self.max_position_size,
                            "max_daily_loss": self.max_daily_loss,
                            "max_drawdown": self.max_drawdown,
                            "current_daily_loss": self.current_daily_loss,
                            "current_drawdown": self.current_drawdown,
                        });
                        responder
                            .send(Ok(QueryResult::Metrics(metrics)))
                            .map_err(|e| {
                                Error::InvalidInput(format!(
                                    "Failed to send metrics response: {:?}",
                                    e
                                ))
                            })
                    }
                    _ => responder
                        .send(Err(Error::InvalidInput(format!("Unsupported query type"))))
                        .map_err(|e| {
                            Error::InvalidInput(format!("Failed to send error response: {:?}", e))
                        }),
                },
            }
        }
    }
}
