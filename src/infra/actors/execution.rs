use super::{Actor, Command, Event, ExecutionEvent, Message, Query, QueryResult, RiskEvent};
use super::{DatabaseEvent, MarketEvent, StrategyEvent};
use crate::core::config::Config;
use crate::core::error::Error;
use crate::domain::dex::DexClient;
use crate::domain::trading::strategy::Position as StrategyPosition;
use crate::domain::trading::strategy::Signal;
use crate::infra::db::repositories::{PositionRepository, TokenRepository};
use crate::infra::db::Database;
use chrono::Utc;
use log::{debug, error, info, trace, warn};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Arc as StdArc;
use tokio::time::{interval, Duration};

#[derive(Clone)]
pub struct ExecutionActor {
    token_repo: Arc<TokenRepository>,
    position_repo: Arc<PositionRepository>,
    dex_client: DexClient,
    message_bus: Arc<super::MessageBus>,
    config: Arc<Config>,
    running: bool,
    positions: Vec<Position>,
    periodic_task_running: StdArc<AtomicBool>,
    position_processing_map: StdArc<tokio::sync::Mutex<HashMap<String, bool>>>,
}

#[derive(Debug, Clone)]
struct Position {
    token_id: String,
    coingecko_id: String,
    entry_price: f64,
    current_price: f64,
    highest_price: f64,
    size: f64,
    unrealized_pnl: f64,
    entry_time: chrono::DateTime<Utc>,
}

