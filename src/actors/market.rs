use super::{Actor, Message, Event, MarketEvent, Command, Query, QueryResult};
use crate::api::market::{MarketApi, MarketDataProvider, MarketDataEvent};
use crate::repositories::TokenRepository;
use crate::error::Error;
use crate::types::market::TokenMetrics;
use tokio::time::{interval, Duration};
use chrono::Utc;
use std::sync::Arc;
use serde_json;
use log::{info, debug, error, warn, trace};
use tokio::sync::mpsc;
use futures::executor;
use std::cmp::max;

pub struct MarketDataActor {
    market_api: MarketApi,
    token_repo: TokenRepository,
    message_bus: Arc<super::MessageBus>,
    scan_interval: u64,
    running: bool,
    using_websocket: bool,
    tokens_to_track: Vec<String>,
    ws_receiver: Option<mpsc::Receiver<MarketDataEvent>>,
}

impl MarketDataActor {
    pub fn new(
        market_api: MarketApi,
        token_repo: TokenRepository,
        message_bus: Arc<super::MessageBus>,
        scan_interval: u64,
    ) -> Self {
        Self {
            market_api,
            token_repo,
            message_bus,
            scan_interval,
            running: false,
            using_websocket: false,
            tokens_to_track: Vec::new(),
            ws_receiver: None,
        }
    }

    pub fn with_websocket(
        market_api: MarketApi,
        token_repo: TokenRepository,
        message_bus: Arc<super::MessageBus>,
        tokens_to_track: Vec<String>,
    ) -> Self {
        Self {
            market_api,
            token_repo,
            message_bus,
            scan_interval: 300, // Default scan interval
            running: false,
            using_websocket: true,
            tokens_to_track,
            ws_receiver: None,
        }
    }

    async fn handle_market_data(&mut self, data: Vec<TokenMetrics>) -> Result<(), Error> {
        let batch_size = data.len();
        let provider_name = self.market_api.name();
        debug!("🔄 [POLLING] [{}]: Processing batch of {} market data points", provider_name, batch_size);
        let mut published_count = 0;
        
        // Get the unique identifier of our MessageBus instance
        let bus_id = format!("{:p}", Arc::as_ptr(&self.message_bus));
        
        // Check if we have subscribers before trying to publish
        let subscriber_count = self.message_bus.debug_subscriber_count("market").await;
        if subscriber_count == 0 {
            warn!("⚠️ No subscribers found for market events on bus [{}] - all messages will be dropped", bus_id);
        } else {
            debug!("✅ Found {} subscribers for market events on bus [{}]", subscriber_count, bus_id);
        }
        
        // Process each token
        for token in data {
            // Store price data in the database
            if let Err(e) = self.token_repo.get_db().store_price_data(&token.id, token.price_usd, token.volume_24h) {
                error!("Failed to store price data for {}: {}", token.id, e);
                continue;
            }
            
            // Publish market event
            let event = Event::Market(MarketEvent::PriceUpdate {
                token_id: token.id.clone(),
                price: token.price_usd,
                volume: Some(token.volume_24h),
                timestamp: Utc::now(),
            });
            
            info!("💲 [POLLING] [{}] Price update for {}: ${:.4} (vol: ${:.2}M) to message bus", 
                provider_name, token.id, token.price_usd, token.volume_24h / 1_000_000.0);
            
            if let Err(e) = self.message_bus.publish(event).await {
                error!("Failed to publish market event for {}: {}", token.id, e);
            } else {
                published_count += 1;
            }
        }
        
        info!("📊 [POLLING] [{}] Published {}/{} market data points to message bus", 
             provider_name, published_count, batch_size);
        Ok(())
    }

