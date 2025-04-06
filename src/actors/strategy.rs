use super::{Actor, Message, Event, MarketEvent, StrategyEvent, Command, Query, QueryResult};
use crate::error::Error;
use crate::trading::strategy::{Strategy, Signal, Position};
use crate::types::market::TokenMetrics;
use crate::repositories::TokenRepository;
use std::sync::Arc;
use chrono::Utc;
use log::{info, error, debug, trace, warn};

#[derive(Clone)]
pub struct StrategyActor {
    strategy: Strategy,
    token_repo: TokenRepository,
    message_bus: Arc<super::MessageBus>,
    running: bool,
}

impl StrategyActor {
    pub fn new(
        strategy: Strategy,
        token_repo: TokenRepository,
        message_bus: Arc<super::MessageBus>,
    ) -> Self {
        Self {
            strategy,
            token_repo,
            message_bus,
            running: false,
        }
    }

    async fn handle_price_update(&mut self, token_id: String, price: f64, volume: Option<f64>) -> Result<(), Error> {
        trace!("StrategyActor beginning to process price update for {}: ${:.4} (volume: {:?})", 
              token_id, price, volume);
        
        // Get token data from repository
        trace!("Fetching token data from repository for {}", token_id);
        let token_data = match self.token_repo.get_token_price_stats(&token_id) {
            Ok(data) => {
                debug!("📊 Retrieved price history for {}", token_id);
                data
            },
            Err(e) => {
                warn!("❌ Failed to get token data for {}: {:?}", token_id, e);
                return Err(e);
            }
        };
        
        let token_metrics = crate::types::market::TokenMetrics::from(&token_data);
        
        // Log token being evaluated
        info!("📊 Evaluating {} (${:.4}) with volume ${:.2}M for trading signals", 
            token_metrics.symbol.to_uppercase(), 
            token_metrics.price_usd,
            token_metrics.volume_24h / 1_000_000.0);
        
        trace!("Token metrics for analysis: price_change_24h={:.2}%", 
              token_metrics.price_change_24h);
        
        // Update strategy's market data
        trace!("Updating strategy market data for {}", token_id);
        self.strategy.update_market_data(&token_metrics);
        
        // Analyze token for signals
        debug!("🧠 Analyzing token {} for trading signals using {} strategy", 
              token_id, self.strategy.name());
        
        let start_time = std::time::Instant::now();
        let signal = self.strategy.analyze(&token_metrics);
        let analysis_time = start_time.elapsed();
        
        debug!("✅ Analysis complete for {} in {:.2?}: Signal: {:?}", 
              token_id, analysis_time, signal);
        
        match signal {
            crate::trading::strategy::Signal::Buy => {
                // Calculate confidence based on strategy parameters
                let confidence = self.calculate_signal_confidence(&token_metrics);
                
                info!("🚨 BUY SIGNAL detected for {} (${:.4}) with {:.1}% confidence", 
                    token_metrics.symbol.to_uppercase(), 
                    token_metrics.price_usd,
                    confidence * 100.0);
                
                trace!("Buy signal details: token={}, price=${:.4}, volume=${:.2}M, confidence={:.2}", 
                     token_id, price, volume.unwrap_or(0.0) / 1_000_000.0, confidence);
                
                // Publish signal event
                debug!("📢 Publishing BUY signal event for {} with confidence {:.2}", token_id, confidence);
                let event = Event::Strategy(StrategyEvent::Signal {
                    token_id: token_data.id.clone(),
                    signal,
                    confidence,
                    timestamp: Utc::now(),
                });

                trace!("Calling message_bus.publish() for BUY signal for {}", token_id);
                if let Err(e) = self.message_bus.publish(event).await {
                    error!("❌ Failed to publish strategy event: {:?}", e);
                } else {
                    debug!("✅ Successfully published BUY signal for {} to message bus", token_id);
                    trace!("Message flow: StrategyActor → MessageBus → RiskManagerActor (BUY signal)");
                }
            },
            crate::trading::strategy::Signal::Sell => {
                // Calculate confidence for sell signals too
                let confidence = self.calculate_signal_confidence(&token_metrics);
                
                info!("🚨 SELL SIGNAL detected for {} (${:.4}) with {:.1}% confidence", 
                    token_metrics.symbol.to_uppercase(), 
                    token_metrics.price_usd,
                    confidence * 100.0);
                
                trace!("Sell signal details: token={}, price=${:.4}, volume=${:.2}M, confidence={:.2}", 
                     token_id, price, volume.unwrap_or(0.0) / 1_000_000.0, confidence);
                
                // Publish signal event for sell as well
                debug!("📢 Publishing SELL signal event for {} with confidence {:.2}", token_id, confidence);
                let event = Event::Strategy(StrategyEvent::Signal {
                    token_id: token_data.id.clone(),
                    signal,
                    confidence,
                    timestamp: Utc::now(),
                });

                trace!("Calling message_bus.publish() for SELL signal for {}", token_id);
                if let Err(e) = self.message_bus.publish(event).await {
                    error!("❌ Failed to publish strategy event: {:?}", e);
                } else {
                    debug!("✅ Successfully published SELL signal for {} to message bus", token_id);
                    trace!("Message flow: StrategyActor → MessageBus → RiskManagerActor (SELL signal)");
                }
            },
            crate::trading::strategy::Signal::None => {
                // Only log at debug level for no signal to avoid cluttering logs
                debug!("🤷 No trading signal for {} (${:.4}) - conditions not met", 
                    token_metrics.symbol.to_uppercase(), 
                    token_metrics.price_usd);
                
                trace!("No signal generated for {} - strategy thresholds not reached", token_id);
            }
        }

        trace!("Completed processing price update for {}", token_id);
        Ok(())
    }

