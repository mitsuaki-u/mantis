pub mod assessment;
pub mod events;
pub mod limits;
pub mod metrics;
pub mod types;

pub use assessment::{assess_token_risk, calculate_position_size};
pub use events::handle_event_internal;
pub use limits::{check_overall_risk_limits, check_risk_limits};
pub use metrics::{update_risk_metrics, update_risk_score, update_token_risk};
pub use types::{RiskState, TokenRisk};

use super::{Actor, Command, Event, Message, Query, QueryResult};
use crate::core::config::Config;
use crate::core::error::{Error, Result};
use crate::domain::trading::risk::RiskManager;
use crate::infra::actors::{ActorRef, MessageBus};
use crate::infra::db::repositories::{PositionRepository, TokenRepository};
use log::{debug, error, info, warn};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};

#[derive(Clone)]
pub struct RiskManagerActor {
    pub risk_manager: RiskManager,
    pub position_repo: Arc<PositionRepository>,
    pub token_repo: Arc<TokenRepository>,
    pub message_bus: Arc<MessageBus>,
    pub config: Arc<Config>,
    pub running: bool,
    pub max_daily_loss_limit: f64,
    pub max_drawdown_limit: f64,
    pub current_daily_loss: f64,
    pub current_drawdown: f64,
    pub token_risks: HashMap<String, f64>,
    pub risk_scores: HashMap<String, f64>,
    pub last_activity: Arc<tokio::sync::Mutex<Instant>>,
    pub execution_actor_ref: ActorRef,
    pub halted_tokens: HashSet<String>,
    // Shutdown coordination
    pub shutdown_tx: Option<broadcast::Sender<()>>,
}

impl RiskManagerActor {
    pub fn new(
        token_repo: Arc<TokenRepository>,
        position_repo: Arc<PositionRepository>,
        message_bus: Arc<MessageBus>,
        config: Arc<Config>,
        execution_actor_ref: ActorRef,
    ) -> Self {
        let domain_max_pos_size = config.trading.max_position_size;
        let domain_max_total_exposure = config.trading.max_total_exposure;
        let risk_manager = RiskManager::new(domain_max_pos_size, domain_max_total_exposure);

        let max_daily_loss_from_config = config.trading.max_daily_loss / 100.0;
        let max_drawdown_from_config = config.trading.max_drawdown / 100.0;

        Self {
            risk_manager,
            position_repo,
            token_repo,
            message_bus,
            config,
            running: false,
            max_daily_loss_limit: max_daily_loss_from_config,
            max_drawdown_limit: max_drawdown_from_config,
            current_daily_loss: 0.0,
            current_drawdown: 0.0,
            token_risks: HashMap::new(),
            risk_scores: HashMap::new(),
            last_activity: Arc::new(tokio::sync::Mutex::new(Instant::now())),
            execution_actor_ref,
            halted_tokens: HashSet::new(),
            shutdown_tx: None,
        }
    }

    async fn touch_last_activity(&mut self) {
        *self.last_activity.lock().await = Instant::now();
    }

    /// Spawn a subscription task with proper shutdown handling
    fn spawn_subscription_task(
        &mut self,
        event_type: crate::infra::actors::EventType,
        bus: Arc<MessageBus>,
        main_event_tx: mpsc::Sender<Event>,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) {
        tokio::spawn(async move {
            let (specific_event_tx, mut specific_event_rx) = mpsc::channel(100);

            // Subscribe to events
            if let Err(e) = bus
                .subscribe(format!("{:?}", event_type), specific_event_tx.clone())
                .await
            {
                error!(
                    "RiskManagerActor failed to subscribe to {:?} events: {}",
                    event_type, e
                );
                return;
            }

            info!("RiskManagerActor subscribed to {:?} events.", event_type);

            loop {
                tokio::select! {
                    // Handle incoming events
                    Some(event) = specific_event_rx.recv() => {
                        if main_event_tx.send(event).await.is_err() {
                            debug!(
                                "RiskManagerActor: Main event channel closed for {:?} forwarding.",
                                event_type
                            );
                            break;
                        }
                    }
                    // Handle shutdown signal
                    _ = shutdown_rx.recv() => {
                        debug!("RiskManagerActor: Shutdown signal received for {:?} subscription", event_type);
                        break;
                    }
                    // Handle channel closure
                    else => {
                        debug!("RiskManagerActor: Event channel closed for {:?}", event_type);
                        break;
                    }
                }
            }

            debug!(
                "RiskManagerActor: {:?} subscription task terminated",
                event_type
            );
        });
    }

    /// Gracefully shutdown all subscription tasks
    async fn shutdown_tasks(&mut self) {
        debug!("RiskManagerActor: Initiating graceful shutdown");

        // Send shutdown signal to all tasks
        if let Some(shutdown_tx) = &self.shutdown_tx {
            let _ = shutdown_tx.send(());
        }

        debug!("RiskManagerActor: Shutdown signal sent to all tasks");
    }
}

