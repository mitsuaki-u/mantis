use super::positions::{self};
use crate::application::errors::{Error, Result};
use crate::config::Config;
use crate::core::constants::{
    ENABLE_PRICE_CROSS_CHECK, MAX_PRICE_DISCREPANCY_THRESHOLD, V3_STANDARD_FEE_PCT,
};
use crate::core::domain::token::TokenData;
use crate::core::domain::trading::Signal;
use crate::core::utils::validation::orders::{
    validate_buy_order, validate_gas_cost, validate_sell_order,
};
use crate::core::utils::validation::price::validate_price_discrepancy;
use crate::infrastructure::database::repositories::{PositionRepository, TokenRepository};
use crate::infrastructure::dex::DexClient;
use crate::infrastructure::retry::with_retry;
use crate::EventRouter;
use log::{debug, error, info, warn};
use rust_decimal::Decimal;
use std::sync::Arc;

/// Get the current network name from config
fn get_current_network_name(config: &Config) -> String {
    config
        .dex
        .network
        .clone()
        .unwrap_or_else(|| "ethereum".to_string())
}

/// Request data for a risk assessment handler call
pub struct RiskAssessmentRequest {
    pub token_id: String,
    pub signal: Signal,
    pub position_size: f64,
    pub signal_metadata: crate::core::domain::trading::SignalMetadata,
}

/// Handle risk assessment and execute buy/sell orders
pub async fn handle_risk_assessment(
    token_repo: &Arc<TokenRepository>,
    position_repo: &Arc<PositionRepository>,
    dex_client: &DexClient,
    event_router: &Arc<EventRouter>,
    config: &Arc<Config>,
    running: bool,
    request: RiskAssessmentRequest,
) -> Result<()> {
    let RiskAssessmentRequest {
        token_id,
        signal,
        position_size,
        signal_metadata,
    } = request;
    info!(
        "[{}] ExecutionActor: Received risk assessment for {}: Signal: {:?}, Size: ${:.2}, Signal Price: ${:.8}, Strategy: {}",
        &signal_metadata.correlation_id[..8], token_id, signal, position_size, signal_metadata.signal_price, signal_metadata.strategy_name
    );

    let _is_paper = !config.trading.live_trading;

    if !running {
        debug!("Execution actor not running, ignoring risk assessment");
        return Ok(());
    }

    // Handle buy orders
    if signal == Signal::Buy {
        execute_buy(
            token_repo,
            dex_client,
            event_router,
            config,
            token_id,
            position_size,
            signal_metadata,
        )
        .await?;
    }
    // Handle sell orders
    else if signal == Signal::Sell {
        // For sell orders, first verify we have a position before attempting execution
        let has_position = match position_repo.position_exists(&token_id).await {
            Ok(exists) => exists,
            Err(e) => {
                error!("Failed to check position exists for {}: {}", token_id, e);
                false
            }
        };

        if !has_position {
            debug!("Ignoring sell signal for {} - no open position", token_id);
            return Ok(());
        }

        execute_sell(
            token_repo,
            position_repo,
            dex_client,
            event_router,
            config,
            token_id,
            signal_metadata,
        )
        .await?;
    }

    Ok(())
}

