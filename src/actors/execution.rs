use super::{Actor, Message, Event, RiskEvent, ExecutionEvent, Command, Query, QueryResult};
use crate::error::Error;
use crate::trading::strategy::Signal;
use crate::repositories::TokenRepository;
use crate::dex::DexClient;
use std::sync::Arc;
use chrono::Utc;
use log::{info, error, debug};
use crate::config::Config;

#[derive(Clone)]
pub struct ExecutionActor {
    token_repo: TokenRepository,
    dex_client: DexClient,
    message_bus: Arc<super::MessageBus>,
    running: bool,
    positions: Vec<Position>,
    config: Config,
}

#[derive(Debug, Clone)]
struct Position {
    token_id: String,
    entry_price: f64,
    size: f64,
    timestamp: chrono::DateTime<Utc>,
}

impl ExecutionActor {
    pub fn new(
        token_repo: TokenRepository,
        dex_client: DexClient,
        message_bus: Arc<super::MessageBus>,
        config: Config,
    ) -> Self {
        Self {
            token_repo,
            dex_client,
            message_bus,
            running: false,
            positions: Vec::new(),
            config,
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

        // Only execute buy orders for now
        if signal == Signal::Buy {
            // Check if we already have a position for this token
            if self.positions.iter().any(|p| p.token_id == token_id) {
                info!("⚠️ Already have an open position for {}, skipping buy signal", token_id);
                return Ok(());
            }

            // Get token data
            let token_data = match self.token_repo.get_token_price_stats(&token_id) {
                Ok(data) => data,
                Err(e) => {
                    error!("Failed to get token data for {}: {:?}", token_id, e);
                    return Err(Error::Api(format!("Failed to get token data: {}", e)));
                }
            };

            let symbol = token_data.symbol.to_uppercase();
            let entry_price = token_data.price_usd;
            
            info!(
                "🔄 Executing BUY order for {} (${:.4}) with position size: ${:.2}",
                symbol, entry_price, position_size
            );

            // Execute buy order (in a real implementation, this would call the exchange API)
            // For now, we'll just simulate the order execution
            let order_result = match self.dex_client.execute_order(
                &token_id,
                position_size,
                entry_price,
                true, // buy = true
            ).await {
                Ok(_) => {
                    info!("✅ Successfully executed buy order for {} at ${:.4}", symbol, entry_price);
                    true
                },
                Err(e) => {
                    error!("Failed to execute buy order for {}: {:?}", symbol, e);
                    false
                }
            };

            if order_result {
                // Record the position
                let position = Position {
                    token_id: token_id.clone(),
                    entry_price,
                    size: position_size,
                    timestamp: Utc::now(),
                };

                // Register the position
                self.positions.push(position);
                
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

                if let Err(e) = self.message_bus.publish(event).await {
                    error!("Failed to publish execution event: {:?}", e);
                }
            }
        }

        Ok(())
    }

    async fn check_positions(&mut self) -> Result<(), Error> {
        if !self.running || self.positions.is_empty() {
            return Ok(());
        }

        let positions_count = self.positions.len();
        info!("🔍 Checking status of {} active positions", positions_count);
        
        // Clone token IDs to avoid borrowing issues
        let token_ids: Vec<String> = self.positions.iter().map(|p| p.token_id.clone()).collect();

        for token_id in token_ids {
            // Get current market data
            let token_data = match self.token_repo.get_token_price_stats(&token_id) {
                Ok(data) => data,
                Err(e) => {
                    error!("Failed to get token data for {}: {:?}", token_id, e);
                    continue;
                }
            };

            let current_price = token_data.price_usd;
            let symbol = token_data.symbol.to_uppercase();

            // Find the position
            let position = match self.positions.iter().find(|p| p.token_id == token_id) {
                Some(p) => p.clone(),
                None => continue, // Position not found (should not happen)
            };

            // Calculate P&L
            let pnl = (current_price - position.entry_price) * position.size;
            let profit_loss_pct = ((current_price / position.entry_price) - 1.0) * 100.0;
            
            info!(
                "📊 Position update for {}: Entry=${:.4}, Current=${:.4}, P/L=${:.2} ({:.2}%)",
                symbol, position.entry_price, current_price, pnl, profit_loss_pct
            );

            // Publish position update event
            let update_event = Event::Execution(ExecutionEvent::PositionUpdate {
                token_id: token_id.clone(),
                current_price,
                pnl,
                timestamp: Utc::now(),
            });

            if let Err(e) = self.message_bus.publish(update_event).await {
                error!("Failed to publish position update event: {:?}", e);
            }

            // Get risk tolerance from config
            let risk_tolerance = self.config.trading.risk_tolerance;
            
            // Check for exit conditions based on risk profile
            let stop_loss_pct = self.config.trading.risk.stop_loss_pct;  // From config
            
            // Apply risk-based take profit adjustment - more aggressive = lower take profit target
            let take_profit_multiplier = match risk_tolerance {
                5 => 0.5,  // Very aggressive: 50% of normal take profit
                4 => 0.6,  // Aggressive: 60% of normal take profit
                3 => 0.7,  // Moderate-Aggressive
                2 => 0.8,  // Moderate
                1 => 0.9,  // Conservative-Moderate
                _ => 1.0,  // Conservative: Standard take profit
            };
            
            let take_profit_pct = self.config.trading.risk.take_profit_pct * take_profit_multiplier;
            
            let should_close = profit_loss_pct <= stop_loss_pct || profit_loss_pct >= take_profit_pct;

            if should_close {
                let close_reason = if profit_loss_pct <= stop_loss_pct {
                    "Stop Loss triggered"
                } else {
                    "Take Profit reached"
                };

                info!(
                    "🚫 Closing position for {}: {} (Entry=${:.4}, Close=${:.4}, P/L=${:.2} ({:.2}%)",
                    symbol, close_reason, position.entry_price, current_price, pnl, profit_loss_pct
                );

                // Execute sell order (in a real implementation, this would call the exchange API)
                let order_result = match self.dex_client.execute_order(
                    &token_id,
                    position.size,
                    current_price,
                    false, // buy = false (i.e., sell)
                ).await {
                    Ok(_) => {
                        info!("✅ Successfully executed sell order for {} at ${:.4}", symbol, current_price);
                        true
                    },
                    Err(e) => {
                        error!("Failed to execute sell order for {}: {:?}", symbol, e);
                        false
                    }
                };

                if order_result {
                    // Remove the position
                    self.positions.retain(|p| p.token_id != token_id);

                    // Publish position closed risk event
                    let risk_event = Event::Risk(RiskEvent::PositionClosed {
                        token_id: token_id.clone(),
                        pnl,
                        timestamp: Utc::now(),
                    });

                    if let Err(e) = self.message_bus.publish(risk_event).await {
                        error!("Failed to publish position closed risk event: {:?}", e);
                    }
                }
            }
        }

        Ok(())
    }
}

impl Actor for ExecutionActor {
    fn start(&mut self) -> Result<(), Error> {
        self.running = true;
        info!("Starting ExecutionActor");
        Ok(())
    }

