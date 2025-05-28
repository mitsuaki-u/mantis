use super::super::{Event, ExecutionEvent};
use super::types::Position;
use crate::core::config::Config;
use crate::core::error::Error;
use crate::core::models::token::TokenData;
use crate::domain::dex::{DexClient, TransactionPriority};
use crate::domain::trading::strategy::{Position as StrategyPosition, Signal};
use crate::infra::actors::MessageBus;
use crate::infra::db::repositories::{PositionRepository, TokenRepository};
use chrono::Utc;
use log::{debug, error, info, warn};
use std::sync::Arc;

/// Handle risk assessment and execute buy/sell orders
pub async fn handle_risk_assessment(
    token_repo: &Arc<TokenRepository>,
    position_repo: &Arc<PositionRepository>,
    dex_client: &DexClient,
    message_bus: &Arc<MessageBus>,
    config: &Arc<Config>,
    positions: &mut Vec<Position>,
    running: bool,
    token_id: String,
    signal: Signal,
    confidence: f64,
    position_size: f64,
) -> Result<(), Error> {
    info!(
        "ExecutionActor: Received risk assessment for {}: Signal: {:?}, Confidence: {}, Size: {}",
        token_id, signal, confidence, position_size
    );

    let _is_paper = config.trading.paper_trading;

    if !running {
        info!("🛑 Execution actor is not running, ignoring risk assessment");
        return Ok(());
    }

    // Handle buy orders
    if signal == Signal::Buy {
        execute_buy_order(
            token_repo,
            position_repo,
            dex_client,
            message_bus,
            config,
            positions,
            token_id,
            signal,
            position_size,
        )
        .await?;
    }
    // Handle sell orders
    else if signal == Signal::Sell {
        execute_sell_order(
            token_repo,
            position_repo,
            dex_client,
            message_bus,
            config,
            positions,
            token_id,
            signal,
            confidence,
            _is_paper,
        )
        .await?;
    }

    Ok(())
}