/// Execute a buy order
async fn execute_buy(
    token_repo: &Arc<TokenRepository>,
    dex_client: &DexClient,
    event_router: &Arc<EventRouter>,
    config: &Arc<Config>,
    token_id: String,
    position_size: f64,
    signal_metadata: crate::events::SignalMetadata,
) -> Result<()> {
    // Get token data with retry logic for improved reliability
    let token_data = match with_retry(
        "get_token_price_stats",
        || token_repo.get_token_price_stats(&token_id),
        3,
        tokio::time::Duration::from_millis(100),
    )
    .await
    {
        Ok(data) => data,
        Err(e) => {
            error!(
                "Failed to get token data for {} after retries: {}",
                token_id, e
            );
            return Ok(());
        }
    };

    // CRITICAL: Validate token_data matches requested token_id
    if token_data.id.to_lowercase() != token_id.to_lowercase() {
        error!(
            "🚨 CRITICAL BUG DETECTED: Requested token {}, but repository returned {}! Aborting trade to prevent wrong token execution.",
            token_id, token_data.id
        );
        return Err(Error::InvalidInput(format!(
            "Token ID mismatch: requested {} but got {}",
            token_id, token_data.id
        )));
    }

    let symbol = token_data.symbol.to_uppercase();
    let entry_price = token_data.price_usd;

    // Convert config to strategy params for validation
    let params = crate::core::domain::StrategyParams::from(config.as_ref());

    let order_validation = validate_buy_order(&token_data, position_size, &params);
    if !order_validation.is_valid {
        error!(
            "Buy order validation failed for {}: {}",
            token_id,
            order_validation
                .reason
                .unwrap_or_else(|| "Unknown reason".to_string())
        );
        return Ok(());
    }

    // Validate signal price hasn't moved too much
    let signal_price =
        crate::core::utils::f64_to_decimal(signal_metadata.signal_price, "signal price")
            .map_err(|e| Error::InvalidInput(format!("{} for {}", e, token_id)))?;

    let price_discrepancy_result = validate_price_discrepancy(
        signal_price,
        entry_price,
        config.trading.max_execution_price_deviation,
        &token_id,
        &signal_metadata.correlation_id,
    );

    if !price_discrepancy_result.is_valid {
        error!(
            "CRITICAL: Rejecting BUY order for {} due to excessive price movement since signal generation: {}",
            symbol, price_discrepancy_result.reason.as_ref().unwrap_or(&"Unknown".to_string())
        );
        error!(
            "❌ Signal Price: ${:.8}, Current Price: ${:.8}, Strategy: {}",
            signal_metadata.signal_price, entry_price, signal_metadata.strategy_name
        );
        return Ok(());
    }

    info!(
        "[{}] Executing BUY order for {} (${:.4}) with position size: ${:.2}",
        &signal_metadata.correlation_id[..8],
        symbol,
        entry_price,
        position_size
    );

    // Get base token address (WETH) for gas estimation
    let base_token_address = config.dex.base_token_address();

    // Estimate actual gas cost for this swap
    let gas_fee_eth = match dex_client
        .estimate_swap_gas_fee(&base_token_address, &token_data.id, position_size)
        .await
    {
        Ok(fee) => fee,
        Err(e) => {
            error!(
                "REJECTING TRADE: Failed to estimate gas fee for {}: {}. Cannot proceed without accurate gas estimate.",
                symbol, e
            );
            return Ok(());
        }
    };

    // Get current ETH price to convert gas cost to USD
    let eth_price_usd = match dex_client.get_eth_price_usd().await {
        Ok(price) => price,
        Err(e) => {
            error!(
                "REJECTING TRADE: Failed to get ETH price for {}: {}. Cannot proceed without accurate price data.",
                symbol, e
            );
            return Ok(());
        }
    };

    let actual_gas_cost_usd = gas_fee_eth * eth_price_usd;

    // Validate gas cost is acceptable
    let gas_validation = validate_gas_cost(
        actual_gas_cost_usd,
        position_size,
        config.trading.max_gas_cost_usd,
        config.trading.max_gas_cost_percentage,
        &symbol,
        &signal_metadata.correlation_id,
    );

    if !gas_validation.is_valid {
        return Ok(());
    }

    // Get DEX fee from actual pool that would be used (for both live and paper trading)
    let dex_fee_pct = match dex_client
        .estimate_swap_output(&base_token_address, &token_data.id, position_size)
        .await
    {
        Ok((_expected_output, fee_pct)) => {
            debug!(
                "📊 Pool fee for {} -> {}: {:.3}% (same pool live trading would use)",
                base_token_address,
                &token_data.id[..10],
                fee_pct * 100.0
            );
            fee_pct
        }
        Err(e) => {
            warn!(
                "Could not get pool fee for {}: {}. Using fallback 0.3%",
                symbol, e
            );
            V3_STANDARD_FEE_PCT
        }
    };
    let dex_fee_usd = position_size * dex_fee_pct;

    let network_name = get_current_network_name(config);

    // For paper trading, simulate the execution without actual blockchain interaction
    if !config.trading.live_trading {
        info!(
            "[{}] 📝 PAPER TRADING: Simulating BUY order for {} on {} - ${:.2} USD for {} tokens",
            &signal_metadata.correlation_id[..8],
            symbol,
            network_name,
            position_size,
            symbol
        );

        // Calculate approximate token amount using current price
        let position_size_decimal =
            crate::core::utils::f64_to_decimal(position_size, "position size")
                .map_err(|e| Error::InvalidInput(format!("{} for {}", e, token_id)))?;

        let approximate_token_amount = if entry_price > Decimal::ZERO {
            position_size_decimal / entry_price
        } else {
            Decimal::ZERO
        };

        info!(
            "📝 PAPER TRADING: Estimated {} tokens to receive: {:.8}",
            symbol, approximate_token_amount
        );

        // Calculate total simulated fees (reuse already-calculated values)
        let simulated_fees = actual_gas_cost_usd + dex_fee_usd;

        info!(
            "📝 PAPER TRADING: Simulated fees - DEX: ${:.2} ({:.3}%) + Gas: ${:.2} = Total: ${:.2}",
            dex_fee_usd,
            dex_fee_pct * 100.0,
            actual_gas_cost_usd,
            simulated_fees
        );

        // Create position in paper trading with simulated fees
        positions::open_position(
            event_router,
            &token_data,
            position_size_decimal,
            approximate_token_amount,
            Some(simulated_fees), // Include simulated fees for realistic P&L
            &signal_metadata.correlation_id,
        )
        .await?;

        return Ok(());
    }

    // Real trading execution
    info!(
        "💰 REAL TRADING: Executing BUY order for {} on {} - ${:.2} USD",
        symbol, network_name, position_size
    );

    // Validate blockchain price matches API price
    let blockchain_price = match dex_client.get_token_price_usd(&token_data.id).await {
        Ok(price) => crate::core::utils::f64_to_decimal(price, "blockchain price")
            .map_err(|e| Error::InvalidInput(format!("{} for {}", e, token_id)))?,
        Err(e) => {
            error!(
                "Failed to get blockchain price for {} before execution: {}",
                token_id, e
            );
            entry_price
        }
    };

    let discrepancy_result = validate_price_discrepancy(
        entry_price,
        blockchain_price,
        MAX_PRICE_DISCREPANCY_THRESHOLD,
        &token_id,
        &signal_metadata.correlation_id,
    );

    if !discrepancy_result.is_valid {
        if ENABLE_PRICE_CROSS_CHECK {
            error!(
                "CRITICAL: Rejecting BUY order for {} due to pre-execution price discrepancy: {}",
                symbol,
                discrepancy_result
                    .reason
                    .as_ref()
                    .unwrap_or(&"Unknown".to_string())
            );
            error!(
                "❌ External Price: ${:.8}, Blockchain Price: ${:.8}",
                entry_price, blockchain_price
            );
            return Ok(());
        } else {
            warn!(
                "Pre-execution price discrepancy detected for {}: {} (cross-check disabled, continuing)",
                token_id,
                discrepancy_result.reason.as_ref().unwrap_or(&"Unknown".to_string())
            );
        }
    }

    // **REAL DEX INTEGRATION**: Execute actual WETH -> Target Token swap
    info!(
        "REAL TRADING: Executing validated DEX swap: WETH -> {} for ${:.2}",
        token_id, position_size
    );

    // Ensure we have enough WETH (auto-wrap ETH if needed)
    // Validates wrap gas cost against same limits as the actual swap
    let eth_price = match dex_client.get_token_price_usd(&base_token_address).await {
        Ok(price) => price,
        Err(e) => {
            error!("Failed to get ETH price for balance check: {}", e);
            return Err(Error::Trading(format!(
                "Cannot check WETH balance without ETH price: {}",
                e
            )));
        }
    };

    let required_weth = position_size / eth_price;
    info!(
        "💰 Checking WETH balance: need {:.4} WETH (${:.2} at ${:.2}/ETH)",
        required_weth, position_size, eth_price
    );

    if let Err(e) = dex_client
        .ensure_weth_balance(
            required_weth,
            position_size,
            config.trading.transaction_priority,
        )
        .await
    {
        error!("Failed to ensure WETH balance: {}", e);
        return Err(Error::Trading(format!(
            "Cannot prepare WETH for trade: {}",
            e
        )));
    }

    // Calculate price limit from signal price and max deviation
    let price_limit =
        Some(signal_metadata.signal_price * (1.0 + config.trading.max_execution_price_deviation));

    // Execute the swap
    let tx_result = dex_client
        .execute_swap(crate::infrastructure::dex::SwapParams {
            token_in: &base_token_address,
            token_out: &token_data.id,
            amount_in: position_size,
            slippage_tolerance: crate::core::constants::DEFAULT_SLIPPAGE_TOLERANCE,
            price_limit,
            priority: config.trading.transaction_priority,
            direction: crate::infrastructure::dex::SwapDirection::Buy,
        })
        .await;

    match tx_result {
        Ok(tx_details) => {
            // Convert WETH amount to USD using current ETH price
            let eth_price_usd = match dex_client.get_token_price_usd(&base_token_address).await {
                Ok(price) => price,
                Err(e) => {
                    error!("Failed to get ETH price for USD conversion: {}", e);
                    return Err(Error::Trading(format!(
                        "Cannot convert WETH to USD for position tracking: {}",
                        e
                    )));
                }
            };
            let actual_usd_spent_f64 = tx_details.amount_in * eth_price_usd;

            info!(
                "📝 SUBMITTED TX: {} | Token: {} | Spent: {:.4} WETH (${:.2} USD) -> {:.8} tokens",
                tx_details.tx_id,
                token_data.symbol,
                tx_details.amount_in,
                actual_usd_spent_f64,
                tx_details.amount_out
            );

            // Create position with actual amounts from the DEX transaction
            let actual_usd_spent =
                crate::core::utils::f64_to_decimal(actual_usd_spent_f64, "amount_in_usd")
                    .map_err(|e| Error::InvalidInput(format!("{} for {}", e, token_id)))?;
            let actual_tokens_received =
                crate::core::utils::f64_to_decimal(tx_details.amount_out, "amount_out")
                    .map_err(|e| Error::InvalidInput(format!("{} for {}", e, token_id)))?;

            positions::open_position(
                event_router,
                &token_data,
                actual_usd_spent,
                actual_tokens_received,
                Some(tx_details.fees_paid),
                &signal_metadata.correlation_id,
            )
            .await?;
        }
        Err(e) => {
            error!(
                "❌ DEX swap failed for {}: {}. Position NOT created.",
                token_id, e
            );

            // CRITICAL: Log failed transaction for tracking (business logic only)
            log_transaction_failure_info(&token_data, position_size, &e);

            return Err(Error::Trading(format!(
                "Failed to execute BUY order for {}: {}",
                token_id, e
            )));
        }
    }

    Ok(())
}

