pub mod events;
pub mod limits;
pub mod operations;

pub use events::handle_event_internal;
pub use limits::check_overall_risk_limits;
pub use operations::{check_token_risk, reset_daily_metrics, update_risk_metrics};

use crate::application::actors::system::actor::{ActorState, LifecycleActor};
use crate::application::actors::system::{Actor, LifecycleState};
use crate::application::errors::Result;
use crate::config::Config;
use crate::events::{Event, EventType};
use crate::infrastructure::database::repositories::{PositionRepository, TokenRepository};
use crate::EventRouter;
use async_trait::async_trait;
use log::{debug, error, info, trace, warn};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Risk metrics for portfolio tracking
#[derive(Debug, Clone)]
pub struct RiskMetrics {
    pub current_daily_loss: f64,
    pub current_drawdown: f64,
    pub max_daily_loss_limit: f64,
    pub max_drawdown_limit: f64,
}

impl RiskMetrics {
    pub fn new(max_daily_loss_limit: f64, max_drawdown_limit: f64) -> Self {
        Self {
            current_daily_loss: 0.0,
            current_drawdown: 0.0,
            max_daily_loss_limit,
            max_drawdown_limit,
        }
    }
}

/// Details of a single position tracked by the risk manager
#[derive(Debug, Clone)]
pub struct PositionDetails {
    pub entry_price: f64,
    pub size: f64, // quantity
    pub current_price: f64,
    pub unrealized_pnl: f64,
}

#[derive(Clone)]
pub struct RiskManagerActor {
    // Actor state management
    state: ActorState,

    // Dependencies
    pub position_repo: Arc<PositionRepository>,
    pub token_repo: Arc<TokenRepository>,
    pub event_router: Arc<EventRouter>,

    // Configuration
    pub config: Arc<Config>,

    // Position tracking state (inline from RiskManager service)
    pub max_position_size: f64,
    pub max_total_exposure: f64,
    current_total_value: f64,
    initial_total_exposure: f64,
    positions: HashMap<String, PositionDetails>,

    // Risk management state
    pub risk_metrics: RiskMetrics,
    pub halted_tokens: HashSet<String>,
}

impl RiskManagerActor {
    pub fn new(
        token_repo: Arc<TokenRepository>,
        position_repo: Arc<PositionRepository>,
        event_router: Arc<EventRouter>,
        config: Arc<Config>,
    ) -> Self {
        Self {
            state: ActorState::new("RiskManagerActor".to_string()),
            position_repo,
            token_repo,
            event_router,
            max_position_size: config.trading.max_position_size,
            max_total_exposure: config.trading.max_total_exposure,
            current_total_value: 0.0,
            initial_total_exposure: 0.0,
            positions: HashMap::new(),
            risk_metrics: RiskMetrics::new(
                config.trading.max_daily_loss / 100.0,
                config.trading.max_drawdown / 100.0,
            ),
            halted_tokens: HashSet::new(),
            config,
        }
    }

    // ========== Position Tracking Methods (inlined from RiskManager service) ==========

    /// Check if opening a new position would exceed max_total_exposure
    pub fn can_open_position(&self, proposed_initial_value_usd: f64) -> bool {
        (self.initial_total_exposure + proposed_initial_value_usd) <= self.max_total_exposure
    }

    /// Add a new position to tracking
    pub fn add_position(&mut self, token_id: &str, size_quantity: f64, entry_price_usd: f64) {
        let initial_value = size_quantity * entry_price_usd;
        let details = PositionDetails {
            entry_price: entry_price_usd,
            size: size_quantity,
            current_price: entry_price_usd,
            unrealized_pnl: 0.0,
        };
        self.positions.insert(token_id.to_string(), details);
        self.initial_total_exposure += initial_value;
        self.current_total_value += initial_value;
    }

    /// Remove a position from tracking
    pub fn remove_position(&mut self, token_id: &str) -> Option<PositionDetails> {
        if let Some(removed_position) = self.positions.remove(token_id) {
            let initial_value_removed = removed_position.size * removed_position.entry_price;
            self.initial_total_exposure -= initial_value_removed;
            self.current_total_value -= removed_position.size * removed_position.current_price;
            Some(removed_position)
        } else {
            None
        }
    }

    /// Update position price and recalculate PnL
    pub fn update_position_price(&mut self, token_id: &str, new_price: f64) -> Option<f64> {
        let mut pnl_change_for_this_update: Option<f64> = None;
        if let Some(position) = self.positions.get_mut(token_id) {
            let old_position_value = position.size * position.current_price;

            position.current_price = new_price;
            position.unrealized_pnl =
                (position.current_price - position.entry_price) * position.size;

            let new_position_value = position.size * position.current_price;
            let value_change = new_position_value - old_position_value;
            self.current_total_value += value_change;
            pnl_change_for_this_update = Some(value_change);

            trace!(
                "RiskManager: Updated token: {}, New Price: {:.4}, Size: {}, Entry: {:.4}, PnL: {:.2}, New Value: {:.2}",
                token_id, new_price, position.size, position.entry_price, position.unrealized_pnl, new_position_value
            );
        } else {
            trace!(
                "RiskManager: update_position_price called for token_id: {} (no position found), price: {}",
                token_id,
                new_price
            );
        }
        pnl_change_for_this_update
    }

