pub mod events;

use crate::application::actors::system::actor::{ActorState, LifecycleActor};
use crate::application::actors::system::{Actor, LifecycleState};
use crate::application::errors::Error;
use crate::core::strategies::traits::{Strategy, TradingStrategy};
use crate::events::{Event, EventType};
use crate::infrastructure::database::repositories::{PositionRepository, TokenRepository};
use crate::infrastructure::dex::DexClient;
use crate::EventRouter;
use async_trait::async_trait;
use log::{debug, info};
use std::sync::Arc;

pub struct StrategyActor {
    pub state: ActorState,
    strategy: Strategy,
    token_repo: Arc<TokenRepository>,
    position_repo: Arc<PositionRepository>,
    event_router: Arc<EventRouter>,
    dex_client: Arc<DexClient>,
    // Risk management parameters for exit conditions
    take_profit: f64,
    stop_loss: f64,
}

impl StrategyActor {
    pub fn new(
        token_repo: Arc<TokenRepository>,
        position_repo: Arc<PositionRepository>,
        strategy: Strategy,
        event_router: Arc<EventRouter>,
        dex_client: Arc<DexClient>,
        take_profit: f64,
        stop_loss: f64,
    ) -> Self {
        Self {
            state: ActorState::new("StrategyActor".to_string()),
            token_repo,
            position_repo,
            event_router,
            dex_client,
            strategy,
            take_profit,
            stop_loss,
        }
    }
}

#[async_trait::async_trait]
impl Actor for StrategyActor {
    fn name(&self) -> &str {
        &self.state.name
    }

    fn is_running(&self) -> bool {
        self.state.running
    }

    async fn start(&mut self) -> Result<(), Error> {
        debug!("Starting StrategyActor");
        self.state.start();
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), Error> {
        info!("Stopping StrategyActor");
        self.state.stop();
        Ok(())
    }

    async fn handle_event(&mut self, event: Event) -> Result<(), Error> {
        self.state.record_activity();

        match event {
            Event::Market(market_event) => self.handle_market_event(market_event).await,
            _ => Ok(()), // Only handles market events
        }
    }

    fn supported_event_types(&self) -> Vec<EventType> {
        vec![EventType::Market]
    }
}

#[async_trait]
impl LifecycleActor for StrategyActor {
    async fn initialize(&mut self) -> Result<(), Error> {
        info!("Initializing StrategyActor");
        self.state.lifecycle_state = LifecycleState::Initialized;

        debug!(
            "StrategyActor initialized with strategy: {}",
            self.strategy.name()
        );
        Ok(())
    }

    async fn cleanup(&mut self) -> Result<(), Error> {
        info!("Cleaning up StrategyActor");

        // Any cleanup tasks specific to strategy actor
        debug!("StrategyActor cleanup completed");
        Ok(())
    }

    fn lifecycle_state(&self) -> LifecycleState {
        self.state.lifecycle_state.clone()
    }
}