    async fn handle_market_data_event(&self, event: MarketDataEvent) -> Result<(), Error> {
        trace!("MarketDataActor: Processing incoming MarketDataEvent");
        let provider_name = self.market_api.name();
        
        match event {
            MarketDataEvent::PriceUpdate { token_id, price, volume, change_24h, timestamp } => {
                trace!("MarketDataActor: Handling price update for {} at {}", token_id, timestamp);
                
                // Update token metadata
                if let Err(e) = self.token_repo.update_token_metadata(&token_id, &token_id) {
                    error!("❌ Failed to update token metadata for {}: {}", token_id, e);
                    return Err(e);
                }
                
                trace!("MarketDataActor: Updated metadata for {}", token_id);
                
                // Store price data
                let vol = volume.unwrap_or(0.0);
                if let Err(e) = self.token_repo.get_db().store_price_data(&token_id, price, vol) {
                    error!("❌ Failed to store price data for {}: {}", token_id, e);
                    return Err(e);
                }
                
                trace!("MarketDataActor: Stored price data for {}", token_id);

                // Add detailed debug logs
                debug!("🔄 [WEBSOCKET] [{}]: Received price update for {}: price=${:.4}, volume=${:.2}M, change_24h={:.2}%", 
                    provider_name, token_id, price, vol / 1_000_000.0, change_24h.unwrap_or(0.0));

                // Publish price update event
                let event = Event::Market(MarketEvent::PriceUpdate {
                    token_id: token_id.clone(),
                    price,
                    volume,
                    timestamp,
                });

                info!("💲 [WEBSOCKET] [{}] Price update for {}: ${:.4} (vol: ${:.2}M, 24h:{:.2}%) to message bus", 
                      provider_name, token_id, price, vol / 1_000_000.0, change_24h.unwrap_or(0.0));
                trace!("MarketDataActor: Calling message_bus.publish() for WebSocket price update for {}", token_id);
                
                if let Err(e) = self.message_bus.publish(event).await {
                    error!("❌ Failed to publish market event from WebSocket for {}: {}", token_id, e);
                    return Err(e);
                } else {
                    debug!("✅ [WEBSOCKET] [{}]: Successfully published price update for {}", provider_name, token_id);
                }
            },
            MarketDataEvent::VolumeUpdate { token_id, volume, timestamp } => {
                trace!("MarketDataActor: Handling volume update for {} at {}", token_id, timestamp);
                
                // Store volume data
                if let Err(e) = self.token_repo.get_db().store_price_data(&token_id, 0.0, volume) {
                    error!("❌ Failed to store volume data for {}: {}", token_id, e);
                    return Err(e);
                }
                
                debug!("📊 [WEBSOCKET] [{}] Volume update for {}: ${:.2}M", provider_name, token_id, volume / 1_000_000.0);
            },
            MarketDataEvent::Error(error_msg) => {
                error!("⚠️ [WEBSOCKET] [{}] Market data error: {}", provider_name, error_msg);
                
                let event = Event::Market(MarketEvent::MarketDataError(error_msg.clone()));
                trace!("MarketDataActor: Publishing market error event: {}", error_msg);
                
                if let Err(e) = self.message_bus.publish(event).await {
                    error!("❌ Failed to publish market error event: {}", e);
                    return Err(e);
                } else {
                    debug!("Published market error event to message bus");
                }
            }
        }

        trace!("MarketDataActor: Successfully processed MarketDataEvent");
        Ok(())
    }

    async fn start_market_data_loop(&mut self) -> Result<(), Error> {
        info!("🔄 MarketDataActor: Starting market data processing loop");
        debug!("🔌 MarketDataActor: Strategy will process data for tokens: {:?}", self.tokens_to_track);
        
        let result = if self.using_websocket && self.market_api.supports_websocket() {
            debug!("📡 Using WebSocket for real-time market data from {}", self.market_api.name());
            self.start_websocket_loop().await
        } else {
            debug!("⏱️ Using polling at {}s interval for market data from {}", 
                  self.scan_interval, self.market_api.name());
            self.start_polling_loop().await
        };
        
        info!("MarketDataActor: Market data loop has ended");
        result
    }

