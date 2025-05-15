use super::{Actor, ActorRef, Message, Command, Query, QueryResult};
use crate::core::error::Error;
use std::sync::{Arc, Once};
use tokio::sync::RwLock;
use std::collections::HashMap;
use log::{info, error, debug, warn};
use std::time::Duration;
use tokio::time::interval;
use chrono::{DateTime, Utc};
use crate::domain::trading::execution::bot::is_forced_shutdown;
use crate::infra::actors::MessageBus;

/// Actor health status levels
#[derive(Debug, Clone, PartialEq)]
pub enum ActorHealthStatus {
    /// Actor is healthy with no recent failures
    Good,
    /// Actor has experienced some failures but is still operational
    Degraded,
    /// Actor is in a critical state and may not be functioning properly
    Critical,
}

/// Detailed state information for an actor
#[derive(Debug, Clone)]
pub struct ActorState {
    /// Whether the actor is currently running
    pub is_running: bool,
    /// Count of failures since last successful recovery
    pub failure_count: u32,
    /// Timestamp of last detected failure
    pub last_failure: Option<DateTime<Utc>>,
    /// Current health status assessment
    pub health_status: ActorHealthStatus,
}

impl ActorState {
    /// Create a new actor state, initially stopped and healthy
    pub fn new() -> Self {
        Self {
            is_running: false,
            failure_count: 0,
            last_failure: None,
            health_status: ActorHealthStatus::Good,
        }
    }
}

// Use a simple Once-based singleton pattern for supervisor - similar to MessageBus
static SUPERVISOR_INIT: Once = Once::new();
static mut GLOBAL_SUPERVISOR_INSTANCE: Option<Arc<SupervisorActor>> = None;

pub struct SupervisorActor {
    actors: Arc<RwLock<HashMap<String, ActorRef>>>,
    message_bus: Arc<super::MessageBus>,
    running: bool,
    /// Health check interval in seconds
    health_check_interval: u64,
    /// Tracks actor states with detailed information
    actor_states: Arc<RwLock<HashMap<String, ActorState>>>,
}

