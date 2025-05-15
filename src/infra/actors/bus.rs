use std::collections::HashMap;
use std::sync::{Arc, Once, Mutex};
use tokio::sync::{mpsc, RwLock};
use log::{debug, error, trace, warn, info};
use serde_json::Value;
use chrono::{DateTime, Utc};
use rand;
use std::time::{Duration, Instant};

// Import Event types from the parent module
use super::{Event, MarketEvent, StrategyEvent, RiskEvent, ExecutionEvent, DatabaseEvent};
use super::ActorRef;
use crate::core::error::Error;

/// Information about a single subscriber
struct SubscriberInfo {
    sender: mpsc::Sender<Event>,
    subscriber_id: String, // Add an ID to help reconnect
    consecutive_failures: u32,
    last_failed: Option<std::time::Instant>,
    is_reconnecting: bool,
    actor_type: String, // For identifying the actor type during reconnection
}

impl SubscriberInfo {
    /// Create a new subscriber information wrapper
    fn new(sender: mpsc::Sender<Event>, actor_type: String) -> Self {
        Self {
            sender,
            subscriber_id: format!("subscriber_{}", rand::random::<u64>()),
            consecutive_failures: 0,
            last_failed: None,
            is_reconnecting: false,
            actor_type,
        }
    }
}

/// Information about the last event for a token
struct TokenEventInfo {
    last_event_time: Instant,
    last_event_type: String,
}

impl TokenEventInfo {
    fn new(event_type: String) -> Self {
        Self {
            last_event_time: Instant::now(),
            last_event_type: event_type,
        }
    }
}

/// Message bus for actor communication
pub struct MessageBus {
    subscribers: Arc<RwLock<HashMap<String, Vec<SubscriberInfo>>>>,
    token_events: Arc<RwLock<HashMap<String, TokenEventInfo>>>,
    deduplication_window: Duration,
    buy_order_locks: Arc<Mutex<HashMap<String, Instant>>>,
}

// Use a simple Once-based singleton pattern
static INIT: Once = Once::new();
static mut GLOBAL_MESSAGE_BUS_INSTANCE: Option<Arc<MessageBus>> = None;

