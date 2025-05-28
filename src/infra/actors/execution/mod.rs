pub mod client;
pub mod events;
pub mod orders;
pub mod positions;
pub mod tasks;
pub mod types;

pub use client::{create_dex_client, fetch_and_log_initial_balance};
pub use events::handle_event_internal;
pub use orders::handle_risk_assessment;
pub use positions::{check_positions, sync_positions_with_database};
pub use tasks::{
    start_periodic_check, start_transaction_status_polling, stop_periodic_check,
    stop_transaction_status_polling,
};
pub use types::Position;

use super::{Actor, Command, Event, Message, Query, QueryResult};
use crate::core::config::Config;
use crate::core::error::Error;
use crate::domain::dex::{DexClient, TransactionStatus};
use crate::infra::actors::MessageBus;
use crate::infra::db::repositories::{PositionRepository, TokenRepository};
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::sync::Arc as StdArc;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

/// Main execution actor for handling trading orders and position management
#[derive(Clone)]
pub struct ExecutionActor {
    pub token_repo: Arc<TokenRepository>,
    pub position_repo: Arc<PositionRepository>,
    pub dex_client: DexClient,
    pub message_bus: Arc<MessageBus>,
    pub config: Arc<Config>,
    pub running: bool,
    pub positions: Vec<Position>,
    pub periodic_task_running: StdArc<AtomicBool>,
    pub position_processing_map: StdArc<tokio::sync::Mutex<HashMap<String, bool>>>,
    pub active_transactions: Arc<tokio::sync::Mutex<HashMap<String, TransactionStatus>>>,
    pub transaction_polling_task_running: StdArc<AtomicBool>,
    // Shutdown coordination
    pub shutdown_tx: Option<broadcast::Sender<()>>,
    pub event_loop_handle: Arc<tokio::sync::Mutex<Option<JoinHandle<()>>>>,
}

