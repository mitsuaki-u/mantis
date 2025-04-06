mod market;
mod strategy;
mod risk;
mod execution;
mod database;
mod supervisor;

pub use market::MarketDataActor;
pub use strategy::StrategyActor;
pub use risk::RiskManagerActor;
pub use execution::ExecutionActor;
pub use database::DatabaseActor;
pub use supervisor::SupervisorActor;

use tokio::sync::{mpsc, oneshot};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::error::Error;
use chrono::{DateTime, Utc};
use std::fmt;
use serde_json::Value;
use log::{error, info, warn, debug, trace};
use lazy_static;

/// Base trait for all actors in the system
pub trait Actor: Send + Sync {
    fn start(&mut self) -> Result<(), Error>;
    fn stop(&mut self) -> Result<(), Error>;
    fn handle_message(&mut self, msg: Message) -> Result<(), Error>;
}

/// Reference to an actor that can be used to send messages
#[derive(Clone)]
pub struct ActorRef {
    pub id: String,
    sender: mpsc::Sender<Message>,
}

impl ActorRef {
    pub fn new(id: String, sender: mpsc::Sender<Message>) -> Self {
        Self { id, sender }
    }

    pub async fn send(&self, msg: Message) -> Result<(), Error> {
        self.sender.send(msg).await.map_err(|e| Error::Task(format!("Failed to send message: {}", e)))
    }
}

/// Core message type for actor communication
#[derive(Debug)]
pub enum Message {
    Event(Event),
    Command(Command),
    Query(Query, oneshot::Sender<Result<QueryResult, Error>>),
}

/// Event types that can be published to the message bus
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
        timestamp: DateTime<Utc>,
    },
    MarketDataError(String),
}

/// Strategy-related events
#[derive(Debug, Clone)]
pub enum StrategyEvent {
    Signal {
        token_id: String,
        signal: crate::trading::strategy::Signal,
        confidence: f64,
        timestamp: DateTime<Utc>,
    },
}

/// Risk-related events
#[derive(Debug, Clone)]
pub enum RiskEvent {
    RiskAssessment {
        token_id: String,
        signal: crate::trading::strategy::Signal,
        confidence: f64,
        position_size: f64,
        timestamp: DateTime<Utc>,
    },
    RiskLimitExceeded {
        limit_type: String,
        current: f64,
        max: f64,
        timestamp: DateTime<Utc>,
    },
    PositionClosed {
        token_id: String,
        pnl: f64,
        timestamp: DateTime<Utc>,
    },
    RiskMetricsUpdate {
        daily_loss: f64,
        drawdown: f64,
        timestamp: DateTime<Utc>,
    },
}

/// Execution-related events
#[derive(Debug, Clone)]
pub enum ExecutionEvent {
    OrderExecuted {
        token_id: String,
        signal: crate::trading::strategy::Signal,
        size: f64,
        price: f64,
        timestamp: DateTime<Utc>,
    },
    PositionUpdate {
        token_id: String,
        current_price: f64,
        pnl: f64,
        timestamp: DateTime<Utc>,
    },
}

/// Database-related events
#[derive(Debug, Clone)]
pub enum DatabaseEvent {
    TokenUpdated {
        token_id: String,
        timestamp: DateTime<Utc>,
    },
    TradeExecuted {
        token_id: String,
        price: f64,
        size: f64,
        is_buy: bool,
        timestamp: DateTime<Utc>,
    },
    PositionUpdated {
        token_id: String,
        price: f64,
        pnl: f64,
        timestamp: DateTime<Utc>,
    },
}

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
    GetMetrics,
}