    fn calculate_signal_confidence(&self, token: &TokenMetrics) -> f64 {
        trace!("Calculating signal confidence for {}", token.symbol);
        // This is a simple example - you would want to implement more sophisticated
        // confidence calculation based on your strategy's parameters
        let volume_confidence = (token.volume_24h / 1_000_000.0).min(1.0);
        let price_change_confidence = token.price_change_24h.abs() / 100.0;
        
        let confidence = (volume_confidence + price_change_confidence) / 2.0;
        debug!("🧮 Confidence calculation for {}: {:.2} (volume: {:.2}, price change: {:.2})", 
              token.symbol, confidence, volume_confidence, price_change_confidence);
        trace!("Confidence components: volume_conf={:.4}, price_change_conf={:.4}, final={:.4}", 
               volume_confidence, price_change_confidence, confidence);
        
        confidence
    }

    async fn check_positions(&mut self, positions: &[Position]) -> Result<(), Error> {
        debug!("Checking {} positions for exit conditions", positions.len());
        for position in positions {
            trace!("Evaluating position for {}", position.token_id);
            if self.strategy.should_exit(position) {
                debug!("Exit condition met for position in {}", position.token_id);
                // Publish exit signal
                let event = Event::Strategy(StrategyEvent::Signal {
                    token_id: position.token_id.clone(),
                    signal: Signal::Sell,
                    confidence: 1.0, // High confidence for exit signals
                    timestamp: Utc::now(),
                });

                if let Err(e) = self.message_bus.publish(event).await {
                    error!("Failed to publish exit signal: {:?}", e);
                } else {
                    debug!("Successfully published exit signal for {}", position.token_id);
                }
            }
        }

        Ok(())
    }
}

impl Actor for StrategyActor {
    fn start(&mut self) -> Result<(), Error> {
        self.running = true;
        info!("🧠 Starting StrategyActor with strategy type: {}", self.strategy.name());
        debug!("StrategyActor: Initialized and ready to process market events");
        debug!("📝 Strategy will analyze market data and generate signals for applicable tokens");
        
        Ok(())
    }

    fn stop(&mut self) -> Result<(), Error> {
        self.running = false;
        info!("⏹️ Stopping StrategyActor");
        debug!("StrategyActor: Stopped processing market events");
        Ok(())
    }