impl ExecutionActor {
    /// Create a new execution actor
    pub fn new(
        token_repo: Arc<TokenRepository>,
        position_repo: Arc<PositionRepository>,
        dex_client: DexClient,
        message_bus: Arc<MessageBus>,
        config: Arc<Config>,
    ) -> Self {
        Self {
            token_repo,
            position_repo,
            dex_client,
            message_bus,
            config,
            running: false,
            positions: Vec::new(),
            periodic_task_running: StdArc::new(AtomicBool::new(false)),
            position_processing_map: StdArc::new(tokio::sync::Mutex::new(HashMap::new())),
            active_transactions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            transaction_polling_task_running: StdArc::new(AtomicBool::new(false)),
            shutdown_tx: None,
            event_loop_handle: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Handle risk assessment using the modular orders module
    async fn handle_risk_assessment_internal(
        &mut self,
        token_id: String,
        signal: crate::domain::trading::strategy::Signal,
        confidence: f64,
        position_size: f64,
    ) -> Result<(), Error> {
        orders::handle_risk_assessment(
            &self.token_repo,
            &self.position_repo,
            &self.dex_client,
            &self.message_bus,
            &self.config,
            &mut self.positions,
            self.running,
            token_id,
            signal,
            confidence,
            position_size,
        )
        .await
    }

    /// Check positions using the modular positions module
    async fn check_positions_internal(&mut self) -> Result<(), Error> {
        positions::check_positions(
            &self.token_repo,
            &self.position_repo,
            &self.message_bus,
            &mut self.positions,
            &self.position_processing_map,
            self.running,
        )
        .await
    }

    /// Start periodic position checking
    async fn start_periodic_check_internal(&self) -> Result<(), Error> {
        let token_repo = self.token_repo.clone();
        let position_repo = self.position_repo.clone();
        let message_bus = self.message_bus.clone();
        let position_processing_map = self.position_processing_map.clone();
        let running = self.running;

        // Create a closure that captures the necessary data
        let check_fn = move || {
            let token_repo = token_repo.clone();
            let position_repo = position_repo.clone();
            let message_bus = message_bus.clone();
            let position_processing_map = position_processing_map.clone();

            async move {
                // Create a temporary positions vector for the check
                // Note: In a real implementation, we'd need to properly handle the shared state
                let mut temp_positions = Vec::new();

                // Sync positions from database first
                if let Err(e) = positions::sync_positions_with_database(
                    &position_repo,
                    &mut temp_positions,
                    running,
                )
                .await
                {
                    error!("Failed to sync positions in periodic check: {}", e);
                    return Err(e);
                }

                // Check positions using the modular function
                positions::check_positions(
                    &token_repo,
                    &position_repo,
                    &message_bus,
                    &mut temp_positions,
                    &position_processing_map,
                    running,
                )
                .await
            }
        };

        tasks::start_periodic_check(self.periodic_task_running.clone(), check_fn).await
    }

    /// Handle events using the modular events module
    async fn handle_event_internal_wrapper(&mut self, event: Event) -> Result<(), Error> {
        // Handle the event directly instead of using a closure to avoid borrowing issues
        match event {
            Event::Risk(risk_event) => match risk_event {
                super::RiskEvent::RiskAssessment {
                    token_id,
                    signal,
                    confidence,
                    position_size,
                    timestamp: _,
                } => {
                    self.handle_risk_assessment_internal(
                        token_id,
                        signal,
                        confidence,
                        position_size,
                    )
                    .await?
                }
                super::RiskEvent::PositionClosed { token_id, .. } => {
                    self.active_transactions.lock().await.remove(&token_id);
                    info!("Position closed for {}. If related to a DEX tx, ensure active_transactions is cleared appropriately.", token_id);
                }
                _ => {
                    // Handle other risk events using the events module
                    events::handle_event_internal(
                        Event::Risk(risk_event),
                        &self.active_transactions,
                        |_, _, _, _| async { Ok(()) },
                    )
                    .await?;
                }
            },
            _ => {
                // Handle non-risk events using the events module
                events::handle_event_internal(
                    event,
                    &self.active_transactions,
                    |_, _, _, _| async { Ok(()) },
                )
                .await?;
            }
        }
        Ok(())
    }

    /// Gracefully shutdown the event loop
    async fn shutdown_event_loop(&mut self) {
        debug!("ExecutionActor: Initiating event loop shutdown");

        // Send shutdown signal
        if let Some(shutdown_tx) = &self.shutdown_tx {
            let _ = shutdown_tx.send(());
        }

        // Wait for event loop to complete with timeout
        if let Some(handle) = self.event_loop_handle.lock().await.take() {
            match tokio::time::timeout(std::time::Duration::from_secs(5), handle).await {
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
}

impl Actor for ExecutionActor {
    fn start(&mut self) -> impl std::future::Future<Output = Result<(), Error>> + Send {
        async move {
            if self.running {
                return Ok(());
            }
            self.running = true;
            info!("🚀 ExecutionActor started");

            // Create shutdown coordination
            let (shutdown_tx, _) = broadcast::channel(1);
            self.shutdown_tx = Some(shutdown_tx.clone());

            // Fetch initial balance using the client module
            client::fetch_and_log_initial_balance(&self.dex_client).await;

            // Start periodic checks for positions
            if let Err(e) = self.start_periodic_check_internal().await {
                error!("Failed to start periodic position check: {}", e);
            }

            // Subscribe to relevant events
            let (event_tx, mut event_rx) = mpsc::channel::<Event>(100);

            // Subscribe to RiskEvent for new orders/signals to execute
            if let Err(e) = self
                .message_bus
                .subscribe(format!("{:?}", super::EventType::Risk), event_tx.clone())
                .await
            {
                error!("ExecutionActor failed to subscribe to Risk events: {}. Actor may not function correctly.", e);
            }

            // Subscribe to DexTransactionEvent for submitted transactions
            if let Err(e) = self
                .message_bus
                .subscribe(
                    format!("{:?}", super::EventType::DexTransaction),
                    event_tx.clone(),
                )
                .await
            {
                error!("ExecutionActor failed to subscribe to DexTransaction events: {}. Status tracking might be affected.", e);
            }

            // Start transaction status polling
            if let Err(e) = tasks::start_transaction_status_polling(
                self.transaction_polling_task_running.clone(),
                self.active_transactions.clone(),
                self.dex_client.clone(),
                self.message_bus.clone(),
            )
            .await
            {
                error!(
                    "ExecutionActor: Failed to start transaction status polling: {}",
                    e
                );
            }

            // Start event handling loop with shutdown coordination
            let mut self_clone = self.clone();
            let mut shutdown_rx = shutdown_tx.subscribe();
            let event_loop_handle = tokio::spawn(async move {
                info!("ExecutionActor event handling loop started.");
                loop {
                    // Check global shutdown flag first
                    if crate::domain::trading::execution::bot::is_forced_shutdown() {
                        info!("ExecutionActor: Global shutdown detected, exiting event loop");
                        break;
                    }

                    tokio::select! {
                        // Handle incoming events
                        Some(event) = event_rx.recv() => {
                            if let Err(e) = self_clone.handle_event_internal_wrapper(event).await {
                                error!("Error handling event in ExecutionActor: {}", e);
                            }
                        }
                        // Handle shutdown signal
                        _ = shutdown_rx.recv() => {
                            debug!("ExecutionActor: Shutdown signal received in event loop");
                            break;
                        }
                        // Handle channel closure
                        else => {
                            debug!("ExecutionActor: Event channel closed");
                            break;
                        }
                    }
                }
                info!("ExecutionActor event handling loop ended.");
            });

            self.event_loop_handle
                .lock()
                .await
                .replace(event_loop_handle);
            Ok(())
        }
    }

    fn stop(&mut self) -> Result<(), Error> {
        info!("🛑 ExecutionActor stopping...");
        self.running = false;

        // Send shutdown signal to event loop
        if let Some(shutdown_tx) = &self.shutdown_tx {
            let _ = shutdown_tx.send(());
        }

        // Stop periodic checks
        if let Err(e) = futures::executor::block_on(tasks::stop_periodic_check(
            self.periodic_task_running.clone(),
        )) {
            error!(
                "ExecutionActor: Error stopping periodic position check: {}",
                e
            );
        }

        // Stop transaction polling
        if let Err(e) = futures::executor::block_on(tasks::stop_transaction_status_polling(
            self.transaction_polling_task_running.clone(),
        )) {
            error!(
                "ExecutionActor: Error stopping transaction status polling: {}",
                e
            );
        }

        // Shutdown event loop gracefully
        if let Err(e) = futures::executor::block_on(async {
            let mut self_mut = self.clone();
            self_mut.shutdown_event_loop().await;
            Ok::<(), Error>(())
        }) {
            error!("ExecutionActor: Error during event loop shutdown: {}", e);
        }

        info!("🛑 ExecutionActor stopped");
        Ok(())
    }

    fn handle_message(
        &mut self,
        msg: Message,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send {
        async move {
            match msg {
                Message::Command(command) => {
                    info!("ExecutionActor received command: {:?}", command);
                    match command {
                        Command::Start => self.start().await?,
                        Command::Stop => self.stop()?,
                        Command::UpdateConfig(_new_config_value) => {
                            info!("ExecutionActor received UpdateConfig command (not fully implemented)");
                        }
                        _ => {
                            error!("ExecutionActor received unhandled Command: {:?}", command)
                        }
                    }
                }
                Message::Query(query, responder) => {
                    info!("ExecutionActor received query: {:?}", query);
                    let result = match query {
                        Query::GetStatus => {
                            Ok(QueryResult::Status("ExecutionActor: Running".to_string()))
                        }
                        Query::GetNativeBalance => {
                            match self.dex_client.get_native_balance().await {
                                Ok(balance) => Ok(QueryResult::NativeBalance(balance)),
                                Err(e) => Err(e),
                            }
                        }
                        _ => Err(Error::NotImplemented(format!(
                            "Query {:?} not implemented for ExecutionActor",
                            query
                        ))),
                    };

                    if let Err(e) = responder.send(result) {
                        error!("Failed to send query response: {:?}", e);
                    }
                }
                Message::Event(event) => {
                    if let Err(e) = self.handle_event_internal_wrapper(event).await {
                        error!("Error handling event message: {}", e);
                    }
                }
            }
            Ok(())
        }
    }
}
