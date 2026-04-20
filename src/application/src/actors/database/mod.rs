pub mod events;
pub mod queuing;

pub use events::handle_event_internal;
pub use queuing::{create_queue, queue_position_update};

use crate::application::actors::system::actor::{ActorState, LifecycleActor};
use crate::application::actors::system::{Actor, Command, LifecycleState, Query, QueryResult};
use crate::application::errors::Result;
use crate::config::Config;
use crate::events::{Event, EventType};
use crate::infrastructure::database::queue::PositionUpdateQueue;
use crate::infrastructure::database::repositories::RepositoryFactory;
use crate::infrastructure::database::task_handler::DatabaseTaskHandler;
use crate::EventRouter;
use async_trait::async_trait;
use log::{debug, error, info, warn};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

pub struct DatabaseActor {
    // New trait-based state management
    state: ActorState,

    // Core functionality
    pub repo_factory: RepositoryFactory,
    pub event_router: Arc<EventRouter>,

    // Configuration
    pub config: Arc<Config>,

    // Redis queue instance for position update batching
    pub position_queue: Option<Arc<PositionUpdateQueue>>,

    // Background task handles
    event_processor_handle: Option<tokio::task::JoinHandle<()>>,
    redis_batch_processor_handle: Option<tokio::task::JoinHandle<()>>,

    // Actor coordination and state
    pub shutdown_flag: Arc<AtomicBool>,
    pub event_sender: mpsc::Sender<Event>,
    pub event_receiver: Option<mpsc::Receiver<Event>>,

    // Metrics and monitoring
    pub last_activity_time: Arc<tokio::sync::Mutex<Instant>>,
}

impl DatabaseActor {
    pub fn new(
        repo_factory: RepositoryFactory,
        event_router: Arc<EventRouter>,
        config: Arc<Config>,
    ) -> Self {
        let position_queue = create_queue(&config);
        let (event_sender, event_receiver) = mpsc::channel(100);

        Self {
            state: ActorState::new("DatabaseActor".to_string()),
            repo_factory,
            event_router,
            config,
            position_queue: position_queue.map(Arc::new),
            event_processor_handle: None,
            redis_batch_processor_handle: None,
            last_activity_time: Arc::new(tokio::sync::Mutex::new(Instant::now())),
            shutdown_flag: Arc::new(AtomicBool::new(false)),
            event_sender,
            event_receiver: Some(event_receiver),
        }
    }

    async fn touch_last_activity(&mut self) {
        *self.last_activity_time.lock().await = Instant::now();
    }

    async fn process_events_internal(&mut self, mut event_rx: mpsc::Receiver<Event>) {
        debug!("DatabaseActor: Starting internal event processing loop");

        while self.state.running {
            tokio::select! {
                Some(event) = event_rx.recv() => {
                    self.touch_last_activity().await;
                    if let Err(e) = self.handle_event(event).await {
                        error!("DatabaseActor: Error processing event: {}", e);
                        self.state.record_error();
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    // Periodic check for shutdown
                    if !self.state.running {
                        break;
                    }
                }
            }
        }

        info!("DatabaseActor: Event processing loop ended");
    }
}

#[async_trait]
impl Actor for DatabaseActor {
    fn name(&self) -> &str {
        &self.state.name
    }

    fn is_running(&self) -> bool {
        self.state.running
    }