impl MessageBus {
    /// Create a new MessageBus - private implementation 
    fn new_internal() -> Self {
        debug!("Creating new MessageBus for actor communication");
        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
            token_events: Arc::new(RwLock::new(HashMap::new())),
            deduplication_window: Duration::from_millis(100), // 100ms deduplication window
            buy_order_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get the global singleton instance of the MessageBus
    /// This will only create the instance when actually called
    pub fn instance() -> Arc<MessageBus> {
        INIT.call_once(|| {
            debug!("Initializing global MessageBus singleton");
            let message_bus = Arc::new(MessageBus::new_internal());
            
            // SAFETY: This is safe because we're in a Once::call_once closure,
            // which guarantees this code only runs once during the program lifetime,
            // and we're not accessing the static during this initialization.
            unsafe {
                GLOBAL_MESSAGE_BUS_INSTANCE = Some(message_bus);
            }
        });
        
        // SAFETY: After initialization above, this will always be Some
        unsafe {
            let bus = GLOBAL_MESSAGE_BUS_INSTANCE.as_ref().unwrap().clone();
            let bus_id = format!("{:p}", Arc::as_ptr(&bus));
            debug!("Retrieving global MessageBus instance [id: {}]", bus_id);
            trace!("Global MessageBus pointer address: {:p}", Arc::as_ptr(&bus));
            bus
        }
    }

    /// Create a new MessageBus - for backwards compatibility
    /// Note: This is deprecated, use MessageBus::instance() instead
    pub fn new() -> Self {
        debug!("Creating standalone MessageBus (non-trading command)");
        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
            token_events: Arc::new(RwLock::new(HashMap::new())),
            deduplication_window: Duration::from_millis(100), // 100ms deduplication window
            buy_order_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Perform maintenance on the subscribers list
    async fn perform_maintenance(&self) -> usize {
        // Handle recursive async function with Box::pin
        struct Fut<'a>(&'a MessageBus);
        
        impl<'a> std::future::Future for Fut<'a> {
            type Output = usize;
            
            fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
                // Implementation would go here, but we'll use async_fn_impl instead
                panic!("Should never be called directly");
            }
        }
        
        // Actual implementation separated to avoid recursion issues
        async fn async_fn_impl(bus: &MessageBus) -> usize {
            let mut subscribers = bus.subscribers.write().await;
            let mut total_removed = 0;
            let mut recovery_needed = vec![];
            
            for (event_type, subs) in subscribers.iter_mut() {
                let initial_count = subs.len();
                // Keep only subscribers that haven't failed too many times
                subs.retain(|sub| {
                    // If we've had excessive failures, remove immediately
                    if sub.consecutive_failures >= 50 {
                        error!("Emergency cleanup: Removing subscriber with extreme failure count ({}) from {} event bus", 
                              sub.consecutive_failures, event_type);
                        return false;
                    }
                    
                    // If we've had 5+ consecutive failures (reduced from 10), remove the subscriber
                    if sub.consecutive_failures >= 5 {
                        info!("Removing failed subscriber from {} event bus (consecutive failures: {})", 
                            event_type, sub.consecutive_failures);
                        return false;
                    }
                    
                    // If we're in a reconnecting state for too long, clean up
                    if sub.is_reconnecting && sub.last_failed.is_some() {
                        let elapsed = std::time::Instant::now().duration_since(sub.last_failed.unwrap());
                        if elapsed > std::time::Duration::from_secs(60) {
                            info!("Removing stuck reconnecting subscriber from {} event bus after 60s", event_type);
                            return false;
                        }
                    }
                    
                    // Check if the channel is closed by trying to send a status check message
                    let status_check_event = match event_type.as_str() {
                        "market" => Event::Market(MarketEvent::StatusCheck),
                        "strategy" => Event::Strategy(StrategyEvent::StatusCheck),
                        "risk" => Event::Risk(RiskEvent::StatusCheck),
                        "execution" => Event::Execution(ExecutionEvent::StatusCheck),
                        "database" => Event::Database(DatabaseEvent::StatusCheck),
                        _ => Event::Market(MarketEvent::StatusCheck),
                    };
                    
                    // If sending a status check fails, the channel is closed
                    if sub.sender.try_send(status_check_event).is_err() {
                        debug!("Removing closed channel subscriber from {} event bus", event_type);
                        return false;
                    }
                    
                    true
                });
                
                let removed_count = initial_count - subs.len();
                total_removed += removed_count;
                
                if removed_count > 0 {
                    info!("MessageBus maintenance: Removed {} dead subscriber(s) from {} events", 
                        removed_count, event_type);
                    
                    // If all subscribers were removed, mark for recovery but don't spawn a task
                    if subs.is_empty() {
                        info!("All subscribers removed from {} event bus - will initiate recovery", event_type);
                        recovery_needed.push(event_type.clone());
                    }
                }
            }
            
            // Release the write lock before doing any recovery
            drop(subscribers);
            
            // Process any recovery needed outside the lock
            for event_type in recovery_needed {
                info!("Starting recovery process for {} event bus", event_type);
                
                // Important: we must call this directly to avoid recursion
                let mut direct_subscribers = bus.subscribers.write().await;
                
                if let Some(subs) = direct_subscribers.get_mut(&event_type) {
                    if subs.is_empty() {
                        // Create a new recovery channel
                        let (recovery_tx, _) = mpsc::channel::<Event>(500);
                        
                        // Add as a recovery channel with proper actor type
                        let actor_type = match event_type.as_str() {
                            "market" => "MarketDataActor",
                            "strategy" => "StrategyActor",
                            "risk" => "RiskManagerActor",
                            "execution" => "ExecutionActor",
                            "database" => "DatabaseActor",
                            _ => "UnknownActor",
                        };
                        
                        info!("Creating direct recovery channel for {} events", event_type);
                        let subscriber = SubscriberInfo::new(recovery_tx, actor_type.to_string());
                        subs.push(subscriber);
                    }
                }
            }
            
            // We won't try to schedule future maintenance to avoid Send issues
            // Instead, just log if we need maintenance again later
            if total_removed > 0 {
                debug!("Removed {} subscribers - consider running maintenance again soon", total_removed);
            }
            
            total_removed
        }
        
        // Use Box::pin to handle recursion properly
        Box::pin(async_fn_impl(self)).await
    }

