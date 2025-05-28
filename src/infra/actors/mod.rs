mod bus;
pub use bus::MessageBus;

use chrono::{DateTime, Utc};
use log::{debug, error, info, trace};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

// Import domain types needed for events
use crate::domain::dex::{TransactionPriority, TransactionStatus};

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
    DexTransaction(DexTransactionEvent),
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
    NewTokenDiscovered {
        token_id: String,
        name: String,
        symbol: String,
        price: f64,
        source: String,
        timestamp: DateTime<Utc>,
    },
    MarketAnomalyDetected {
        token_id: String,
        anomaly_type: String,
        description: String,
        severity: String,
        timestamp: DateTime<Utc>,
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
    PositionOpened {
        token_id: String,
        position_id: String,
        amount: f64,
        price: f64,
        timestamp: DateTime<Utc>,
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
    RiskLimitExceeded {
        limit_type: String,
        current: f64,
        max: f64,
        timestamp: chrono::DateTime<chrono::Utc>,
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
    InsufficientFunds {
        token_id: String,
        current_balance_usd: f64,
        required_balance_usd: f64,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    TradeSizeAdjusted {
        token_id: String,
        original_size_usd: f64,
        adjusted_size_usd: f64,
        reason: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    TradingHalted {
        token_id: String,
        reason: String,
        timestamp: DateTime<Utc>,
    },
}

/// Execution-related events
#[derive(Debug, Clone)]
pub enum ExecutionEvent {
    OrderExecuted {
        canonical_token_id: String,
        provider_token_id: String,
        signal: crate::trading::strategy::Signal,
        executed_value_usd: f64,
        token_quantity: f64,
        price_per_token: f64,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    OrderFailed {
        token_id: String,
        order_id: Option<String>,
        reason: String,
        timestamp: DateTime<Utc>,
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

/// DEX Transaction related events
#[derive(Debug, Clone)]
pub enum DexTransactionEvent {
    Submitted {
        tx_id: String,
        submitted_details: Option<SubmittedTransactionInfo>,
        submission_time: DateTime<Utc>,
        priority: TransactionPriority,
    },
    StatusUpdated {
        status: TransactionStatus,
    },
}

/// Supporting struct for initial submission details
#[derive(Debug, Clone)]
pub struct SubmittedTransactionInfo {
    pub from_token_address: String,
    pub to_token_address: String,
    pub amount_in_f64: f64,
    pub slippage_tolerance: Option<f64>,
    pub price_limit: Option<f64>,
    pub dex_name: String,
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
    GetNativeBalance,
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
    NativeBalance(f64),
}

/// Event types for subscription
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum EventType {
    Market,
    Strategy,
    Risk,
    Execution,
    Database,
    DexTransaction,
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
            // Check global shutdown flag
            if crate::domain::trading::execution::bot::is_forced_shutdown() {
                info!("Actor {}: Global shutdown detected, exiting", task_name);
                break;
            }

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
