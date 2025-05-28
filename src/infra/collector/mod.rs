use crate::core::config::Config;
use crate::core::error::{Error, Result};
use crate::domain::trading::execution::bot::is_forced_shutdown;
use crate::infra::actors::{Event, MarketEvent, MessageBus};
use crate::infra::api::market::{create_market_api, MarketDataEvent, MarketDataProvider};
use crate::infra::db::repositories::TokenRepository;
use chrono::Utc;
use log::{debug, error, info, trace, warn};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

/// Data collector that periodically fetches market data and stores it in the database
pub struct DataCollector {
    /// Is the collector running?
    running: Arc<AtomicBool>,
    /// Interval between data collections in seconds
    collection_interval: u64,
    /// Market API instance
    market_api: Option<Box<dyn MarketDataProvider>>,
    /// Token repository
    token_repo: Arc<TokenRepository>,
    /// Task handle for polling collection
    task_handle: Option<tokio::task::JoinHandle<()>>,
    /// WebSocket task handle
    ws_task_handle: Option<tokio::task::JoinHandle<()>>,
    /// Config
    config: Arc<Config>,
    /// Message bus for publishing market events
    message_bus: Option<Arc<MessageBus>>,
    /// WebSocket connection status
    ws_connected: Arc<AtomicBool>,
    /// Current tokens being tracked via WebSocket
    tracked_tokens: Arc<tokio::sync::RwLock<HashSet<String>>>,
    /// Channel for WebSocket reconnect requests
    reconnect_sender: Option<mpsc::Sender<()>>,
}

