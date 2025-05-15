use super::{Actor, Command, Event, MarketEvent, Message, Query, QueryResult};
use crate::config::Config;
use crate::core::error::Error;
use crate::core::models::market::TokenMetrics;
use crate::core::models::token::TokenData;
use crate::infra::actors::MessageBus;
use crate::infra::api::market::{MarketDataEvent, MarketDataProvider};
use crate::infra::db::database::TokenMetadata;
use crate::infra::db::repositories::CachingRepository;
use crate::infra::db::repositories::TokenRepository;
use async_trait::async_trait;
use chrono::Utc;
use log::{debug, error, info, trace, warn};
use serde_json;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;
use tokio::time::interval;

pub struct MarketDataActor {
    market_api: Box<dyn MarketDataProvider + Send + Sync>,
    token_repo: Arc<TokenRepository>,
    message_bus: Arc<MessageBus>,
    scan_interval: u64,
    running: bool,
    using_websocket: bool,
    tokens_to_track: Vec<String>,
    wide_scan_mode: bool,
    ws_receiver: Option<mpsc::Receiver<MarketDataEvent>>,
    collection_task: Arc<RwLock<Option<tokio::task::JoinHandle<Result<(), Error>>>>>,
    config: Arc<Config>,
    last_activity: Arc<Mutex<Instant>>,
    last_scan_duration: Arc<Mutex<Option<Duration>>>,
    total_events_processed: Arc<AtomicU64>,
    total_api_calls: Arc<AtomicU64>,
}

impl MarketDataActor {
    pub fn new(
        config: Arc<Config>,
        market_api: Box<dyn MarketDataProvider + Send + Sync>,
        token_repo: Arc<TokenRepository>,
        message_bus: Arc<MessageBus>,
    ) -> Self {
        let scan_interval = config.data_collection.interval;

        let default_tokens = TokenData::default_tracked_tokens();
        let tokens_to_track: Vec<String> = default_tokens;

        let using_websocket = market_api.supports_websocket();
        let wide_scan_mode = config.trading.wide_scan_mode;

        debug!(
            "MarketDataActor init: WebSocket={}, Provider={}, ScanInterval={}s, WideScan={}",
            using_websocket,
            market_api.name(),
            scan_interval,
            wide_scan_mode
        );

        Self {
            config,
            market_api,
            token_repo,
            message_bus,
            running: false,
            tokens_to_track,
            scan_interval,
            using_websocket,
            wide_scan_mode,
            ws_receiver: None,
            last_activity: Arc::new(Mutex::new(Instant::now())),
            last_scan_duration: Arc::new(Mutex::new(None)),
            total_events_processed: Arc::new(AtomicU64::new(0)),
            total_api_calls: Arc::new(AtomicU64::new(0)),
            collection_task: Arc::new(RwLock::new(None)),
        }
    }

    pub fn with_message_bus(mut self, bus: Arc<MessageBus>) -> Self {
        self.message_bus = bus;
        self
    }

    fn should_track_token(&self, symbol: &str) -> bool {
        self.tokens_to_track.is_empty() || self.tokens_to_track.contains(&symbol.to_lowercase())
    }

