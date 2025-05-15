use super::{Actor, DatabaseEvent, ExecutionEvent, Message, RiskEvent};
use crate::core::config::Config;
use crate::core::error::Error;
use crate::core::models::market::TokenMetrics;
use crate::domain::trading::strategy::{Position, Signal, Strategy};
use crate::infra::actors::MessageBus;
use crate::infra::actors::{Command, Event, MarketEvent, Query, QueryResult, StrategyEvent};
use crate::infra::db::repositories::TokenRepository;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use log::{debug, error, info, trace, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::Semaphore;

// Define a constant for the default concurrency limit
const DEFAULT_STRATEGY_DB_CONCURRENCY: usize = 10;

#[derive(Clone)]
pub struct StrategyActor {
    strategy: Strategy,
    token_repo: Arc<TokenRepository>,
    message_bus: Arc<MessageBus>,
    config: Arc<Config>,
    running: bool,
    last_signal_time: Option<chrono::DateTime<chrono::Utc>>,
    db_concurrency_limiter: Arc<Semaphore>,
}

impl StrategyActor {
    pub fn new(
        token_repo: Arc<TokenRepository>,
        strategy: Strategy,
        message_bus: Arc<MessageBus>,
        config: Arc<Config>,
    ) -> Self {
        // Limit concurrent DB operations originating from strategy processing
        // Use a hardcoded default for now, as config field doesn't exist
        // let db_concurrency_limit = config.trading.strategy_db_concurrency.unwrap_or(10);
        let db_concurrency_limit = DEFAULT_STRATEGY_DB_CONCURRENCY;
        info!(
            "StrategyActor DB concurrency limit set to: {}",
            db_concurrency_limit
        );

        Self {
            token_repo,
            message_bus,
            strategy,
            running: false,
            config,
            last_signal_time: None,
            db_concurrency_limiter: Arc::new(Semaphore::new(db_concurrency_limit)),
        }
    }

    fn calculate_signal_confidence(&self, token: &TokenMetrics) -> f64 {
        trace!("Calculating signal confidence for {}", token.symbol);
        // This is a simple example - you would want to implement more sophisticated
        // confidence calculation based on your strategy's parameters
        let volume_confidence = (token.volume_24h / 1_000_000.0).min(1.0);
        let price_change_confidence = token.price_change_24h.abs() / 100.0;

        let confidence = (volume_confidence + price_change_confidence) / 2.0;
        debug!(
            "🧮 Confidence calculation for {}: {:.2} (volume: {:.2}, price change: {:.2})",
            token.symbol, confidence, volume_confidence, price_change_confidence
        );
        trace!(
            "Confidence components: volume_conf={:.4}, price_change_conf={:.4}, final={:.4}",
            volume_confidence,
            price_change_confidence,
            confidence
        );

        confidence
    }
}

impl Actor for StrategyActor {
    fn start(&mut self) -> impl std::future::Future<Output = Result<(), Error>> + Send {
        async move {
            self.running = true;
            info!(
                "🧠 Starting StrategyActor with strategy type: {}",
                self.strategy.name()
            );
            debug!("StrategyActor: Initialized and ready to process market events");
            debug!(
                "📝 Strategy will analyze market data and generate signals for applicable tokens"
            );

            Ok(())
        }
    }

    fn stop(&mut self) -> Result<(), Error> {
        self.running = false;
        info!("⏹️ Stopping StrategyActor");
        debug!("StrategyActor: Stopped processing market events");
        Ok(())
    }

    /// Handle incoming messages
    fn handle_message(
        &mut self,
        msg: Message,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send {
        async move {
            trace!("📨 StrategyActor received message: {:?}", msg);

            match msg {
                Message::Event(event) => match event {
                    Event::Market(market_event) => {
                        trace!("📊 Processing market event: {:?}", market_event);
                        match market_event {
                            MarketEvent::PriceUpdate {
                                token_id,
                                price,
                                volume,
                                timestamp: _,
                            } => {
                                if !self.running {
                                    trace!("🛑 StrategyActor ignoring price update - not running");
                                    return Ok(());
                                }

                                // Clone needed data BEFORE spawning task
                                let token_id_clone = token_id.clone();
                                let price_clone = price;
                                let volume_clone = volume;
                                let limiter = self.db_concurrency_limiter.clone();
                                let mut this = self.clone();

                                // Log the token ID received from the event
                                debug!(
                                    "EVENT RECEIVED: Price update for token_id: '{}', price: {:.4}",
                                    token_id, price
                                );

                                tokio::spawn(async move {
                                    let permit = match limiter.acquire().await {
                                        Ok(p) => p,
                                        Err(_) => {
                                            error!("TASK FAILED (Semaphore Closed): Could not acquire DB permit for {}. Task aborted.", token_id_clone);
                                            return;
                                        }
                                    };

                                    let task_start_time = std::time::Instant::now();
                                    // Log the token ID being processed by this specific task
                                    debug!(
                                        "TASK SPAWNED: Strategy price update for '{}': ${:.4}",
                                        token_id_clone, price_clone
                                    );

                                    let db_timeout = Duration::from_secs(15);
                                    let db_call_start_time = std::time::Instant::now();

                                    let token_data_result = tokio::time::timeout(
                                        db_timeout,
                                        this.token_repo.get_token_price_stats(&token_id_clone),
                                    )
                                    .await;

                                    let token_data = match token_data_result {
                                        Ok(Ok(data)) => {
                                            debug!(
                                                "TASK DB READ SUCCESS for '{}'. Took: {:?}",
                                                token_id_clone,
                                                db_call_start_time.elapsed()
                                            );
                                            // Log the fetched TokenData
                                            debug!(
                                                "TASK DB DATA for '{}': {:?}",
                                                token_id_clone, data
                                            );
                                            let mut updated_data = data;
                                            // Ensure the price/volume from the triggering event are used if newer
                                            // (DB data might be slightly stale)
                                            updated_data.price_usd = price_clone;
                                            if let Some(vol) = volume_clone {
                                                updated_data.volume_24h = vol;
                                            }
                                            updated_data.last_updated = Some(Utc::now()); // Update timestamp
                                            updated_data
                                        }
                                        Ok(Err(e)) => {
                                            error!(
                                                    "TASK FAILED (DB Error): Error getting token metadata for '{}': {:?}. DB call took: {:?}, Total Elapsed: {:?}",
                                                    token_id_clone, e, db_call_start_time.elapsed(), task_start_time.elapsed()
                                                );
                                            return; // Permit dropped implicitly
                                        }
                                        Err(_) => {
                                            error!(
                                                    "TASK TIMEOUT (DB Read): Timeout getting token metadata for '{}' after {:?}. Total Elapsed: {:?}",
                                                    token_id_clone, db_timeout, task_start_time.elapsed()
                                                );
                                            return; // Permit dropped implicitly
                                        }
                                    };

                                    // Convert TokenData to TokenMetrics
                                    let token_metrics =
                                        crate::types::market::TokenMetrics::from(&token_data);
                                    // Log the resulting TokenMetrics
                                    debug!(
                                        "TASK METRICS for '{}': {:?}",
                                        token_id_clone, token_metrics
                                    );

                                    // Skip tokens with invalid prices (redundant check?)
                                    if token_metrics.price_usd <= 0.0 {
                                        debug!("⚠️ TASK SKIPPED (Invalid Price): Skipping signal analysis for '{}' - invalid price: ${:.4}. Elapsed: {:?}", 
                                               token_id_clone, token_metrics.price_usd, task_start_time.elapsed());
                                        return; // Permit dropped implicitly
                                    }

                                    let analysis_timeout = Duration::from_secs(10);
                                    let analysis_start_time = std::time::Instant::now();
                                    debug!(
                                        "TASK: Calling strategy analysis for {}...",
                                        token_id_clone
                                    );

                                    let analysis_result = tokio::time::timeout(analysis_timeout, async {
                                        debug!("  TASK: -> update_market_data for {}", token_id_clone);
                                        this.strategy.update_market_data(&token_metrics);
                                        debug!("  TASK: <- update_market_data finished for {}. Calling analyze_for_entry...", token_id_clone);
                                        let should_enter = this.strategy.analyze_for_entry(&token_metrics);
                                        debug!("  TASK: <- analyze_for_entry finished for {}. Result: {}", token_id_clone, should_enter);
                                        should_enter
                                    }).await;

                                    match analysis_result {
                                        Ok(should_enter) => {
                                            debug!("TASK: Strategy analysis completed for {}. Analysis took: {:?}. Should enter: {}", 
                                                   token_id_clone, analysis_start_time.elapsed(), should_enter);
                                            if should_enter {
                                                let confidence = this
                                                    .calculate_signal_confidence(&token_metrics);

                                                let event =
                                                    Event::Strategy(StrategyEvent::Signal {
                                                        token_id: token_id_clone.clone(),
                                                        signal: Signal::Buy,
                                                        confidence,
                                                        timestamp: Utc::now(),
                                                    });

                                                debug!("TASK: Attempting to publish BUY signal for {}...", token_id_clone);
                                                let publish_start_time = std::time::Instant::now();

                                                if let Err(e) =
                                                    this.message_bus.publish(event).await
                                                {
                                                    error!("TASK ERROR (Publish Signal): Failed to publish entry signal for {}: {:?}. Publish took: {:?}, Total Elapsed: {:?}", 
                                                           token_id_clone, e, publish_start_time.elapsed(), task_start_time.elapsed());
                                                } else {
                                                    info!("✅ TASK SUCCESS (Buy Signal): Published BUY signal for {} with confidence {:.2}. Publish took: {:?}, Analysis took: {:?}, Total Elapsed: {:?}", 
                                                          token_id_clone, confidence, publish_start_time.elapsed(), analysis_start_time.elapsed(), task_start_time.elapsed());
                                                }
                                            } else {
                                                debug!("TASK COMPLETED (No Signal): No entry signal for {}. Analysis took: {:?}, Total Elapsed: {:?}", 
                                                           token_id_clone, analysis_start_time.elapsed(), task_start_time.elapsed());
                                            }
                                        }
                                        Err(_) => {
                                            error!("TASK TIMEOUT (Analysis): Strategy analysis timed out for token {} after {:?}. Total Elapsed: {:?}", 
                                                   token_id_clone, analysis_timeout, task_start_time.elapsed());
                                        }
                                    }
                                });

                                Ok(())
                            }
                            MarketEvent::MarketDataError(e) => {
                                warn!("⚠️ StrategyActor received market data error: {:?}", e);
                                Ok(())
                            }
                            MarketEvent::StatusCheck => {
                                trace!("📡 Received status check event in StrategyActor");
                                Ok(())
                            }
                            _ => {
                                trace!("StrategyActor ignoring unhandled event type");
                                Ok(())
                            }
                        }
                    }
                    _ => Ok(()),
                },
                Message::Command(cmd) => match cmd {
                    Command::Start => {
                        self.running = true;
                        info!("▶️ StrategyActor started");
                        Ok(())
                    }
                    Command::Stop => {
                        self.running = false;
                        info!("⏹️ StrategyActor stopped");
                        Ok(())
                    }
                    Command::UpdateConfig(config) => {
                        debug!("🔧 Updating StrategyActor configuration");

                        if let Some(threshold) = config.get("threshold").and_then(|v| v.as_f64()) {
                            info!("🔄 Updated strategy threshold to {}", threshold);
                        }

                        Ok(())
                    }
                    Command::MaintenanceDb => {
                        debug!(
                            "MaintenanceDb command received by StrategyActor - no action needed"
                        );
                        Ok(())
                    }
                    Command::StartMaintenanceScheduler => {
                        debug!("StartMaintenanceScheduler command received by StrategyActor - no action needed");
                        Ok(())
                    }
                    _ => {
                        warn!("Unsupported command for StrategyActor");
                        Ok(())
                    }
                },
                Message::Query(query, responder) => match query {
                    Query::GetStatus => {
                        let status = format!("StrategyActor running: {}", self.running);
                        responder
                            .send(Ok(QueryResult::Status(status)))
                            .map_err(|e| Error::Task(format!("Failed to send response: {:?}", e)))
                    }
                    Query::GetMetrics => {
                        let metrics = serde_json::json!({
                            "running": self.running,
                            "strategy": self.strategy.name(),
                        });
                        responder
                            .send(Ok(QueryResult::Metrics(metrics)))
                            .map_err(|e| Error::Task(format!("Failed to send response: {:?}", e)))
                    }
                    _ => responder
                        .send(Err(Error::Task("Unsupported query".to_string())))
                        .map_err(|e| {
                            Error::Task(format!("Failed to send error response: {:?}", e))
                        }),
                },
            }
        }
    }
}
