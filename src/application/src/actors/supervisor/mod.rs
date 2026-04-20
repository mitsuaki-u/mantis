use crate::application::actors::system::actor::{ActorState, LifecycleActor};
use crate::application::actors::system::{
    Actor, ActorRef, Command, LifecycleState, Message, Query, QueryResult,
};
use crate::application::app::is_forced_shutdown;
use crate::application::errors::Error;
use crate::events::EventType;
use async_trait::async_trait;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::sync::{Arc, Once};
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::interval;

// Use a simple Once-based singleton pattern for supervisor - similar to EventRouter
static SUPERVISOR_INIT: Once = Once::new();
static mut GLOBAL_SUPERVISOR_INSTANCE: Option<Arc<SupervisorActor>> = None;

/// The supervisor actor coordinates and monitors other actors in the system
#[derive(Clone)]
pub struct SupervisorActor {
    // New trait-based state management
    state: ActorState,

    // Core functionality
    actors: Arc<RwLock<HashMap<String, ActorRef>>>,

    // Metrics and monitoring
    health_check_interval: Duration,
}

impl Default for SupervisorActor {
    fn default() -> Self {
        Self::new()
    }
}

impl SupervisorActor {
    pub fn new() -> Self {
        Self {
            state: ActorState::new("SupervisorActor".to_string()),
            actors: Arc::new(RwLock::new(HashMap::new())),
            health_check_interval: Duration::from_secs(30), // Default to 30 seconds
        }
    }

    /// Register this SupervisorActor instance as the global singleton
    pub fn register_as_global(supervisor: Arc<SupervisorActor>) {
        SUPERVISOR_INIT.call_once(|| {
            debug!("Registering global SupervisorActor singleton");

            // SAFETY: This is safe because we're in a Once::call_once closure,
            // which guarantees this code only runs once during the program lifetime
            unsafe {
                GLOBAL_SUPERVISOR_INSTANCE = Some(supervisor);
            }

            debug!("Global SupervisorActor singleton registered successfully");
        });
    }

    /// Configure health check interval in seconds
    pub fn with_health_check_interval(mut self, interval_seconds: u64) -> Self {
        self.health_check_interval = Duration::from_secs(interval_seconds);
        self
    }

    pub async fn register_actor(&self, id: String, actor: ActorRef) -> Result<(), Error> {
        let mut actors = self.actors.write().await;
        actors.insert(id.clone(), actor.clone());
        debug!("Registered actor: {}", id);
        Ok(())
    }

    pub async fn get_actor(&self, id: &str) -> Option<ActorRef> {
        let actors = self.actors.read().await;
        actors.get(id).cloned()
    }

    /// Start all registered actors
    pub async fn start_all_actors(&self) -> Result<(), Error> {
        debug!("🔄 SupervisorActor starting all registered actors");
        let actors = self.actors.read().await;

        for (id, actor) in actors.iter() {
            debug!("Starting actor: {}", id);
            if let Err(e) = actor.send(Message::Command(Command::Start)) {
                error!("Failed to start actor {}: {}", id, e);
            } else {
                debug!("Successfully sent start command to actor: {}", id);
            }
        }

        info!("🔄 SupervisorActor finished starting all actors");
        Ok(())
    }

    /// Stop all registered actors
    pub async fn stop_all_actors(&self) -> Result<(), Error> {
        info!("🔄 SupervisorActor stopping all registered actors");
        let actors = self.actors.read().await;

        for (id, actor) in actors.iter() {
            debug!("Stopping actor: {}", id);
            if let Err(e) = actor.send(Message::Command(Command::Stop)) {
                error!("Failed to stop actor {}: {}", id, e);
            } else {
                info!("Successfully sent stop command to actor: {}", id);
            }
        }

        info!("🔄 SupervisorActor finished stopping all actors");
        Ok(())
    }

    /// Unified actor recovery method - stop, wait, start
    /// This is used by all recovery mechanisms for consistency
    async fn recover_actor_internal(actor_ref: &ActorRef, id: &str) -> Result<(), Error> {
        info!("Recovering actor '{}'", id);

        // Stop the actor
        if let Err(e) = actor_ref.send(Message::Command(Command::Stop)) {
            warn!("Failed to send stop command to {}: {}", id, e);
            // Continue anyway - actor might already be stopped
        }

        // Wait for graceful shutdown
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Start the actor
        actor_ref
            .send(Message::Command(Command::Start))
            .map_err(|e| Error::Task(format!("Failed to restart actor {}: {}", id, e)))?;

        info!("Successfully recovered actor '{}'", id);
        Ok(())
    }

    /// Restart a specific actor
    pub async fn restart_actor(&self, id: &str) -> Result<(), Error> {
        let actor_ref = self
            .get_actor(id)
            .await
            .ok_or_else(|| Error::NotFound(format!("Actor not found: {}", id)))?;

        Self::recover_actor_internal(&actor_ref, id).await
    }