    async fn handle_market_data(&mut self, data_result: Result<Vec<TokenMetrics>, Error>) {
        let start_time = Instant::now();
        self.total_api_calls.fetch_add(1, Ordering::Relaxed);

        match data_result {
            Ok(token_metrics_vec) => {
                trace!("Received {} TokenMetrics from API", token_metrics_vec.len());
                let metrics_count = token_metrics_vec.len();
                let mut processed_event_count = 0;
                for metrics in token_metrics_vec {
                    let lowercased_token_id = metrics.id.to_lowercase();
                    if !self.wide_scan_mode
                        && !self.tokens_to_track.is_empty()
                        && !self.tokens_to_track.contains(&lowercased_token_id)
                    {
                        trace!(
                            "Skipping untracked token (from TokenMetrics): {} ({})",
                            metrics.id,
                            lowercased_token_id
                        );
                        continue;
                    }

                    let event = MarketDataEvent::PriceUpdate {
                        token_id: metrics.id,
                        price: metrics.price_usd,
                        volume: Some(metrics.volume_24h),
                        change_24h: Some(metrics.price_change_24h),
                        timestamp: metrics.last_updated,
                    };

                    self.total_events_processed.fetch_add(1, Ordering::Relaxed);
                    if let Err(e) = self.handle_market_data_event(event).await {
                        error!(
                            "Error processing single market data event (from TokenMetrics): {}",
                            e
                        );
                    }
                    processed_event_count += 1;
                }
                debug!(
                    "Processed {} PriceUpdate events from {} TokenMetrics received.",
                    processed_event_count, metrics_count
                );
            }
            Err(e) => {
                error!(
                    "Error passed to handle_market_data (expecting Vec<TokenMetrics>): {}",
                    e
                );
                let error_event = Event::Market(MarketEvent::MarketDataError(e.to_string()));
                if let Err(pub_err) = self.message_bus.publish(error_event).await {
                    error!("Failed to publish market data error event: {}", pub_err);
                }
            }
        }
        *self.last_scan_duration.lock().unwrap() = Some(start_time.elapsed());
    }

    async fn handle_market_data_event(&mut self, event: MarketDataEvent) -> Result<(), Error> {
        trace!("Handling market data event: {:?}", event);
        match event {
            MarketDataEvent::PriceUpdate {
                token_id,
                price,
                volume,
                change_24h,
                timestamp,
            } => {
                let token_id_clone = token_id.clone();
                let vol = volume.unwrap_or(0.0);
                let name = token_id_clone.clone(); // Placeholder, actual name might differ
                let symbol = token_id_clone.clone(); // Placeholder, actual symbol might differ

                let metadata = TokenMetadata {
                    name,
                    symbol,
                    decimals: 0,
                    updated_at: timestamp,
                };

                let token_repo_clone = self.token_repo.clone();
                let bus_clone = self.message_bus.clone();

                let db_store_closure = {
                    let repo = token_repo_clone.clone();
                    let tid = token_id_clone.clone();
                    let price_db = price;
                    let vol_db = vol;
                    let change_24h_db = change_24h;
                    let timestamp_db = timestamp;
                    move || async move {
                        debug!(
                            "DB store closure: {} price={}, vol={}, change24h={:?}, ts={}",
                            tid, price_db, vol_db, change_24h_db, timestamp_db
                        );
                        Ok(())
                    }
                };

                let store_result = token_repo_clone
                    .store_in_cache_and_db(
                        &format!("token:meta:{}", token_id_clone),
                        &metadata,
                        db_store_closure,
                    )
                    .await;

                if let Err(e) = store_result {
                    error!(
                        "Failed to store token data (cache/db) for {}: {}",
                        token_id_clone, e
                    );
                }

                let event_to_publish = Event::Market(MarketEvent::PriceUpdate {
                    token_id: token_id_clone.clone(),
                    price,
                    volume: Some(vol),
                    timestamp,
                });
                if let Err(e) = bus_clone.publish(event_to_publish).await {
                    error!(
                        "Failed to publish price update event for {}: {}",
                        token_id_clone, e
                    );
                }
            }
            MarketDataEvent::VolumeUpdate { .. } => {
                // Handle VolumeUpdate if necessary, or ensure PriceUpdate is sufficient
                trace!("Ignoring MarketDataEvent::VolumeUpdate for now, assuming PriceUpdate covers volume.");
            }
            MarketDataEvent::Error(err) => {
                error!("Received market data error event: {}", err);
                let error_event = Event::Market(MarketEvent::MarketDataError(err.to_string()));
                if let Err(e) = self.message_bus.publish(error_event).await {
                    error!("Failed to publish market data error event: {}", e);
                }
            }
        }
        Ok(())
    }