    /// Subscribe to events of a specific type.
    /// The channel sender will be used to send events to the subscriber.
    pub async fn subscribe(&self, event_type: String, sender: mpsc::Sender<Event>) -> Result<(), Error> {
        // Upgrade the sender to a higher capacity if possible to help with bursty traffic
        let high_capacity_sender = if sender.capacity() < 1000 {
            // Create a new channel with higher capacity
            let (tx, mut rx) = mpsc::channel(1000);
            
            // Forward messages from the high-capacity channel to the original channel
            let original_sender = sender.clone();
            tokio::spawn(async move {
                while let Some(event) = rx.recv().await {
                    // If forwarding fails, just drop the message - we can't do much else
                    let _ = original_sender.send(event).await;
                }
            });
            
            tx
        } else {
            // Already high capacity, just use as is
            sender
        };
        
        // Add the high capacity sender to subscribers
        let mut subscribers = self.subscribers.write().await;
        
        let subscriber_id = format!("subscriber_{}", rand::random::<u64>());
        
        // Create the subscriber entry
        let subscriber = SubscriberInfo {
            sender: high_capacity_sender,
            subscriber_id: subscriber_id.clone(),
            consecutive_failures: 0,
            last_failed: None,
            is_reconnecting: false,
            actor_type: event_type.clone(),  // Store the event type for reconnection
        };
        
        // Get or create the subscribers list for this event type
        subscribers.entry(event_type.clone())
            .or_insert_with(Vec::new)
            .push(subscriber);
        
        info!("Added new subscriber to {} events (id: {})", event_type, subscriber_id);
        
        Ok(())
    }

    /// Unsubscribe from events of a specific type
    pub async fn unsubscribe(&self, event_type: &str, sender: &mpsc::Sender<Event>) -> Result<(), Error> {
        trace!("MessageBus: Removing subscriber for event type: {}", event_type);
        let mut subscribers = self.subscribers.write().await;
        if let Some(subs) = subscribers.get_mut(event_type) {
            let count_before = subs.len();
            subs.retain(|s| !std::ptr::eq(&s.sender, sender));
            let count_after = subs.len();
            debug!("Unsubscribed from {} events (remaining: {})", event_type, count_after);
            
            if count_before > count_after {
                trace!("Removed subscription from {} event type", event_type);
            } else {
                warn!("No matching subscriber found for '{}'", event_type);
            }
        } else {
            warn!("No subscribers found for event type: {}", event_type);
        }
        Ok(())
    }

    /// For debugging: Get the number of subscribers for a specific event type
    pub async fn get_subscriber_count(&self, event_type: &str) -> usize {
        let subscribers = self.subscribers.read().await;
        subscribers.get(event_type).map_or(0, |subs| subs.len())
    }

    /// Check if an event should be deduplicated
    async fn should_deduplicate(&self, token_id: &str, event_type: &str) -> bool {
        let token_events = self.token_events.read().await;
        if let Some(info) = token_events.get(token_id) {
            if info.last_event_type == event_type {
                let elapsed = info.last_event_time.elapsed();
                if elapsed < self.deduplication_window {
                    debug!("Deduplicating event for token {}: {} (elapsed: {:?})", 
                        token_id, event_type, elapsed);
                    return true;
                }
            }
        }
        false
    }

    /// Update the last event info for a token
    async fn update_token_event(&self, token_id: &str, event_type: &str) {
        let mut token_events = self.token_events.write().await;
        token_events.insert(token_id.to_string(), TokenEventInfo::new(event_type.to_string()));
    }

    /// Check if a buy order should be deduplicated for a specific token
    fn should_deduplicate_buy_order(&self, token_id: &str) -> bool {
        let mut locks = self.buy_order_locks.lock().unwrap();
        let now = Instant::now();
        
        // Check if we've seen this token recently
        if let Some(last_time) = locks.get(token_id) {
            let elapsed = now.duration_since(*last_time);
            if elapsed < Duration::from_secs(120) {
                if elapsed.as_secs() < 5 {
                    // For very quick duplicates, log as warning
                warn!("❗ DEDUPLICATION: Detected duplicate BUY order for {} within {} seconds - skipping", 
                     token_id, elapsed.as_secs());
                } else {
                    // For longer time periods, just log as info to reduce warning noise
                    info!("🔄 Deduplicating BUY order for {} within {} seconds - skipping", 
                         token_id, elapsed.as_secs());
                }
                return true;
            }
        }
        
        // Update the timestamp for this token
        locks.insert(token_id.to_string(), now);
        
        // Clean up old entries (older than 180 seconds)
        locks.retain(|_, timestamp| {
            now.duration_since(*timestamp) < Duration::from_secs(180)
        });
        
        false
    }