    /// Get current total value of all positions
    pub fn get_current_total_value(&self) -> f64 {
        self.current_total_value
    }

    /// Get initial total exposure
    pub fn get_initial_total_exposure(&self) -> f64 {
        self.initial_total_exposure
    }

    /// Get a specific position's details
    pub fn get_position_details(&self, token_id: &str) -> Option<&PositionDetails> {
        self.positions.get(token_id)
    }

    /// Get all position details
    pub fn get_all_positions(&self) -> &HashMap<String, PositionDetails> {
        &self.positions
    }

    // ========== Database Sync ==========

    /// Sync risk manager's in-memory position tracking with database state
    /// This is critical for correct position limit enforcement after bot restarts
    pub async fn sync_positions_with_database(&mut self) -> Result<()> {
        info!("🔄 RiskManager: Syncing in-memory position tracking with database...");

        match self.position_repo.get_open_positions().await {
            Ok(db_positions) => {
                // Clear existing in-memory positions
                self.positions.clear();
                self.current_total_value = 0.0;
                self.initial_total_exposure = 0.0;

                let mut synced_count = 0;

                // Load all database positions into risk manager
                for db_pos in db_positions {
                    let normalized_token_id =
                        crate::core::domain::token::TokenData::normalize_token_id(&db_pos.token_id);

                    // Add position to in-memory risk tracking
                    self.add_position(&normalized_token_id, db_pos.size, db_pos.entry_price);

                    // Update current price if available
                    self.update_position_price(&normalized_token_id, db_pos.current_price);

                    synced_count += 1;
                    debug!(
                        "RiskManager: Synced position {} (norm: {}) - Size: {:.6}, Entry: ${:.6}, Current: ${:.6}",
                        db_pos.token_id, normalized_token_id, db_pos.size, db_pos.entry_price, db_pos.current_price
                    );
                }

                let in_memory_count = self.positions.len();
                let max_positions = self.config.trading.max_positions;

                info!(
                    "✅ RiskManager: Position sync complete - {}/{} positions loaded into memory (max: {})",
                    in_memory_count, synced_count, max_positions
                );

                // Check if we're at or over the limit
                if in_memory_count >= max_positions {
                    warn!(
                        "⚠️ RiskManager: Already at position limit ({}/{}) - new trades will be blocked until positions are closed",
                        in_memory_count, max_positions
                    );
                }

                Ok(())
            }
            Err(e) => {
                error!(
                    "❌ RiskManager: Failed to sync positions with database: {}",
                    e
                );
                Err(e)
            }
        }
    }
}

#[async_trait]
impl Actor for RiskManagerActor {
    fn name(&self) -> &str {
        &self.state.name
    }

    fn is_running(&self) -> bool {
        self.state.running
    }

    async fn start(&mut self) -> Result<()> {
        debug!("Starting RiskManagerActor");
        self.state.start();
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Stopping RiskManagerActor");
        self.state.stop();
        Ok(())
    }

    async fn handle_event(&mut self, event: Event) -> Result<()> {
        self.state.record_activity();
        events::handle_event_internal(self, event).await
    }

    fn supported_event_types(&self) -> Vec<EventType> {
        vec![
            EventType::Strategy,
            EventType::Market,
            EventType::Risk,
            EventType::Execution,
        ]
    }
}

#[async_trait]
impl LifecycleActor for RiskManagerActor {
    async fn initialize(&mut self) -> Result<()> {
        info!("Initializing RiskManagerActor");
        self.state.lifecycle_state = LifecycleState::Initialized;

        debug!(
            "RiskManagerActor initialized with limits - Daily Loss: {:.1}% of portfolio (${:.0}), Drawdown: {:.1}%",
            self.risk_metrics.max_daily_loss_limit * 100.0,
            self.config.trading.max_total_exposure,
            self.risk_metrics.max_drawdown_limit * 100.0
        );

        // Critical: Sync existing database positions with in-memory risk tracking
        // This prevents the bot from opening too many positions after restarts
        if let Err(e) = self.sync_positions_with_database().await {
            error!(
                "Failed to sync positions during RiskManagerActor initialization: {}",
                e
            );
            // Don't fail initialization - let the actor start but log the issue
            warn!("RiskManagerActor continuing without position sync - position limits may not be enforced correctly");
        }

        Ok(())
    }

    async fn cleanup(&mut self) -> Result<()> {
        info!("RiskManagerActor cleanup completed");
        Ok(())
    }

    fn lifecycle_state(&self) -> LifecycleState {
        self.state.lifecycle_state.clone()
    }
}