impl Actor for RiskManagerActor {
    fn start(&mut self) -> impl std::future::Future<Output = Result<()>> + Send {
        self.running = true;
        info!("RiskManagerActor started.");

        // Create shutdown coordination
        let (shutdown_tx, _) = broadcast::channel(1);
        self.shutdown_tx = Some(shutdown_tx.clone());

        let (event_tx, mut event_rx) = mpsc::channel(500);
        let mut self_clone = self.clone();

        // Spawn subscription tasks with shutdown coordination
        let event_types = [
            crate::infra::actors::EventType::Market,
            crate::infra::actors::EventType::Strategy,
            crate::infra::actors::EventType::Execution,
            crate::infra::actors::EventType::Database,
            crate::infra::actors::EventType::DexTransaction,
        ];

        for event_type in event_types {
            let shutdown_rx = shutdown_tx.subscribe();
            self_clone.spawn_subscription_task(
                event_type,
                self.message_bus.clone(),
                event_tx.clone(),
                shutdown_rx,
            );
        }

        async move {
            let mut activity_check_interval = tokio::time::interval(Duration::from_secs(30));
            let mut maintenance_interval = tokio::time::interval(Duration::from_secs(300));

            info!("RiskManagerActor main event loop starting.");

            loop {
                // Check global shutdown flag first
                if crate::domain::trading::execution::bot::is_forced_shutdown() {
                    info!("RiskManagerActor: Global shutdown detected, exiting event loop");
                    break;
                }

                tokio::select! {
                    // Handle incoming events
                    Some(event) = event_rx.recv() => {
                        if let Err(e) = handle_event_internal(&mut self_clone, event).await {
                            error!("RiskManagerActor: Error handling event: {}", e);
                        }
                    }
                    // Periodic activity check
                    _ = activity_check_interval.tick() => {
                        if !self_clone.running {
                            break;
                        }
                        let last_activity = *self_clone.last_activity.lock().await;
                        if last_activity.elapsed() > Duration::from_secs(600) {
                            warn!("RiskManagerActor: No significant activity detected for a while. Consider checking system health.");
                        }
                    }
                    // Periodic maintenance
                    _ = maintenance_interval.tick() => {
                        if !self_clone.running {
                            break;
                        }
                        debug!("RiskManagerActor: Performing periodic maintenance (if any).");
                    }
                    // Handle shutdown
                    else => {
                        info!("RiskManagerActor event channel closed. Shutting down.");
                        break;
                    }
                }

                // Check running state
                if !self_clone.running {
                    info!("RiskManagerActor stop signal received, exiting event loop.");
                    break;
                }
            }

            // Cleanup on shutdown
            self_clone.shutdown_tasks().await;
            info!("RiskManagerActor shutdown completed.");
            Ok(())
        }
    }

    fn stop(&mut self) -> Result<()> {
        info!("RiskManagerActor stopping...");
        self.running = false;

        // Send shutdown signal to all tasks
        if let Some(shutdown_tx) = &self.shutdown_tx {
            let _ = shutdown_tx.send(());
        }

        info!("RiskManagerActor stop signal sent.");
        Ok(())
    }

    fn handle_message(
        &mut self,
        msg: Message,
    ) -> impl std::future::Future<Output = Result<()>> + Send {
        let mut self_clone = self.clone();
        async move {
            match msg {
                Message::Event(event) => {
                    if let Err(e) = handle_event_internal(&mut self_clone, event).await {
                        error!("RiskManagerActor: Error handling event from Message: {}", e);
                    }
                }
                Message::Command(cmd) => {
                    debug!("RiskManagerActor received command: {:?}", cmd);
                    match cmd {
                        Command::Start => {
                            self_clone.running = true;
                            info!("RiskManagerActor started via command");
                        }
                        Command::Stop => {
                            self_clone.running = false;
                            info!("RiskManagerActor stopped via command");

                            // Send shutdown signal
                            if let Some(shutdown_tx) = &self_clone.shutdown_tx {
                                let _ = shutdown_tx.send(());
                            }
                        }
                        Command::UpdateConfig(new_config_values) => {
                            info!(
                                "RiskManagerActor received UpdateConfig command. Details: {:?}",
                                new_config_values
                            );
                        }
                        _ => warn!("RiskManagerActor: Unhandled command: {:?}", cmd),
                    }
                }
                Message::Query(query, responder) => {
                    debug!("RiskManagerActor received query: {:?}", query);
                    let result = match query {
                        Query::GetStatus => Ok(QueryResult::Status(format!(
                            "Running: {}, Health: Good",
                            self_clone.running
                        ))),
                        _ => {
                            warn!("RiskManagerActor: Unhandled query: {:?}", query);
                            Err(Error::NotImplemented(
                                "Query not supported by RiskManagerActor".to_string(),
                            ))
                        }
                    };
                    if responder.send(result).is_err() {
                        error!("RiskManagerActor: Failed to send query response");
                    }
                }
            }
            Ok(())
        }
    }
}
