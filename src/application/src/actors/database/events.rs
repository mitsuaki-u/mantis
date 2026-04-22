use super::queuing::queue_position_update;
use super::DatabaseActor;
use crate::application::errors::Result;
use crate::application::events::{
    DexTransactionEvent, Event, ExecutionEvent, MarketEvent, RiskEvent, StrategyEvent,
};
use crate::core::domain::token::TokenData;
use crate::core::domain::trading::{Position as StrategyPosition, Signal};
use log::{debug, error, info, warn};

/// Handle all types of events for the DatabaseActor
pub async fn handle_event_internal(actor: &mut DatabaseActor, event: Event) -> Result<()> {
    match event {
        Event::Market(market_event) => handle_market_event(actor, market_event).await,
        Event::Execution(execution_event) => handle_execution_event(actor, execution_event).await,
        Event::Risk(risk_event) => handle_risk_event(actor, risk_event).await,
        Event::Strategy(strategy_event) => handle_strategy_event(actor, strategy_event).await,
        Event::AIAdvisor(_) => Ok(()), // DatabaseActor does not persist AI decisions yet
        Event::DexTransaction(dex_event) => handle_dex_transaction_event(actor, *dex_event).await,
    }
}

/// Handle market-related events
async fn handle_market_event(actor: &mut DatabaseActor, event: MarketEvent) -> Result<()> {
    match event {
        MarketEvent::PriceUpdate {
            token_id,
            price,
            volume,
            symbol,
            name,
            decimals,
            timestamp,
        } => {
            let token_id_norm = TokenData::normalize_token_id(&token_id);
            let token_repo = actor.repo_factory.token_repository();
            let position_repo = actor.repo_factory.position_repository();

            // 1. Store token metadata (symbol, name, decimals)
            let metadata = crate::infrastructure::database::pool::TokenMetadata {
                name,
                symbol,
                decimals: decimals as i32,
                updated_at: timestamp,
            };

            if let Err(e) = token_repo
                .update_token_metadata_full(&token_id_norm, &metadata)
                .await
            {
                error!(
                    "DBActor: Failed to store token metadata for {}: {}",
                    token_id_norm, e
                );
            }

            // 2. Store price data for ALL tokens (market history)
            let vol = match volume {
                Some(v) => v,
                None => {
                    warn!(
                        "DBActor: Missing volume data for {} at ${:.6} - API may be degraded, storing 0.0",
                        token_id_norm, price
                    );
                    0.0
                }
            };

            if let Err(e) = token_repo
                .store_price_data(&token_id_norm, price, vol)
                .await
            {
                error!(
                    "DBActor: Failed to store price data for {}: {}",
                    token_id_norm, e
                );
            }

            // 3. Update positions if one exists
            match position_repo.get_position_by_token_id(&token_id_norm).await {
                Ok(Some((_position_id, position))) => {
                    // Calculate P&L for the position update
                    let size = position.size;
                    let entry_price = position.entry_price;
                    let pnl = (price - entry_price) * size;

                    debug!(
                        "DBActor: MarketEvent::PriceUpdate for {} with open position - Price: ${:.6}, P&L: ${:.2}",
                        token_id_norm, price, pnl
                    );

                    // Queue position update for batch processing (or write directly if Redis disabled)
                    queue_position_update(actor, token_id_norm, price, pnl, timestamp).await?;
                }
                Ok(None) => {
                    // No open position for this token, only price storage needed (already logged by token_repo)
                }
                Err(e) => {
                    error!(
                        "DBActor: Failed to check position for {}: {}",
                        token_id_norm, e
                    );
                }
            }
            Ok(())
        }
        // All other market events don't require database persistence
        _ => Ok(()),
    }
}

