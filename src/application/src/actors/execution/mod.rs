pub mod events;
pub mod orders;
pub mod positions;
pub mod tasks;

pub use events::handle_event_internal;
pub use orders::handle_risk_assessment;
pub use positions::Position;
pub use tasks::{start_transaction_status_polling, stop_transaction_status_polling};

use crate::application::actors::system::actor::{ActorState, LifecycleActor};
use crate::application::actors::system::{Actor, LifecycleState};
use crate::application::errors::Error;
use crate::config::Config;
use crate::events::{Event, EventType};

use crate::infrastructure::database::repositories::{PositionRepository, TokenRepository};
use crate::infrastructure::dex::{DexClient, TransactionStatus};
use crate::EventRouter;
use async_trait::async_trait;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

/// Main execution actor for handling trading orders and position management
pub struct ExecutionActor {
    state: ActorState,

    // Core functionality
    pub token_repo: Arc<TokenRepository>,
    pub position_repo: Arc<PositionRepository>,
    pub dex_client: Arc<DexClient>,
    pub event_router: Arc<EventRouter>,
    pub config: Arc<Config>,

    pub periodic_task_running: Arc<AtomicBool>,
    pub position_processing_map: Arc<tokio::sync::Mutex<HashMap<String, bool>>>,
    pub active_transactions: Arc<tokio::sync::Mutex<HashMap<String, TransactionStatus>>>,
    pub transaction_polling_task_running: Arc<AtomicBool>,

    // Shutdown coordination
    pub shutdown_tx: Option<broadcast::Sender<()>>,
    pub event_loop_handle: Arc<tokio::sync::Mutex<Option<JoinHandle<()>>>>,
}

impl Clone for ExecutionActor {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            token_repo: self.token_repo.clone(),
            position_repo: self.position_repo.clone(),
            dex_client: self.dex_client.clone(),
            event_router: self.event_router.clone(),
            config: self.config.clone(),
            periodic_task_running: self.periodic_task_running.clone(),
            position_processing_map: self.position_processing_map.clone(),
            active_transactions: self.active_transactions.clone(),
            transaction_polling_task_running: self.transaction_polling_task_running.clone(),
            shutdown_tx: self.shutdown_tx.clone(),
            event_loop_handle: self.event_loop_handle.clone(),
        }
    }
}

impl ExecutionActor {
    /// Create a new execution actor
    pub fn new(
        token_repo: Arc<TokenRepository>,
        position_repo: Arc<PositionRepository>,
        dex_client: Arc<DexClient>,
        event_router: Arc<EventRouter>,
        config: Arc<Config>,
    ) -> Self {
        Self {
            state: ActorState::new("ExecutionActor".to_string()),
            token_repo,
            position_repo,
            dex_client,
            event_router,
            config,
            periodic_task_running: Arc::new(AtomicBool::new(false)),
            position_processing_map: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            active_transactions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            transaction_polling_task_running: Arc::new(AtomicBool::new(false)),
            shutdown_tx: None,
            event_loop_handle: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Gracefully shutdown the event loop
    async fn shutdown_event_loop(&mut self) {
        debug!("ExecutionActor: Initiating event loop shutdown");

        // Send shutdown signal
        if let Some(shutdown_tx) = &self.shutdown_tx {
            if let Err(e) = shutdown_tx.send(()) {
                warn!(
                    "Failed to send shutdown signal: {}. Event loop may not shut down gracefully.",
                    e
                );
            }
        }

        // Wait for event loop to complete with timeout
        if let Some(handle) = self
            .event_loop_handle
            .try_lock()
            .map_err(|_| warn!("Failed to acquire event loop handle lock during shutdown"))
            .ok()
            .and_then(|mut guard| guard.take())
        {
            match tokio::time::timeout(
                std::time::Duration::from_secs(
                    crate::application::constants::EXECUTION_TASK_TIMEOUT_SECS,
                ),
                handle,
            )
            .await
            {
                Ok(Ok(())) => {
                    debug!("ExecutionActor: Event loop shutdown gracefully");
                }
                Ok(Err(e)) => {
                    warn!("ExecutionActor: Event loop panicked during shutdown: {}", e);
                }
                Err(_) => {
                    warn!("ExecutionActor: Event loop shutdown timed out");
                }
            }
        }
    }

    /// Handle events by delegating to events module with full context
    async fn handle_event_internal_wrapper(&mut self, event: Event) -> Result<(), Error> {
        let ctx = events::ExecutionContext {
            active_transactions: &self.active_transactions,
            token_repo: &self.token_repo,
            position_repo: &self.position_repo,
            dex_client: &self.dex_client,
            event_router: &self.event_router,
            config: &self.config,
            running: self.state.running,
        };

        events::handle_event_internal(event, ctx).await
    }
}

#[async_trait]
impl Actor for ExecutionActor {
    fn name(&self) -> &str {
        &self.state.name
    }

    fn is_running(&self) -> bool {
        self.state.running
    }

    // Override start/stop with custom logic (adapted from old BaseActor logic)
    async fn start(&mut self) -> Result<(), Error> {
        debug!("Starting ExecutionActor with custom startup logic");
        self.state.start();

        // Position updates now handled via MarketEvent::PriceUpdate
        // No periodic task needed

        Ok(())
    }

    async fn stop(&mut self) -> Result<(), Error> {
        info!("Stopping ExecutionActor with custom shutdown logic");
        self.state.stop();

        // Stop periodic tasks
        self.periodic_task_running
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.transaction_polling_task_running
            .store(false, std::sync::atomic::Ordering::Relaxed);

        // Shutdown event loop
        self.shutdown_event_loop().await;

        info!("🛑 ExecutionActor stopped");
        Ok(())
    }

    // Override event handling for Risk events
    async fn handle_event(&mut self, event: Event) -> Result<(), Error> {
        self.state.record_activity();
        self.handle_event_internal_wrapper(event).await
    }

    fn supported_event_types(&self) -> Vec<EventType> {
        vec![EventType::Risk, EventType::Execution]
    }
}

#[async_trait]
impl LifecycleActor for ExecutionActor {
    async fn initialize(&mut self) -> Result<(), Error> {
        info!("Initializing ExecutionActor");
        self.state.lifecycle_state = LifecycleState::Initialized;

        // Note: Balance and wallet info is already logged during DEX client creation
        debug!("ExecutionActor initialized with DEX client");
        Ok(())
    }

    async fn cleanup(&mut self) -> Result<(), Error> {
        info!("Cleaning up ExecutionActor");

        // Stop transaction polling task if running
        if self
            .transaction_polling_task_running
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            if let Err(e) = tasks::stop_transaction_status_polling(
                self.transaction_polling_task_running.clone(),
            )
            .await
            {
                error!("Failed to stop transaction polling during cleanup: {}", e);
            }
        }

        // Shutdown event loop
        self.shutdown_event_loop().await;

        debug!("ExecutionActor cleanup completed");
        Ok(())
    }

    fn lifecycle_state(&self) -> LifecycleState {
        self.state.lifecycle_state.clone()
    }
}