impl SupervisorActor {
    pub fn new(message_bus: Arc<super::MessageBus>) -> Self {
        debug!("Creating new SupervisorActor");
        Self {
            actors: Arc::new(RwLock::new(HashMap::new())),
            message_bus,
            running: true,
            health_check_interval: 30, // Default to 30 seconds
            actor_states: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Register this SupervisorActor instance as the global singleton
    pub fn register_as_global(supervisor: Arc<SupervisorActor>) {
        SUPERVISOR_INIT.call_once(|| {
            info!("Registering global SupervisorActor singleton");
            
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
        self.health_check_interval = interval_seconds;
        self
    }

    pub async fn register_actor(&self, id: String, actor: ActorRef) -> Result<(), Error> {
        let mut actors = self.actors.write().await;
        actors.insert(id.clone(), actor.clone());
        
        // Also initialize state tracking
        let mut states = self.actor_states.write().await;
        states.insert(id.clone(), ActorState::new()); // Initialize with default state
        
        info!("Registered actor: {}", id);
        Ok(())
    }

    pub async fn unregister_actor(&self, id: &str) -> Result<(), Error> {
        let mut actors = self.actors.write().await;
        actors.remove(id);
        
        // Clean up state tracking
        let mut states = self.actor_states.write().await;
        states.remove(id);
        
        info!("Unregistered actor: {}", id);
        Ok(())
    }

    pub async fn get_actor(&self, id: &str) -> Option<ActorRef> {
        let actors = self.actors.read().await;
        actors.get(id).cloned()
    }

    /// Start all registered actors
    pub async fn start_all_actors(&self) -> Result<(), Error> {
        info!("🔄 SupervisorActor starting all registered actors");
        let actors = self.actors.read().await;
        let mut states = self.actor_states.write().await;
        
        for (id, actor) in actors.iter() {
            debug!("Starting actor: {}", id);
            if let Err(e) = actor.send(Message::Command(Command::Start)).await {
                error!("Failed to start actor {}: {}", id, e);
                if let Some(state) = states.get_mut(id) {
                    state.is_running = false;
                }
            } else {
                if let Some(state) = states.get_mut(id) {
                    state.is_running = true;
                }
                info!("Successfully started actor: {}", id);
            }
        }
        
        info!("🔄 SupervisorActor finished starting all actors");
        Ok(())
    }

    /// Stop all registered actors
    pub async fn stop_all_actors(&self) -> Result<(), Error> {
        info!("🔄 SupervisorActor stopping all registered actors");
        let actors = self.actors.read().await;
        let mut states = self.actor_states.write().await;
        
        for (id, actor) in actors.iter() {
            debug!("Stopping actor: {}", id);
            if let Err(e) = actor.send(Message::Command(Command::Stop)).await {
                error!("Failed to stop actor {}: {}", id, e);
            } else {
                if let Some(state) = states.get_mut(id) {
                    state.is_running = false;
                }
                info!("Successfully stopped actor: {}", id);
            }
        }
        
        info!("🔄 SupervisorActor finished stopping all actors");
        Ok(())
    }

    /// Start a specific actor by ID
    pub async fn start_actor(&self, id: &str) -> Result<(), Error> {
        if let Some(actor) = self.get_actor(id).await {
            info!("SupervisorActor starting actor: {}", id);
            actor.send(Message::Command(Command::Start)).await?;
            
            // Update state tracking
            let mut states = self.actor_states.write().await;
            if let Some(state) = states.get_mut(id) {
                state.is_running = true;
            }
            
            Ok(())
        } else {
            Err(Error::NotFound(format!("Actor not found: {}", id)))
        }
    }

    /// Stop a specific actor by ID
    pub async fn stop_actor(&self, id: &str) -> Result<(), Error> {
        if let Some(actor) = self.get_actor(id).await {
            info!("SupervisorActor stopping actor: {}", id);
            actor.send(Message::Command(Command::Stop)).await?;
            
            // Update state tracking
            let mut states = self.actor_states.write().await;
            if let Some(state) = states.get_mut(id) {
                state.is_running = false;
            }
            
            Ok(())
        } else {
            Err(Error::NotFound(format!("Actor not found: {}", id)))
        }
    }

    /// Restart a specific actor
    pub async fn restart_actor(&self, id: &str) -> Result<(), Error> {
        let actor_ref = match self.actors.read().await.get(id) {
            Some(actor_ref) => actor_ref.clone(),
            None => return Err(Error::InvalidInput(format!("Actor with ID '{}' not found", id))),
        };
        
        info!("Restarting actor '{}'", id);
        
        // Send stop command
        let stop_result = actor_ref.send(crate::actors::Message::Command(crate::actors::Command::Stop)).await;
        
        // Small delay to ensure proper shutdown
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        
        // Send start command
        let start_result = actor_ref.send(crate::actors::Message::Command(crate::actors::Command::Start)).await;
        
        // Reset failure counter for this actor
        let mut states = self.actor_states.write().await;
        if let Some(state) = states.get_mut(id) {
            state.failure_count = 0;
            state.last_failure = None;
            state.health_status = ActorHealthStatus::Good;
            state.is_running = true;
        }
        
        match (stop_result, start_result) {
            (Ok(_), Ok(_)) => {
                info!("Successfully restarted actor '{}'", id);
                if let Some(state) = states.get_mut(id) {
                    state.health_status = ActorHealthStatus::Degraded;
                    state.failure_count = 0;
                    state.last_failure = None;
                }
            },
            (Err(e), Ok(_)) => {
                warn!("Actor '{}' stop command failed but start succeeded: {}", id, e);
                // Still consider this a success since the actor is running
                if let Some(state) = states.get_mut(id) {
                    state.health_status = ActorHealthStatus::Degraded;
                    state.failure_count = 0;
                    state.last_failure = None;
                }
            },
            (_, Err(e)) => {
                error!("Failed to restart actor '{}': {}", id, e);
                // We'll try again next health check
            }
        }
        
        Ok(())
    }

    /// Get status from a specific actor
    pub async fn get_actor_status(&self, id: &str) -> Result<String, Error> {
        if let Some(actor) = self.get_actor(id).await {
            let (tx, rx) = tokio::sync::oneshot::channel();
            actor.send(Message::Query(Query::GetStatus, tx)).await?;
            match rx.await {
                Ok(Ok(QueryResult::Status(status))) => Ok(status),
                Ok(Ok(_)) => Err(Error::InvalidInput("Unexpected query result type".to_string())),
                Ok(Err(e)) => Err(e),
                Err(e) => Err(Error::Task(format!("Failed to receive status: {}", e))),
            }
        } else {
            Err(Error::NotFound(format!("Actor not found: {}", id)))
        }
    }

    /// Get metrics from a specific actor
    pub async fn get_actor_metrics(&self, id: &str) -> Result<serde_json::Value, Error> {
        if let Some(actor) = self.get_actor(id).await {
            let (tx, rx) = tokio::sync::oneshot::channel();
            actor.send(Message::Query(Query::GetMetrics, tx)).await?;
            match rx.await {
                Ok(Ok(QueryResult::Metrics(metrics))) => Ok(metrics),
                Ok(Ok(_)) => Err(Error::InvalidInput("Unexpected query result type".to_string())),
                Ok(Err(e)) => Err(e),
                Err(e) => Err(Error::Task(format!("Failed to receive metrics: {}", e))),
            }
        } else {
            Err(Error::NotFound(format!("Actor not found: {}", id)))
        }
    }
    
    /// Start a background task to monitor actor health
    pub async fn watch_actors(&self) -> Result<(), Error> {
        info!("🔍 Starting actor health monitoring with interval: {}s", self.health_check_interval);
        
        // Clone the necessary Arc references for the task
        let actors = self.actors.clone();
        let actor_states = self.actor_states.clone();
        let health_check_interval = self.health_check_interval;
        
        // Spawn a task to periodically check actor health
        tokio::spawn(async move {
            let mut interval_timer = interval(Duration::from_secs(health_check_interval));
            
            loop {
                interval_timer.tick().await;
                
                // Skip health check if system shutdown has been triggered
                if is_forced_shutdown() {
                    info!("Health monitoring detected system shutdown, exiting");
                    break;
                }
                
                debug!("Performing actor health check");
                
                // Get the actors
                let actor_refs = actors.read().await;
                
                for (id, actor_ref) in actor_refs.iter() {
                    // Check actor status through query
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    match actor_ref.send(Message::Query(Query::GetStatus, tx)).await {
                        Ok(()) => {
                            // Set a timeout for the response to detect hung actors
                            match tokio::time::timeout(
                                Duration::from_secs(5),
                                rx
                            ).await {
                                Ok(Ok(Ok(QueryResult::Status(status)))) => {
                                    let is_running = status.contains("running: true");
                                    let mut states = actor_states.write().await;
                                    
                                    if let Some(state) = states.get_mut(id) {
                                        let previous_state = state.is_running;
                                        
                                        // Update the state
                                        state.is_running = is_running;
                                        
                                        // If state changed, log it
                                        if previous_state != is_running {
                                            if is_running {
                                                info!("Actor {} has started running", id);
                                                // Reset failure count if actor started running
                                                state.failure_count = 0;
                                                state.health_status = ActorHealthStatus::Good;
                                            } else {
                                                warn!("Actor {} has stopped running unexpectedly", id);
                                                
                                                // Increment failure count
                                                state.failure_count += 1;
                                                state.last_failure = Some(Utc::now());
                                                
                                                // Update health status based on failure count
                                                if state.failure_count >= 3 {
                                                    state.health_status = ActorHealthStatus::Critical;
                                                    error!("Actor {} has failed too many times, no longer attempting recovery", id);
                                                } else if state.failure_count > 0 {
                                                    state.health_status = ActorHealthStatus::Degraded;
                                                    
                                                    // If this is a critical actor, attempt recovery
                                                    if ["market", "strategy", "risk", "execution"].contains(&id.as_str()) {
                                                        info!("Attempting to recover actor: {} (failure count: {})", id, state.failure_count);
                                                        
                                                        // Drop lock before potential long operation
                                                        drop(states);
                                                        
                                                        // Attempt to restart the actor
                                                        match actor_ref.send(Message::Command(Command::Start)).await {
                                                            Ok(()) => info!("Recovery attempt for {} initiated", id),
                                                            Err(e) => error!("Failed to initiate recovery for {}: {}", id, e),
                                                        }
                                                        
                                                        break; // Exit the loop after recovery attempt
                                                    }
                                                }
                                            }
                                        } else if is_running {
                                            // If still running, ensure health status is good
                                            if state.health_status != ActorHealthStatus::Good {
                                                state.health_status = ActorHealthStatus::Good;
                                            }
                                        }
                                    }
                                },
                                Ok(Ok(Ok(_))) => {
                                    warn!("Unexpected response type from actor {}", id);
                                },
                                Ok(Ok(Err(e))) => {
                                    error!("Error in status response from actor {}: {}", id, e);
                                    
                                    // Update failure state
                                    let mut states = actor_states.write().await;
                                    if let Some(state) = states.get_mut(id) {
                                        state.failure_count += 1;
                                        state.last_failure = Some(Utc::now());
                                        state.health_status = if state.failure_count >= 3 {
                                            ActorHealthStatus::Critical
                                        } else {
                                            ActorHealthStatus::Degraded
                                        };
                                    }
                                },
                                Ok(Err(e)) => {
                                    error!("Failed to receive response from actor {}: {}", id, e);
                                    
                                    // Update failure state
                                    let mut states = actor_states.write().await;
                                    if let Some(state) = states.get_mut(id) {
                                        state.failure_count += 1;
                                        state.last_failure = Some(Utc::now());
                                        state.health_status = if state.failure_count >= 3 {
                                            ActorHealthStatus::Critical
                                        } else {
                                            ActorHealthStatus::Degraded
                                        };
                                    }
                                },
                                Err(_) => {
                                    error!("Actor {} status check timed out - actor may be hung", id);
                                    
                                    // Update failure state for timeout (most severe)
                                    let mut states = actor_states.write().await;
                                    if let Some(state) = states.get_mut(id) {
                                        state.failure_count += 1;
                                        state.last_failure = Some(Utc::now());
                                        state.health_status = ActorHealthStatus::Critical;
                                    }
                                }
                            }
                        },
                        Err(e) => {
                            error!("Failed to send status query to actor {}: {}", id, e);
                            
                            // Update failure state
                            let mut states = actor_states.write().await;
                            if let Some(state) = states.get_mut(id) {
                                state.failure_count += 1;
                                state.last_failure = Some(Utc::now());
                                state.health_status = ActorHealthStatus::Critical;
                            }
                        }
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
        let states = self.actor_states.read().await;
        
        let mut report = serde_json::json!({
            "supervisor_running": self.running,
            "actors": {},
            "total_actors": actors.len(),
            "running_actors": 0,
            "failing_actors": 0
        });
        
        let mut running_count = 0;
        let mut failing_count = 0;
        
        for (id, _) in actors.iter() {
            let actor_state = states.get(id).cloned().unwrap_or_else(ActorState::new);
            let status = if actor_state.is_running { "running" } else { "stopped" };
            let health = match actor_state.health_status {
                ActorHealthStatus::Good => "good",
                ActorHealthStatus::Degraded => "degraded", 
                ActorHealthStatus::Critical => "critical"
            };
            
            report["actors"][id] = serde_json::json!({
                "status": status,
                "health": health,
                "failure_count": actor_state.failure_count,
                "last_failure": actor_state.last_failure.map(|dt| dt.to_rfc3339())
            });
            
            if actor_state.is_running {
                running_count += 1;
            }
            
            if actor_state.failure_count > 0 {
                failing_count += 1;
            }
        }
        
        report["running_actors"] = serde_json::json!(running_count);
        report["failing_actors"] = serde_json::json!(failing_count);
        
        // Add system metrics
        report["system"] = serde_json::json!({
            "uptime_seconds": 0, // TODO: Track actual uptime
            "memory_usage_mb": 0, // TODO: Track actual memory usage
            "overall_health": if failing_count > 2 {
                "Critical"
            } else if failing_count > 0 {
                "Degraded"
            } else {
                "Good"
            }
        });
        
        Ok(report)
    }

    /// Force reconnection of problematic actors - called when communication failures are detected
    pub async fn force_reconnect_actor(&self, actor_type: &str) -> Result<(), Error> {
        info!("Supervisor initiating emergency reconnection procedure for {}", actor_type);
        
        let actors = self.actors.read().await;
        
        // Build a list of matching actors:
        // 1. Try for exact match first
        // 2. Then try for case-insensitive partial match (e.g., "strategy" would match "StrategyActor")
        let actor_type_lower = actor_type.to_lowercase();
        
        let matching_actors: Vec<(String, ActorRef)> = actors
            .iter()
            .filter(|(id, _)| {
                // Exact match
                if id.as_str() == actor_type {
                    return true;
                }
                
                // Case-insensitive contains match for actor types vs IDs
                // This handles cases like "strategy" vs "StrategyActor"
                let id_lower = id.to_lowercase();
                if id_lower.contains(&actor_type_lower) || actor_type_lower.contains(&id_lower) {
                    return true;
                }
                
                // Add additional mapping for specific known cases
                match (id.as_str(), actor_type) {
                    ("strategy", "StrategyActor") | ("StrategyActor", "strategy") => true,
                    ("risk", "RiskManagerActor") | ("RiskManagerActor", "risk") => true,
                    ("execution", "ExecutionActor") | ("ExecutionActor", "execution") => true,
                    ("market", "MarketDataActor") | ("MarketDataActor", "market") => true,
                    ("database", "DatabaseActor") | ("DatabaseActor", "database") => true,
                    _ => false,
                }
            })
            .map(|(id, actor)| (id.clone(), actor.clone()))
            .collect();
        
        if matching_actors.is_empty() {
            warn!("No actors found matching type: {}", actor_type);
            return Ok(());
        }
        
        // Log what we're about to do
        info!("Found {} actors to reconnect for type {}", matching_actors.len(), actor_type);
        
        for (id, actor_ref) in matching_actors {
            // Record that we're attempting emergency recovery
            {
                let mut states = self.actor_states.write().await;
                if let Some(state) = states.get_mut(&id) {
                    state.failure_count += 1;
                    state.last_failure = Some(Utc::now());
                    state.health_status = ActorHealthStatus::Critical;
                }
            }
            
            // Perform the actual restart
            info!("☢️ EMERGENCY RECOVERY: Force restarting actor '{}' due to communication failures", id);
            
            // Make sure actor is first stopped
            if let Err(e) = actor_ref.send(Message::Command(Command::Stop)).await {
                error!("Failed to stop actor during emergency recovery: {}", e);
                // Continue anyway - try to force start
            }
            
            // Wait longer than normal to ensure proper shutdown
            tokio::time::sleep(Duration::from_secs(1)).await;
            
            // Then try to restart it
            match actor_ref.send(Message::Command(Command::Start)).await {
                Ok(_) => {
                    info!("Successfully restarted actor '{}' during emergency recovery", id);
                    
                    // Update state to reflect recovery
                    let mut states = self.actor_states.write().await;
                    if let Some(state) = states.get_mut(&id) {
                        state.is_running = true;
                    }
                },
                Err(e) => {
                    error!("Failed to restart actor '{}' during emergency recovery: {}", id, e);
                }
            }
        }
        
        Ok(())
    }

    /// Create new communication channels for a specific actor type
    /// This is used when the MessageBus detects persistent failures
    pub async fn regenerate_communication_channels(&self, actor_type: &str) -> Result<(), Error> {
        info!("Regenerating communication channels for actor type: {}", actor_type);
        
        // This requires internal knowledge of how your channels are set up
        // In a more complete implementation, you would:
        // 1. Create new channels for the specific actor type
        // 2. Update the actor's references to use these new channels
        // 3. Re-subscribe the actor to the message bus with the new channels
        
        // For now, we'll just restart the actor which effectively recreates channels
        self.force_reconnect_actor(actor_type).await
    }

    /// Static method to find the supervisor actor from anywhere in the codebase
    pub async fn find_supervisor() -> Option<Arc<Self>> {
        // Use the global instance if it has been registered
        info!("Looking up supervisor actor for recovery");
        
        // SAFETY: This is safe because we're only reading the static value
        // and we're not modifying it. We'll only get None if it's not initialized yet
        let maybe_supervisor = unsafe { GLOBAL_SUPERVISOR_INSTANCE.as_ref().cloned() };
        
        if let Some(supervisor) = &maybe_supervisor {
            info!("Found global supervisor instance");
        } else {
            warn!("Unable to locate supervisor - automatic recovery limited");
        }
        
        maybe_supervisor
    }

    /// Establish connections between actors for event flow
    ///
    /// Sets up the subscription pipeline that defines how events flow through the system.
    /// This method handles the boilerplate of creating channels, setting up subscriptions,
    /// and spawning forwarder tasks.
    pub async fn establish_actor_connections(&self, connections: &[(&str, &str)]) -> Result<HashMap<String, bool>, Error> {
        info!("SupervisorActor: Establishing actor connections");
        let message_bus = self.message_bus.clone();
        let actors = self.actors.read().await;
        let mut success_map = HashMap::new();
        
        // Default settings for channels
        let buffer_size = 500;
        let max_retries = 3;
        
        // Create each connection defined in the connection map
        for (source, target) in connections {
            // Get the target actor reference
            if let Some(target_ref) = actors.get(*target) {
                // Create subscription pipeline from source event type to target actor
                match message_bus.create_subscription_pipeline(
                    *source,
                    Some(target_ref.clone()),
                    buffer_size,
                    max_retries
                ).await {
                    Ok(_) => {
                        let connection_id = format!("{}→{}", source, target);
                        success_map.insert(connection_id, true);
                        info!("Successfully established actor connection: {} → {}", source, target);
                    },
                    Err(e) => {
                        let connection_id = format!("{}→{}", source, target);
                        success_map.insert(connection_id, false);
                        error!("Failed to establish actor connection {} → {}: {}", source, target, e);
                        // Continue with other connections rather than failing
                    }
                }
            } else {
                warn!("Target actor '{}' not found - connection from {} skipped", target, source);
            }
        }
        
        // Log summary of connections
        let success_count = success_map.values().filter(|&v| *v).count();
        let total_count = connections.len();
        
        if success_count == total_count {
            info!("All actor connections ({}/{}) successfully established", success_count, total_count);
        } else {
            warn!("Some actor connections failed: {}/{} successful", success_count, total_count);
        }
        
        Ok(success_map)
    }
    
    /// Set up database subscriptions for event logging
    ///
    /// Creates subscriptions from all event sources to the database actor for event recording.
    pub async fn establish_database_connections(&self, database_actor_id: &str) -> Result<HashMap<String, bool>, Error> {
        info!("SupervisorActor: Setting up database event subscriptions");
        
        // Get the database actor reference
        let actors = self.actors.read().await;
        let database_ref = match actors.get(database_actor_id) {
            Some(actor_ref) => actor_ref.clone(),
            None => {
                error!("Database actor '{}' not found - cannot set up event logging", database_actor_id);
                return Err(Error::NotFound(format!("Database actor not found: {}", database_actor_id)));
            }
        };
        
        // Create database subscriptions through the message bus
        let message_bus = self.message_bus.clone();
        let result = message_bus.create_database_subscriptions(
            Some(database_ref),
            500, // Buffer size
            3    // Max retries
        ).await?;
        
        Ok(result)
    }
}

impl Actor for SupervisorActor {
    async fn start(&mut self) -> Result<(), Error> {
        self.running = true;
        info!("Starting SupervisorActor");
        
        // Start health monitoring if not already running
        let _ = self.watch_actors().await;
        
        Ok(())
    }

    fn stop(&mut self) -> Result<(), Error> {
        self.running = false;
        info!("Stopping SupervisorActor");
        Ok(())
    }

    async fn handle_message(&mut self, msg: Message) -> Result<(), Error> {
        match msg {
            Message::Event(_) => {
                // The MessageBus already handles event forwarding to subscribers
                // No need for the Supervisor to act as a message forwarder
                Ok(())
            },
            Message::Command(cmd) => match cmd {
                Command::Start => {
                    self.start().await
                },
                Command::Stop => {
                    self.stop()
                },
                Command::UpdateConfig(config) => {
                    // Update supervisor parameters from config
                    if let Some(log_level) = config.get("log_level").and_then(|v| v.as_str()) {
                        // Update logging level
                        info!("Updated log level to {}", log_level);
                    }
                    
                    // Update health check interval if specified
                    if let Some(interval) = config.get("health_check_interval").and_then(|v| v.as_u64()) {
                        self.health_check_interval = interval;
                        info!("Updated health check interval to {}s", interval);
                    }
                    
                    Ok(())
                },
                Command::MaintenanceDb => {
                    // Forward maintenance command to database actor
                    info!("Forwarding database maintenance command to database actor");
                    
                    match self.get_actor("database").await {
                        Some(db_actor) => {
                            db_actor.send(Message::Command(Command::MaintenanceDb)).await
                                .map_err(|e| Error::Task(format!("Failed to forward maintenance command: {}", e)))
                        },
                        None => {
                            error!("Cannot forward maintenance command: database actor not found");
                            Err(Error::NotFound("Database actor not found".to_string()))
                        }
                    }
                },
                Command::StartMaintenanceScheduler => {
                    // Forward maintenance scheduler command to database actor
                    info!("Forwarding start maintenance scheduler command to database actor");
                    
                    match self.get_actor("database").await {
                        Some(db_actor) => {
                            db_actor.send(Message::Command(Command::StartMaintenanceScheduler)).await
                                .map_err(|e| Error::Task(format!("Failed to forward maintenance scheduler command: {}", e)))
                        },
                        None => {
                            error!("Cannot forward maintenance scheduler command: database actor not found");
                            Err(Error::NotFound("Database actor not found".to_string()))
                        }
                    }
                },
            },
            Message::Query(query, responder) => match query {
                Query::GetStatus => {
                    let actors_count = self.actors.read().await.len();
                    let status = format!("SupervisorActor running: {}, monitoring {} actors", 
                                        self.running, actors_count);
                    responder.send(Ok(QueryResult::Status(status)))
                        .map_err(|e| Error::InvalidInput(format!("Failed to send status response: {:?}", e)))
                },
                Query::GetMetrics => {
                    // Return metrics with async actor count
                    let actors_count = self.actors.read().await.len();
                    let metrics = serde_json::json!({
                        "running": self.running,
                        "health_check_interval": self.health_check_interval,
                        "actor_count": actors_count,
                    });
                    responder.send(Ok(QueryResult::Metrics(metrics)))
                        .map_err(|e| Error::InvalidInput(format!("Failed to send metrics response: {:?}", e)))
                },
                _ => {
                    responder.send(Err(Error::InvalidInput(format!("Unsupported query type"))))
                        .map_err(|e| Error::InvalidInput(format!("Failed to send error response: {:?}", e)))
                }
            },
        }
    }
}

// Add Clone impl for SupervisorActor
impl Clone for SupervisorActor {
    fn clone(&self) -> Self {
        Self {
            actors: self.actors.clone(),
            message_bus: self.message_bus.clone(),
            running: self.running,
            health_check_interval: self.health_check_interval,
            actor_states: self.actor_states.clone(),
        }
    }
} 