impl DataCollector {
    /// Create a new data collector
    pub fn new(
        token_repo: Arc<TokenRepository>,
        collection_interval: u64,
        config: Arc<Config>,
        message_bus: Option<Arc<MessageBus>>,
    ) -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            collection_interval,
            market_api: None,
            token_repo,
            task_handle: None,
            ws_task_handle: None,
            config,
            message_bus,
            ws_connected: Arc::new(AtomicBool::new(false)),
            tracked_tokens: Arc::new(tokio::sync::RwLock::new(HashSet::new())),
            reconnect_sender: None,
        }
    }

    /// Start the data collection process
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting market data collector...");
        self.running.store(true, Ordering::SeqCst);

        // Initialize the market API provider using the factory function
        self.market_api = Some(create_market_api(&self.config));
        info!(
            "Market API provider initialized: {}",
            self.market_api.as_ref().unwrap().name()
        );

        let (reconnect_sender, reconnect_receiver) = mpsc::channel::<()>(10);
        self.reconnect_sender = Some(reconnect_sender.clone());

        let interval_duration = Duration::from_secs(self.collection_interval);
        let running_clone = self.running.clone();
        let token_repo_clone = self.token_repo.clone();
        let collection_interval_clone = self.collection_interval;
        let message_bus_clone = self.message_bus.clone();
        let tracked_tokens_clone = self.tracked_tokens.clone();
        let ws_connected_clone = self.ws_connected.clone();
        let market_api_clone_for_poll = self.market_api.as_ref().unwrap().clone_box();
        let config_clone = self.config.clone();

        let has_websocket_support = self
            .market_api
            .as_ref()
            .map_or(false, |api| api.supports_websocket());

        let handle = tokio::spawn(async move {
            info!(
                "Market data poll-based collection task started with interval {}s",
                collection_interval_clone
            );
            let mut interval_timer = interval(interval_duration);

            while running_clone.load(Ordering::SeqCst) {
                if is_forced_shutdown() {
                    info!(
                        "Force shutdown detected, terminating market data collector polling task"
                    );
                    break;
                }
                interval_timer.tick().await;

                let provider = market_api_clone_for_poll.as_ref();
                info!(
                    "Using {} provider for poll-based data collection",
                    provider.name()
                );

                match provider
                    .get_market_data(config_clone.trading.wide_scan_mode, &[])
                    .await
                {
                    Ok(token_metrics_list) => {
                        info!(
                            "📊 Collected market data for {} tokens (polling)",
                            token_metrics_list.len()
                        );
                        let token_data_list: Vec<crate::core::models::token::TokenData> =
                            token_metrics_list
                                .iter()
                                .map(crate::core::models::token::TokenData::from)
                                .collect();

                        let mut successes = 0;
                        let mut failures = 0;
                        let mut new_tokens = HashSet::new();

                        for token in token_data_list {
                            if let Err(e) = token_repo_clone
                                .update_token_metadata(&token.id, &token.symbol, &token.name)
                                .await
                            {
                                error!("Failed to update metadata for {}: {}", token.id, e);
                            }

                            if has_websocket_support {
                                new_tokens.insert(token.id.clone());
                            }

                            match token_repo_clone
                                .store_price_data(&token.id, token.price_usd, token.volume_24h)
                                .await
                            {
                                Ok(_) => {
                                    successes += 1;
                                    if let Some(bus) = &message_bus_clone {
                                        let market_event =
                                            Event::Market(MarketEvent::PriceUpdate {
                                                token_id: token.id.clone(),
                                                price: token.price_usd,
                                                volume: Some(token.volume_24h),
                                                timestamp: Utc::now(),
                                            });
                                        if let Err(e) = bus.publish(market_event).await {
                                            debug!(
                                                "Failed to publish market data event for {}: {}",
                                                token.symbol, e
                                            );
                                        } else {
                                            trace!(
                                                "Successfully published market data event for {}",
                                                token.symbol
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!(
                                        "Failed to store price data for {}: {}",
                                        token.symbol, e
                                    );
                                    failures += 1;
                                }
                            }
                        }
                        if has_websocket_support {
                            let mut current_tokens = tracked_tokens_clone.write().await;
                            if *current_tokens != new_tokens {
                                info!(
                                    "Token list changed (polling), updating WebSocket subscription"
                                );
                                *current_tokens = new_tokens;
                                // Use a local clone of reconnect_sender for this specific if block
                                let local_reconnect_sender = reconnect_sender.clone();
                                if ws_connected_clone.load(Ordering::SeqCst) {
                                    if let Err(e) = local_reconnect_sender.send(()).await {
                                        // Use local_reconnect_sender
                                        error!("Failed to send WebSocket reconnect request: {}", e);
                                    }
                                }
                            }
                        }
                        if failures > 0 {
                            warn!(
                                "Data collection completed with {} successes and {} failures",
                                successes, failures
                            );
                        } else {
                            info!(
                                "Data collection completed successfully for {} tokens",
                                successes
                            );
                        }
                    }
                    Err(e) => {
                        error!("Failed to fetch market data (polling): {}", e);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
            info!("Market data collector polling task terminated");
        });
        self.task_handle = Some(handle);

        if has_websocket_support {
            self.setup_websocket_connection(reconnect_receiver).await?; // Pass receiver here
        }
        Ok(())
    }

    /// Setup WebSocket connection with reconnection support
    async fn setup_websocket_connection(
        &mut self,
        mut reconnect_receiver: mpsc::Receiver<()>,
    ) -> Result<()> {
        let market_api_for_ws = match &self.market_api {
            Some(api) => api.clone_box(),
            None => {
                return Err(Error::InvalidInput(
                    "Market API not initialized".to_string(),
                ))
            }
        };

        // This clone will be moved into the tokio::spawn task
        let provider_for_ws_task = market_api_for_ws.clone_box();
        let ws_connected_clone = self.ws_connected.clone();
        let running_clone = self.running.clone();
        let tracked_tokens_clone = self.tracked_tokens.clone();
        let message_bus_clone = self.message_bus.clone(); // Clone message_bus for the task

        let ws_task_handle = tokio::spawn(async move {
            loop {
                if !running_clone.load(Ordering::SeqCst) || is_forced_shutdown() {
                    info!("WebSocket connection loop terminating (shutdown).");
                    ws_connected_clone.store(false, Ordering::SeqCst);
                    let _ = provider_for_ws_task.disconnect_websocket().await; // Attempt disconnect
                    break;
                }

                // Create a new channel for this connection attempt
                let (ws_event_sender, mut ws_event_receiver) =
                    mpsc::channel::<MarketDataEvent>(256);

                let tokens_to_subscribe = {
                    // Scoped to release lock quickly
                    let lock = tracked_tokens_clone.read().await;
                    lock.iter().cloned().collect::<Vec<_>>()
                };

                if tokens_to_subscribe.is_empty() {
                    info!("No tokens to track via WebSocket, pausing connection attempt.");
                    ws_connected_clone.store(false, Ordering::SeqCst);
                    let _ = provider_for_ws_task.disconnect_websocket().await;
                    // Wait for a reconnect signal or a timeout before retrying
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(30)) => {},
                        _ = reconnect_receiver.recv() => {
                            info!("Reconnect signal received while no tokens were tracked.");
                        }
                    }
                    continue; // Re-evaluate tokens to track
                }

                info!(
                    "Attempting to connect WebSocket for tokens: {:?}",
                    tokens_to_subscribe
                );

                match provider_for_ws_task
                    .connect_websocket(tokens_to_subscribe.clone(), ws_event_sender)
                    .await
                {
                    Ok(_) => {
                        info!(
                            "Successfully connected to WebSocket for tokens: {:?}",
                            tokens_to_subscribe
                        );
                        ws_connected_clone.store(true, Ordering::SeqCst);

                        // Event processing loop
                        loop {
                            tokio::select! {
                                _ = tokio::time::sleep(Duration::from_secs(5)), if !running_clone.load(Ordering::SeqCst) => {
                                    info!("WebSocket event loop: Shutdown signal during sleep.");
                                    break; // Exit inner loop to disconnect
                                }
                                Some(event_result) = ws_event_receiver.recv() => {
                                    if !running_clone.load(Ordering::SeqCst) { break; }
                                    match event_result {
                                        MarketDataEvent::PriceUpdate { token_id, price, volume, change_24h, timestamp } => {
                                            trace!("WS Price Update: {} @ {} (Vol: {:?}, Change: {:?})", token_id, price, volume, change_24h);
                                            if let Some(bus) = &message_bus_clone {
                                                let market_event = Event::Market(MarketEvent::PriceUpdate {
                                                    token_id, price, volume, timestamp,
                                                });
                                                if let Err(e) = bus.publish(market_event).await {
                                                    warn!("Failed to publish WS PriceUpdate event: {}", e);
                                                }
                                            }
                                        }
                                        MarketDataEvent::VolumeUpdate { token_id, volume, timestamp } => {
                                            trace!("WS Volume Update: {} - {}", token_id, volume);
                                             if let Some(bus) = &message_bus_clone {
                                                let market_event = Event::Market(MarketEvent::VolumeUpdate {
                                                    token_id, volume, timestamp,
                                                });
                                                if let Err(e) = bus.publish(market_event).await {
                                                    warn!("Failed to publish WS VolumeUpdate event: {}", e);
                                                }
                                            }
                                        }
                                        MarketDataEvent::Error(e_str) => {
                                            error!("WebSocket Provider Error: {}", e_str);
                                            // This error might mean the connection is dead. Break to reconnect.
                                            break;
                                        }
                                    }
                                }
                                _ = reconnect_receiver.recv() => {
                                    info!("Reconnect signal received. Terminating current WebSocket connection to re-establish.");
                                    let _ = provider_for_ws_task.disconnect_websocket().await; // Attempt disconnect
                                    ws_connected_clone.store(false, Ordering::SeqCst);
                                    break; // Break inner loop to trigger reconnection
                                }
                                else => {
                                    // Channel closed, provider likely dropped the sender or panicked.
                                    warn!("WebSocket event channel closed unexpectedly. Attempting reconnect.");
                                    let _ = provider_for_ws_task.disconnect_websocket().await;
                                    ws_connected_clone.store(false, Ordering::SeqCst);
                                    break; // Break inner loop
                                }
                            }
                            if !running_clone.load(Ordering::SeqCst) || is_forced_shutdown() {
                                break;
                            }
                        }
                        // After breaking inner loop (e.g. for reconnect or error)
                        let _ = provider_for_ws_task.disconnect_websocket().await;
                        ws_connected_clone.store(false, Ordering::SeqCst);
                    }
                    Err(e) => {
                        error!("Failed to connect to WebSocket: {}. Retrying in 10s.", e);
                        ws_connected_clone.store(false, Ordering::SeqCst);
                        let _ = provider_for_ws_task.disconnect_websocket().await; // Ensure clean state
                        tokio::time::sleep(Duration::from_secs(10)).await; // Wait before retrying connection
                    }
                }
                // Check running state before looping again for connection attempt
                if !running_clone.load(Ordering::SeqCst) || is_forced_shutdown() {
                    info!("WebSocket connection loop terminating (shutdown before next attempt).");
                    let _ = provider_for_ws_task.disconnect_websocket().await;
                    ws_connected_clone.store(false, Ordering::SeqCst);
                    break;
                }
            }
            info!("WebSocket connection task ended.");
        });

        self.ws_task_handle = Some(ws_task_handle);
        info!("WebSocket connection manager task spawned.");
        Ok(())
    }

    /// Update the list of tokens to track via WebSocket
    pub async fn update_tracked_tokens(&self, new_tokens: Vec<String>) {
        let mut current_tokens = self.tracked_tokens.write().await;
        let new_token_set: HashSet<String> = new_tokens.into_iter().collect();

        if *current_tokens != new_token_set {
            info!(
                "Updating tracked tokens for WebSocket to: {:?}",
                new_token_set
            );
            *current_tokens = new_token_set;
            // If WebSocket is connected, trigger a reconnect to update subscriptions
            if self.ws_connected.load(Ordering::SeqCst) {
                if let Some(sender) = &self.reconnect_sender {
                    if let Err(e) = sender.send(()).await {
                        error!("Failed to send WebSocket reconnect signal: {}", e);
                    }
                }
            }
        }
    }

    /// Stop the data collection process
    pub async fn stop(&mut self) -> Result<()> {
        info!("Stopping market data collector...");
        self.running.store(false, Ordering::SeqCst);

        // Abort the polling task
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
            match handle.await {
                Ok(_) => info!("Market data polling task stopped gracefully."),
                Err(e) if e.is_cancelled() => {
                    info!("Market data polling task cancellation confirmed.")
                }
                Err(e) => error!("Error stopping market data polling task: {}", e),
            }
        }

        // Abort the WebSocket task
        if let Some(handle) = self.ws_task_handle.take() {
            handle.abort();
            match handle.await {
                Ok(_) => info!("WebSocket task stopped gracefully."),
                Err(e) if e.is_cancelled() => info!("WebSocket task cancellation confirmed."),
                Err(e) => error!("Error stopping WebSocket task: {}", e),
            }
        }
        // Explicitly disconnect if the provider supports it
        if let Some(provider) = self.market_api.as_mut() {
            if provider.supports_websocket() {
                // This might be tricky if disconnect is async and stop is sync
                // For now, assume provider handles its own cleanup on Drop or explicit disconnect
                // provider.disconnect_ws().await.unwrap_or_else(|e| {
                //     error!("Error during WebSocket disconnect: {}", e);
                // });
                info!(
                    "WebSocket disconnect should be handled by the provider or task termination."
                );
            }
        }

        self.ws_connected.store(false, Ordering::SeqCst);
        info!("Market data collector stopped.");
        Ok(())
    }

    /// Check if the collector is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Check if WebSocket is connected
    pub fn is_websocket_connected(&self) -> bool {
        self.ws_connected.load(Ordering::SeqCst)
    }
}