    async fn start_market_data_loop(&mut self) -> Result<(), Error> {
        let provider_name = self.market_api.name();
        info!("MarketDataActor: Starting market data processing loop");
        info!("   - Provider: [{}]", provider_name);
        info!(
            "   - Tokens to track: {}",
            if self.tokens_to_track.is_empty() && self.wide_scan_mode {
                "ALL (wide scan)".to_string()
            } else {
                self.tokens_to_track.join(", ")
            }
        );

        let result = if self.using_websocket && self.market_api.supports_websocket() {
            info!(
                "Attempting to use real-time WebSocket data from [{}]",
                provider_name
            );
            self.start_websocket_loop().await
        } else {
            info!(
                "Using polling data from [{}] at {}s intervals",
                provider_name, self.scan_interval
            );
            self.start_polling_loop().await
        };

        info!("MarketDataActor: Market data loop has ended");
        result
    }

    async fn start_polling_loop(&mut self) -> Result<(), Error> {
        let provider_name = self.market_api.name().to_string();
        info!(
            "MarketDataActor: Starting POLLING loop with interval of {}s using [{}]",
            self.scan_interval, provider_name
        );
        let mut interval_timer = interval(Duration::from_secs(self.scan_interval));
        let mut poll_count = 0;

        while self.running {
            trace!("MarketDataActor: Waiting for next polling interval");
            interval_timer.tick().await;
            if !self.running {
                break;
            } // Check again after tick

            poll_count += 1;
            debug!("[POLLING] [{}]: Poll #{} - Fetching market data (tokens_to_track: {:?}, wide_scan: {})", 
                  provider_name, poll_count, self.tokens_to_track, self.wide_scan_mode);

            let data_result = self
                .market_api
                .get_market_data(self.wide_scan_mode, &self.tokens_to_track)
                .await;
            self.handle_market_data(data_result).await; // handle_market_data logs errors internally

            if poll_count % 10 == 0 {
                info!(
                    "[POLLING] [{}]: Completed {} polling cycles",
                    provider_name, poll_count
                );
            }
        }

        info!(
            "MarketDataActor: [{}] Polling loop ended after {} polls",
            provider_name, poll_count
        );
        Ok(())
    }