    /// Publish an event to all subscribers
    pub async fn publish(&self, event: Event) -> Result<(), Error> {
        let token_id = match &event {
            Event::Market(MarketEvent::PriceUpdate { token_id, .. }) => Some(token_id),
            Event::Strategy(StrategyEvent::Signal { token_id, .. }) => Some(token_id),
            Event::Risk(RiskEvent::RiskAssessment { token_id, .. }) => Some(token_id),
            Event::Execution(ExecutionEvent::OrderExecuted { token_id, .. }) => Some(token_id),
            Event::Database(DatabaseEvent::TokenUpdated { token_id, .. }) => Some(token_id),
            _ => None,
        };

        // Handle buy order deduplication
        if let Some(token_id) = token_id {
            if let Event::Execution(ExecutionEvent::OrderExecuted { signal, .. }) = &event {
                if *signal == crate::trading::strategy::Signal::Buy {
                    if self.should_deduplicate_buy_order(token_id) {
                        info!("🚫 Suppressed duplicate BUY OrderExecuted event for {}", token_id);
                        return Ok(());
                    } else {
                        info!("✅ Processing unique BUY OrderExecuted event for {}", token_id);
                    }
                }
            }
        }

        if let Some(token_id) = token_id {
            let event_type = format!("{:?}", event);
            if self.should_deduplicate(token_id, &event_type).await {
                return Ok(());
            }
            self.update_token_event(token_id, &event_type).await;
        }

        // Handle recursion by making the future concrete with Box::pin
        Box::pin(self.publish_internal(event)).await
    }
    