/// Results that can be returned from queries
#[derive(Debug)]
pub enum QueryResult {
    Status(String),
    Metrics(Value),
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

/// Message bus for actor communication
pub struct MessageBus {
    subscribers: Arc<RwLock<HashMap<String, Vec<SubscriberInfo>>>>
}

/// Information about a subscriber with a status flag
struct SubscriberInfo {
    sender: mpsc::Sender<Event>,
    last_failed: Option<std::time::Instant>,
    consecutive_failures: usize,
}

impl SubscriberInfo {
    fn new(sender: mpsc::Sender<Event>) -> Self {
        Self {
            sender,
            last_failed: None,
            consecutive_failures: 0,
        }
    }
}

// Add a lazy static singleton for the MessageBus
lazy_static::lazy_static! {
    static ref GLOBAL_MESSAGE_BUS: Arc<MessageBus> = {
        info!("🌐 Initializing global MessageBus singleton");
        Arc::new(MessageBus::new_internal())
    };
}

impl MessageBus {
    /// Create a new MessageBus - private implementation
    fn new_internal() -> Self {
        debug!("📨 Creating new MessageBus for actor communication (internal)");
        debug!("Initializing MessageBus with empty subscriber map");
        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get the global singleton instance of the MessageBus
    pub fn instance() -> Arc<MessageBus> {
        let bus_id = format!("{:p}", Arc::as_ptr(&GLOBAL_MESSAGE_BUS));
        info!("🌐 Retrieving global MessageBus instance [id: {}]", bus_id);
        trace!("Global MessageBus pointer address: {:p}", Arc::as_ptr(&GLOBAL_MESSAGE_BUS));
        GLOBAL_MESSAGE_BUS.clone()
    }

    /// Create a new MessageBus - for backwards compatibility
    /// Note: This is deprecated, use MessageBus::instance() instead
    pub fn new() -> Self {
        warn!("⚠️ Using deprecated MessageBus::new() - consider using MessageBus::instance() for shared bus");
        info!("📨 Creating new MessageBus for actor communication");
        debug!("Initializing MessageBus with empty subscriber map");
        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Perform maintenance on the subscribers list
    /// Removes any subscribers that have failed too many times
    async fn perform_maintenance(&self) {
        let mut subscribers = self.subscribers.write().await;
        
        for (event_type, subs) in subscribers.iter_mut() {
            let initial_count = subs.len();
            // Keep only subscribers that haven't failed too many times
            subs.retain(|sub| {
                // If we've had 10+ consecutive failures and the last one was recent, remove the subscriber
                if sub.consecutive_failures >= 10 {
                    if let Some(last_failed) = sub.last_failed {
                        // Only remove if the failure was in the last 30 seconds
                        if last_failed.elapsed() < std::time::Duration::from_secs(30) {
                            debug!("🧹 Removing failed subscriber from {} event bus (consecutive failures: {})", 
                                event_type, sub.consecutive_failures);
                            return false;
                        }
                    }
                }
                true
            });
            
            let removed_count = initial_count - subs.len();
            if removed_count > 0 {
                info!("🧹 MessageBus maintenance: Removed {} dead subscriber(s) from {} events", 
                    removed_count, event_type);
            }
        }
    }

    pub async fn subscribe(&self, event_type: String, sender: mpsc::Sender<Event>) -> Result<(), Error> {
        trace!("MessageBus: Adding new subscriber for event type: {}", event_type);
        
        // Perform maintenance first to clean up any dead subscribers
        self.perform_maintenance().await;
        
        let mut subscribers = self.subscribers.write().await;
        let count_before = subscribers.get(&event_type).map_or(0, |v| v.len());
        
        // Create a subscriber info wrapper
        let subscriber = SubscriberInfo::new(sender);
        
        subscribers.entry(event_type.clone()).or_insert_with(Vec::new).push(subscriber);
        let count_after = subscribers.get(&event_type).map_or(0, |v| v.len());
        info!("📌 Subscription added for {} events (total subscribers: {})", event_type, count_after);
        
        if count_after > count_before {
            debug!("🔗 New subscription created: {} event type → subscriber #{}", event_type, count_after);
            trace!("MessageBus: Successfully added new subscriber for '{}' (#{} for this event type)", 
                  event_type, count_after);
        } else {
            warn!("⚠️ MessageBus: Failed to add new subscriber for '{}'", event_type);
        }
        
        Ok(())
    }

    pub async fn unsubscribe(&self, event_type: &str, sender: &mpsc::Sender<Event>) -> Result<(), Error> {
        trace!("MessageBus: Removing subscriber for event type: {}", event_type);
        let mut subscribers = self.subscribers.write().await;
        if let Some(subs) = subscribers.get_mut(event_type) {
            let count_before = subs.len();
            subs.retain(|s| !std::ptr::eq(&s.sender, sender));
            let count_after = subs.len();
            info!("🔌 Unsubscribed from {} events (remaining subscribers: {})", event_type, count_after);
            
            if count_before > count_after {
                debug!("✂️ Removed subscription from {} event type", event_type);
                trace!("MessageBus: Successfully removed subscriber for '{}', {} remaining", event_type, count_after);
            } else {
                warn!("⚠️ MessageBus: No matching subscriber found for '{}'", event_type);
            }
        } else {
            warn!("⚠️ MessageBus: No subscribers found for event type: {}", event_type);
        }
        Ok(())
    }

    /// For debugging: Get the number of subscribers for a specific event type
    pub async fn debug_subscriber_count(&self, event_type: &str) -> usize {
        let subscribers = self.subscribers.read().await;
        subscribers.get(event_type).map_or(0, |subs| subs.len())
    }

    pub async fn publish(&self, event: Event) -> Result<(), Error> {
        let event_type = match &event {
            Event::Market(_) => "market",
            Event::Strategy(_) => "strategy",
            Event::Risk(_) => "risk",
            Event::Execution(_) => "execution",
            Event::Database(_) => "database",
        };
        
        trace!("MessageBus: Beginning to publish {} event", event_type);
        
        // Perform maintenance first to clean up any dead subscribers
        self.perform_maintenance().await;
        
        // Debug log the current subscribers at higher log level for troubleshooting
        let subscriber_count = self.subscribers.read().await.get(event_type).map_or(0, |s| s.len());
        let bus_id = format!("{:p}", self as *const MessageBus);
        
        // Enhanced debug info at INFO level for troubleshooting
        if subscriber_count > 0 {
            info!("🔔 MessageBus [{}]: Publishing {} event to {} subscribers", 
                 bus_id, event_type, subscriber_count);
        } else {
            warn!("⚠️ MessageBus [{}]: No subscribers found for {} events - message will be dropped", 
                 bus_id, event_type);
        }
        
        // Detailed subscriber tracing
        if let Some(subs) = self.subscribers.read().await.get(event_type) {
            trace!("MessageBus subscribers for {} events: {} channels", event_type, subs.len());
            for (i, _) in subs.iter().enumerate() {
                trace!("MessageBus: Subscriber #{} for {} is active", i+1, event_type);
            }
        }
        
        // Add detailed logging for event publishing
        match &event {
            Event::Market(MarketEvent::PriceUpdate { token_id, price, volume, timestamp }) => {
                info!("📬 MessageBus: Publishing market price update for {}: ${:.4} at {}", 
                     token_id, price, timestamp);
                debug!("📊 Price update: {} at ${:.4} with volume ${:.2}M", 
                      token_id, price, volume.unwrap_or(0.0) / 1_000_000.0);
                
                let subscriber_count = self.subscribers.read().await.get(event_type).map_or(0, |s| s.len());
                if subscriber_count > 0 {
                    debug!("📨 Sending to {} subscribers of market events", subscriber_count);
                    trace!("Market event will be routed to {} subscribers: {:?}", 
                          event_type, self.subscribers.read().await.get(event_type).map(|s| s.len()));
                } else {
                    warn!("⚠️ No subscribers for market events! Price update for {} will not be delivered", token_id);
                }
            },
            Event::Strategy(StrategyEvent::Signal { token_id, signal, confidence, timestamp }) => {
                info!("📬 MessageBus: Publishing strategy signal {:?} for {} with {:.1}% confidence at {}", 
                     signal, token_id, confidence * 100.0, timestamp);
                debug!("🎯 {} signal for {} with {:.1}% confidence", 
                      format!("{:?}", signal).to_uppercase(), token_id, confidence * 100.0);
                
                let subscriber_count = self.subscribers.read().await.get(event_type).map_or(0, |s| s.len());
                if subscriber_count > 0 {
                    debug!("📨 Sending to {} subscribers of strategy events", subscriber_count);
                    trace!("Strategy event will be routed to {} subscribers: {:?}", 
                          event_type, self.subscribers.read().await.get(event_type).map(|s| s.len()));
                } else {
                    warn!("⚠️ No subscribers for strategy events! Signal for {} will not be delivered", token_id);
                }
            },
            Event::Risk(RiskEvent::RiskAssessment { token_id, signal, confidence, position_size, timestamp }) => {
                info!("📬 MessageBus: Publishing risk assessment for {} with signal {:?} at {}", 
                     token_id, signal, timestamp);
                debug!("⚖️ Risk assessment: {} signal for {} with size ${:.2} and {:.1}% confidence", 
                      format!("{:?}", signal).to_uppercase(), token_id, position_size, confidence * 100.0);
                
                let subscriber_count = self.subscribers.read().await.get(event_type).map_or(0, |s| s.len());
                if subscriber_count > 0 {
                    debug!("📨 Sending to {} subscribers of risk events", subscriber_count);
                } else {
                    warn!("⚠️ No subscribers for risk events! Assessment for {} will not be delivered", token_id);
                }
            },
            Event::Execution(ExecutionEvent::OrderExecuted { token_id, signal, size, price, timestamp }) => {
                info!("📬 MessageBus: Publishing order execution for {} at ${:.4} (size: {:.4}) at {}", 
                     token_id, price, size, timestamp);
                debug!("💰 Order executed: {} {} at ${:.4} with size ${:.2}", 
                      format!("{:?}", signal).to_uppercase(), token_id, price, size);
                
                let subscriber_count = self.subscribers.read().await.get(event_type).map_or(0, |s| s.len());
                if subscriber_count > 0 {
                    debug!("📨 Sending to {} subscribers of execution events", subscriber_count);
                } else {
                    warn!("⚠️ No subscribers for execution events! Execution for {} will not be delivered", token_id);
                }
            },
            _ => {
                debug!("📬 MessageBus: Publishing {} event to {} subscribers", 
                       event_type,
                       self.subscribers.read().await.get(event_type).map_or(0, |s| s.len()));
                trace!("Generic event of type {} being published", event_type);
            }
        }

        // Get mutable reference to subscribers to update failure status
        let mut subscribers = self.subscribers.write().await;
        let subscribers_opt = subscribers.get_mut(event_type);
        
        if let Some(subs) = subscribers_opt {
            if subs.is_empty() {
                debug!("⚠️ MessageBus: No subscribers for {} events - message dropped", event_type);
                return Ok(());
            }
            
            // Send to all subscribers with the lock held, but for a short time
            trace!("MessageBus: Sending event to {} subscribers", subs.len());
            let mut success_count = 0;
            let mut failure_count = 0;
            
            // Use try_send instead of send, which is non-blocking
            let start_time = std::time::Instant::now();
            let subs_len = subs.len();  // Cache the length to avoid immutable borrow

            for (index, sub) in subs.iter_mut().enumerate() {
                trace!("MessageBus: Attempting to send {} event to subscriber {}/{}", 
                      event_type, index + 1, subs_len);
                
                // Use try_send which doesn't block, so we don't deadlock while holding the write lock
                match sub.sender.try_send(event.clone()) {
                    Ok(_) => {
                        // Reset consecutive failures on success
                        if sub.consecutive_failures > 0 {
                            sub.consecutive_failures = 0;
                            sub.last_failed = None;
                        }
                        success_count += 1;
                    },
                    Err(e) => {
                        // Only log if this is a new failure
                        if sub.consecutive_failures == 0 {
                            warn!("❌ Failed to send event to subscriber {}/{}: {}", 
                                index + 1, subs_len, e);
                        }
                        
                        // Update failure tracking
                        sub.consecutive_failures += 1;
                        sub.last_failed = Some(std::time::Instant::now());
                        failure_count += 1;
                    }
                }
            }
            let delivery_time = start_time.elapsed();
            
            if failure_count > 0 {
                warn!("⚠️ MessageBus: Failed to deliver {} event to {} out of {} subscribers", 
                     event_type, failure_count, subs_len);
            }
            
            if success_count > 0 {
                debug!("✅ MessageBus: {} event delivered to {}/{} subscribers in {:.2?}", 
                      event_type, success_count, subs_len, delivery_time);
            } else if subs_len > 0 {
                error!("🚫 MessageBus: Failed to deliver {} event to ANY subscribers!", event_type);
            }
        } else {
            debug!("⚠️ MessageBus: No subscribers for {} events - message dropped", event_type);
        }
        
        Ok(())
    }
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::Market(e) => write!(f, "Market: {:?}", e),
            Event::Strategy(e) => write!(f, "Strategy: {:?}", e),
            Event::Risk(e) => write!(f, "Risk: {:?}", e),
            Event::Execution(e) => write!(f, "Execution: {:?}", e),
            Event::Database(e) => write!(f, "Database: {:?}", e),
        }
    }
}

/// Helper function to create a new actor task
pub async fn spawn_actor<A: Actor + Send + 'static>(
    mut actor: A,
    mut receiver: mpsc::Receiver<Message>,
    id: String,
) -> Result<ActorRef, Error> {
    // Create a new channel to get a sender for the ActorRef
    let (sender, _) = mpsc::channel(100);
    let actor_ref = ActorRef::new(id.clone(), sender);
    
    // Spawn a task to handle incoming messages
    tokio::spawn(async move {
        debug!("Actor task started for {}", id);
        while let Some(msg) = receiver.recv().await {
            if let Err(e) = actor.handle_message(msg) {
                error!("Error handling message in {}: {}", id, e);
            }
        }
        warn!("Actor task for {} has terminated - channel closed", id);
    });

    Ok(actor_ref)
}

/// Helper function to spawn an actor task without creating an ActorRef
pub async fn spawn_actor_task<A: Actor + Send + 'static>(
    mut actor: A,
    mut receiver: mpsc::Receiver<Message>,
    id: &str,
) -> Result<(), Error> {
    let id = id.to_string();  // Clone the string to avoid lifetime issues
    
    tokio::spawn(async move {
        debug!("Actor task started for {}", id);
        while let Some(msg) = receiver.recv().await {
            if let Err(e) = actor.handle_message(msg) {
                error!("Error handling message in {}: {}", id, e);
            }
        }
        warn!("Actor task for {} has terminated - channel closed", id);
    });

    Ok(())
} 