/// Execute a buy order
async fn execute_buy_order(
    token_repo: &Arc<TokenRepository>,
    position_repo: &Arc<PositionRepository>,
    dex_client: &DexClient,
    message_bus: &Arc<MessageBus>,
    config: &Arc<Config>,
    positions: &mut Vec<Position>,
    token_id: String,
    signal: Signal,
    position_size: f64,
) -> Result<(), Error> {
    // Get token data
    let token_data = match token_repo.get_token_price_stats(&token_id).await {
        Ok(data) => data,
        Err(e) => {
            error!("Failed to get token data for {}: {}", token_id, e);
            return Err(Error::Api(format!("Failed to get token data: {}", e)));
        }
    };

    let symbol = token_data.symbol.to_uppercase();
    let entry_price = token_data.price_usd;

    // Extra verification - check if token exists in DB
    let token_exists_in_db = match token_repo.token_exists(&token_id).await {
        Ok(exists) => exists,
        Err(e) => {
            error!("Failed to check token existence for {}: {}", token_id, e);
            return Err(Error::Database(e.to_string()));
        }
    };

    if !token_exists_in_db {
        error!(
            "Token {} not found in database, cannot create position.",
            token_id
        );
        return Err(Error::NotFound(format!("Token {} not found", token_id)));
    }

    // Final validation of price
    if entry_price <= 0.0 {
        error!(
            "❌ Rejecting BUY order for {} due to invalid price: ${:.4}",
            symbol, entry_price
        );
        return Ok(());
    }

    info!(
        "🔄 Executing BUY order for {} (${:.4}) with position size: ${:.2}",
        symbol, entry_price, position_size
    );

    // Validate position size
    if position_size <= 0.0 {
        info!(
            "Skipping position creation for {} due to zero or negative size: ${:.4}",
            symbol, position_size
        );
        return Ok(());
    }

    // Execute buy order
    let from_token_address = config.dex.testnet_stablecoin_address.clone();
    let to_token_address = token_id.clone();
    let canonical_id = TokenData::normalize_token_id(&token_id);
    let amount_to_spend_usd = position_size;
    let slippage_tolerance = 0.005; // 0.5% slippage

    let swap_result = match dex_client
        .execute_swap(
            &from_token_address,
            &to_token_address,
            amount_to_spend_usd,
            slippage_tolerance,
            None,
            TransactionPriority::Standard,
        )
        .await
    {
        Ok(tx_details) => {
            info!(
                "✅ Successfully executed BUY swap for {} ({} -> {}). TxID: {}. Amount Out: {:.6} {}. Effective Price: {:.4}",
                symbol, tx_details.token_in_address, tx_details.token_out_address,
                tx_details.tx_id, tx_details.amount_out, tx_details.token_out_address, tx_details.actual_price
            );
            Some(tx_details)
        }
        Err(e) => {
            error!("Failed to execute BUY swap for {}: {:?}", symbol, e);
            None
        }
    };

    if let Some(tx_details) = swap_result {
        let actual_entry_price = tx_details.actual_price;
        let quantity_bought = tx_details.amount_out;

        if quantity_bought <= 0.0 || actual_entry_price <= 0.0 {
            error!(
                "❌ BUY swap for {} resulted in invalid quantity ({}) or price ({}). Not recording position.", 
                symbol, quantity_bought, actual_entry_price
            );
            return Ok(());
        }

        let position = Position {
            token_id: token_id.clone(),
            entry_price: actual_entry_price,
            current_price: actual_entry_price,
            highest_price: actual_entry_price,
            size: quantity_bought,
            unrealized_pnl: 0.0,
            entry_time: Utc::now(),
        };

        // Register the position
        positions.push(position.clone());

        // Record position in database
        let strategy_position = StrategyPosition::new(
            canonical_id.clone(),
            to_token_address.clone(),
            actual_entry_price,
            quantity_bought,
            Utc::now(),
        );

        // Check if position already exists
        let position_exists = match position_repo.position_exists(&token_id).await {
            Ok(exists) => exists,
            Err(e) => {
                error!("Failed to check if position exists: {}", e);
                false
            }
        };

        if position_exists {
            info!(
                "Position for {} already exists in database, skipping record_position_with_trade",
                token_id
            );
        } else {
            if let Err(e) = position_repo
                .record_position_with_trade(
                    &strategy_position,
                    position.entry_price,
                    position.size,
                    position.entry_time,
                )
                .await
            {
                if e.to_string().contains("UNIQUE constraint failed") {
                    info!(
                        "Position for {} was created by another process, continuing normally",
                        token_id
                    );
                } else {
                    error!("Failed to record position in database: {}", e);
                }
            } else {
                info!("✅ Successfully recorded position with trade in database directly");
            }
        }

        info!(
            "📈 New position opened for {}: ${:.2} at ${:.4}",
            symbol, position_size, actual_entry_price
        );

        // Publish execution event
        let event = Event::Execution(ExecutionEvent::OrderExecuted {
            canonical_token_id: canonical_id.clone(),
            provider_token_id: to_token_address.clone(),
            signal,
            executed_value_usd: tx_details.amount_in,
            token_quantity: tx_details.amount_out,
            price_per_token: actual_entry_price,
            timestamp: Utc::now(),
        });

        let db_subscribers = message_bus.get_subscriber_count("database").await;
        let execution_subscribers = message_bus.get_subscriber_count("execution").await;

        debug!("DIAGNOSTICS: ExecutionActor publishing BUY OrderExecuted event for {} - db_subscribers={}, execution_subscribers={}",
              symbol, db_subscribers, execution_subscribers);

        match message_bus.publish(event).await {
            Ok(_) => info!(
                "📣 Successfully published BUY execution event for {} (${:.4}) with size ${:.2}",
                symbol, actual_entry_price, position_size
            ),
            Err(e) => error!(
                "❌ Failed to publish BUY execution event for {}: {:?}",
                symbol, e
            ),
        }
    }

    Ok(())
}