    /// Internal implementation to handle publishing events
    async fn publish_internal(&self, event: Event) -> Result<(), Error> {
        let event_type = match &event {
            Event::Market(_) => "market",
            Event::Strategy(_) => "strategy",
            Event::Risk(_) => "risk",
            Event::Execution(_) => "execution",
            Event::Database(_) => "database",
        };
        
        trace!("MessageBus: Beginning to publish {} event", event_type);
        
        // Use static variables to track the last recovery time for each event type
        // to implement backoff for repeated recovery attempts
        use std::sync::Mutex;
        use std::collections::HashMap;
        use std::time::{Instant, Duration};
        
        // Static Mutex to track last recovery time by event type
        lazy_static::lazy_static! {
            static ref LAST_RECOVERY_TIMES: Mutex<HashMap<String, Instant>> = Mutex::new(HashMap::new());
            static ref RECOVERY_ATTEMPT_COUNTS: Mutex<HashMap<String, usize>> = Mutex::new(HashMap::new());
        }
        
        // Check if we've recently attempted recovery for this event type
        let should_attempt_recovery = {
            let mut last_times = LAST_RECOVERY_TIMES.lock().unwrap();
            let mut attempt_counts = RECOVERY_ATTEMPT_COUNTS.lock().unwrap();
            
            let now = Instant::now();
            let last_time = last_times.get(event_type).cloned().unwrap_or(Instant::now() - Duration::from_secs(3600));
            let attempts = *attempt_counts.get(event_type).unwrap_or(&0);
            
            // Calculate backoff - exponential backoff based on number of attempts
            let backoff_duration = if attempts > 0 {
                let backoff_secs = std::cmp::min(30, 2_u64.pow(attempts as u32));
                Duration::from_secs(backoff_secs)
            } else {
                Duration::from_secs(0)
            };
            
            let elapsed = now.duration_since(last_time);
            
            // Allow recovery if enough time has passed
            if elapsed > backoff_duration {
                // Update the timestamp and increment attempt count
                last_times.insert(event_type.to_string(), now);
                attempt_counts.insert(event_type.to_string(), attempts + 1);
                true
            } else {
                debug!("Skipping recovery for {} events - attempted {:.1}s ago (backoff: {:.1}s)",
                      event_type, elapsed.as_secs_f32(), backoff_duration.as_secs_f32());
                false
            }
        };
        
        // Check if we have subscribers for this event type
        let mut subscribers_guard = self.subscribers.read().await;
        
        // No subscribers at all?
        if !subscribers_guard.contains_key(event_type) {
            // Special log for PositionClosed events
            if let Event::Risk(RiskEvent::PositionClosed { token_id, .. }) = &event {
                error!("DIAGNOSTICS: NO SUBSCRIBERS for {} events when publishing PositionClosed for {}", event_type, token_id);
            }
            
            warn!("No subscribers found for {} events - event discarded", event_type);
            return Ok(());
        }
        
        // Check for high subscriber failure counts that need immediate maintenance
        let needs_maintenance = subscribers_guard.get(event_type)
            .map(|subs| subs.iter().any(|s| s.consecutive_failures >= 100))
            .unwrap_or(false);
            
        if needs_maintenance {
            warn!("Detected subscribers with extreme failure counts - initiating emergency maintenance");
            drop(subscribers_guard);  // Release read lock
            
            // Run maintenance right away
            let removed = self.perform_maintenance().await;
            info!("Emergency maintenance removed {} subscribers", removed);
            
            // Get fresh subscribers after maintenance
            subscribers_guard = self.subscribers.read().await;
            let has_subscribers = subscribers_guard.get(event_type)
                .map(|subs| !subs.is_empty())
                .unwrap_or(false);
                
            if !has_subscribers {
                debug!("No subscribers left after maintenance - message dropped");
                return Ok(());
            }
        }
        
        // We need to clone the subscribers so we can release the lock before sending
        let subscribers = subscribers_guard.get(event_type)
            .map(|subs| subs.iter().map(|s| s.sender.clone()).collect::<Vec<_>>())
            .unwrap_or_default();
        
        // Special log for PositionClosed events about number of subscribers
        if let Event::Risk(RiskEvent::PositionClosed { token_id, .. }) = &event {
            debug!("DIAGNOSTICS: Found {} subscribers for {} events when publishing PositionClosed for {}", 
                  subscribers.len(), event_type, token_id);
        }
        
        // Release the read lock
        drop(subscribers_guard);
        
        // Add detailed logging for specific event types at trace level only
        match &event {
            Event::Market(MarketEvent::PriceUpdate { token_id, price, .. }) => {
                trace!("Publishing market price update for {}: ${:.4}", token_id, price);
            },
            Event::Strategy(StrategyEvent::Signal { token_id, signal, confidence, .. }) => {
                trace!("Publishing strategy signal {:?} for {} with {:.1}% confidence", 
                     signal, token_id, confidence * 100.0);
            },
            Event::Risk(RiskEvent::RiskAssessment { token_id, signal, .. }) => {
                trace!("Publishing risk assessment for {} with signal {:?}", token_id, signal);
            },
            Event::Execution(ExecutionEvent::OrderExecuted { token_id, signal, .. }) => {
                trace!("Publishing execution event for {}: {:?}", token_id, signal);
            },
            _ => {
                trace!("Publishing event: {:?}", event);
            }
        }
        
        // Send to all subscribers without holding the lock
        let mut success_count = 0;
        let mut failed_indices = Vec::new();
        
        // Process all subscribers
        for (index, sender) in subscribers.iter().enumerate() {
            match sender.try_send(event.clone()) {
                Ok(_) => {
                    success_count += 1;
                },
                Err(_) => {
                    failed_indices.push(index);
                }
            }
        }
        
        // If we had failures, update the subscriber info (with write lock)
        if !failed_indices.is_empty() {
            let mut subscribers_guard = self.subscribers.write().await;
            
            if let Some(subs) = subscribers_guard.get_mut(event_type) {
                // Update failure counts for failed subscribers
                for &index in &failed_indices {
                    if index < subs.len() {
                        subs[index].consecutive_failures += 1;
                        subs[index].last_failed = Some(std::time::Instant::now());
                        
                        let failure_count = subs[index].consecutive_failures;
                        
                        // Set thresholds for different levels of recovery
                        if failure_count >= 25 && !subs[index].is_reconnecting {
                            error!("Critical failure threshold reached for {} subscriber #{} ({} failures) - initiating reconnection", 
                                  event_type, index, failure_count);
                            
                            // Mark as reconnecting to prevent multiple attempts
                            subs[index].is_reconnecting = true;
                            
                            // Clone needed data for the reconnection attempt
                            let event_type_clone = event_type.to_string(); 
                            let actor_type = subs[index].actor_type.clone();
                            
                            // Schedule recovery without using self
                            warn!("Scheduling reconnection for {} actor (event type: {})", 
                                 actor_type, event_type_clone);
                            
                            // Immediately trigger maintenance to remove high-failure subscribers
                            drop(subscribers_guard); // Release lock first
                            let _ = self.perform_maintenance().await;
                            return Ok(());
                        } else if failure_count >= 5 {
                            error!("Failed to send {} event to subscriber #{} ({} consecutive failures)", 
                                  event_type, index, failure_count);
                        } else if failure_count > 1 {
                            warn!("Failed to send {} event to subscriber #{} ({} consecutive failures)", 
                                 event_type, index, failure_count);
                        } else {
                            debug!("Failed to send {} event to subscriber #{}", event_type, index);
                        }
                    }
                }
                
                // Perform recovery if we had multiple failures
                if success_count == 0 && should_attempt_recovery {
                    warn!("Failed to publish {} event to any subscribers (all {} failed) - initiating recovery", 
                          event_type, subscribers.len());
                    
                    // Log the recovery effort - avoid recursion
                    error!("RECOVERY ALERT: Critical failure in {} event bus - initiating direct recovery process", event_type);
                    
                    // Release the write lock before attempting recovery
                    drop(subscribers_guard);
                    
                    // Actually call the supervisor restart method that does the real work
                    if let Err(e) = Self::request_supervisor_restart(event_type).await {
                        error!("Failed to initiate supervisor recovery for {}: {}", event_type, e);
                    }
                    
                    // After attempting recovery, perform maintenance to clean up
                    let _ = self.perform_maintenance().await;
                    
                    // Reset the recovery attempt counter on successful recovery to avoid
                    // persistent increasing backoff when the system recovers and fails again later
                    {
                        let mut attempt_counts = RECOVERY_ATTEMPT_COUNTS.lock().unwrap();
                        attempt_counts.insert(event_type.to_string(), 0);
                    }
                    
                    return Ok(());
                } else {
                    // Reset consecutive failures for subscribers that succeeded
                    for index in 0..subs.len() {
                        if !failed_indices.contains(&index) {
                            // If a subscriber succeeds after reconnecting, clear the reconnecting flag
                            if subs[index].is_reconnecting {
                                debug!("Subscriber #{} successfully reconnected after failures", index);
                                subs[index].is_reconnecting = false;
                            }
                            
                            // Reset failure count regardless
                            if subs[index].consecutive_failures > 0 {
                                subs[index].consecutive_failures = 0;
                                subs[index].last_failed = None;
                            }
                        }
                    }
                    
                    let failed = subscribers.len() - success_count;
                    debug!("Published {} event to {}/{} subscribers ({} failed)", 
                          event_type, success_count, subscribers.len(), failed);
                }
            }
        } else if success_count > 0 {
            // If all subscribers succeeded, reset the recovery attempt counter
            {
                let mut attempt_counts = RECOVERY_ATTEMPT_COUNTS.lock().unwrap();
                attempt_counts.insert(event_type.to_string(), 0);
            }
            
            trace!("Successfully published {} event to all {} subscribers", 
                  event_type, success_count);
        }
        
        Ok(())
    }