    fn stop(&mut self) -> Result<(), Error> {
        self.running = false;
        info!("Stopping ExecutionActor");
        Ok(())
    }

    fn handle_message(&mut self, msg: Message) -> Result<(), Error> {
        match msg {
            Message::Event(event) => match event {
                Event::Risk(RiskEvent::RiskAssessment { token_id, signal, confidence, position_size, .. }) => {
                    if self.running {
                        let mut this = self.clone();
                        tokio::spawn(async move {
                            if let Err(e) = this.handle_risk_assessment(token_id, signal, confidence, position_size).await {
                                error!("Error handling risk assessment: {:?}", e);
                            }
                        });
                    }
                    Ok(())
                },
                Event::Risk(RiskEvent::RiskLimitExceeded { .. }) => {
                    // Stop execution when risk limits are exceeded
                    self.running = false;
                    info!("Stopping execution due to risk limits");
                    Ok(())
                },
                _ => Ok(()),
            },
            Message::Command(cmd) => match cmd {
                Command::Start => {
                    self.running = true;
                    info!("ExecutionActor received start command");
                    Ok(())
                },
                Command::Stop => {
                    self.running = false;
                    info!("ExecutionActor received stop command");
                    Ok(())
                },
                Command::UpdateConfig(config) => {
                    // Update execution parameters from config
                    if let Some(slippage) = config.get("max_slippage").and_then(|v| v.as_f64()) {
                        // Update max slippage tolerance
                        info!("Updated max slippage to {}", slippage);
                    }
                    Ok(())
                },
            },
            Message::Query(query, responder) => match query {
                Query::GetStatus => {
                    let status = format!(
                        "ExecutionActor running: {}, Active Positions: {}",
                        self.running,
                        self.positions.len()
                    );
                    responder.send(Ok(QueryResult::Status(status)))
                        .map_err(|e| Error::Task(format!("Failed to send status response: {:?}", e)))
                },
                Query::GetMetrics => {
                    let metrics = serde_json::json!({
                        "running": self.running,
                        "active_positions": self.positions.len(),
                        "positions": self.positions.iter().map(|p| {
                            serde_json::json!({
                                "token_id": p.token_id,
                                "entry_price": p.entry_price,
                                "size": p.size,
                                "timestamp": p.timestamp,
                            })
                        }).collect::<Vec<_>>(),
                    });
                    responder.send(Ok(QueryResult::Metrics(metrics)))
                        .map_err(|e| Error::Task(format!("Failed to send metrics response: {:?}", e)))
                },
                _ => {
                    responder.send(Err(Error::Task("Unsupported query type".to_string())))
                        .map_err(|e| Error::Task(format!("Failed to send error response: {:?}", e)))
                },
            },
        }
    }
} 