/// Execute a sell order
async fn execute_sell(
    token_repo: &Arc<TokenRepository>,
    position_repo: &Arc<PositionRepository>,
    dex_client: &DexClient,
    event_router: &Arc<EventRouter>,
    config: &Arc<Config>,
    token_id: String,
    signal_metadata: crate::events::SignalMetadata,
) -> Result<()> {
    // Get the position from database
    let (_position_id, position_data) =
        match position_repo.get_position_by_token_id(&token_id).await? {
            Some(pos) => pos,
            None => {
                warn!("No position found for {} in sell order execution", token_id);
                return Ok(());
            }
        };

    info!(
        "Executing SELL order for {} (Position: {} tokens @ ${:.8})",
        position_data.token_id, position_data.size, position_data.entry_price
    );

    // Get current token data
    let token_data = match token_repo.get_token_price_stats(&token_id).await {
        Ok(data) => data,
        Err(e) => {
            error!(
                "Failed to get token data for sell order {}: {}",
                token_id, e
            );
            return Err(e);
        }
    };

    let current_price = token_data.price_usd;
    let symbol = token_data.symbol.to_uppercase();

    let order_validation = validate_sell_order(&token_data, position_data.size);
    if !order_validation.is_valid {
        error!(
            "Sell order validation failed for {}: {}",
            token_id,
            order_validation
                .reason
                .unwrap_or_else(|| "Unknown reason".to_string())
        );
        return Ok(());
    }

    // Validate signal price hasn't moved too much
    let signal_price =
        crate::core::utils::f64_to_decimal(signal_metadata.signal_price, "signal price")
            .map_err(|e| Error::InvalidInput(format!("{} for {}", e, token_id)))?;

    let price_discrepancy_result = validate_price_discrepancy(
        signal_price,
        current_price,
        config.trading.max_execution_price_deviation,
        &token_id,
        &signal_metadata.correlation_id,
    );

    if !price_discrepancy_result.is_valid {
        error!(
            "CRITICAL: Rejecting SELL order for {} due to excessive price movement since signal generation: {}",
            symbol, price_discrepancy_result.reason.as_ref().unwrap_or(&"Unknown".to_string())
        );
        error!(
            "❌ Signal Price: ${:.8}, Current Price: ${:.8}, Strategy: {}",
            signal_metadata.signal_price, current_price, signal_metadata.strategy_name
        );
        return Ok(());
    }

    info!(
        "Pre-execution price validation passed for SELL {}: Signal=${:.8}, Current=${:.8}, Strategy={}",
        symbol, signal_metadata.signal_price, current_price, signal_metadata.strategy_name
    );

    // Calculate expected USD value (used for paper trading estimates)
    let position_size_decimal =
        crate::core::utils::f64_to_decimal(position_data.size, "position size")
            .map_err(|e| Error::InvalidInput(format!("{} for {}", e, position_data.token_id)))?;
    let expected_usd_value = position_size_decimal * current_price;

    info!(
        "💰 Expected USD value from sale: ${:.2} (Current price: ${:.8})",
        expected_usd_value, current_price
    );

    // Convert expected USD value to f64 for fee calculations
    let expected_usd_f64 =
        crate::core::utils::decimal_to_f64(expected_usd_value, "expected USD value")
            .map_err(|e| Error::InvalidInput(format!("{} for {}", e, token_id)))?;

    // Estimate gas cost for this swap (for both live and paper trading)
    let base_token_address = config.dex.base_token_address();
    let gas_fee_eth = match dex_client
        .estimate_swap_gas_fee(&token_data.id, &base_token_address, expected_usd_f64)
        .await
    {
        Ok(fee) => fee,
        Err(e) => {
            warn!(
                "Failed to estimate gas fee for SELL {}: {}. Using fallback.",
                symbol, e
            );
            0.01 // Fallback: ~0.01 ETH
        }
    };

    let eth_price_usd = match dex_client.get_eth_price_usd().await {
        Ok(price) => price,
        Err(e) => {
            warn!(
                "Failed to get ETH price for SELL {}: {}. Using fallback $3000.",
                symbol, e
            );
            3000.0
        }
    };

    let actual_gas_cost_usd = gas_fee_eth * eth_price_usd;

    // Get DEX fee from actual pool that would be used (for both live and paper trading)
    let dex_fee_pct = match dex_client
        .estimate_swap_output(&token_data.id, &base_token_address, expected_usd_f64)
        .await
    {
        Ok((_expected_output, fee_pct)) => {
            debug!(
                "📊 Pool fee for {} -> {}: {:.3}%",
                &token_data.id[..10],
                base_token_address,
                fee_pct * 100.0
            );
            fee_pct
        }
        Err(e) => {
            warn!(
                "Could not get pool fee for {}: {}. Using fallback 0.3%",
                symbol, e
            );
            V3_STANDARD_FEE_PCT
        }
    };
    let dex_fee_usd = expected_usd_f64 * dex_fee_pct;

    let network_name = get_current_network_name(config);

    if !config.trading.live_trading {
        info!(
            "📝 PAPER TRADING: Simulating SELL order for {} on {} - {} tokens for ~${:.2}",
            symbol, network_name, position_data.size, expected_usd_value
        );

        // Calculate total simulated fees (reuse already-calculated values)
        let simulated_fees = actual_gas_cost_usd + dex_fee_usd;

        info!(
            "📝 PAPER TRADING: Simulated fees - DEX: ${:.2} ({:.3}%) + Gas: ${:.2} = Total: ${:.2}",
            dex_fee_usd,
            dex_fee_pct * 100.0,
            actual_gas_cost_usd,
            simulated_fees
        );

        // Close position in paper trading with simulated fees
        positions::close_position(
            event_router,
            &position_data,
            expected_usd_value,
            current_price,
            Some(simulated_fees), // Include simulated fees for realistic P&L
            &signal_metadata.correlation_id,
        )
        .await?;

        return Ok(());
    }

    // Real trading execution
    info!(
        "💰 REAL TRADING: Executing SELL order for {} on {} - {} tokens",
        symbol, network_name, position_data.size
    );

    // Validate blockchain price matches API price
    let blockchain_price = match dex_client.get_token_price_usd(&token_id).await {
        Ok(price) => crate::core::utils::f64_to_decimal(price, "blockchain price")
            .map_err(|e| Error::InvalidInput(format!("{} for {}", e, token_id)))?,
        Err(e) => {
            error!(
                "Failed to get blockchain price for {} before sell execution: {}",
                token_id, e
            );
            current_price
        }
    };

    let discrepancy_result = validate_price_discrepancy(
        current_price,
        blockchain_price,
        MAX_PRICE_DISCREPANCY_THRESHOLD,
        &token_id,
        &signal_metadata.correlation_id,
    );

    if !discrepancy_result.is_valid {
        if ENABLE_PRICE_CROSS_CHECK {
            error!(
                "CRITICAL: Rejecting SELL order for {} due to pre-execution price discrepancy: {}",
                symbol,
                discrepancy_result
                    .reason
                    .as_ref()
                    .unwrap_or(&"Unknown".to_string())
            );
            error!(
                "❌ External Price: ${:.8}, Blockchain Price: ${:.8}",
                current_price, blockchain_price
            );
            return Ok(());
        } else {
            warn!(
                "Pre-execution price discrepancy detected for {}: {} (cross-check disabled, continuing)",
                token_id,
                discrepancy_result.reason.as_ref().unwrap_or(&"Unknown".to_string())
            );
        }
    }

    // **REAL DEX INTEGRATION**: Execute actual Token -> WETH swap
    info!(
        "REAL TRADING: Executing validated DEX swap: {} -> WETH ({} tokens)",
        token_id, position_data.size
    );

    // Calculate price limit from signal price and max deviation
    // For SELL, we want minimum price (signal_price - deviation)
    let price_limit =
        Some(signal_metadata.signal_price * (1.0 - config.trading.max_execution_price_deviation));

    // Execute the swap
    let base_token_address = config.dex.base_token_address();
    let tx_result = dex_client
        .execute_swap(crate::infrastructure::dex::SwapParams {
            token_in: &token_data.id,
            token_out: &base_token_address,
            amount_in: position_data.size,
            slippage_tolerance: crate::core::constants::DEFAULT_SLIPPAGE_TOLERANCE,
            price_limit,
            priority: config.trading.transaction_priority,
            direction: crate::infrastructure::dex::SwapDirection::Sell,
        })
        .await;

    match tx_result {
        Ok(tx_details) => {
            // Convert WETH amount to USD using current ETH price
            let eth_price_usd = match dex_client.get_token_price_usd(&base_token_address).await {
                Ok(price) => price,
                Err(e) => {
                    error!("Failed to get ETH price for USD conversion: {}", e);
                    return Err(Error::Trading(format!(
                        "Cannot convert WETH to USD for P&L tracking: {}",
                        e
                    )));
                }
            };
            let actual_usd_received_f64 = tx_details.amount_out * eth_price_usd;

            info!(
                "DEX swap completed! TX: {} | Received: {:.4} WETH (${:.2} USD) | Actual price: ${:.8}",
                tx_details.tx_id, tx_details.amount_out,
                actual_usd_received_f64, tx_details.actual_price
            );

            // Close position with actual execution results (in USD)
            let actual_usd_received =
                crate::core::utils::f64_to_decimal(actual_usd_received_f64, "amount_out_usd")
                    .map_err(|e| Error::InvalidInput(format!("{} for {}", e, token_id)))?;
            let actual_exit_price =
                crate::core::utils::f64_to_decimal(tx_details.actual_price, "actual_price")
                    .map_err(|e| Error::InvalidInput(format!("{} for {}", e, token_id)))?;

            positions::close_position(
                event_router,
                &position_data,
                actual_usd_received,
                actual_exit_price,
                Some(tx_details.fees_paid), // Pass actual fees from transaction
                &signal_metadata.correlation_id,
            )
            .await?;
        }
        Err(e) => {
            error!(
                "❌ DEX swap failed for {}: {}. Position preserved.",
                token_id, e
            );
            return Err(Error::Trading(format!(
                "Failed to execute SELL order for {}: {}",
                token_id, e
            )));
        }
    }

    Ok(())
}

/// Log failed transaction - BUSINESS LOGIC ONLY
pub fn log_transaction_failure_info(
    token_data: &TokenData,
    position_size: f64,
    error: &Error,
) -> Option<String> {
    // Extract tx hash from error if available
    let error_msg = format!("{}", error);
    let tx_hash = if error_msg.contains("0x") {
        // Try to extract tx hash from error message
        error_msg
            .split("0x")
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .map(|s| format!("0x{}", s))
    } else {
        None
    };

    // Enhanced logging for failed transactions - no database persistence
    warn!(
        "📝 FAILED TX: {:?} | Token: {} | Size: ${:.2} | Error: {}",
        tx_hash.as_deref().unwrap_or("unknown"),
        token_data.symbol,
        position_size,
        error_msg,
    );

    // Return tx_hash for caller to use if needed
    tx_hash
}