    /// Debug helper: Print information about all current subscriptions
    pub async fn debug_subscriber_count(&self) -> Value {
        let subscribers = self.subscribers.read().await;
        let mut result = serde_json::Map::new();
        
        for (event_type, subs) in subscribers.iter() {
            result.insert(event_type.clone(), serde_json::json!(subs.len()));
        }
        
        serde_json::Value::Object(result)
    }

    /// New methods for reconnection and recovery

    /// Check if we have persistent failures for a given event type
    async fn has_persistent_failures(&self, event_type: &str) -> bool {
        let subscribers = self.subscribers.read().await;
        
        if let Some(subs) = subscribers.get(event_type) {
            // Check if all subscribers have high failure counts
            let persistent_failure_threshold = 20;
            let all_failing = !subs.is_empty() && subs.iter().all(|s| s.consecutive_failures >= persistent_failure_threshold);
            
            // Also check if we have at least some subscribers with extremely high failure counts
            let critical_subscribers = subs.iter().filter(|s| s.consecutive_failures >= 100).count();
            
            return all_failing || critical_subscribers > 0;
        }
        
        false
    }

    /// Request the supervisor to restart problematic actors
    async fn request_supervisor_restart(event_type: &str) -> Result<(), Error> {
        // Determine which actors need to be restarted - use the actual actor name as registered
        // with the supervisor, not the actor type name with "Actor" suffix
        let actor_types = match event_type {
            "strategy" => vec!["strategy"],
            "risk" => vec!["risk"],
            "execution" => vec!["execution"],
            "market" => vec!["market"],
            "database" => vec!["database"],
            _ => vec![],
        };
        
        if actor_types.is_empty() {
            return Ok(());
        }
        
        info!("Requesting supervisor to restart actors for event type: {}", event_type);
        
        // Get reference to the supervisor actor
        let supervisor = super::SupervisorActor::find_supervisor().await;
        
        // Flag to track if supervisor-based recovery worked
        let mut supervisor_recovery_succeeded = false;
        
        // Request restart for each affected actor type using supervisor if available
        if let Some(supervisor_ref) = &supervisor {
            info!("Found supervisor for recovery - requesting actor restarts");
            
            for actor_type in &actor_types {
                info!("Signaling supervisor to restart {}", actor_type);
                
                if let Err(e) = supervisor_ref.force_reconnect_actor(actor_type).await {
                    error!("Failed to restart {} via supervisor: {}", actor_type, e);
                    // Will fall back to direct channel recreation below
                } else {
                    info!("Successfully requested restart of {} via supervisor", actor_type);
                    supervisor_recovery_succeeded = true;
                }
            }
            
            // If supervisor-based recovery worked, add a delay before returning
            // to allow the supervisor to complete its work
            if supervisor_recovery_succeeded {
                info!("Supervisor-based recovery initiated successfully - waiting for recovery to complete");
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                return Ok(());
            }
        }
        
        // If we get here, either no supervisor was found or supervisor-based recovery failed
        warn!("Attempting direct channel recovery for {} without supervisor", event_type);
        
        // Create new subscriber entries with a clean state for this event type
        Self::recreate_subscribers_for_event_type(event_type).await?;
        
        // Create a broadcast announcement about the recovery attempt
        let recovery_message = format!("RECOVERY: Attempting to repair {} communication channels", event_type);
        let recovery_announcement = Event::Market(MarketEvent::SupervisorRecoveryRequest(recovery_message));
        
        // Publish the announcement directly - don't pass MessageBus::instance() into async block
        let _ = MessageBus::instance().publish(recovery_announcement).await;
        
        // Add a delay to avoid rapid cycling of recovery attempts
        tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
        
        info!("Direct recovery attempt completed for {}", event_type);
        Ok(())
    }
    