    // Override start/stop with custom logic
    async fn start(&mut self) -> Result<()> {
        debug!("Starting DatabaseActor with custom startup logic");
        self.state.start();

        // Start event processing task (handles EventRouter events)
        if let Some(event_rx) = self.event_receiver.take() {
            let mut actor_clone = self.clone();
            let handle = tokio::spawn(async move {
                actor_clone.process_events_internal(event_rx).await;
            });
            self.event_processor_handle = Some(handle);
        }

        // Start Redis batch processing task (drains Redis queue to PostgreSQL)
        // Only start if not already running
        if self.redis_batch_processor_handle.is_none() {
            if let Some(pos_queue) = &self.position_queue {
                info!(
                    "Starting Redis batch processor (interval: {}s, batch_size: {})",
                    crate::infrastructure::constants::DEFAULT_BATCH_INTERVAL_SECS,
                    crate::infrastructure::constants::DEFAULT_BATCH_SIZE
                );

                let pos_queue = pos_queue.clone();
                let repo_factory = self.repo_factory.clone();
                let shutdown_flag = self.shutdown_flag.clone();

                let queue_handle = tokio::spawn(async move {
                    let mut interval = tokio::time::interval(Duration::from_secs(
                        crate::infrastructure::constants::DEFAULT_BATCH_INTERVAL_SECS,
                    ));
                    let mut interval_counter = 0u64;

                    info!("Redis batch processor: Started successfully");

                    loop {
                        interval.tick().await;
                        interval_counter += 1;

                        // Check for shutdown
                        if shutdown_flag.load(Ordering::Relaxed) {
                            info!("Redis batch processor: Shutdown detected, draining queue before exit...");

                            // Final drain attempt - ignore errors during shutdown
                            let _ = DatabaseTaskHandler::process_position_batch(
                                (*repo_factory.position_repository()).clone(),
                                (*pos_queue).clone(),
                                crate::infrastructure::constants::DEFAULT_BATCH_SIZE,
                            )
                            .await;

                            info!("Redis batch processor: Graceful shutdown complete");
                            break;
                        }

                        // Process position updates
                        if let Err(e) = DatabaseTaskHandler::process_position_batch(
                            (*repo_factory.position_repository()).clone(),
                            (*pos_queue).clone(),
                            crate::infrastructure::constants::DEFAULT_BATCH_SIZE,
                        )
                        .await
                        {
                            error!(
                                "Redis batch processor: Failed to process position batch: {}",
                                e
                            );
                        }

                        // Process delayed items (retry backoff)
                        if let Err(e) = pos_queue.process_delayed_items() {
                            debug!(
                                "Redis batch processor: Position delayed items processing: {}",
                                e
                            );
                        }

                        // Handle stuck operations every 10 minutes (20 intervals at 30s = 600s)
                        if interval_counter.is_multiple_of(20) {
                            debug!("Redis batch processor: Running stuck operation recovery...");

                            const STUCK_TIMEOUT_SECS: u64 = 300; // 5 minutes

                            match pos_queue.handle_stuck_operations(STUCK_TIMEOUT_SECS) {
                                Ok(recovered) if recovered > 0 => {
                                    warn!("Redis batch processor: Recovered {} stuck position operations", recovered);
                                }
                                Err(e) => {
                                    warn!("Redis batch processor: Failed to handle stuck position operations: {}", e);
                                }
                                _ => {}
                            }
                        }

                        // Log queue stats periodically (every 10 intervals = 5 minutes at 30s)
                        if interval_counter.is_multiple_of(10) {
                            if let Ok(pos_stats) = pos_queue.get_queue_stats() {
                                debug!("Queue stats [positions]: pending={}, processing={}, delayed={}, failed={}",
                                   pos_stats.pending, pos_stats.processing, pos_stats.delayed, pos_stats.failed);
                            }
                        }
                    }
                });

                self.redis_batch_processor_handle = Some(queue_handle);
            } else {
                warn!("Redis batch processor: Redis queue not available, skipping background task");
            }
        }

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Stopping DatabaseActor with custom shutdown logic");
        self.state.stop();

        self.shutdown_flag.store(true, Ordering::Relaxed);

        // Stop Redis batch processor (will drain queues gracefully)
        if let Some(handle) = self.redis_batch_processor_handle.take() {
            // Give it time to finish draining
            tokio::time::sleep(Duration::from_millis(500)).await;
            if !handle.is_finished() {
                handle.abort();
            }
            info!("DatabaseActor: Redis batch processor stopped");
        }

        // Stop event processor
        if let Some(handle) = self.event_processor_handle.take() {
            handle.abort();
            info!("DatabaseActor: Event processor stopped");
        }

        Ok(())
    }

    async fn handle_command(&mut self, cmd: Command) -> Result<()> {
        debug!("DatabaseActor received command: {:?}", cmd);
        match cmd {
            Command::Start => {
                debug!("DatabaseActor received Start command");
                self.start().await
            }
            Command::Stop => {
                info!("DatabaseActor received Stop command");
                self.stop().await
            }
            Command::UpdateConfig(_new_config_values) => {
                info!("DatabaseActor received UpdateConfig command");
                debug!("DatabaseActor ignoring UpdateConfig (ConfigurableActor removed)");
                Ok(())
            }
        }
    }

    async fn handle_query(
        &mut self,
        query: Query,
        responder: tokio::sync::oneshot::Sender<Result<QueryResult>>,
    ) -> Result<()> {
        debug!("DatabaseActor received query: {:?}", query);
        self.touch_last_activity().await;

        let result = match query {
            Query::GetStatus => {
                let status = format!(
                    "Running: {}, Health: {:?}",
                    self.state.running, self.state.health_status
                );
                Ok(QueryResult::Status(status))
            }
        };

        if responder.send(result).is_err() {
            error!("DatabaseActor: Failed to send query response");
        }
        Ok(())
    }

    async fn handle_event(&mut self, event: Event) -> Result<()> {
        self.state.record_activity();
        self.touch_last_activity().await;
        events::handle_event_internal(self, event).await
    }

    fn supported_event_types(&self) -> Vec<EventType> {
        vec![
            EventType::Market,
            EventType::Execution,
            EventType::Risk,
            EventType::Strategy,
            EventType::DexTransaction,
        ]
    }
}

#[async_trait]
impl LifecycleActor for DatabaseActor {
    async fn initialize(&mut self) -> Result<()> {
        info!("Initializing DatabaseActor");
        self.state.lifecycle_state = LifecycleState::Initialized;

        // Test database connectivity
        if let Err(e) = self.repo_factory.token_repository().health_check().await {
            error!(
                "Database connectivity check failed during initialization: {}",
                e
            );
            return Err(e);
        }

        debug!("DatabaseActor initialized with queue-based persistence");
        Ok(())
    }

    async fn cleanup(&mut self) -> Result<()> {
        info!("Cleaning up DatabaseActor");

        // Stop event processing task
        if let Some(handle) = self.event_processor_handle.take() {
            handle.abort();
            info!("Aborted database event processor");
        }

        debug!("DatabaseActor cleanup completed");
        Ok(())
    }

    fn lifecycle_state(&self) -> LifecycleState {
        self.state.lifecycle_state.clone()
    }
}

impl Clone for DatabaseActor {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            repo_factory: self.repo_factory.clone(),
            event_router: self.event_router.clone(),
            config: self.config.clone(),
            position_queue: self.position_queue.clone(),
            event_processor_handle: None, // Task handles are not cloneable
            redis_batch_processor_handle: None,
            shutdown_flag: self.shutdown_flag.clone(),
            event_sender: self.event_sender.clone(),
            event_receiver: None, // Can't clone receiver
            last_activity_time: self.last_activity_time.clone(),
        }
    }
}
