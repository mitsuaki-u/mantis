use crate::application::errors::Result;
use async_trait::async_trait;
use log::info;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::oneshot;

use crate::events::{Event, EventType};

/// Commands that can be sent to actors
#[derive(Debug)]
pub enum Command {
    Start,
    Stop,
    UpdateConfig(Value),
}

/// Queries that can be sent to actors
#[derive(Debug)]
pub enum Query {
    GetStatus,
}

/// Results that can be returned from queries
#[derive(Debug)]
pub enum QueryResult {
    Status(String),
}

/// Messages that can be sent to actors
#[derive(Debug)]
pub enum Message {
    Command(Command),
    Query(Query, oneshot::Sender<Result<QueryResult>>),
    Event(Box<Event>),
}

/// A reference to an actor that can be used to send messages
pub type ActorRef = tokio::sync::mpsc::UnboundedSender<Message>;

/// Health status of an actor
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HealthStatus {
    Created,
    Healthy,
    Degraded,
    Critical,
    Stopped,
}

/// Lifecycle state of an actor
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LifecycleState {
    Created,
    Initialized,
    Running,
    Stopped,
}

/// Shared state for all actors with lifecycle management
#[derive(Debug, Clone)]
pub struct ActorState {
    pub name: String,
    pub running: bool,
    pub health_status: HealthStatus,
    pub lifecycle_state: LifecycleState,
}

impl ActorState {
    pub fn new(name: String) -> Self {
        Self {
            name,
            running: false,
            health_status: HealthStatus::Created,
            lifecycle_state: LifecycleState::Created,
        }
    }

    pub fn start(&mut self) {
        self.running = true;
        self.lifecycle_state = LifecycleState::Running;
        self.health_status = HealthStatus::Healthy;
    }

    pub fn stop(&mut self) {
        self.running = false;
        self.lifecycle_state = LifecycleState::Stopped;
        self.health_status = HealthStatus::Stopped;
    }

    pub fn record_activity(&mut self) {
        // Placeholder for future activity tracking if needed
    }

    pub fn record_error(&mut self) {
        // Mark health as degraded on errors
        if self.health_status == HealthStatus::Healthy {
            self.health_status = HealthStatus::Degraded;
        } else if self.health_status == HealthStatus::Degraded {
            self.health_status = HealthStatus::Critical;
        }
    }
}

/// Simplified Actor trait that consolidates all functionality
#[async_trait]
pub trait Actor: Send + Sync + 'static {
    // ---- Basic Actor Info ----
    fn name(&self) -> &str;
    fn is_running(&self) -> bool;

    // ---- Lifecycle Methods (with sensible defaults) ----
    async fn start(&mut self) -> Result<()> {
        info!("Starting actor: {}", self.name());
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Stopping actor: {}", self.name());
        Ok(())
    }

    // ---- Health Status (basic only) ----
    async fn health_status(&self) -> HealthStatus {
        if self.is_running() {
            HealthStatus::Healthy
        } else {
            HealthStatus::Stopped
        }
    }

    // ---- Main Message Entry Point ----
    async fn handle_message(&mut self, msg: Message) -> Result<()> {
        match msg {
            Message::Command(cmd) => self.handle_command(cmd).await,
            Message::Query(query, responder) => self.handle_query(query, responder).await,
            Message::Event(event) => self.handle_event(*event).await,
        }
    }

    // ---- Message Handlers (can be overridden by specific actors) ----
    async fn handle_command(&mut self, cmd: Command) -> Result<()> {
        match cmd {
            Command::Start => self.start().await,
            Command::Stop => self.stop().await,
            Command::UpdateConfig(_config) => {
                info!("Actor {} received UpdateConfig command - override handle_command to implement config updates", self.name());
                Ok(())
            }
        }
    }

    async fn handle_query(
        &mut self,
        query: Query,
        responder: oneshot::Sender<Result<QueryResult>>,
    ) -> Result<()> {
        let result = match query {
            Query::GetStatus => Ok(QueryResult::Status(format!(
                "{} running: {}",
                self.name(),
                self.is_running()
            ))),
        };

        let _ = responder.send(result);
        Ok(())
    }

    async fn handle_event(&mut self, _event: Event) -> Result<()> {
        // Default: ignore all events - specific actors can override this
        Ok(())
    }

    fn supported_event_types(&self) -> Vec<EventType> {
        vec![]
    }
}

// ---- Optional Specialized Traits for Complex Actors ----
// These are only implemented when actually needed, not required by default

/// Lifecycle management capabilities (optional)
#[async_trait]
pub trait LifecycleActor {
    async fn initialize(&mut self) -> Result<()>;
    async fn cleanup(&mut self) -> Result<()>;
    fn lifecycle_state(&self) -> LifecycleState;
}

/// Spawn an actor and return a reference to it
pub async fn spawn_actor<T>(actor: T, name: String) -> Result<ActorRef>
where
    T: Actor + Send + 'static,
{
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    let actor_ref = tx.clone();

    // Spawn the actor task
    tokio::spawn(async move {
        let mut actor = actor;

        // Actor will be started by supervisor - don't start automatically
        log::debug!("Actor {} spawned (waiting for Start command)", name);

        // Message processing loop
        while let Some(message) = rx.recv().await {
            match message {
                Message::Command(Command::Stop) => {
                    log::info!("Stopping actor {}", name);
                    if let Err(e) = actor.stop().await {
                        log::error!("Error stopping actor {}: {}", name, e);
                    }
                    break;
                }
                _ => {
                    // Route all other messages through the actor's handle_message method
                    if let Err(e) = actor.handle_message(message).await {
                        log::error!("Error handling message in actor {}: {}", name, e);
                    }
                }
            }
        }

        log::info!("Actor {} stopped", name);
    });

    Ok(actor_ref)
}