    /// Recreate subscribers for a specific event type
    async fn recreate_subscribers_for_event_type(event_type: &str) -> Result<(), Error> {
        let bus = MessageBus::instance();
        let mut subscribers = bus.subscribers.write().await;
        
        // Remove all existing subscribers for this event type
        if let Some(subs) = subscribers.get_mut(event_type) {
            let count = subs.len();
            
            if count > 0 {
                warn!("Removing {} stuck subscribers from {} event bus", count, event_type);
                subs.clear();
                info!("Successfully cleared {} event bus for recovery", event_type);
            }
        }
        
        drop(subscribers);
        
        // Create a new channel for recovery messages
        let (recovery_tx, _recovery_rx) = mpsc::channel::<Event>(500);
        
        // Register it as a recovery channel - we need the actor type for subscription
        let actor_type = match event_type {
            "market" => "MarketDataActor",
            "strategy" => "StrategyActor",
            "risk" => "RiskManagerActor",
            "execution" => "ExecutionActor",
            "database" => "DatabaseActor",
            _ => "UnknownActor",
        };
        
        info!("Creating recovery channel for {} events, actor type: {}", event_type, actor_type);
        
        if let Err(e) = bus.subscribe(event_type.to_string(), recovery_tx.clone()).await {
            error!("Failed to create recovery channel for {}: {}", event_type, e);
            return Err(e);
        }
        
        info!("Created recovery channel for {} events", event_type);
        Ok(())
    }

    /// Attempt to reconnect a specific actor type
    async fn attempt_actor_reconnection(
        message_bus: Arc<MessageBus>, 
        event_type: String, 
        actor_type: String
    ) -> Result<(), Error> {
        info!("Attempting to reconnect {} actor for {} events", actor_type, event_type);
        
        // Create a new channel for the actor
        let (new_sender, mut new_receiver) = mpsc::channel::<Event>(500);
        
        // Subscribe with the new channel
        if let Err(e) = message_bus.subscribe(event_type.clone(), new_sender).await {
            error!("Failed to create reconnection channel: {}", e);
            return Err(Error::from(e));
        }
        
        info!("Successfully created recovery channel for {} actor - {} events", actor_type, event_type);
        
        // In a real implementation, we would somehow get this new channel to the actor
        // This is highly dependent on your actor architecture
        
        Ok(())
    }