    async fn start_websocket_loop(&mut self) -> Result<(), Error> {
        let provider_name = self.market_api.name().to_string();
        info!(
            "MarketDataActor: Attempting to start WebSocket loop with [{}]",
            provider_name
        );

        let mut connection_attempts = 0;
        let max_connection_attempts = 5;
        let mut ws_sender_for_connection: Option<mpsc::Sender<MarketDataEvent>> = None;

        while self.running {
            connection_attempts += 1;
            if connection_attempts > max_connection_attempts {
                error!(
                    "Failed to connect to WebSocket after {} attempts. Falling back to polling.",
                    max_connection_attempts
                );
                self.using_websocket = false; // Ensure we don't try WS again until restart
                return self.start_polling_loop().await;
            }

            let (ws_sender, ws_receiver) = mpsc::channel::<MarketDataEvent>(256); // Increased buffer
            self.ws_receiver = Some(ws_receiver);
            ws_sender_for_connection = Some(ws_sender.clone()); // Keep sender for connection attempt

            match self
                .market_api
                .connect_websocket(
                    self.tokens_to_track.clone(),
                    ws_sender_for_connection.unwrap(), // Safe due to Some above
                )
                .await
            {
                Ok(_) => {
                    info!(
                        "Successfully connected to [{}] WebSocket on attempt {}",
                        provider_name, connection_attempts
                    );
                    connection_attempts = 0; // Reset for future reconnections within this session
                    break; // Proceed to event loop
                }
                Err(e) => {
                    warn!(
                        "WebSocket connection attempt {} to [{}] failed: {}",
                        connection_attempts, provider_name, e
                    );
                    self.ws_receiver = None; // Clear receiver as connection failed
                    if !self.running {
                        return Ok(());
                    }
                    let backoff_secs = 1 << (connection_attempts - 1); // 1, 2, 4, 8, 16
                    info!("Retrying WebSocket connection in {}s...", backoff_secs);
                    tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                    if !self.running {
                        return Ok(());
                    }
                }
            }
        }

        if !self.running || self.ws_receiver.is_none() {
            info!("WebSocket loop not starting as actor is not running or connection failed.");
            return Ok(());
        }

        info!(
            "MarketDataActor: WebSocket event processing loop started for [{}]",
            provider_name
        );
        let mut supplemental_poll_timer =
            interval(Duration::from_secs(std::cmp::max(300, self.scan_interval))); // e.g., 5 min poll
        let mut ws_events_count = 0;
        let mut last_ws_activity = Instant::now();
        let ws_inactivity_threshold =
            Duration::from_secs(std::cmp::max(600, self.scan_interval * 2)); // e.g., 10 min

        while self.running {
            if let Some(current_ws_receiver) = &mut self.ws_receiver {
                tokio::select! {
                    biased; // Prioritize WS events

                    maybe_event = current_ws_receiver.recv() => {
                        if !self.running { break; }
                        match maybe_event {
                            Some(event) => {
                                last_ws_activity = Instant::now();
                                ws_events_count += 1;
                                if let Err(e) = self.handle_market_data_event(event).await {
                                    error!("Error handling WebSocket market data event: {}", e);
                                }
                                if ws_events_count % 100 == 0 {
                                    debug!("[WEBSOCKET] [{}] Processed {} real-time events.", provider_name, ws_events_count);
                                }
                            }
                            None => { // WebSocket channel closed by the sender (provider)
                                warn!("[WEBSOCKET] [{}] channel closed by provider. Attempting to reconnect.", provider_name);
                                self.ws_receiver = None; // Indicate receiver is gone
                                break; // Break select to re-enter outer while for reconnection
                            }
                        }
                    }

                    _ = supplemental_poll_timer.tick() => {
                        if !self.running { break; }
                        info!("[WEBSOCKET] [{}] Performing supplemental poll.", provider_name);
                        let data_result = self.market_api.get_market_data(
                            self.wide_scan_mode,
                            &self.tokens_to_track
                        ).await;
                        self.handle_market_data(data_result).await;
                        // No need to reset last_ws_activity here, poll is supplemental
                    }

                    // Watchdog for complete WS silence
                    _ = tokio::time::sleep(Duration::from_secs(60)) => { // Check every minute
                        if !self.running { break; }
                        if last_ws_activity.elapsed() > ws_inactivity_threshold {
                            warn!("[WEBSOCKET] [{}] No activity for over {:?}. Assuming stale connection. Attempting reconnect.",
                                provider_name, ws_inactivity_threshold);
                            self.ws_receiver = None; // Indicate receiver is gone
                            break; // Break select to re-enter outer while for reconnection
                        }
                    }
                }
            } else {
                // ws_receiver is None, means we need to reconnect
                info!(
                    "[WEBSOCKET] [{}] Receiver is None, attempting to reconnect...",
                    provider_name
                );
                // This will loop back to the connection attempt logic at the start of the outer while loop
                // Small delay before retrying connection logic
                if !self.running {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
                if !self.running {
                    break;
                }
                break; // Break from select and re-enter main WS loop for reconnection attempt
            }
        }

        if self.running {
            // If loop exited because of WS issue, not actor stopping
            warn!("[WEBSOCKET] [{}] loop exited unexpectedly. Will attempt to restart if actor still running.", provider_name);
        }

        info!(
            "[WEBSOCKET] [{}] processing loop ended. Processed {} events.",
            provider_name, ws_events_count
        );
        // Attempt to gracefully disconnect if we initiated the connection
        if self.market_api.supports_websocket() {
            // Check if disconnect is even relevant
            info!(
                "[WEBSOCKET] Attempting to disconnect from [{}]",
                provider_name
            );
            if let Err(e) = (self.market_api.disconnect_websocket()).await {
                warn!(
                    "Error disconnecting WebSocket from [{}]: {}",
                    provider_name, e
                );
            } else {
                info!(
                    "[WEBSOCKET] Successfully disconnected from [{}]",
                    provider_name
                );
            }
        }
        self.ws_receiver = None; // Ensure it's cleared
        Ok(())
    }
}

#[async_trait::async_trait]
impl Actor for MarketDataActor {
    fn start(&mut self) -> impl std::future::Future<Output = Result<(), Error>> + Send {
        async move {
            info!("MarketDataActor starting...");
            self.running = true;
            Ok(())
        }
    }