    async fn start_polling_loop(&mut self) -> Result<(), Error> {
        let provider_name = self.market_api.name().to_string();
        info!("⏱️ MarketDataActor: Starting POLLING loop with interval of {}s using [{}]", 
              self.scan_interval, provider_name);
        let mut interval = interval(Duration::from_secs(self.scan_interval));
        let mut poll_count = 0;
        let mut total_data_points = 0;
        
        while self.running {
            trace!("MarketDataActor: Waiting for next polling interval");
            interval.tick().await;
            poll_count += 1;
            
            debug!("⏰ [POLLING] [{}]: Poll #{} - Fetching market data", 
                  provider_name, poll_count);
            match self.market_api.get_market_data().await {
                Ok(data) => {
                    let data_count = data.len();
                    total_data_points += data_count;
                    info!("📈 [POLLING] [{}]: Poll #{} - Fetched {} tokens (total: {} data points processed)", 
                         provider_name, poll_count, data_count, total_data_points);
                    
                    if let Err(e) = self.handle_market_data(data).await {
                        error!("❌ Error handling market data: {}", e);
                    } else {
                        debug!("✅ [POLLING] [{}]: Successfully processed data batch for poll #{}", provider_name, poll_count);
                    }
                },
                Err(e) => {
                    error!("❌ [POLLING] [{}]: Failed to fetch market data (poll #{}): {}", provider_name, poll_count, e);
                    trace!("MarketDataActor: Publishing market error event");
                    let event = Event::Market(MarketEvent::MarketDataError(e.to_string()));
                    if let Err(e) = self.message_bus.publish(event).await {
                        error!("❌ Failed to publish market error event: {}", e);
                    } else {
                        debug!("📢 Published market error event to message bus");
                    }
                }
            }
        }

        info!("⏹️ MarketDataActor: [{}] Polling loop ended after {} polls (processed {} total data points)", 
             provider_name, poll_count, total_data_points);
        Ok(())
    }

    async fn start_websocket_loop(&mut self) -> Result<(), Error> {
        // Create a new mpsc channel for WebSocket messages
        let (ws_sender, ws_receiver) = mpsc::channel(100);
        self.ws_receiver = Some(ws_receiver);
        
        // Get provider name before mutable borrow
        let provider_name = self.market_api.name().to_string();
        
        // Connect to WebSocket with all tokens
        info!("🌐 MarketDataActor: Starting WebSocket connection to [{}] for {} tokens", 
              provider_name, self.tokens_to_track.len());
        
        if let Err(e) = self.market_api.connect_websocket(self.tokens_to_track.clone(), ws_sender).await {
            error!("❌ Failed to connect to WebSocket: {}", e);
            return Err(e);
        }
        
        info!("✅ Connected to [{}] WebSocket successfully", provider_name);
        
        // Create a polling timer for supplemental data
        let poll_interval = max(300, self.scan_interval); // At least 5 minutes
        let mut poll_timer = interval(Duration::from_secs(poll_interval));
        let mut ws_events_count = 0;
        
        info!("📡 MarketDataActor: Starting WebSocket processing loop with supplemental polling every {}s", poll_interval);
        
        while self.running {
            if let Some(receiver) = &mut self.ws_receiver {
                trace!("MarketDataActor: Waiting for WebSocket events or poll timer");
                tokio::select! {
                    // Handle WebSocket events
                    Some(event) = receiver.recv() => {
                        trace!("MarketDataActor: Received WebSocket event");
                        ws_events_count += 1;
                        
                        if let Err(e) = self.handle_market_data_event(event).await {
                            error!("❌ Error handling WebSocket market data event: {}", e);
                        } else {
                            if ws_events_count % 100 == 0 {
                                info!("📊 [WEBSOCKET] [{}] Processed {} real-time events so far", 
                                     provider_name, ws_events_count);
                            }
                        }
                    },
                    // Periodically poll for full market data
                    _ = poll_timer.tick() => {
                        debug!("⏰ [SUPPLEMENTAL POLL] [{}]: Fetching complete market data set", provider_name);
                        
                        match self.market_api.get_market_data().await {
                            Ok(data) => {
                                // Store data length before moving
                                let data_len = data.len();
                                
                                // Filter for only tracked tokens
                                let tracked_tokens = self.tokens_to_track.clone();
                                let filtered_data = data.into_iter()
                                    .filter(|token| tracked_tokens.contains(&token.id))
                                    .collect::<Vec<_>>();
                                
                                info!("📈 [SUPPLEMENTAL POLL] [{}]: Fetched {} tokens (filtered to {} tracked tokens)",
                                    provider_name, data_len, filtered_data.len());
                                
                                if let Err(e) = self.handle_market_data(filtered_data).await {
                                    error!("❌ Error handling supplemental poll data: {}", e);
                                } else {
                                    debug!("✅ [SUPPLEMENTAL POLL] [{}]: Successfully processed supplemental data", 
                                          provider_name);
                                }
                            },
                            Err(e) => {
                                error!("❌ [SUPPLEMENTAL POLL] [{}]: Failed to fetch market data: {}", 
                                      provider_name, e);
                            }
                        }
                    }
                }
            } else {
                error!("❌ WebSocket receiver not initialized");
                break;
            }
        }

        // Disconnect WebSocket when exiting
        info!("📴 MarketDataActor: Disconnecting [{}] WebSocket after processing {} events", 
             provider_name, ws_events_count);
        if let Err(e) = self.market_api.disconnect_websocket().await {
            error!("❌ Error disconnecting WebSocket: {}", e);
        } else {
            debug!("WebSocket disconnected successfully");
        }

        info!("⏹️ MarketDataActor: [{}] WebSocket loop ended", provider_name);
        Ok(())
    }
}