/// Execute a sell order
async fn execute_sell_order(
    token_repo: &Arc<TokenRepository>,
    position_repo: &Arc<PositionRepository>,
    dex_client: &DexClient,
    message_bus: &Arc<MessageBus>,
    config: &Arc<Config>,
    positions: &mut Vec<Position>,
    token_id: String,
    signal: Signal,
    confidence: f64,
    _is_paper: bool,
) -> Result<(), Error> {
    // Verify if this token has a position in the database
    let token_exists = match token_repo.token_exists(&token_id).await {
        Ok(exists) => exists,
        Err(e) => {
            error!("Failed to check if token {} exists: {}", token_id, e);
            false
        }
    };

    let pos_in_db_exists = match position_repo.position_exists(&token_id).await {
        Ok(exists) => exists,
        Err(e) => {
            error!("Failed to check if position exists for {}: {}", token_id, e);
            false
        }
    };

    // Check if we have a position in memory
    let position_in_memory = positions
        .iter()
        .find(|p| p.token_id.to_lowercase() == token_id.to_lowercase())
        .cloned();

    let pos_in_memory_exists = position_in_memory.is_some();

    // If neither database nor memory has a position, this is a phantom sell signal
    if !pos_in_db_exists && !pos_in_memory_exists {
        info!("📊 Diagnostic information for phantom sell signal:");
        info!("  - Token exists in database: {}", token_exists);
        info!("  - Paper trading mode: {}", _is_paper);
        info!("  - Position in memory: {}", pos_in_memory_exists);
        info!("  - Position in database: {}", pos_in_db_exists);
        info!("⚠️ This could indicate a race condition, stale data, or a mismatch between strategy and execution state");
        info!(
            "⚠️ Ignoring sell signal for {} as no position exists to sell",
            token_id
        );
        return Ok(());
    }

    if let Some(_pos) = &position_in_memory {
        info!(
            "✅ Found position for {} in memory: Entry=${:.4}, Size=${:.2}, PnL=${:.2}",
            token_id, _pos.entry_price, _pos.size, _pos.unrealized_pnl
        );
    } else {
        info!(
            "⚠️ No position found in memory for {}, checking database...",
            token_id
        );
    }

    // Get current market data
    let current_price = match token_repo.get_token_price_stats(&token_id).await {
        Ok(stats) => stats.price_usd,
        Err(e) => {
            error!("Failed to get price stats for {}: {:?}", token_id, e);
            return Err(Error::Api(format!("Failed to get price stats: {}", e)));
        }
    };

    info!(
        "🛒 Processing SELL order for {} at price ${:.4}",
        token_id, current_price
    );

    // Call process_sell_order to actually execute the sell
    process_sell_order(
        token_repo,
        position_repo,
        dex_client,
        message_bus,
        config,
        positions,
        &token_id,
        signal,
        confidence,
        _is_paper,
    )
    .await?;

    Ok(())
}