    fn stop(&mut self) -> Result<(), Error> {
        info!("MarketDataActor stopping...");
        self.running = false;
        Ok(())
    }

    fn handle_message(
        &mut self,
        msg: Message,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send {
        async move {
            *self.last_activity.lock().unwrap() = Instant::now();
            let current_status = get_actor_status(self);

            match msg {
                Message::Event(event) => match event {
                    Event::Market(market_event) => {
                        let md_event_result = match market_event {
                            MarketEvent::PriceUpdate {
                                token_id,
                                price,
                                volume,
                                timestamp,
                            } => Ok(MarketDataEvent::PriceUpdate {
                                token_id,
                                price,
                                volume,
                                change_24h: None,
                                timestamp,
                            }),
                            MarketEvent::VolumeUpdate {
                                token_id,
                                volume,
                                timestamp,
                            } => Ok(MarketDataEvent::VolumeUpdate {
                                token_id,
                                volume,
                                timestamp,
                            }),
                            MarketEvent::MarketDataError(err_msg) => Err(Error::Api(err_msg)),
                            _ => Err(Error::NotImplemented(
                                "Unhandled MarketEvent variant".to_string(),
                            )),
                        };

                        match md_event_result {
                            Ok(md_event) => {
                                self.total_events_processed.fetch_add(1, Ordering::Relaxed);
                                if let Err(e) = self.handle_market_data_event(md_event).await {
                                    error!("Error handling market data event: {}", e);
                                }
                            }
                            Err(e) => error!("Failed to process MarketEvent: {}", e),
                        }
                    }
                    _ => trace!("MarketDataActor ignoring non-market event: {:?}", event),
                },
                Message::Query(query, responder) => match query {
                    Query::GetStatus => {
                        let _ = responder.send(Ok(QueryResult::Status(current_status)));
                    }
                    Query::GetMetrics => {
                        let metrics = get_actor_metrics(self).await;
                        let _ = responder.send(Ok(QueryResult::Metrics(metrics)));
                    }
                    Query::GetMaintenanceStatus => {
                        let _ = responder.send(Ok(QueryResult::Status(
                            "Market actor does not perform DB maintenance".to_string(),
                        )));
                    }
                    _ => warn!("Received unhandled Query: {:?}", query),
                },
                Message::Command(cmd) => match cmd {
                    Command::Start => {
                        info!("MarketDataActor received Start command");
                        if self.collection_task.read().await.is_some() {
                            warn!("MarketDataActor Start command received, but collection task is already running.");
                            return Ok(());
                        }
                        self.start().await?; // Sets self.running = true

                        let mut actor_clone_for_task = self.clone();
                        let collection_handle = tokio::spawn(async move {
                            info!(
                                "Market data collection task started for provider: {}",
                                actor_clone_for_task.market_api.name()
                            );
                            let mut backoff_secs = 1;
                            let max_backoff_secs = 60;

                            while actor_clone_for_task.running {
                                if let Err(e) = actor_clone_for_task.start_market_data_loop().await
                                {
                                    error!(
                                        "Market data loop error: {}. Will retry in {}s",
                                        e, backoff_secs
                                    );
                                    if !actor_clone_for_task.running {
                                        info!("Actor stopped during error handling, exiting collection task.");
                                        break;
                                    }
                                    tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                                    backoff_secs =
                                        std::cmp::min(backoff_secs * 2, max_backoff_secs);
                                } else {
                                    // If loop exited cleanly but actor still supposed to be running (e.g. WS disconnect then reconnect failed)
                                    if actor_clone_for_task.running {
                                        warn!("Market data loop exited. Actor still running, attempting restart of loop after short delay.");
                                        backoff_secs = 1; // Reset backoff
                                        tokio::time::sleep(Duration::from_secs(5)).await;
                                    // Brief pause before restarting loop
                                    } else {
                                        info!("Market data loop exited because actor is no longer running.");
                                        break;
                                    }
                                }
                                if !actor_clone_for_task.running {
                                    // Final check
                                    info!("Actor no longer running, exiting collection task.");
                                    break;
                                }
                            }
                            info!(
                                "Market data collection task finished for provider: {}",
                                actor_clone_for_task.market_api.name()
                            );
                            Ok(())
                        });
                        *self.collection_task.write().await = Some(collection_handle);
                        info!("MarketDataActor data collection task scheduled.");
                    }
                    Command::Stop => {
                        info!("MarketDataActor received Stop command");
                        self.stop()?; // Sets self.running = false

                        if let Some(handle) = self.collection_task.write().await.take() {
                            info!("Aborting market data collection task...");
                            handle.abort();
                            match handle.await {
                                Ok(_) => info!("Market data collection task aborted successfully."),
                                Err(e) if e.is_cancelled() => info!(
                                    "Market data collection task abortion confirmed (cancelled)."
                                ),
                                Err(e) => error!("Error awaiting aborted task: {}", e),
                            }
                        } else {
                            info!("No active market data collection task to stop.");
                        }

                        // Disconnect WebSocket if it was used by this instance
                        if self.market_api.supports_websocket() {
                            // Check if provider supports it
                            // The disconnect is now handled at the end of start_websocket_loop
                            // to ensure it's called by the task that owns the connection.
                            // However, if the task was aborted abruptly, we might want a fallback.
                            // For now, rely on the task's own cleanup.
                            info!("WebSocket disconnect is handled by the collection task upon its termination.");
                        }
                    }
                    _ => warn!("Received unhandled Command: {:?}", cmd),
                },
            }
            Ok(())
        }
    }
}

fn get_actor_status(actor: &MarketDataActor) -> String {
    format!(
        "MarketDataActor running: {}, Using WebSocket: {}, Tokens Tracked: {}",
        actor.running,
        actor.using_websocket,
        actor.tokens_to_track.len()
    )
}

async fn get_actor_metrics(actor: &MarketDataActor) -> serde_json::Value {
    let last_activity_elapsed = actor.last_activity.lock().unwrap().elapsed().as_secs();
    let collection_task_active = actor.collection_task.read().await.is_some();

    serde_json::json!({
        "running": actor.running,
        "using_websocket": actor.using_websocket,
        "provider": actor.market_api.name(),
        "scan_interval_seconds": actor.scan_interval,
        "tokens_tracked_count": actor.tokens_to_track.len(),
        "last_scan_duration_ms": actor.last_scan_duration.lock().unwrap().map_or(0, |d| d.as_millis()),
        "last_activity_seconds_ago": last_activity_elapsed,
        "total_events_processed": actor.total_events_processed.load(Ordering::Relaxed),
        "total_api_calls": actor.total_api_calls.load(Ordering::Relaxed),
        "collection_task_active": collection_task_active,
    })
}

impl Clone for MarketDataActor {
    fn clone(&self) -> Self {
        // ws_receiver is instance-specific and managed by the task, so new clone gets None.
        // collection_task is specific to the main actor instance, clone doesn't get it.
        Self {
            market_api: self.market_api.clone_box(),
            token_repo: self.token_repo.clone(),
            message_bus: self.message_bus.clone(),
            scan_interval: self.scan_interval,
            running: self.running, // Cloned state, task will operate on its cloned 'running'
            using_websocket: self.using_websocket,
            tokens_to_track: self.tokens_to_track.clone(),
            wide_scan_mode: self.wide_scan_mode,
            ws_receiver: None,
            collection_task: Arc::new(RwLock::new(None)), // Each clone, if it were to spawn, would manage its own.
            config: self.config.clone(),
            last_activity: self.last_activity.clone(), // Share activity tracking for main actor
            last_scan_duration: self.last_scan_duration.clone(),
            total_events_processed: self.total_events_processed.clone(),
            total_api_calls: self.total_api_calls.clone(),
        }
    }
}
