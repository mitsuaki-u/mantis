pub mod tasks;

use crate::application::actors::system::actor::{ActorState, LifecycleActor};
use crate::application::actors::system::{Actor, Command, LifecycleState};
use crate::application::errors::Result;
use crate::config::Config;
use crate::events::{Event, EventType};
use crate::infrastructure::market::providers::MarketDataProvider;
use crate::EventRouter;
use async_trait::async_trait;
use log::{debug, info, warn};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

// Type alias for the collection task handle
pub type MarketDataCollectionTaskHandle = Arc<RwLock<Option<tokio::task::JoinHandle<Result<()>>>>>;

pub struct MarketDataActor {
    pub state: ActorState,

    // Core functionality
    pub market_api: Box<dyn MarketDataProvider + Send + Sync>,
    pub event_router: Arc<EventRouter>,

    // Configuration
    pub config: Arc<Config>,

    // Market data settings
    pub scan_interval: u64,
    pub tokens_to_track: Vec<String>,
    pub max_tokens_to_scan: usize,

    // Runtime state
    pub collection_task: MarketDataCollectionTaskHandle,

    // Metrics and monitoring
    pub last_activity: Arc<Mutex<Instant>>,
    pub last_scan_duration: Arc<Mutex<Option<Duration>>>,
}

impl MarketDataActor {
    pub fn new(
        config: Arc<Config>,
        market_api: Box<dyn MarketDataProvider + Send + Sync>,
        event_router: Arc<EventRouter>,
    ) -> Self {
        let scan_interval = config.data_collection.scan_interval_secs;
        let max_tokens_to_scan = config.trading.max_tokens_to_scan;

        // Normalize configured tokens to lowercase for consistent comparison
        let tokens_to_track: Vec<String> = config
            .trading
            .tokens_to_track
            .iter()
            .map(|token| token.to_lowercase())
            .collect();

        if !tokens_to_track.is_empty() {
            debug!(
                "Tracking {} explicitly configured tokens: {:?}",
                tokens_to_track.len(),
                tokens_to_track
            );
        } else {
            debug!(
                "Auto-discovery mode: will scan up to {} tokens{}",
                max_tokens_to_scan,
                if max_tokens_to_scan == 0 {
                    " (unlimited)"
                } else {
                    ""
                }
            );
        }

        debug!(
            "MarketDataActor init: Provider={}, ScanInterval={}s, MaxTokens={}",
            market_api.name(),
            scan_interval,
            max_tokens_to_scan
        );

        Self {
            state: ActorState::new("MarketDataActor".to_string()),
            config,
            market_api,
            event_router,
            tokens_to_track,
            scan_interval,
            max_tokens_to_scan,
            last_activity: Arc::new(Mutex::new(Instant::now())),
            last_scan_duration: Arc::new(Mutex::new(None)),
            collection_task: Arc::new(RwLock::new(None)),
        }
    }
}

#[async_trait::async_trait]
impl Actor for MarketDataActor {
    fn name(&self) -> &str {
        &self.state.name
    }

    fn is_running(&self) -> bool {
        self.state.running
    }

    // Override start/stop with custom logic
    async fn start(&mut self) -> Result<()> {
        debug!("Starting MarketDataActor with custom startup logic");
        self.state.start();

        // Start the market data collection task
        self.start_market_data_collection_task().await?;

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Stopping MarketDataActor with custom shutdown logic");
        self.state.stop();

        // Stop the market data collection task
        self.stop_market_data_collection_task().await?;

        Ok(())
    }

    // Override command handling for Start/Stop with custom logic
    async fn handle_command(&mut self, cmd: Command) -> Result<()> {
        match cmd {
            Command::Start => {
                debug!("MarketDataActor received Start command");
                self.start().await
            }
            Command::Stop => {
                info!("MarketDataActor received Stop command");
                self.stop().await
            }
            _ => {
                debug!(
                    "Actor {} received command (default: no action)",
                    self.name()
                );
                Ok(())
            }
        }
    }

    async fn handle_event(&mut self, event: Event) -> Result<()> {
        self.state.record_activity();
        // MarketDataActor is poll-driven, not event-driven - ignore all events
        let _ = event;
        Ok(())
    }

    fn supported_event_types(&self) -> Vec<EventType> {
        // Poll-driven actor - doesn't subscribe to any events
        vec![]
    }
}

impl Clone for MarketDataActor {
    fn clone(&self) -> Self {
        // collection_task is specific to the main actor instance, clone doesn't get it.
        Self {
            state: self.state.clone(), // Preserve the current state including running status
            market_api: self.market_api.clone_box(),
            event_router: self.event_router.clone(),
            config: self.config.clone(),
            scan_interval: self.scan_interval,
            tokens_to_track: self.tokens_to_track.clone(),
            max_tokens_to_scan: self.max_tokens_to_scan,
            collection_task: Arc::new(RwLock::new(None)), // Each clone, if it were to spawn, would manage its own.
            last_activity: self.last_activity.clone(),    // Share activity tracking for main actor
            last_scan_duration: self.last_scan_duration.clone(),
        }
    }
}

#[async_trait]
impl LifecycleActor for MarketDataActor {
    async fn initialize(&mut self) -> Result<()> {
        info!("Initializing MarketDataActor");
        self.state.lifecycle_state = LifecycleState::Initialized;

        // Perform any initialization tasks
        debug!(
            "MarketDataActor initialized with provider: {}",
            self.market_api.name()
        );
        Ok(())
    }

    async fn cleanup(&mut self) -> Result<()> {
        info!("Cleaning up MarketDataActor");

        // Task already stopped in stop() method - this is defensive cleanup
        if let Some(handle) = self.collection_task.write().await.take() {
            warn!("Collection task still present during cleanup - aborting");
            handle.abort();
        }

        Ok(())
    }

    fn lifecycle_state(&self) -> LifecycleState {
        self.state.lifecycle_state.clone()
    }
}