/// Process a sell order by executing the swap and updating positions
pub async fn process_sell_order(
    token_repo: &Arc<TokenRepository>,
    position_repo: &Arc<PositionRepository>,
    dex_client: &DexClient,
    message_bus: &Arc<MessageBus>,
    config: &Arc<Config>,
    positions: &mut Vec<Position>,
    token_id: &str,
    signal: Signal,
    confidence: f64,
    _is_paper: bool,
) -> Result<(), Error> {
    // Find the position to sell
    let position_index = positions
        .iter()
        .position(|p| p.token_id.to_lowercase() == token_id.to_lowercase());

    let position = if let Some(idx) = position_index {
        positions[idx].clone()
    } else {
        warn!(
            "No position found in memory for {}, cannot execute sell",
            token_id
        );
        return Ok(());
    };

    // Get token data for symbol
    let token_data = match token_repo.get_token_price_stats(token_id).await {
        Ok(data) => data,
        Err(e) => {
            error!("Failed to get token data for {}: {}", token_id, e);
            return Err(Error::Api(format!("Failed to get token data: {}", e)));
        }
    };

    let symbol = token_data.symbol.to_uppercase();
    let current_price = token_data.price_usd;

    info!(
        "🔄 Executing SELL order for {} (${:.4}) with confidence: {:.2}",
        symbol, current_price, confidence
    );

    // Execute sell order
    let from_token_address = token_id.to_string();
    let to_token_address = config.dex.testnet_stablecoin_address.clone();
    let amount_to_sell = position.size; // Sell the entire position
    let slippage_tolerance = 0.005; // 0.5% slippage

    let swap_result = match dex_client
        .execute_swap(
            &from_token_address,
            &to_token_address,
            amount_to_sell,
            slippage_tolerance,
            None,
            crate::domain::dex::TransactionPriority::Standard,
        )
        .await
    {
        Ok(tx_details) => {
            info!(
                "✅ Successfully executed SELL swap for {} ({} -> {}). TxID: {}. Amount Out: {:.6} {}. Effective Price: {:.4}",
                symbol, tx_details.token_in_address, tx_details.token_out_address,
                tx_details.tx_id, tx_details.amount_out, tx_details.token_out_address, tx_details.actual_price
            );
            Some(tx_details)
        }
        Err(e) => {
            error!("Failed to execute SELL swap for {}: {:?}", symbol, e);
            None
        }
    };

    if let Some(tx_details) = swap_result {
        let actual_exit_price = tx_details.actual_price;
        let proceeds_usd = tx_details.amount_out;

        // Calculate realized P&L
        let realized_pnl = (actual_exit_price - position.entry_price) * position.size;

        info!(
            "💰 Position closed for {}: Entry=${:.4}, Exit=${:.4}, Size={:.6}, Realized P&L=${:.2}",
            symbol, position.entry_price, actual_exit_price, position.size, realized_pnl
        );

        // Remove position from memory
        if let Some(idx) = position_index {
            positions.remove(idx);
        }

        // Close position in database
        // First get the position ID and details from the database
        let position_result = match position_repo.get_position_by_token_id(token_id).await {
            Ok(Some((position_id, db_position))) => {
                // Use record_position_close_with_trade with the correct parameters
                let close_args = crate::infra::db::repositories::position::RecordCloseArgs {
                    token_id,
                    exit_price: actual_exit_price,
                    size: position.size,
                    entry_price: db_position.entry_price,
                    entry_time: db_position.entry_time,
                    exit_time: Utc::now(),
                };

                position_repo
                    .record_position_close_with_trade(position_id, close_args)
                    .await
            }
            Ok(None) => {
                warn!(
                    "No position found in database for {}, cannot close",
                    token_id
                );
                Ok(
                    crate::infra::db::repositories::position::CompletedPosition {
                        id: 0,
                        token_id: token_id.to_string(),
                        size: position.size,
                        entry_price: position.entry_price,
                        exit_price: actual_exit_price,
                        entry_time: position.entry_time,
                        exit_time: Utc::now(),
                        profit: realized_pnl,
                        roi: 0.0,
                        fees: 0.0,
                        net_profit: realized_pnl,
                    },
                )
            }
            Err(e) => {
                error!("Failed to get position from database: {}", e);
                Err(e)
            }
        };

        if let Err(e) = position_result {
            error!("Failed to close position in database: {}", e);
        } else {
            info!("✅ Successfully closed position in database");
        }

        // Publish execution event
        let canonical_id = crate::core::models::token::TokenData::normalize_token_id(token_id);
        let event = super::super::Event::Execution(super::super::ExecutionEvent::OrderExecuted {
            canonical_token_id: canonical_id.clone(),
            provider_token_id: from_token_address.clone(),
            signal,
            executed_value_usd: proceeds_usd,
            token_quantity: position.size,
            price_per_token: actual_exit_price,
            timestamp: Utc::now(),
        });

        match message_bus.publish(event).await {
            Ok(_) => info!(
                "📣 Successfully published SELL execution event for {} (${:.4}) with proceeds ${:.2}",
                symbol, actual_exit_price, proceeds_usd
            ),
            Err(e) => error!(
                "❌ Failed to publish SELL execution event for {}: {:?}",
                symbol, e
            ),
        }
    }

    Ok(())
}