    /// Get status from a specific actor
    pub async fn get_actor_status(&self, id: &str) -> Result<String, Error> {
        if let Some(actor) = self.get_actor(id).await {
            let (tx, rx) = tokio::sync::oneshot::channel();
            if let Err(e) = actor.send(Message::Query(Query::GetStatus, tx)) {
                return Err(Error::InvalidInput(format!(
                    "Failed to send query to actor {}: {}",
                    id, e
                )));
            }
            match rx.await {
                Ok(Ok(QueryResult::Status(status))) => Ok(status),
                Ok(Err(e)) => Err(e),
                Err(e) => Err(Error::Task(format!("Failed to receive status: {}", e))),
            }
        } else {
            Err(Error::NotFound(format!("Actor not found: {}", id)))
        }
    }

    /// Start a background task to monitor actor health
    pub async fn watch_actors(&self) -> Result<(), Error> {
        info!(
            "🔍 Starting actor health monitoring with interval: {}s",
            self.health_check_interval.as_secs()
        );

        let actors = self.actors.clone();
        let health_check_interval = self.health_check_interval;

        tokio::spawn(async move {
            let mut interval_timer = interval(health_check_interval);
            let mut failure_counts: HashMap<String, u32> = HashMap::new();

            loop {
                interval_timer.tick().await;

                if is_forced_shutdown() {
                    info!("Health monitoring detected system shutdown, exiting");
                    break;
                }

                debug!("Performing actor health check");

                // Collect failed actors in this cycle
                let mut failed_actors = Vec::new();

                {
                    let actor_refs = actors.read().await;

                    for (id, actor_ref) in actor_refs.iter() {
                        let (tx, rx) = tokio::sync::oneshot::channel();

                        // Try to get actor status with timeout
                        let actor_ok = match actor_ref.send(Message::Query(Query::GetStatus, tx)) {
                            Ok(()) => {
                                matches!(
                                    tokio::time::timeout(Duration::from_secs(5), rx).await,
                                    Ok(Ok(Ok(QueryResult::Status(_))))
                                )
                            }
                            Err(_) => false,
                        };

                        if !actor_ok {
                            warn!("Actor {} is not responding or failed health check", id);
                            let count = failure_counts.entry(id.clone()).or_insert(0);
                            *count += 1;

                            if *count < 3 {
                                failed_actors.push((id.clone(), actor_ref.clone()));
                            } else {
                                error!("Actor {} has failed {} times, giving up", id, count);
                            }
                        } else {
                            // Actor is healthy - reset failure count
                            failure_counts.insert(id.clone(), 0);
                        }
                    }
                } // Release read lock

                // Recover all failed actors in this cycle
                for (id, actor_ref) in failed_actors {
                    info!(
                        "Attempting to recover actor: {} (failure count: {})",
                        id,
                        failure_counts.get(&id).unwrap_or(&0)
                    );

                    if let Err(e) = Self::recover_actor_internal(&actor_ref, &id).await {
                        error!("Failed to recover actor {}: {}", id, e);
                    } else {
                        info!("Successfully recovered actor {}", id);
                        failure_counts.insert(id, 0);
                    }
                }
            }

            info!("Actor health monitoring task has terminated");
        });

        Ok(())
    }

    /// Get health status of all actors
    pub async fn get_health_report(&self) -> Result<serde_json::Value, Error> {
        let actors = self.actors.read().await;

        let mut report = serde_json::json!({
            "supervisor_running": self.state.running,
            "total_actors": actors.len(),
            "registered_actors": []
        });

        let mut actor_list = Vec::new();
        for id in actors.keys() {
            actor_list.push(id.clone());
        }

        report["registered_actors"] = serde_json::json!(actor_list);

        Ok(report)
    }
}

#[async_trait]
impl Actor for SupervisorActor {
    fn name(&self) -> &str {
        &self.state.name
    }

    fn is_running(&self) -> bool {
        self.state.running
    }

    // Override event handling if needed
    async fn handle_event(&mut self, _event: crate::events::Event) -> Result<(), Error> {
        // The EventRouter already handles event forwarding to subscribers
        // No need for the Supervisor to act as a message forwarder
        Ok(())
    }

    fn supported_event_types(&self) -> Vec<EventType> {
        vec![] // Supervisor doesn't need to handle specific events
    }
}

#[async_trait]
impl LifecycleActor for SupervisorActor {
    async fn initialize(&mut self) -> Result<(), Error> {
        info!("Initializing SupervisorActor");
        self.state.lifecycle_state = LifecycleState::Initialized;

        debug!("SupervisorActor initialized for actor coordination");
        Ok(())
    }

    async fn cleanup(&mut self) -> Result<(), Error> {
        info!("Cleaning up SupervisorActor");

        // Stop all managed actors
        if let Err(e) = self.stop_all_actors().await {
            error!("Failed to stop all actors during cleanup: {}", e);
        }

        debug!("SupervisorActor cleanup completed");
        Ok(())
    }

    fn lifecycle_state(&self) -> LifecycleState {
        self.state.lifecycle_state.clone()
    }
}