impl Actor for MarketDataActor {
    fn start(&mut self) -> Result<(), Error> {
        self.running = true;
        
        // Log the unique pointer address of the MessageBus for debugging
        let bus_id = format!("{:p}", Arc::as_ptr(&self.message_bus));
        info!("🚀 Starting MarketDataActor with MessageBus [id: {}]", bus_id);
        
        // Explain the data source strategy - use local variables to avoid borrow issues
        let provider_name = self.market_api.name();
        let using_websocket = self.using_websocket;
        let supports_websocket = self.market_api.supports_websocket();
        let scan_interval = self.scan_interval;
        let tokens_to_track = self.tokens_to_track.clone();
        
        if using_websocket && supports_websocket {
            info!("📡 DATA SOURCE STRATEGY: Using a hybrid approach for market data from {}:", provider_name);
            info!("    1️⃣ PRIMARY: Real-time WebSocket connection for {} specific tokens", 
                  if tokens_to_track.is_empty() { "default".to_string() } 
                  else { tokens_to_track.len().to_string() });
            info!("    2️⃣ SECONDARY: Supplemental API polling every {}s for broader market data", 
                  scan_interval * 10);
            info!("    ℹ️ WebSocket updates will be tagged as [WEBSOCKET] [{}] in logs", provider_name);
            info!("    ℹ️ Polling updates will be tagged as [POLLING] [{}] or [SUPPLEMENTAL POLL] [{}] in logs", 
                 provider_name, provider_name);
            
            if !tokens_to_track.is_empty() {
                let token_list = if tokens_to_track.len() <= 5 {
                    tokens_to_track.join(", ")
                } else {
                    format!("{} and {} more", 
                            tokens_to_track[0..5].join(", "),
                            tokens_to_track.len() - 5)
                };
                info!("    📊 WebSocket tokens: {}", token_list);
            }
        } else {
            info!("📡 DATA SOURCE STRATEGY: Using regular API polling every {}s from {}", 
                 scan_interval, provider_name);
            info!("    ℹ️ All updates will be tagged as [POLLING] [{}] in logs", provider_name);
            info!("    ℹ️ WebSocket not available with current provider or configuration");
        }
        
        // Check for existing subscribers before we start publishing
        let subscriber_count = executor::block_on(async {
            self.message_bus.debug_subscriber_count("market").await
        });
        
        if subscriber_count == 0 {
            warn!("⚠️ No subscribers found for market events! Publisher started but messages will be dropped.");
            warn!("⚠️ Please ensure a subscriber is connected to the MessageBus before starting MarketDataActor.");
        } else {
            info!("✅ Found {} subscribers for market events - events will be delivered", subscriber_count);
        }
        
        // Initialize WebSocket if enabled and supported
        if self.using_websocket && self.market_api.supports_websocket() {
            info!("Initializing WebSocket connection for real-time market data");
            
            // If no tokens are specified, add some default ones
            if self.tokens_to_track.is_empty() {
                self.tokens_to_track = vec!["bitcoin".to_string(), "ethereum".to_string()];
                info!("No tokens specified for tracking, using defaults: {:?}", self.tokens_to_track);
            }
        }
        
        // Spawn the market data task
        let mut actor_clone = self.clone();
        tokio::spawn(async move {
            if let Err(e) = actor_clone.start_market_data_loop().await {
                error!("Market data loop error: {}", e);
            }
        });
        
        Ok(())
    }

