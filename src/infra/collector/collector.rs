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
use tokio::task::JoinHandle;
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

        let (reconnect_sender, _reconnect_receiver) = mpsc::channel::<()>(10);
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
                                if ws_connected_clone.load(Ordering::SeqCst) {
                                    if let Err(e) = reconnect_sender.send(()).await {
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
            self.setup_websocket_connection().await?;
        }
        Ok(())
    }

    /// Setup WebSocket connection with reconnection support
    async fn setup_websocket_connection(&mut self) -> Result<()> {
        let market_api_for_ws = match &self.market_api {
            Some(api) => api.clone_box(),
            None => {
                return Err(Error::InvalidInput(
                    "Market API not initialized".to_string(),
                ))
            }
        };

        let provider_for_ws = market_api_for_ws.as_ref();

        if !provider_for_ws.supports_websocket() {
            warn!(
                "Provider '{}' does not support WebSockets. WebSocket setup skipped.",
                provider_for_ws.name()
            );
            return Ok(());
        }
        info!(
            "Setting up WebSocket connection for real-time market data with provider: {}",
            provider_for_ws.name()
        );

        let running_ws_clone = self.running.clone();
        let ws_connected_ws_clone = self.ws_connected.clone();
        let tracked_tokens_ws_clone = self.tracked_tokens.clone();
        let message_bus_ws_clone = self.message_bus.clone();
        let token_repo_ws_clone = self.token_repo.clone();

        let (reconnect_tx, mut reconnect_rx_ws) = mpsc::channel::<()>(10);
        self.reconnect_sender = Some(reconnect_tx);
        let (ws_event_sender, mut ws_event_receiver_ws) = mpsc::channel::<MarketDataEvent>(100);

        let ws_handle = tokio::spawn({
            let running_task_clone = running_ws_clone.clone();
            let ws_connected_task_clone = ws_connected_ws_clone.clone();
            let tracked_tokens_task_clone = tracked_tokens_ws_clone.clone();
            let mut provider_in_task = market_api_for_ws.clone_box();

            async move {
                info!("🔌 WebSocket real-time data collection task started");
                let mut backoff_secs = 1;
                const MAX_BACKOFF: u64 = 60;

                while running_task_clone.load(Ordering::SeqCst) {
                    if is_forced_shutdown() {
                        info!("Force shutdown detected, terminating WebSocket connection task");
                        break;
                    }
                    let tokens = {
                        tracked_tokens_task_clone
                            .read()
                            .await
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                    };
                    if tokens.is_empty() {
                        info!("No tokens to track via WebSocket yet, waiting...");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                    info!(
                        "Connecting to WebSocket for {} tokens with {}",
                        tokens.len(),
                        provider_in_task.name()
                    );

                    if !provider_in_task.supports_websocket() {
                        error!(
                            "Provider '{}' lost WebSocket support unexpectedly!",
                            provider_in_task.name()
                        );
                        tokio::time::sleep(Duration::from_secs(30)).await;
                        continue;
                    }

                    match provider_in_task
                        .connect_websocket(tokens, ws_event_sender.clone())
                        .await
                    {
                        Ok(()) => {
                            info!(
                                "🟢 Successfully connected to WebSocket with {}",
                                provider_in_task.name()
                            );
                            ws_connected_task_clone.store(true, Ordering::SeqCst);
                            backoff_secs = 1;
                        }
                        Err(e) => {
                            error!(
                                "❌ Failed to connect to WebSocket with {}: {}",
                                provider_in_task.name(),
                                e
                            );
                            ws_connected_task_clone.store(false, Ordering::SeqCst);
                            info!("Retrying WebSocket connection in {} seconds", backoff_secs);
                            tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                            backoff_secs = std::cmp::min(backoff_secs * 2, MAX_BACKOFF);
                            continue;
                        }
                    }
                    tokio::select! {
                        _ = reconnect_rx_ws.recv() => {
                            info!("🔄 Reconnect request received, restarting WebSocket connection with {}", provider_in_task.name());
                            if let Err(e) = provider_in_task.disconnect_websocket().await {
                                error!("Error disconnecting WebSocket: {}", e);
                            }
                            ws_connected_task_clone.store(false, Ordering::SeqCst);
                        },
                        _ = tokio::time::sleep(Duration::from_secs(300)) => {
                            if !ws_connected_task_clone.load(Ordering::SeqCst) {
                                info!("WebSocket connection appears down with {}, reconnecting loop will trigger", provider_in_task.name());
                                break;
                            }
                            debug!("WebSocket heartbeat: connection to {} active", provider_in_task.name());
                        }
                    }
                }
                if ws_connected_task_clone.load(Ordering::SeqCst) {
                    info!(
                        "Disconnecting WebSocket from {} on task termination",
                        provider_in_task.name()
                    );
                    if let Err(e) = provider_in_task.disconnect_websocket().await {
                        error!("Error disconnecting WebSocket: {}", e);
                    }
                    ws_connected_task_clone.store(false, Ordering::SeqCst);
                }
                info!("WebSocket connection task terminated");
            }
        });

        let ws_processor_handle = tokio::spawn({
            let running_proc_clone = running_ws_clone.clone();
            async move {
                info!("📊 WebSocket event processor task started");
                let mut price_updates_batch: Vec<(String, f64, f64)> = Vec::with_capacity(50);
                let mut last_flush_time = tokio::time::Instant::now();
                const FLUSH_INTERVAL_MS: u64 = 100;
                const BATCH_SIZE: usize = 20;

                while running_proc_clone.load(Ordering::SeqCst) {
                    if is_forced_shutdown() {
                        info!("Force shutdown detected, terminating WebSocket event processor");
                        break;
                    }
                    let now = tokio::time::Instant::now();
                    let should_flush_time = now.duration_since(last_flush_time).as_millis()
                        >= FLUSH_INTERVAL_MS as u128;
                    let should_flush_size = price_updates_batch.len() >= BATCH_SIZE;

                    if !price_updates_batch.is_empty() && (should_flush_time || should_flush_size) {
                        debug!(
                            "Flushing batch of {} price updates from WebSocket",
                            price_updates_batch.len()
                        );
                        let repo_clone_flush = token_repo_ws_clone.clone();
                        let updates_to_flush = std::mem::take(&mut price_updates_batch);
                        let handle_flush = tokio::spawn(async move {
                            repo_clone_flush
                                .batch_store_price_data(&updates_to_flush)
                                .await
                        });
                        match handle_flush.await {
                            Ok(Ok(_)) => trace!("WebSocket batch store successful"),
                            Ok(Err(e)) => {
                                error!("Failed to store WebSocket batch price data: {}", e)
                            }
                            Err(e) => error!("WebSocket batch store task failed: {}", e),
                        }
                        last_flush_time = now;
                    }

                    match tokio::time::timeout(
                        std::time::Duration::from_millis(50),
                        ws_event_receiver_ws.recv(),
                    )
                    .await
                    {
                        Ok(Some(event)) => match event {
                            MarketDataEvent::PriceUpdate {
                                token_id,
                                price,
                                volume,
                                timestamp,
                                change_24h,
                            } => {
                                debug!(
                                    "🔄 WebSocket price update: {} = ${:.4} (24h: {:?} %)",
                                    token_id,
                                    price,
                                    change_24h.map(|c| c * 100.0)
                                );
                                let vol = volume.unwrap_or(0.0);
                                price_updates_batch.push((token_id.clone(), price, vol));
                                if let Some(bus) = &message_bus_ws_clone {
                                    let market_event = Event::Market(MarketEvent::PriceUpdate {
                                        token_id: token_id.clone(),
                                        price: price,
                                        volume: volume,
                                        timestamp: timestamp,
                                    });
                                    if let Err(e) = bus.publish(market_event).await {
                                        error!("Failed to publish WebSocket price update: {}", e);
                                    }
                                }
                            }
                            MarketDataEvent::VolumeUpdate {
                                token_id,
                                volume,
                                timestamp,
                            } => {
                                debug!(
                                    "🔄 WebSocket volume update: {} = ${:.2}M",
                                    token_id,
                                    volume / 1_000_000.0
                                );
                                price_updates_batch.push((token_id.clone(), 0.0, volume));
                                if let Some(bus) = &message_bus_ws_clone {
                                    let market_event = Event::Market(MarketEvent::VolumeUpdate {
                                        token_id: token_id.clone(),
                                        volume: volume,
                                        timestamp: timestamp,
                                    });
                                    if let Err(e) = bus.publish(market_event).await {
                                        error!("Failed to publish WebSocket volume update: {}", e);
                                    }
                                }
                            }
                            MarketDataEvent::Error(err) => {
                                error!("❌ WebSocket error received in processor: {}", err);
                                if !price_updates_batch.is_empty() {
                                    let repo_clone_final_err = token_repo_ws_clone.clone();
                                    let updates_final_err =
                                        std::mem::take(&mut price_updates_batch);
                                    let _ = tokio::spawn(async move {
                                        if let Err(e) = repo_clone_final_err
                                            .batch_store_price_data(&updates_final_err)
                                            .await
                                        {
                                            error!(
                                                "Failed to store final batch on WS error: {}",
                                                e
                                            );
                                        }
                                    })
                                    .await;
                                }
                            }
                        },
                        Ok(None) => {
                            info!("WebSocket event channel closed, terminating processor.");
                            if !price_updates_batch.is_empty() {
                                let repo_clone_final_close = token_repo_ws_clone.clone();
                                let updates_final_close = std::mem::take(&mut price_updates_batch);
                                let _ = tokio::spawn(async move {
                                    if let Err(e) = repo_clone_final_close
                                        .batch_store_price_data(&updates_final_close)
                                        .await
                                    {
                                        error!("Failed to store final batch on WS close: {}", e);
                                    }
                                })
                                .await;
                            }
                            break;
                        }
                        Err(_) => { /* Timeout, continue */ }
                    }
                }
                if running_proc_clone.load(Ordering::SeqCst) && !price_updates_batch.is_empty() {
                    let repo_clone_final_exit = token_repo_ws_clone.clone();
                    let updates_final_exit = std::mem::take(&mut price_updates_batch);
                    let _ = tokio::spawn(async move {
                        if let Err(e) = repo_clone_final_exit
                            .batch_store_price_data(&updates_final_exit)
                            .await
                        {
                            error!("Failed to store final batch on processor exit: {}", e);
                        }
                    })
                    .await;
                }
                info!("WebSocket event processor task terminated");
            }
        });

        self.ws_task_handle = Some(tokio::spawn(async move {
            let (result1, result2) = tokio::join!(ws_handle, ws_processor_handle);
            if let Err(e) = result1 {
                error!("WebSocket connection task error: {}", e);
            }
            if let Err(e) = result2 {
                error!("WebSocket event processor task error: {}", e);
            }
            info!("All WebSocket tasks terminated");
        }));
        Ok(())
    }

    /// Check if the configured provider supports WebSocket
    fn has_websocket_support(&self) -> bool {
        self.market_api.as_ref().map_or(false, |api| {
            let supported = api.supports_websocket();
            if supported {
                info!("🔌 WebSocket support ENABLED for provider: {}", api.name());
            } else {
                info!(
                    "🔌 WebSocket support DISABLED for provider: {}. Fallback to polling.",
                    api.name()
                );
            }
            supported
        })
    }

    /// Stop the data collection process
    pub async fn stop(&mut self) -> Result<()> {
        info!("Stopping market data collector...");
        self.running.store(false, Ordering::SeqCst);

        if self.ws_connected.load(Ordering::SeqCst) {
            if let Some(api) = &self.market_api {
                if let Err(e) = api.disconnect_websocket().await {
                    error!("Error disconnecting WebSocket: {}", e);
                }
            }
            self.ws_connected.store(false, Ordering::SeqCst);
        }

        if let Some(handle) = self.ws_task_handle.take() {
            match handle.await {
                Ok(_) => info!("WebSocket tasks stopped successfully"),
                Err(e) => error!("Failed to stop WebSocket tasks: {}", e),
            }
        }

        if let Some(handle) = self.task_handle.take() {
            match handle.await {
                Ok(_) => {
                    info!("Market data collector stopped successfully");
                    Ok(())
                }
                Err(e) => {
                    error!("Failed to stop market data collector: {}", e);
                    Err(Error::Task(format!(
                        "Failed to stop market data collector: {}",
                        e
                    )))
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

    /// Check if WebSocket is connected
    pub fn is_websocket_connected(&self) -> bool {
        self.ws_connected.load(Ordering::SeqCst)
    }
}