impl ExecutionActor {
    pub fn new(
        token_repo: Arc<TokenRepository>,
        position_repo: Arc<PositionRepository>,
        dex_client: DexClient,
        message_bus: Arc<super::MessageBus>,
        config: Arc<Config>,
    ) -> Self {
        Self {
            token_repo,
            position_repo,
            dex_client,
            message_bus,
            config,
            running: false,
            positions: Vec::new(),
            periodic_task_running: StdArc::new(AtomicBool::new(false)),
            position_processing_map: StdArc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    pub async fn create_dex_client(config: &Config) -> Result<DexClient, Error> {
        if config.dex.testnet {
            info!(
                "🧪 Creating testnet DEX client for network: {:?}",
                config.dex.network
            );

            // Create testnet client
            let mut client = DexClient::new_testnet(config)?;

            // Try to connect wallet if available
            if let Some(wallet_config) = &config.dex.wallet {
                let private_key = if let Some(env_var) = &wallet_config.private_key_env {
                    std::env::var(env_var).map_err(|_| {
                        Error::Config(format!(
                            "Cannot load private key from environment variable: {}",
                            env_var
                        ))
                    })?
                } else if let Some(file_path) = &wallet_config.private_key_file {
                    std::fs::read_to_string(file_path)
                        .map_err(|e| {
                            Error::Config(format!(
                                "Cannot read private key file: {} - {}",
                                file_path, e
                            ))
                        })?
                        .trim()
                        .to_string()
                } else {
                    return Err(Error::Config(
                        "No private key configuration found for testnet trading".to_string(),
                    ));
                };

                // Connect wallet
                client.connect_wallet(&private_key).await?;
                info!("🔑 Successfully connected wallet for testnet trading");
            } else {
                warn!("⚠️ No wallet configuration found - testnet trading will not work without a wallet");
            }

            Ok(client)
        } else if config.trading.paper_trading {
            info!("📝 Creating paper trading DEX client");
            Ok(DexClient::new_paper_trading())
        } else {
            info!("🔴 Creating live trading DEX client");
            Ok(DexClient::new_live())
        }
    }

    async fn handle_risk_assessment(
        &mut self,
        token_id: String,
        signal: Signal,
        confidence: f64,
        position_size: f64,
    ) -> Result<(), Error> {
        info!(
            "📋 Processing risk assessment for {}: Signal={:?}, Confidence={:.1}%, Position Size=${:.2}",
            token_id, signal, confidence * 100.0, position_size
        );

        if !self.running {
            info!("🛑 Execution actor is not running, ignoring risk assessment");
            return Ok(());
        }

        // Handle buy orders
        if signal == Signal::Buy {
            // Get token data
            let token_data = match self.token_repo.get_token_price_stats(&token_id).await {
                Ok(data) => data,
                Err(e) => {
                    error!("Failed to get token data for {}: {}", token_id, e);
                    return Err(Error::Api(format!("Failed to get token data: {}", e)));
                }
            };

            let symbol = token_data.symbol.to_uppercase();
            let entry_price = token_data.price_usd;

            // Extra verification - check if token exists in DB (proxy for having basic data)
            let token_exists_in_db = match self.token_repo.token_exists(&token_id).await {
                Ok(exists) => exists,
                Err(e) => {
                    error!("Failed to check token existence for {}: {}", token_id, e);
                    return Err(Error::Database(e.to_string()));
                }
            };

            // If token doesn't even exist in the tokens table, something is wrong.
            if !token_exists_in_db {
                error!(
                    "Token {} not found in database, cannot create position.",
                    token_id
                );
                return Err(Error::NotFound(format!("Token {} not found", token_id)));
            }

            // Final validation of price - this is the critical check that prevents bad trades
            if entry_price <= 0.0 {
                error!(
                    "❌ Rejecting BUY order for {} due to invalid price: ${:.4}",
                    symbol, entry_price
                );
                return Ok(());
            }

            // Log whether we have verified price data
            if !token_exists_in_db {
                warn!("⚠️ Creating position for {} without verified price data, using retrieved price: ${:.4}", 
                     symbol, entry_price);
            }

            info!(
                "🔄 Executing BUY order for {} (${:.4}) with position size: ${:.2}",
                symbol, entry_price, position_size
            );

            // Validate position size
            if position_size <= 0.0 {
                info!(
                    "Skipping position creation for {} due to zero or negative size: ${:.4}",
                    symbol, position_size
                );
                return Ok(());
            }

            // Execute buy order
            let order_result = match self
                .dex_client
                .execute_order(
                    &token_id,
                    position_size,
                    entry_price,
                    true, // buy = true
                )
                .await
            {
                Ok(_) => {
                    info!(
                        "✅ Successfully executed buy order for {} at ${:.4}",
                        symbol, entry_price
                    );
                    true
                }
                Err(e) => {
                    error!("Failed to execute buy order for {}: {:?}", symbol, e);
                    false
                }
            };

            if order_result {
                // Record the position
                let position = Position {
                    token_id: token_id.clone(),
                    coingecko_id: token_id.clone(),
                    entry_price,
                    current_price: entry_price,
                    highest_price: entry_price,
                    size: position_size,
                    unrealized_pnl: 0.0,
                    entry_time: Utc::now(),
                };

                // Register the position
                self.positions.push(position.clone());

                // Also record the position in the database directly
                // This ensures the position is saved even if event forwarding fails
                let strategy_position = StrategyPosition {
                    token_id: position.token_id.clone(),
                    provider_id: position.token_id.clone(), // Using token_id as provider_id
                    coingecko_id: position.token_id.clone(), // Also use token_id for coingecko_id
                    entry_price: position.entry_price,
                    current_price: position.current_price,
                    highest_price: position.highest_price,
                    size: position.size,
                    unrealized_pnl: position.unrealized_pnl,
                    entry_time: position.entry_time,
                };

                // Check if position already exists in database before trying to create it
                let is_paper = self.config.trading.paper_trading;
                let position_exists = match self.position_repo.position_exists(&token_id).await {
                    Ok(exists) => exists,
                    Err(e) => {
                        error!("Failed to check if position exists: {}", e);
                        false
                    }
                };

                if position_exists {
                    info!("Position for {} already exists in database, skipping record_position_with_trade", token_id);
                } else {
                    // Only try to create position if it doesn't already exist
                    if let Err(e) = self
                        .position_repo
                        .record_position_with_trade(
                            &strategy_position,
                            position.entry_price,
                            position.size,
                            position.entry_time,
                        )
                        .await
                    {
                        // Check if this is a unique constraint error, which means the position was created in parallel
                        if e.to_string().contains("UNIQUE constraint failed") {
                            info!("Position for {} was created by another process, continuing normally", token_id);
                        } else {
                            error!("Failed to record position in database: {}", e);
                        }
                    } else {
                        info!("✅ Successfully recorded position with trade in database directly");
                    }
                }

                info!(
                    "📈 New position opened for {}: ${:.2} at ${:.4}",
                    symbol, position_size, entry_price
                );

                // Publish execution event
                let event = Event::Execution(ExecutionEvent::OrderExecuted {
                    token_id: token_id.clone(),
                    signal,
                    size: position_size,
                    price: entry_price,
                    timestamp: Utc::now(),
                });

                // Check subscriber counts before publishing to help diagnose delivery issues
                let db_subscribers = self.message_bus.get_subscriber_count("database").await;
                let execution_subscribers =
                    self.message_bus.get_subscriber_count("execution").await;

                debug!("DIAGNOSTICS: ExecutionActor publishing BUY OrderExecuted event for {} - db_subscribers={}, execution_subscribers={}",
                      symbol, db_subscribers, execution_subscribers);

                match self.message_bus.publish(event).await {
                    Ok(_) => info!("📣 Successfully published BUY execution event for {} (${:.4}) with size ${:.2}",
                                 symbol, entry_price, position_size),
                    Err(e) => error!("❌ Failed to publish BUY execution event for {}: {:?}", symbol, e)
                }

                // Force an immediate position check after creating a new position
                // This ensures we start monitoring this position right away
                let mut this = self.clone();
                tokio::spawn(async move {
                    // Small delay to let things settle
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    info!("Running immediate position check after new position creation");
                    if let Err(e) = this.check_positions().await {
                        error!("Error in post-creation position check: {:?}", e);
                    }
                });
            }
        }
        // Handle sell orders
        else if signal == Signal::Sell {
            // First, verify if this token has a position in the database
            let token_exists = match self.token_repo.token_exists(&token_id).await {
                Ok(exists) => exists,
                Err(e) => {
                    error!("Failed to check if token {} exists: {}", token_id, e);
                    false
                }
            };

            let is_paper = self.config.trading.paper_trading;
            let pos_in_db_exists = match self.position_repo.position_exists(&token_id).await {
                Ok(exists) => exists,
                Err(e) => {
                    error!("Failed to check if position exists for {}: {}", token_id, e);
                    false
                }
            };

            // Check if we have a position in memory
            let position_in_memory = self
                .positions
                .iter()
                .find(|p| p.token_id.to_lowercase() == token_id.to_lowercase())
                .cloned();

            let pos_in_memory_exists = position_in_memory.is_some();

            // If neither database nor memory has a position, this is a phantom sell signal
            if !pos_in_db_exists && !pos_in_memory_exists {
                // Log detailed diagnostic information to help trace the issue
                info!("📊 Diagnostic information for phantom sell signal:");
                info!("  - Token exists in database: {}", token_exists);
                info!("  - Paper trading mode: {}", is_paper);
                info!("  - Position in memory: {}", pos_in_memory_exists);
                info!("  - Position in database: {}", pos_in_db_exists);

                // This might be a race condition or a bug in the strategy logic
                info!("⚠️ This could indicate a race condition, stale data, or a mismatch between strategy and execution state");
                info!(
                    "⚠️ Ignoring sell signal for {} as no position exists to sell",
                    token_id
                );
                return Ok(());
            }

            // Find position in memory (case-insensitive comparison) - clone option to avoid ownership issues
            let position_in_memory = self
                .positions
                .iter()
                .find(|p| p.token_id.to_lowercase() == token_id.to_lowercase())
                .cloned();

            if let Some(pos) = &position_in_memory {
                info!(
                    "✅ Found position for {} in memory: Entry=${:.4}, Size=${:.2}, PnL=${:.2}",
                    token_id, pos.entry_price, pos.size, pos.unrealized_pnl
                );
            } else {
                info!(
                    "⚠️ No position found in memory for {}, checking database...",
                    token_id
                );
            }

            // Check database for position existence - more reliable than memory
            let position_exists = match self.position_repo.position_exists(&token_id).await {
                Ok(exists) => {
                    if exists {
                        info!(
                            "✅ Position for {} exists in database (is_paper={})",
                            token_id, is_paper
                        );
                        exists
                    } else {
                        error!("❌ No position exists in database for {} (is_paper={}). Aborting sell order!", token_id, is_paper);
                        return Ok(());
                    }
                }
                Err(e) => {
                    error!(
                        "❌ Error checking if position exists in database for {}: {}. Aborting!",
                        token_id, e
                    );
                    return Ok(());
                }
            };

            // Get the full position details if it exists
            let position_in_db = if position_exists {
                match self.position_repo.get_open_positions().await {
                    Ok(positions) => {
                        let position = positions
                            .into_iter()
                            .find(|p| p.token_id.to_lowercase() == token_id.to_lowercase());

                        if let Some(pos) = &position {
                            info!(
                                "✅ Successfully loaded full position details for {} from database",
                                token_id
                            );
                        } else {
                            error!("❓ Position exists but couldn't be loaded for {}. Possible race condition.", token_id);
                        }

                        position
                    }
                    Err(e) => {
                        error!("❌ Failed to load positions from database: {}", e);
                        None
                    }
                }
            } else {
                None
            };

            // Clone position values for logging to avoid borrow after move errors
            let pos_in_memory_exists = position_in_memory.is_some();
            let pos_in_db_exists = position_in_db.clone().is_some();

            // If position found in database but not in memory, update our memory
            if position_in_db.is_some() && position_in_memory.is_none() {
                info!(
                    "📝 Adding database position for {} to in-memory positions",
                    token_id
                );
                if let Some(pos) = position_in_db.clone() {
                    // Convert from database Position to local Position
                    let execution_position = Position {
                        token_id: pos.token_id.clone(),
                        coingecko_id: pos.coingecko_id.clone(),
                        entry_price: pos.entry_price,
                        current_price: pos.current_price,
                        highest_price: pos.highest_price,
                        size: pos.size,
                        unrealized_pnl: pos.unrealized_pnl,
                        entry_time: pos.entry_time,
                    };
                    self.positions.push(execution_position);
                }
            }

            // Use position from database if available, otherwise use memory position
            let position = if position_in_db.is_some() {
                let db_pos = position_in_db.unwrap();
                // Convert database position to our execution position type
                Some(Position {
                    token_id: db_pos.token_id.clone(),
                    coingecko_id: db_pos.coingecko_id.clone(),
                    entry_price: db_pos.entry_price,
                    current_price: db_pos.current_price,
                    highest_price: db_pos.highest_price,
                    size: db_pos.size,
                    unrealized_pnl: db_pos.unrealized_pnl,
                    entry_time: db_pos.entry_time,
                })
            } else {
                position_in_memory
            };

            // Check if we have a validated position to sell
            if let Some(position) = position {
                // Get current market data
                let current_price = match self
                    .token_repo
                    .get_token_price_stats(&position.token_id)
                    .await
                {
                    Ok(stats) => stats.price_usd,
                    Err(e) => {
                        error!(
                            "Failed to get price stats for {}: {:?}",
                            position.token_id, e
                        );
                        return Err(Error::Api(format!("Failed to get price stats: {}", e)));
                    }
                };

                info!(
                    "🛒 Processing SELL order for {} at price ${:.4}",
                    position.token_id, current_price
                );

                // *** FIX: Call process_sell_order to actually execute the sell ***
                // Confidence is passed but not currently used by process_sell_order
                if let Err(e) = self
                    .process_sell_order(&position.token_id, confidence)
                    .await
                {
                    error!(
                        "Failed to process sell order for {}: {}",
                        position.token_id, e
                    );
                    // Decide if we should return the error or just log it
                    // return Err(e);
                }
            } else {
                // Log detailed diagnostic information to help trace the issue
                info!("📊 Diagnostic information for phantom sell signal:");
                info!("  - Token exists in database: {}", token_exists);
                info!("  - Paper trading mode: {}", is_paper);
                info!("  - Position in memory: {}", pos_in_memory_exists);
                info!("  - Position in database: {}", pos_in_db_exists);

                // This might be a race condition or a bug in the strategy logic
                info!("⚠️ This could indicate a race condition, stale data, or a mismatch between strategy and execution state");
                info!(
                    "⚠️ Ignoring sell signal for {} as no position exists to sell",
                    token_id
                );
            }
        }

        Ok(())
    }

    async fn sync_positions_with_database(&mut self) -> Result<(), Error> {
        let is_paper = self.config.trading.paper_trading;

        // Use the position repository that's already part of the ExecutionActor
        match self.position_repo.get_open_positions().await {
            Ok(db_positions) => {
                // Store length before moving
                let position_count = db_positions.len();

                // Clear existing positions and reload from database
                self.positions.clear();

                for db_pos in db_positions {
                    let position = Position {
                        token_id: db_pos.token_id.clone(),
                        coingecko_id: db_pos.coingecko_id.clone(),
                        entry_price: db_pos.entry_price,
                        current_price: db_pos.current_price,
                        highest_price: db_pos.highest_price,
                        size: db_pos.size,
                        unrealized_pnl: db_pos.unrealized_pnl,
                        entry_time: db_pos.entry_time,
                    };
                    self.positions.push(position);
                }
                info!(
                    "✅ Successfully synchronized {} positions from database",
                    position_count
                );
                Ok(())
            }
            Err(e) => {
                let err_msg = format!("Failed to get positions from database: {}", e);
                error!("{}", err_msg);
                // Convert directly to Error::Database
                Err(Error::Database(err_msg))
            }
        }
    }

    async fn process_sell_order(&mut self, token_id: &str, confidence: f64) -> Result<(), Error> {
        let is_paper = self.config.trading.paper_trading;

        // First sync positions with database to ensure we have latest state
        self.sync_positions_with_database().await?;

        // Find position in memory (case-insensitive comparison)
        let position = self
            .positions
            .iter()
            .find(|p| p.token_id.to_lowercase() == token_id.to_lowercase())
            .cloned();

        match position {
            Some(pos) => {
                // Get the latest market data to ensure we have fresh prices
                let current_price = match self.token_repo.get_token_price_stats(token_id).await {
                    Ok(stats) => stats.price_usd,
                    Err(e) => {
                        error!("Failed to get latest price for {}: {}", token_id, e);
                        // Fallback to position's stored price, but log a warning
                        warn!(
                            "Using stored price ${:.4} for {} instead of fresh market data",
                            pos.current_price, token_id
                        );
                        pos.current_price
                    }
                };

                info!("Found position for {} in memory after sync: Entry=${:.4}, Size=${:.2}, Current=${:.4}", 
                      token_id, pos.entry_price, pos.size, current_price);

                // Execute sell order with fresh price data
                let order_result = match self
                    .dex_client
                    .execute_order(
                        token_id,
                        pos.size,
                        current_price, // Use the fresh price
                        false,         // buy = false for sell
                    )
                    .await
                {
                    Ok(_) => {
                        info!(
                            "✅ Successfully executed sell order for {} at ${:.4}",
                            token_id, current_price
                        );
                        true
                    }
                    Err(e) => {
                        error!("Failed to execute sell order for {}: {:?}", token_id, e);
                        false
                    }
                };

                if order_result {
                    // Remove position from memory
                    self.positions
                        .retain(|p| p.token_id.to_lowercase() != token_id.to_lowercase());

                    // Calculate updated PnL with fresh price
                    let pnl = (current_price - pos.entry_price) * pos.size;

                    // Create position close event with fresh price data
                    let risk_event = Event::Risk(RiskEvent::PositionClosed {
                        token_id: token_id.to_string(),
                        pnl,
                        timestamp: Utc::now(),
                        entry_price: pos.entry_price,
                        exit_price: current_price, // Use the fresh price
                        size: pos.size,
                        entry_time: pos.entry_time,
                        delete_position: true,
                    });

                    // Add diagnostic logging
                    debug!("🔍 DIAGNOSTIC: ExecutionActor sending PositionClosed event for token={} with pnl=${:.2}", token_id, pnl);

                    // Publish event
                    if let Err(e) = self.message_bus.publish(risk_event).await {
                        error!("Failed to publish position close event: {:?}", e);
                    } else {
                        debug!("🔍 DIAGNOSTIC: ExecutionActor successfully published PositionClosed event for {}", token_id);
                    }
                }
                Ok(())
            }
            None => {
                error!("❌ No position found for {} after database sync", token_id);
                Ok(())
            }
        }
    }

    async fn check_positions(&mut self) -> Result<(), Error> {
        if !self.running {
            debug!("Position check skipped - ExecutionActor not running");
            return Ok(());
        }

        // Sync positions with database first
        if let Err(e) = self.sync_positions_with_database().await {
            error!("Failed to sync positions with database: {:?}", e);
            return Err(e);
        }

        // Create a copy of positions to avoid iterator invalidation
        let positions_to_check = self.positions.clone();
        debug!("Checking {} positions", positions_to_check.len());

        // Process each position - only update states, don't make exit decisions
        for position in positions_to_check {
            let token_id = &position.token_id;

            // Check if this position is already being processed
            let processing_map = self.position_processing_map.clone();
            let mut processing_lock = processing_map.lock().await;
            if let Some(true) = processing_lock.get(token_id) {
                debug!(
                    "Position for {} is already being processed, skipping",
                    token_id
                );
                continue;
            }

            // Mark this position as being processed
            processing_lock.insert(token_id.clone(), true);
            drop(processing_lock);

            // Get current market data
            let current_price = match self.token_repo.get_token_price_stats(token_id).await {
                Ok(stats) => stats.price_usd,
                Err(e) => {
                    error!(
                        "Failed to get price stats for {}: {}. Skipping position check.",
                        token_id, e
                    );
                    let mut processing_lock = processing_map.lock().await;
                    processing_lock.remove(token_id);
                    continue;
                }
            };

            // Calculate P&L and metrics
            let pnl = (current_price - position.entry_price) * position.size;
            let profit_loss_pct = (current_price / position.entry_price - 1.0) * 100.0;

            // Update position state
            let mut position = position.clone();
            if current_price > position.highest_price {
                position.highest_price = current_price;
                debug!(
                    "Updated highest price for {}: ${:.4} -> ${:.4}",
                    token_id, position.highest_price, current_price
                );
            }

            position.current_price = current_price;
            position.unrealized_pnl = pnl;

            debug!(
                "Position state update for {}: Entry=${:.4}, Current=${:.4} ({:.2}%), PnL=${:.2}",
                token_id, position.entry_price, current_price, profit_loss_pct, pnl
            );

            // Update position in memory
            let pos_idx = self.positions.iter().position(|p| p.token_id == *token_id);
            if let Some(idx) = pos_idx {
                self.positions[idx] = position.clone();
            }

            // Update database
            if let Err(e) = self
                .position_repo
                .update_position(
                    &position.token_id,
                    current_price,
                    position.highest_price, // Pass existing highest price
                                            // unrealized_pnl is calculated internally by update_position now
                )
                .await
            {
                error!(
                    "Failed to update position data for {}: {}",
                    position.token_id, e
                );
            }

            // Create and publish the position update event
            let event = Event::Execution(ExecutionEvent::PositionUpdate {
                token_id: token_id.to_string(),
                current_price: current_price,
                pnl: pnl,
                timestamp: Utc::now(),
            });

            // Add diagnostic logging
            debug!("🔍 DIAGNOSTIC: ExecutionActor sending PositionUpdate event for token={} with price=${:.4} pnl=${:.2}", 
                  token_id, current_price, pnl);

            if let Err(e) = self.message_bus.publish(event).await {
                error!("Failed to publish position update event: {}", e);
            } else {
                debug!("🔍 DIAGNOSTIC: ExecutionActor successfully published PositionUpdate event for {}", token_id);
            }

            // Remove from processing map
            let mut processing_lock = processing_map.lock().await;
            processing_lock.remove(token_id);
        }

        Ok(())
    }

    async fn start_periodic_check(&self) -> Result<(), Error> {
        let running = self.periodic_task_running.clone();

        if running.load(Ordering::SeqCst) {
            debug!("Periodic position check is already running");
            return Ok(());
        }

        running.store(true, Ordering::SeqCst);

        let mut actor_clone = self.clone();

        info!("Starting periodic position check every 30 seconds");
        debug!(
            "Position check timer created, positions count: {}",
            actor_clone.positions.len()
        );

        tokio::spawn(async move {
            let mut check_interval = interval(Duration::from_secs(30));
            debug!("Position check interval initialized");

            // Run an initial check immediately
            debug!("Running initial position check");
            match actor_clone.check_positions().await {
                Ok(_) => debug!("Initial position check completed successfully"),
                Err(e) => error!("Error in initial position check: {:?}", e),
            }

            // Track consecutive failures to prevent rapid cycling on persistent errors
            let mut consecutive_failures = 0;

            while running.load(Ordering::SeqCst) {
                check_interval.tick().await;

                debug!("Periodic timer triggered, running position check");
                debug!("Current positions count: {}", actor_clone.positions.len());

                match actor_clone.check_positions().await {
                    Ok(_) => {
                        debug!("Position check completed successfully");
                        consecutive_failures = 0; // Reset on success
                    }
                    Err(e) => {
                        error!("Error in periodic position check: {:?}", e);
                        consecutive_failures += 1;

                        // If we've had too many consecutive failures, add a backoff
                        if consecutive_failures > 3 {
                            let backoff = Duration::from_secs(5 * consecutive_failures as u64);
                            warn!(
                                "Multiple position check failures ({}) - backing off for {:?}",
                                consecutive_failures, backoff
                            );
                            tokio::time::sleep(backoff).await;
                        }
                    }
                }
            }

            info!("Periodic position check task has ended");
        });

        Ok(())
    }

    async fn stop_periodic_check(&self) -> Result<(), Error> {
        info!("Stopping periodic position check");
        self.periodic_task_running.store(false, Ordering::SeqCst);
        Ok(())
    }
}

impl Actor for ExecutionActor {
    fn start(&mut self) -> impl std::future::Future<Output = Result<(), Error>> + Send {
        async move {
            self.running = true;
            info!("Starting ExecutionActor");

            // Log current positions
            debug!(
                "ExecutionActor positions at start: {}",
                self.positions.len()
            );

            // Run periodic check immediately, not just on timer
            let mut this1 = self.clone();
            tokio::spawn(async move {
                debug!("Running immediate position check");
                if let Err(e) = this1.check_positions().await {
                    error!("Error in immediate position check: {:?}", e);
                }
            });

            // Start periodic timer with more verbose logging
            let this2 = self.clone();
            tokio::spawn(async move {
                debug!("About to start periodic position check");
                if let Err(e) = this2.start_periodic_check().await {
                    error!("Failed to start periodic position check: {:?}", e);
                } else {
                    debug!("Successfully started periodic position check");
                }
            });

            Ok(())
        }
    }

    fn stop(&mut self) -> Result<(), Error> {
        self.running = false;
        info!("Stopping ExecutionActor");

        let this = self.clone();
        tokio::spawn(async move {
            if let Err(e) = this.stop_periodic_check().await {
                error!("Failed to stop periodic position check: {:?}", e);
            }
        });

        Ok(())
    }

    fn handle_message(
        &mut self,
        msg: Message,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send {
        async move {
            match msg {
                Message::Event(event) => match event {
                    Event::Risk(RiskEvent::RiskAssessment {
                        token_id,
                        signal,
                        confidence,
                        position_size,
                        ..
                    }) => {
                        if self.running {
                            let mut this = self.clone();
                            tokio::spawn(async move {
                                if let Err(e) = this
                                    .handle_risk_assessment(
                                        token_id,
                                        signal,
                                        confidence,
                                        position_size,
                                    )
                                    .await
                                {
                                    error!("Error handling risk assessment: {:?}", e);
                                }
                            });
                        }
                        Ok(())
                    }
                    Event::Risk(RiskEvent::RiskLimitExceeded { .. }) => {
                        // Stop execution when risk limits are exceeded
                        self.running = false;
                        info!("Stopping execution due to risk limits");
                        Ok(())
                    }
                    // Ignore status check events, these are just for channel health verification
                    Event::Risk(RiskEvent::StatusCheck)
                    | Event::Market(MarketEvent::StatusCheck)
                    | Event::Strategy(StrategyEvent::StatusCheck)
                    | Event::Execution(ExecutionEvent::StatusCheck)
                    | Event::Database(DatabaseEvent::StatusCheck) => {
                        trace!("Received status check event, ignoring");
                        Ok(())
                    }
                    Event::Strategy(StrategyEvent::Signal {
                        token_id: _,
                        signal,
                        confidence: _,
                        ..
                    }) => {
                        // IMPORTANT: No longer directly processing sell signals from Strategy events
                        // All trade signals (both buy and sell) should go through the risk assessment pathway
                        if signal == Signal::Sell {
                            debug!("Received sell signal from Strategy event. Ignoring - sell signals should go through risk assessment.");
                        }
                        Ok(())
                    }
                    Event::Database(DatabaseEvent::PositionUpdated {
                        token_id,
                        price: _,
                        pnl: _,
                        ..
                    }) => {
                        if self.running {
                            debug!(
                                "📊 ExecutionActor received position update for {}",
                                token_id
                            );

                            // Clone what we need for the async task
                            let mut this = self.clone();
                            let token_id_clone = token_id.clone();

                            // Update our in-memory position cache
                            tokio::spawn(async move {
                                if let Err(e) = this.sync_positions_with_database().await {
                                    error!(
                                        "Failed to sync positions after database update: {:?}",
                                        e
                                    );
                                } else {
                                    debug!(
                                        "✅ Successfully synced positions after update for {}",
                                        token_id_clone
                                    );
                                }
                            });
                        }
                        Ok(())
                    }
                    _ => Ok(()),
                },
                Message::Command(cmd) => match cmd {
                    Command::Start => self.start().await,
                    Command::Stop => self.stop(),
                    Command::UpdateConfig(config) => {
                        // Update execution parameters from config
                        if let Some(slippage) = config.get("max_slippage").and_then(|v| v.as_f64())
                        {
                            // Update max slippage tolerance
                            info!("Updated max slippage to {}", slippage);
                        }
                        Ok(())
                    }
                    Command::MaintenanceDb => {
                        // Ignore maintenance command - not relevant for execution actor
                        debug!("Ignoring maintenance command - not relevant for ExecutionActor");
                        Ok(())
                    }
                    Command::StartMaintenanceScheduler => {
                        // Ignore maintenance scheduler command - not relevant for execution actor
                        debug!("Ignoring maintenance scheduler command - not relevant for ExecutionActor");
                        Ok(())
                    }
                },
                Message::Query(query, responder) => match query {
                    Query::GetStatus => {
                        let status = format!(
                            "ExecutionActor running: {}, Active Positions: {}",
                            self.running,
                            self.positions.len()
                        );
                        responder
                            .send(Ok(QueryResult::Status(status)))
                            .map_err(|e| {
                                Error::Task(format!("Failed to send status response: {:?}", e))
                            })
                    }
                    Query::GetMetrics => {
                        let metrics = serde_json::json!({
                            "running": self.running,
                            "active_positions": self.positions.len(),
                            "positions": self.positions.iter().map(|p| {
                                serde_json::json!({
                                    "token_id": p.token_id,
                                    "entry_price": p.entry_price,
                                    "size": p.size,
                                    "timestamp": p.entry_time,
                                })
                            }).collect::<Vec<_>>(),
                        });
                        responder
                            .send(Ok(QueryResult::Metrics(metrics)))
                            .map_err(|e| {
                                Error::Task(format!("Failed to send metrics response: {:?}", e))
                            })
                    }
                    _ => responder
                        .send(Err(Error::Task("Unsupported query type".to_string())))
                        .map_err(|e| {
                            Error::Task(format!("Failed to send error response: {:?}", e))
                        }),
                },
            }
        }
    }
}
