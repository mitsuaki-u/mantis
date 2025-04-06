use tokio::time::{interval, Duration};
use tokio::task::JoinHandle;
use crate::api::market::MarketApi;
use crate::repositories::{RepositoryFactory, PriceRepository, TokenRepository};
use crate::error::{Error, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use log::{info, error, warn, debug, trace};
use chrono::Utc;
use crate::actors::{Event, MarketEvent, MessageBus};
use crate::trading::bot::is_forced_shutdown;
use crate::config::Config;

/// Data collector that periodically fetches market data and stores it in the database
pub struct DataCollector {
    /// Is the collector running?
    running: Arc<AtomicBool>,
    /// Interval between data collections in seconds
    collection_interval: u64,
    /// Market API instance
    market_api: Option<MarketApi>,
    /// Price repository
    price_repo: Arc<PriceRepository>,
    /// Token repository
    token_repo: Arc<TokenRepository>,
    /// Task handle
    task_handle: Option<tokio::task::JoinHandle<()>>,
    /// Config
    config: Arc<Config>,
    /// Message bus for publishing market events
    message_bus: Option<Arc<MessageBus>>,
}

impl DataCollector {
    /// Create a new data collector
    pub fn new(
        price_repo: Arc<PriceRepository>,
        token_repo: Arc<TokenRepository>,
        collection_interval: u64,
        config: Arc<Config>,
        message_bus: Option<Arc<MessageBus>>,
    ) -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            collection_interval,
            market_api: None,
            price_repo,
            token_repo,
            task_handle: None,
            config,
            message_bus,
        }
    }
    
    /// Start the data collection process
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting market data collector...");
        self.running.store(true, Ordering::SeqCst);

        let interval_duration = Duration::from_secs(self.collection_interval);
        let running = self.running.clone();
        let config = self.config.clone();
        let price_repo = self.price_repo.clone();
        let token_repo = self.token_repo.clone();
        let collection_interval = self.collection_interval;
        let message_bus = self.message_bus.clone();

        let handle = tokio::spawn(async move {
            info!("Market data collector started with interval {}s", collection_interval);
            let mut interval_timer = interval(interval_duration);
            
            while running.load(Ordering::SeqCst) {
                // Check for forced shutdown signal from the trading bot
                if is_forced_shutdown() {
                    info!("Force shutdown detected, terminating market data collector task");
                    break;
                }
                
                interval_timer.tick().await;
                
                // Create a new MarketApi instance for each collection cycle
                let market_api = if let Some(ref coingecko_key) = config.api_keys.coingecko {
                    info!("Using CoinGecko provider for data collection");
                    MarketApi::new(false)
                } else if let Some(ref cryptocompare_key) = config.api_keys.cryptocompare {
                    info!("Using CryptoCompare provider for data collection");
                    MarketApi::new(false)
                } else {
                    info!("Using default provider without API key for data collection");
                    MarketApi::new(false)
                };
                
                match market_api.get_token_data().await {
                    Ok(token_data) => {
                        info!("📊 Collected market data for {} tokens", token_data.len());
                        
                        let mut successes = 0;
                        let mut failures = 0;
                        
                        for token in &token_data {
                            // Update token metadata
                            if let Err(e) = token_repo.update_token_metadata(&token.id, &token.symbol) {
                                error!("Failed to update token metadata for {}: {}", token.symbol, e);
                                failures += 1;
                                continue;
                            }
                            
                            // Store price data
                            match price_repo.store_price_data(&token.id, token.price_usd, token.volume_24h) {
                                Ok(_) => {
                                    successes += 1;
                                    
                                    // Publish market event if we have a message bus
                                    if let Some(bus) = &message_bus {
                                        trace!("Publishing market price update for {}: ${:.4}", token.id, token.price_usd);
                                        
                                        // Create market price update event
                                        let market_event = Event::Market(MarketEvent::PriceUpdate {
                                            token_id: token.id.clone(),
                                            price: token.price_usd,
                                            volume: Some(token.volume_24h),
                                            timestamp: Utc::now(),
                                        });
                                        
                                        // Publish event to message bus
                                        let bus_id = format!("{:p}", Arc::as_ptr(bus));
                                        debug!("🔄 Data collector publishing price update for {} (${:.4}) to message bus [id: {}]", 
                                              token.symbol, token.price_usd, bus_id);
                                        
                                        // Check subscriber count for debugging
                                        let subscriber_count = bus.debug_subscriber_count("market").await;
                                        if subscriber_count == 0 {
                                            warn!("⚠️ No subscribers found for market events on bus [{}] - message will be dropped", bus_id);
                                        } else {
                                            debug!("✅ Found {} subscribers for market events", subscriber_count);
                                        }
                                        
                                        if let Err(e) = bus.publish(market_event).await {
                                            error!("❌ Failed to publish market data event for {}: {}", token.symbol, e);
                                        } else {
                                            debug!("✅ Successfully published market data event for {}", token.symbol);
                                        }
                                    }
                                },
                                Err(e) => {
                                    error!("Failed to store price data for {}: {}", token.symbol, e);
                                    failures += 1;
                                }
                            }
                        }
                        
                        if failures > 0 {
                            warn!("Data collection completed with {} successes and {} failures", 
                                successes, failures);
                        } else {
                            info!("Data collection completed successfully for {} tokens", successes);
                        }
                    },
                    Err(e) => {
                        error!("Failed to fetch market data: {}", e);
                        
                        // Wait a bit longer on errors to avoid hammering the API
                        tokio::time::sleep(Duration::from_secs(60)).await;
                    }
                }
            }
            
            info!("Market data collector task terminated");
        });
        
        self.task_handle = Some(handle);
        Ok(())
    }
    
    /// Stop the data collection process
    pub async fn stop(&mut self) -> Result<()> {
        info!("Stopping market data collector...");
        self.running.store(false, Ordering::SeqCst);
        
        if let Some(handle) = self.task_handle.take() {
            match handle.await {
                Ok(_) => {
                    info!("Market data collector stopped successfully");
                    Ok(())
                },
                Err(e) => {
                    error!("Failed to stop market data collector: {}", e);
                    Err(Error::Task(format!("Failed to stop market data collector: {}", e)))
                }
            }
        } else {
            info!("Market data collector was not running");
            Ok(())
        }
    }
    
    /// Check if the collector is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
} 