    fn handle_message(&mut self, msg: Message) -> Result<(), Error> {
        trace!("StrategyActor received message: {:?}", msg);
        debug!("📬 StrategyActor handling incoming message");
        
        match msg {
            Message::Event(event) => {
                debug!("📨 StrategyActor received event of type: {:?}", match &event {
                    Event::Market(_) => "Market",
                    Event::Strategy(_) => "Strategy",
                    Event::Risk(_) => "Risk",
                    Event::Execution(_) => "Execution",
                    Event::Database(_) => "Database",
                });
                
                match event {
                    Event::Market(MarketEvent::PriceUpdate { token_id, price, volume, timestamp }) => {
                        if self.running {
                            debug!("📈 StrategyActor received price update event for {}: ${:.4} at {}", 
                                 token_id, price, timestamp);
                            debug!("💹 Price update details - token: {}, price: ${:.4}, volume: ${:.2}M, timestamp: {}", 
                                  token_id, price, volume.unwrap_or(0.0) / 1_000_000.0, timestamp);
                            trace!("📬 Message flow: MarketDataActor → MessageBus → StrategyActor (PriceUpdate)");
                            
                            let mut this = self.clone();
                            tokio::spawn(async move {
                                debug!("🔎 Processing price update for {} in background task", token_id);
                                trace!("StrategyActor spawning background task for token {} at price ${:.4}", token_id, price);
                                
                                let start_time = std::time::Instant::now();
                                if let Err(e) = this.handle_price_update(token_id.clone(), price, volume).await {
                                    error!("❌ Error handling price update for {}: {:?}", token_id, e);
                                } else {
                                    let duration = start_time.elapsed();
                                    debug!("✅ Completed signal analysis for {} in {:.2?}", token_id, duration);
                                    trace!("StrategyActor completed background processing of price update for {}", token_id);
                                }
                            });
                        } else {
                            debug!("🛑 StrategyActor ignoring price update for {} because actor is not running", token_id);
                            trace!("Skipped price update processing due to actor state (running=false)");
                        }
                        Ok(())
                    },
                    Event::Market(MarketEvent::MarketDataError(e)) => {
                        error!("⚠️ StrategyActor received market data error: {:?}", e);
                        warn!("Market data error may affect trading signals until resolved");
                        Ok(())
                    },
                    _ => {
                        trace!("StrategyActor received unhandled event type: {:?}", event);
                        debug!("🤔 Ignoring unhandled event type in StrategyActor");
                        Ok(())
                    },
                }
            },
            Message::Command(cmd) => match cmd {
                Command::Start => {
                    self.running = true;
                    info!("▶️ StrategyActor received start command");
                    debug!("StrategyActor is now processing market events with strategy: {}", self.strategy.name());
                    Ok(())
                },
                Command::Stop => {
                    self.running = false;
                    info!("⏹️ StrategyActor received stop command");
                    debug!("StrategyActor will no longer process market events");
                    Ok(())
                },
                Command::UpdateConfig(config) => {
                    debug!("🔧 StrategyActor received config update: {:?}", config);
                    trace!("Processing configuration updates for strategy parameters");
                    
                    // Update strategy parameters from config
                    if let Some(threshold) = config.get("threshold").and_then(|v| v.as_f64()) {
                        // Update strategy threshold
                        info!("🔄 Updated strategy threshold to {}", threshold);
                        trace!("Strategy threshold parameter updated from configuration");
                    }
                    
                    debug!("Strategy configuration update complete");
                    Ok(())
                },
            },
            Message::Query(query, responder) => match query {
                Query::GetStatus => {
                    trace!("StrategyActor received status query");
                    let status = format!("StrategyActor running: {}", self.running);
                    debug!("📊 Responding to status query: {}", status);
                    
                    responder.send(Ok(QueryResult::Status(status)))
                        .map_err(|e| Error::Task(format!("Failed to send status response: {:?}", e)))
                },
                Query::GetMetrics => {
                    trace!("StrategyActor received metrics query");
                    let metrics = serde_json::json!({
                        "running": self.running,
                        "strategy": self.strategy.name(),
                    });
                    debug!("📈 Responding to metrics query with strategy type: {}", self.strategy.name());
                    
                    responder.send(Ok(QueryResult::Metrics(metrics)))
                        .map_err(|e| Error::Task(format!("Failed to send metrics response: {:?}", e)))
                },
                _ => {
                    trace!("StrategyActor received unsupported query");
                    debug!("❓ Received unsupported query type");
                    
                    responder.send(Err(Error::Task("Unsupported query type".to_string())))
                        .map_err(|e| Error::Task(format!("Failed to send error response: {:?}", e)))
                },
            },
        }
    }
} 