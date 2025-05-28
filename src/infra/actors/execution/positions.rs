use super::super::{Event, ExecutionEvent};
use super::types::Position;
use crate::core::error::Error;
use crate::infra::actors::MessageBus;
use crate::infra::db::repositories::{PositionRepository, TokenRepository};
use chrono::Utc;
use log::{debug, error, info};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Synchronize positions with database
pub async fn sync_positions_with_database(
    position_repo: &Arc<PositionRepository>,
    positions: &mut Vec<Position>,
    running: bool,
) -> Result<(), Error> {
    if !running {
        return Ok(());
    }

    debug!("Syncing positions with database...");
    match position_repo.get_open_positions().await {
        Ok(db_positions) => {
            // Store length before moving
            let position_count = db_positions.len();

            // Clear existing positions and reload from database
            positions.clear();

            for db_pos in db_positions {
                let position = Position {
                    token_id: db_pos.token_id.clone(),
                    entry_price: db_pos.entry_price,
                    current_price: db_pos.current_price,
                    highest_price: db_pos.highest_price,
                    size: db_pos.size,
                    unrealized_pnl: db_pos.unrealized_pnl,
                    entry_time: db_pos.entry_time,
                };
                positions.push(position);
            }
            info!(
                "✅ Successfully synchronized {} positions from database",
                position_count
            );
            Ok(())
        }
        Err(e) => {
            let err_msg = format!("Failed to get positions from database: {}", e);
            error!("{}", err_msg);
            Err(Error::Database(err_msg))
        }
    }
}

/// Check and update all positions
pub async fn check_positions(
    token_repo: &Arc<TokenRepository>,
    position_repo: &Arc<PositionRepository>,
    message_bus: &Arc<MessageBus>,
    positions: &mut Vec<Position>,
    position_processing_map: &Arc<Mutex<HashMap<String, bool>>>,
    running: bool,
) -> Result<(), Error> {
    if !running {
        debug!("Position check skipped - ExecutionActor not running");
        return Ok(());
    }

    // Sync positions with database first
    if let Err(e) = sync_positions_with_database(position_repo, positions, running).await {
        error!("Failed to sync positions with database: {:?}", e);
        return Err(e);
    }

    // Create a copy of positions to avoid iterator invalidation
    let positions_to_check = positions.clone();
    debug!("Checking {} positions", positions_to_check.len());

    // Process each position - only update states, don't make exit decisions
    for position in positions_to_check {
        let token_id = &position.token_id;

        // Check if this position is already being processed
        let processing_map = position_processing_map.clone();
        let mut processing_lock = processing_map.lock().await;
        if let Some(true) = processing_lock.get(token_id) {
            debug!(
                "Position for {} is already being processed, skipping",
                token_id
            );
            continue;
        }

        // Mark this position as being processed
        processing_lock.insert(token_id.clone(), true);
        drop(processing_lock);

        // Get current market data
        let current_price = match token_repo.get_token_price_stats(token_id).await {
            Ok(stats) => stats.price_usd,
            Err(e) => {
                error!(
                    "Failed to get price stats for {}: {}. Skipping position check.",
                    token_id, e
                );
                let mut processing_lock = processing_map.lock().await;
                processing_lock.remove(token_id);
                continue;
            }
        };

        // Calculate P&L and metrics
        let pnl = (current_price - position.entry_price) * position.size;
        let profit_loss_pct = (current_price / position.entry_price - 1.0) * 100.0;

        // Update position state
        let mut position = position.clone();
        if current_price > position.highest_price {
            position.highest_price = current_price;
            debug!(
                "Updated highest price for {}: ${:.4} -> ${:.4}",
                token_id, position.highest_price, current_price
            );
        }

        position.current_price = current_price;
        position.unrealized_pnl = pnl;

        debug!(
            "Position state update for {}: Entry=${:.4}, Current=${:.4} ({:.2}%), PnL=${:.2}",
            token_id, position.entry_price, current_price, profit_loss_pct, pnl
        );

        // Update position in memory
        let pos_idx = positions.iter().position(|p| p.token_id == *token_id);
        if let Some(idx) = pos_idx {
            positions[idx] = position.clone();
        }

        // Update database
        if let Err(e) = position_repo
            .update_position(&position.token_id, current_price, position.highest_price)
            .await
        {
            error!(
                "Failed to update position data for {}: {}",
                position.token_id, e
            );
        }

        // Create and publish the position update event
        let event = Event::Execution(ExecutionEvent::PositionUpdate {
            token_id: token_id.to_string(),
            current_price,
            pnl,
            timestamp: Utc::now(),
        });

        // Add diagnostic logging
        debug!("🔍 DIAGNOSTIC: ExecutionActor sending PositionUpdate event for token={} with price=${:.4} pnl=${:.2}", 
              token_id, current_price, pnl);

        if let Err(e) = message_bus.publish(event).await {
            error!("Failed to publish position update event: {}", e);
        } else {
            debug!(
                "🔍 DIAGNOSTIC: ExecutionActor successfully published PositionUpdate event for {}",
                token_id
            );
        }

        // Remove from processing map
        let mut processing_lock = processing_map.lock().await;
        processing_lock.remove(token_id);
    }

    Ok(())
}