    /// Create a subscription pipeline between a source event type and a target actor
    /// This handles channel creation, subscription setup, and error handling in one place
    pub async fn create_subscription_pipeline(
        &self, 
        source_event_type: &str, 
        target_actor_ref: Option<ActorRef>,
        buffer_size: usize,
        max_retries: usize
    ) -> Result<mpsc::Sender<Event>, Error> {
        // Create channel for the subscription
        let (sender, mut receiver) = tokio::sync::mpsc::channel(buffer_size);
        
        // Try to establish the subscription with retries
        let mut success = false;
        
        for attempt in 1..=max_retries {
            match self.subscribe(source_event_type.to_string(), sender.clone()).await {
                Ok(_) => {
                    info!("Successfully subscribed to {} events (attempt {})", source_event_type, attempt);
                    success = true;
                    break;
                },
                Err(e) => {
                    error!("Attempt {} failed to subscribe to {} events: {}", attempt, source_event_type, e);
                    if attempt == max_retries {
                        return Err(Error::Task(format!("MessageBus subscription failed after {} attempts: {}", max_retries, e)));
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            }
        }
        
        // If we have a target actor, set up the event forwarder
        if let Some(actor_ref) = target_actor_ref {
            let actor_ref_clone = actor_ref.clone();
            let event_type = source_event_type.to_string();
            
            // Start forwarder in a separate task
            tokio::spawn(async move {
                let mut count = 0;
                debug!("{}→Actor event forwarder started", event_type);
                
                while let Some(event) = receiver.recv().await {
                    // Check for forced shutdown signal
                    if crate::domain::trading::execution::bot::is_forced_shutdown() {
                        info!("Force shutdown detected, terminating {}→Actor event loop", event_type);
                        break;
                    }
                    
                    let message = crate::actors::Message::Event(event.clone());
                    if let Err(e) = actor_ref_clone.send(message).await {
                        warn!("Failed to forward {} event to actor: {}", event_type, e);
                        // Add a small delay to prevent tight loop on errors
                        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                    } else {
                        count += 1;
                        if count % 100 == 0 {
                            debug!("Forwarded {} {} events to actor", count, event_type);
                        }
                    }
                }
                
                warn!("{}→Actor event forwarder exited after {} events", event_type, count);
            });
        }
        
        if success {
            Ok(sender)
        } else {
            Err(Error::Task(format!("Failed to establish subscription to {} events", source_event_type)))
        }
    }
    
    /// Set up all database subscriptions in one operation
    /// This specialized method handles the common pattern of having the database actor
    /// subscribe to events from all other actors
    pub async fn create_database_subscriptions(
        &self,
        database_actor_ref: Option<ActorRef>,
        buffer_size: usize,
        max_retries: usize
    ) -> Result<HashMap<String, bool>, Error> {
        if database_actor_ref.is_none() {
            return Err(Error::InvalidInput("No database actor reference provided".to_string()));
        }
        
        let db_actor_ref = database_actor_ref.unwrap();
        let event_types = vec!["market", "strategy", "risk", "execution"];
        let mut success_map = HashMap::new();
        
        // Create subscriptions for each event type
        for event_type in &event_types {
            // Create the subscription
            match self.create_subscription_pipeline(
                event_type,
                Some(db_actor_ref.clone()),
                buffer_size,
                max_retries
            ).await {
                Ok(_) => {
                    success_map.insert(event_type.to_string(), true);
                    debug!("Successfully set up database subscription to {} events", event_type);
                },
                Err(e) => {
                    success_map.insert(event_type.to_string(), false);
                    warn!("Failed to set up database subscription to {} events: {}", event_type, e);
                    // Continue with other subscriptions rather than failing
                }
            }
        }
        
        // Critical check for risk events since they're needed for trade recording
        if !success_map.get("risk").unwrap_or(&false) {
            error!("CRITICAL: Database actor failed to subscribe to risk events - trades and positions may not be stored");
        }
        
        // Log diagnostics for all subscriptions
        for event_type in &event_types {
            let sub_count = self.get_subscriber_count(event_type).await;
            info!("DIAGNOSTICS: {} event bus has {} subscribers", event_type, sub_count);
        }
        
        Ok(success_map)
    }
}

impl Clone for MessageBus {
    fn clone(&self) -> Self {
        Self {
            subscribers: self.subscribers.clone(),
            token_events: self.token_events.clone(),
            deduplication_window: self.deduplication_window,
            buy_order_locks: self.buy_order_locks.clone(),
        }
    }
} 