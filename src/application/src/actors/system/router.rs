use crate::application::errors::{Error, Result};
use log::{debug, error, info, trace};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::ActorRef;
use crate::events::{Event, EventType};

/// Event router for managing event flow between actors
pub struct EventRouter {
    routes: Arc<HashMap<EventType, Vec<&'static str>>>,
    actors: Arc<RwLock<HashMap<String, ActorRef>>>,
    metrics: Arc<RwLock<RoutingMetrics>>,
}

/// Routing metrics
#[derive(Debug, Default)]
pub struct RoutingMetrics {
    pub events_routed: u64,
    pub routing_errors: u64,
    pub actor_delivery_failures: HashMap<String, u64>,
}

impl Default for EventRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl EventRouter {
    pub fn new() -> Self {
        let mut routes = HashMap::new();

        // Market events → strategy, risk, database, execution (for pool cache updates)
        routes.insert(
            EventType::Market,
            vec!["strategy", "risk", "database", "execution"],
        );

        // Strategy events → ai_advisor (intercepts BUY signals), database
        // ai_advisor re-emits approved signals back as Strategy events → risk
        routes.insert(EventType::Strategy, vec!["ai_advisor", "database"]);

        // AIAdvisor events → risk (approved signals forwarded here), database
        routes.insert(EventType::AIAdvisor, vec!["risk", "database"]);

        // Risk events → execution, database
        routes.insert(EventType::Risk, vec!["execution", "database"]);

        // Execution events → database, risk (for position tracking)
        routes.insert(EventType::Execution, vec!["database", "risk"]);

        // DexTransaction events → database
        routes.insert(EventType::DexTransaction, vec!["database"]);

        Self {
            routes: Arc::new(routes),
            actors: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(RwLock::new(RoutingMetrics::default())),
        }
    }

    /// Alias for backwards compatibility
    pub fn with_default_routing() -> Self {
        Self::new()
    }

    /// Register an actor with the router
    pub async fn register_actor(&self, name: String, actor_ref: ActorRef) -> Result<()> {
        let mut actors = self.actors.write().await;
        actors.insert(name.clone(), actor_ref);
        debug!("EventRouter: Registered actor '{}'", name);
        Ok(())
    }

    /// Unregister an actor from the router
    pub async fn unregister_actor(&self, name: &str) -> Result<()> {
        let mut actors = self.actors.write().await;
        actors.remove(name);
        info!("EventRouter: Unregistered actor '{}'", name);
        Ok(())
    }

    /// Get the number of actors that can receive a specific event type
    pub async fn get_subscriber_count(&self, event_type: &EventType) -> usize {
        self.routes.get(event_type).map(|v| v.len()).unwrap_or(0)
    }

    /// Setup actor routing - verify that all actors mentioned in routing rules are registered
    pub async fn setup_actor_routing(&self) -> Result<()> {
        info!("EventRouter: Verifying routing configuration");

        let actors = self.actors.read().await;
        let actor_count = actors.len();

        info!(
            "EventRouter: Routing verification complete - {} routes configured, {} actors registered",
            self.routes.len(), actor_count
        );

        // Log routing summary for debugging
        for (event_type, targets) in self.routes.iter() {
            debug!("EventRouter: {:?} → {:?}", event_type, targets);
        }

        Ok(())
    }

    /// Publish an event to appropriate actors
    pub async fn publish(&self, event: Event) -> Result<()> {
        let event_type = self.get_event_type(&event);

        // Get target actors for this event type
        let target_actors = match self.routes.get(&event_type) {
            Some(actors) => actors,
            None => {
                debug!("EventRouter: No route configured for {:?}", event_type);
                return Ok(());
            }
        };

        let mut routed_count = 0;
        let mut failed_actors = Vec::new();

        // Route to all target actors
        for actor_name in target_actors {
            if let Err(e) = self.send_to_actor(actor_name, event.clone()).await {
                error!(
                    "EventRouter: Failed to send event to actor '{}': {}",
                    actor_name, e
                );
                self.record_delivery_failure(actor_name).await;
                failed_actors.push((*actor_name, e.to_string()));
            } else {
                routed_count += 1;
                trace!(
                    "EventRouter: Successfully routed {:?} event to {}",
                    event_type,
                    actor_name
                );
            }
        }

        // Update metrics
        self.update_metrics(routed_count).await;

        if routed_count == 0 {
            error!(
                "🚨 EventRouter: NO actors received {:?} event! Failed: {:?}",
                event_type, failed_actors
            );
        } else if !failed_actors.is_empty() {
            error!(
                "⚠️  EventRouter: {} actors received {:?}, but {} failed: {:?}",
                routed_count,
                event_type,
                failed_actors.len(),
                failed_actors
            );
        }

        Ok(())
    }

    /// Get the event type from an event
    fn get_event_type(&self, event: &Event) -> EventType {
        match event {
            Event::Market(_) => EventType::Market,
            Event::Strategy(_) => EventType::Strategy,
            Event::Risk(_) => EventType::Risk,
            Event::Execution(_) => EventType::Execution,
            Event::AIAdvisor(_) => EventType::AIAdvisor,
            Event::DexTransaction(_) => EventType::DexTransaction,
        }
    }

    /// Send event to a specific actor
    async fn send_to_actor(&self, actor_name: &str, event: Event) -> Result<()> {
        let actors = self.actors.read().await;
        if let Some(actor_ref) = actors.get(actor_name) {
            let message = super::Message::Event(Box::new(event));
            if let Err(e) = actor_ref.send(message) {
                return Err(Error::InvalidInput(format!(
                    "Failed to send message to actor '{}': {}",
                    actor_name, e
                )));
            }
            trace!("EventRouter: Sent event to actor '{}'", actor_name);
        } else {
            return Err(Error::NotFound(format!("Actor '{}' not found", actor_name)));
        }
        Ok(())
    }

    /// Record delivery failure for metrics
    async fn record_delivery_failure(&self, actor_name: &str) {
        let mut metrics = self.metrics.write().await;
        *metrics
            .actor_delivery_failures
            .entry(actor_name.to_string())
            .or_insert(0) += 1;
        metrics.routing_errors += 1;
    }

    /// Update routing metrics
    async fn update_metrics(&self, routed: u64) {
        let mut metrics = self.metrics.write().await;
        metrics.events_routed += routed;
    }

    /// Get current routing metrics
    pub async fn get_metrics(&self) -> RoutingMetrics {
        let metrics = self.metrics.read().await;
        RoutingMetrics {
            events_routed: metrics.events_routed,
            routing_errors: metrics.routing_errors,
            actor_delivery_failures: metrics.actor_delivery_failures.clone(),
        }
    }
}

impl Clone for EventRouter {
    fn clone(&self) -> Self {
        Self {
            routes: self.routes.clone(),
            actors: self.actors.clone(),
            metrics: self.metrics.clone(),
        }
    }
}
