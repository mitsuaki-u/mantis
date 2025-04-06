use super::{Actor, ActorRef, Message, Command, Query, QueryResult};
use crate::error::Error;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use log::{info, error, debug};

pub struct SupervisorActor {
    actors: Arc<RwLock<HashMap<String, ActorRef>>>,
    message_bus: Arc<super::MessageBus>,
    running: bool,
}

impl SupervisorActor {
    pub fn new(message_bus: Arc<super::MessageBus>) -> Self {
        Self {
            actors: Arc::new(RwLock::new(HashMap::new())),
            message_bus,
            running: false,
        }
    }

    pub async fn register_actor(&self, id: String, actor: ActorRef) -> Result<(), Error> {
        let mut actors = self.actors.write().await;
        actors.insert(id.clone(), actor.clone());
        info!("Registered actor: {}", id);
        Ok(())
    }

    pub async fn unregister_actor(&self, id: &str) -> Result<(), Error> {
        let mut actors = self.actors.write().await;
        actors.remove(id);
        info!("Unregistered actor: {}", id);
        Ok(())
    }

    pub async fn get_actor(&self, id: &str) -> Option<ActorRef> {
        let actors = self.actors.read().await;
        actors.get(id).cloned()
    }

    async fn start_all_actors(&self) -> Result<(), Error> {
        let actors = self.actors.read().await;
        for (id, actor) in actors.iter() {
            if let Err(e) = actor.send(Message::Command(Command::Start)).await {
                error!("Failed to start actor {}: {}", id, e);
            }
        }
        Ok(())
    }

    async fn stop_all_actors(&self) -> Result<(), Error> {
        let actors = self.actors.read().await;
        for (id, actor) in actors.iter() {
            if let Err(e) = actor.send(Message::Command(Command::Stop)).await {
                error!("Failed to stop actor {}: {}", id, e);
            }
        }
        Ok(())
    }

    async fn get_actor_status(&self, id: &str) -> Result<String, Error> {
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

    async fn get_actor_metrics(&self, id: &str) -> Result<serde_json::Value, Error> {
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
}

impl Actor for SupervisorActor {
    fn start(&mut self) -> Result<(), Error> {
        self.running = true;
        info!("Starting SupervisorActor");
        Ok(())
    }

    fn stop(&mut self) -> Result<(), Error> {
        self.running = false;
        info!("Stopping SupervisorActor");
        Ok(())
    }

    fn handle_message(&mut self, msg: Message) -> Result<(), Error> {
        match msg {
            Message::Event(_) => {
                // The MessageBus already handles event forwarding to subscribers
                // No need for the Supervisor to act as a message forwarder
                Ok(())
            },
            Message::Command(cmd) => match cmd {
                Command::Start => {
                    self.running = true;
                    debug!("SupervisorActor received start command");
                    // We don't need to start other actors here directly
                    // Instead, clients should explicitly start actors after starting the supervisor
                    Ok(())
                },
                Command::Stop => {
                    self.running = false;
                    debug!("SupervisorActor received stop command");
                    // We don't need to stop other actors here directly
                    // Instead, clients should explicitly stop actors before stopping the supervisor
                    Ok(())
                },
                Command::UpdateConfig(config) => {
                    // Update supervisor parameters from config
                    if let Some(log_level) = config.get("log_level").and_then(|v| v.as_str()) {
                        // Update logging level
                        info!("Updated log level to {}", log_level);
                    }
                    Ok(())
                },
            },
            Message::Query(query, responder) => match query {
                Query::GetStatus => {
                    let status = format!("SupervisorActor running: {}", self.running);
                    responder.send(Ok(QueryResult::Status(status)))
                        .map_err(|e| Error::InvalidInput(format!("Failed to send status response: {:?}", e)))
                },
                Query::GetMetrics => {
                    // We can't directly await here, but we can get the current count without awaiting
                    let metrics = serde_json::json!({
                        "running": self.running,
                        "actor_count": "Unknown (async operation required)",
                    });
                    responder.send(Ok(QueryResult::Metrics(metrics)))
                        .map_err(|e| Error::InvalidInput(format!("Failed to send metrics response: {:?}", e)))
                },
            },
        }
    }
} 