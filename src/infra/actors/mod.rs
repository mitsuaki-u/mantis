mod bus;
pub use bus::MessageBus;

use chrono::{DateTime, Utc};
use log::{debug, error, info, trace, warn};
use serde_json::Value;
use std::sync::Arc;
use std::sync::Once;
use tokio::sync::{mpsc, oneshot};

/// Base trait for all actors in the system
pub trait Actor: Send + Sync + 'static {
    fn start(
        &mut self,
    ) -> impl std::future::Future<Output = Result<(), crate::error::Error>> + Send;
    fn stop(&mut self) -> Result<(), crate::error::Error>;
    fn handle_message(
        &mut self,
        msg: Message,
    ) -> impl std::future::Future<Output = Result<(), crate::error::Error>> + Send;
}

/// Events that can be passed between actors
#[derive(Debug, Clone)]
pub enum Event {
    Market(MarketEvent),
    Strategy(StrategyEvent),
    Risk(RiskEvent),
    Execution(ExecutionEvent),
    Database(DatabaseEvent),
}

/// Market-related events
#[derive(Debug, Clone)]
pub enum MarketEvent {
    PriceUpdate {
        token_id: String,
        price: f64,
        volume: Option<f64>,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    VolumeUpdate {
        token_id: String,
        volume: f64,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    MarketDataError(String),
    StatusCheck,
    /// Special event for recovery to identify reconnection requests
    SupervisorRecoveryRequest(String),
}

/// Strategy-related events
#[derive(Debug, Clone)]
pub enum StrategyEvent {
    Signal {
        token_id: String,
        signal: crate::trading::strategy::Signal,
        confidence: f64,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    StatusCheck,
}

/// Risk-related events
#[derive(Debug, Clone)]
pub enum RiskEvent {
    RiskAssessment {
        token_id: String,
        signal: crate::trading::strategy::Signal,
        confidence: f64,
        position_size: f64,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    RiskLimitExceeded {
        limit_type: String,
        current: f64,
        max: f64,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    PositionClosed {
        token_id: String,
        pnl: f64,
        timestamp: chrono::DateTime<chrono::Utc>,
        entry_price: f64,
        exit_price: f64,
        size: f64,
        entry_time: chrono::DateTime<chrono::Utc>,
        delete_position: bool,
    },
    RiskMetricsUpdate {
        daily_loss: f64,
        drawdown: f64,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    InvalidSignalReceived {
        token_id: String,
        reason: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    StatusCheck,
}

/// Execution-related events
#[derive(Debug, Clone)]
pub enum ExecutionEvent {
    OrderExecuted {
        token_id: String,
        signal: crate::trading::strategy::Signal,
        size: f64,
        price: f64,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    PositionUpdate {
        token_id: String,
        current_price: f64,
        pnl: f64,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    StatusCheck,
}

/// Database-related events
#[derive(Debug, Clone)]
pub enum DatabaseEvent {
    TokenUpdated {
        token_id: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    TradeExecuted {
        token_id: String,
        price: f64,
        size: f64,
        is_buy: bool,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    PositionUpdated {
        token_id: String,
        price: f64,
        pnl: f64,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    PositionClosed {
        token_id: String,
        exit_price: f64,
        entry_price: f64,
        size: f64,
        timestamp: chrono::DateTime<chrono::Utc>,
        deleted: bool,
    },
    StatusCheck,
}

/// Commands that can be sent to actors
#[derive(Debug)]
pub enum Command {
    Start,
    Stop,
    UpdateConfig(Value),
    MaintenanceDb,
    StartMaintenanceScheduler,
}

/// Queries that can be sent to actors
#[derive(Debug)]
pub enum Query {
    GetStatus,
    GetMetrics,
    GetMaintenanceStatus,
}

/// Results that can be returned from queries
#[derive(Debug)]
pub enum QueryResult {
    Status(String),
    Metrics(Value),
    MaintenanceStatus {
        last_run: Option<DateTime<Utc>>,
        next_run: Option<DateTime<Utc>>,
    },
    TradeHistory(Vec<crate::infra::db::repositories::trade::CompletedTrade>),
    TradingStats(crate::infra::db::repositories::trade::TradingStats),
    Success,
}

/// Event types for subscription
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum EventType {
    Market,
    Strategy,
    Risk,
    Execution,
    Database,
}

/// Messages that can be sent to actors
#[derive(Debug)]
pub enum Message {
    Command(Command),
    Query(
        Query,
        oneshot::Sender<Result<QueryResult, crate::error::Error>>,
    ),
    Event(Event),
}

/// A reference to an actor that can be used to send messages
pub struct ActorRef {
    sender: mpsc::Sender<Message>,
}

impl ActorRef {
    /// Create a new actor reference
    pub fn new(sender: mpsc::Sender<Message>) -> Self {
        Self { sender }
    }

    /// Send a message to the actor
    pub async fn send(&self, msg: Message) -> Result<(), crate::error::Error> {
        self.sender.send(msg).await.map_err(|e| {
            crate::error::Error::Task(format!("Failed to send message to actor: {}", e))
        })
    }
}

impl Clone for ActorRef {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}

/// Create a channel for communication with an actor
pub fn create_actor_channel() -> (mpsc::Sender<Message>, mpsc::Receiver<Message>) {
    mpsc::channel(100)
}

/// Spawn an actor in a separate task
pub async fn spawn_actor<A: Actor>(
    mut actor: A,
    name: String,
) -> Result<ActorRef, crate::error::Error> {
    let (sender, mut receiver) = create_actor_channel();
    let actor_ref = ActorRef::new(sender.clone());

    debug!("Spawning actor: {}", name);

    // Clone the name since it will be moved into the task
    let task_name = name.clone();

    // Spawn a new task for the actor
    tokio::spawn(async move {
        debug!("Actor task started: {}", task_name);

        // Process messages as they arrive
        while let Some(msg) = receiver.recv().await {
            trace!("Received message for actor {}: {:?}", task_name, msg);

            match actor.handle_message(msg).await {
                Ok(_) => {}
                Err(e) => {
                    error!("Error handling message in {} actor: {}", task_name, e);
                }
            }
        }

        debug!("Actor task stopped: {}", task_name);
    });

    debug!("Spawned actor: {}", name);
    Ok(actor_ref)
}

// Re-export actor implementations
pub mod database;
pub mod execution;
pub mod market;
pub mod risk;
pub mod strategy;
pub mod supervisor;

pub use database::DatabaseActor;
pub use execution::ExecutionActor;
pub use market::MarketDataActor;
pub use risk::RiskManagerActor;
pub use strategy::StrategyActor;
pub use supervisor::SupervisorActor;