    fn stop(&mut self) -> Result<(), Error> {
        self.running = false;
        info!("Stopping MarketDataActor");
        Ok(())
    }

    fn handle_message(&mut self, msg: Message) -> Result<(), Error> {
        match msg {
            Message::Command(cmd) => match cmd {
                Command::Start => {
                    self.running = true;
                    debug!("MarketDataActor received start command");
                    Ok(())
                },
                Command::Stop => {
                    self.running = false;
                    debug!("MarketDataActor received stop command");
                    Ok(())
                },
                Command::UpdateConfig(config) => {
                    if let Some(interval) = config.get("scan_interval").and_then(|v| v.as_u64()) {
                        self.scan_interval = interval;
                        info!("Updated scan interval to {} seconds", interval);
                    }
                    if let Some(use_ws) = config.get("use_websocket").and_then(|v| v.as_bool()) {
                        self.using_websocket = use_ws;
                        info!("Updated WebSocket usage to {}", use_ws);
                    }
                    if let Some(tokens) = config.get("tokens_to_track").and_then(|v| v.as_array()) {
                        self.tokens_to_track = tokens.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect();
                        info!("Updated tokens to track: {}", self.tokens_to_track.join(", "));
                    }
                    Ok(())
                },
            },
            Message::Query(query, responder) => match query {
                Query::GetStatus => {
                    let status = format!(
                        "MarketDataActor running: {}, WebSocket: {}, Provider: {}", 
                        self.running, 
                        self.using_websocket,
                        self.market_api.name()
                    );
                    responder.send(Ok(QueryResult::Status(status)))
                        .map_err(|e| Error::Task(format!("Failed to send status response: {:?}", e)))
                },
                Query::GetMetrics => {
                    let metrics = serde_json::json!({
                        "scan_interval": self.scan_interval,
                        "running": self.running,
                        "using_websocket": self.using_websocket,
                        "provider": self.market_api.name(),
                        "tokens_tracked": self.tokens_to_track,
                    });
                    responder.send(Ok(QueryResult::Metrics(metrics)))
                        .map_err(|e| Error::Task(format!("Failed to send metrics response: {:?}", e)))
                },
                _ => {
                    responder.send(Err(Error::Task("Unsupported query type".to_string())))
                        .map_err(|e| Error::Task(format!("Failed to send error response: {:?}", e)))
                },
            },
            Message::Event(_) => {
                // MarketDataActor doesn't handle events
                Ok(())
            },
        }
    }
}

// Implement Clone for MarketDataActor to allow spawning the task
impl Clone for MarketDataActor {
    fn clone(&self) -> Self {
        // Create a new channel for the clone
        let (ws_sender, ws_receiver) = if self.ws_receiver.is_some() {
            let (sender, receiver) = mpsc::channel(100);
            (Some(sender), Some(receiver))
        } else {
            (None, None)
        };

        Self {
            market_api: self.market_api.clone(),
            token_repo: self.token_repo.clone(),
            message_bus: self.message_bus.clone(),
            scan_interval: self.scan_interval,
            running: self.running,
            using_websocket: self.using_websocket,
            tokens_to_track: self.tokens_to_track.clone(),
            ws_receiver,
        }
    }
} 