/// Handle execution-related events
async fn handle_execution_event(actor: &mut DatabaseActor, event: ExecutionEvent) -> Result<()> {
    match event {
        ExecutionEvent::OrderExecuted {
            token_id,
            provider_token_id,
            signal,
            executed_value_usd,
            token_quantity,
            price_per_token,
            timestamp,
            entry_price,
            entry_time,
            actual_fees,
            correlation_id,
        } => {
            let corr_id_str = correlation_id.as_deref().unwrap_or("unknown");
            info!(
                "[{}] DBActor: OrderExecuted Event for {} (provider ID: {}) - Signal: {:?}, Value: {}, Quantity: {}, Price: {}, Timestamp: {}",
                &corr_id_str[..8.min(corr_id_str.len())], token_id, provider_token_id, signal, executed_value_usd, token_quantity, price_per_token, timestamp
            );

            match signal {
                Signal::Buy => {
                    // BUY: Create new position
                    let final_strategy_position_data = StrategyPosition {
                        token_id: token_id.clone(),
                        provider_id: provider_token_id.clone(),
                        entry_price: price_per_token,
                        current_price: price_per_token,
                        highest_price: price_per_token,
                        size: token_quantity,
                        entry_time: timestamp,
                        unrealized_pnl: 0.0,
                    };

                    // BUY position must be recorded immediately for position tracking
                    let db_position_id = actor
                        .repo_factory
                        .position_repository()
                        .record_position_with_trade(
                            &final_strategy_position_data,
                            price_per_token,
                            executed_value_usd,
                            timestamp,
                        )
                        .await
                        .map_err(|e| {
                            error!(
                                "[{}] DatabaseActor: CRITICAL - Failed to record BUY position/trade for token {}: {}. Aborting event processing.",
                                &corr_id_str[..8.min(corr_id_str.len())], token_id, e
                            );
                            crate::application::errors::Error::Trading(format!("Failed to record BUY position for token {}: {}", token_id, e))
                        })?;

                    info!(
                        "[{}] DatabaseActor: Recorded new position (DB ID: {}) and BUY trade for token {} from OrderExecuted event",
                        &corr_id_str[..8.min(corr_id_str.len())], db_position_id, token_id
                    );

                    // Release the position slot reservation after successful position creation
                    if let Some(ref corr_id) = correlation_id {
                        if let Err(e) = actor
                            .repo_factory
                            .position_repository()
                            .release_reservation(corr_id)
                            .await
                        {
                            warn!(
                                "[{}] Failed to release position reservation after successful creation: {}",
                                &corr_id[..8], e
                            );
                        } else {
                            debug!("[{}] Released position slot reservation", &corr_id[..8]);
                        }
                    }
                }
                Signal::Sell => {
                    // SELL: Close existing position
                    // Validate we have the required fields for position close
                    let entry_price_val = entry_price.ok_or_else(|| {
                        crate::application::errors::Error::Trading(format!(
                            "OrderExecuted(SELL) missing entry_price for {}",
                            token_id
                        ))
                    })?;
                    let entry_time_val = entry_time.ok_or_else(|| {
                        crate::application::errors::Error::Trading(format!(
                            "OrderExecuted(SELL) missing entry_time for {}",
                            token_id
                        ))
                    })?;

                    // Get the position_id from database
                    let position_repo = actor.repo_factory.position_repository();
                    let position_id = match position_repo.get_position_by_token_id(&token_id).await
                    {
                        Ok(Some((id, _pos))) => id,
                        Ok(None) => {
                            error!(
                                "DatabaseActor: No open position found for {} to close",
                                token_id
                            );
                            return Err(crate::application::errors::Error::Trading(format!(
                                "No position to close for {}",
                                token_id
                            )));
                        }
                        Err(e) => {
                            error!(
                                "DatabaseActor: Failed to fetch position for {}: {}",
                                token_id, e
                            );
                            return Err(e);
                        }
                    };

                    // SELL position must be closed immediately
                    let close_args =
                        crate::infrastructure::database::repositories::position::RecordCloseArgs {
                            token_id: &token_id,
                            exit_price: price_per_token,
                            size: token_quantity,
                            entry_price: entry_price_val,
                            entry_time: entry_time_val,
                            exit_time: timestamp,
                        };

                    match position_repo
                        .record_position_close_with_trade(position_id, close_args, actual_fees)
                        .await
                    {
                        Ok(completed_position) => {
                            info!(
                                "DatabaseActor: Closed position {} for token {} - P&L: ${:.2}, ROI: {:.2}%",
                                position_id, token_id, completed_position.profit, completed_position.roi
                            );
                        }
                        Err(e) => {
                            error!(
                                "DatabaseActor: CRITICAL - Failed to close position for {}: {}. Aborting event processing.",
                                token_id, e
                            );
                            return Err(crate::application::errors::Error::Trading(format!(
                                "Failed to close position for {}: {}",
                                token_id, e
                            )));
                        }
                    }
                }
                Signal::Hold | Signal::NoAction => {
                    // These signals don't result in order execution, so we shouldn't receive them
                    // Log a warning but don't fail
                    warn!(
                        "DatabaseActor: Received OrderExecuted event with unexpected signal {:?} for {}",
                        signal, token_id
                    );
                }
            }
            Ok(())
        }
        // Position updates now handled via MarketEvent::PriceUpdate for real-time updates
        // All other execution events don't require database persistence
        _ => Ok(()),
    }
}

/// Handle risk-related events
async fn handle_risk_event(_actor: &mut DatabaseActor, _event: RiskEvent) -> Result<()> {
    Ok(())
}

/// Handle strategy-related events
async fn handle_strategy_event(_actor: &mut DatabaseActor, _event: StrategyEvent) -> Result<()> {
    Ok(())
}

/// Handle DEX transaction events
async fn handle_dex_transaction_event(
    actor: &mut DatabaseActor,
    event: DexTransactionEvent,
) -> Result<()> {
    match event {
        DexTransactionEvent::StatusUpdated { status, details } => {
            let tx_repo = actor.repo_factory.transaction_repository();
            tx_repo
                .update_from_transaction_event(&status, details.as_deref())
                .await
                .map_err(|e| {
                    error!(
                        "DatabaseActor: CRITICAL - Failed to update transaction: {}",
                        e
                    );
                    crate::application::errors::Error::Database(format!(
                        "Transaction update failed: {}",
                        e
                    ))
                })?;
            Ok(())
        }
        // All other DEX transaction events don't require database persistence
        _ => Ok(()),
    }
}
