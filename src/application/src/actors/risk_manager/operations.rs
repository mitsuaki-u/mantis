/// Risk Manager Operations
///
/// This module contains reusable operational logic for the RiskManagerActor:
/// - State synchronization with features layer
/// - Position size calculations
/// - Risk metric updates
/// - Constraint enforcement
use super::RiskManagerActor;
use crate::application::errors::Result;
use crate::core::domain::market::TokenMetrics;
use crate::core::domain::trading::Signal;
use crate::core::risk::{
    calculate_updated_daily_loss, calculate_updated_drawdown, check_token_volatility,
    reset_daily_loss,
};
use log::{debug, error, info, warn};
use std::convert::TryFrom;

/// Check token volatility to validate if it's safe to trade
/// No longer calculates complex risk scores - just checks volatility
pub async fn check_token_risk(actor: &RiskManagerActor, token_id: &str) -> Result<bool> {
    debug!("Checking volatility for token: {}", token_id);

    // Get token data from repository
    match actor.token_repo.get_token_price_stats(token_id).await {
        Ok(token_stats) => {
            // Convert TokenData to TokenMetrics (handles Decimal to f64 conversion)
            match TokenMetrics::try_from(&token_stats) {
                Ok(metrics_f64) => {
                    // Simple volatility check
                    let max_volatility = actor.config.trading.max_volatility_24h;
                    let passes_check = check_token_volatility(&metrics_f64, max_volatility);

                    if passes_check {
                        debug!(
                            "Token {} passed volatility check: {:.1}% (max: {:.1}%)",
                            token_id,
                            metrics_f64.price_change_24h.abs(),
                            max_volatility
                        );
                    }

                    Ok(passes_check)
                }
                Err(e) => {
                    warn!(
                        "Failed to convert token stats to metrics for {}: {}",
                        token_id, e
                    );
                    // Reject tokens we can't assess
                    Ok(false)
                }
            }
        }
        Err(e) => {
            warn!("Failed to get token stats for {}: {}", token_id, e);
            // Reject tokens we can't assess
            Ok(false)
        }
    }
}

/// Update risk metrics based on PnL
/// Uses pure functions from features layer to calculate new values
pub async fn update_risk_metrics(actor: &mut RiskManagerActor, pnl: f64) -> Result<()> {
    debug!("Updating risk metrics with PnL: ${:.2}", pnl);

    // Use pure functions from features layer
    // Daily loss and drawdown are calculated as percentages of max_total_exposure
    let portfolio_value = actor.config.trading.max_total_exposure;
    let new_daily_loss =
        calculate_updated_daily_loss(actor.risk_metrics.current_daily_loss, pnl, portfolio_value);
    let new_drawdown =
        calculate_updated_drawdown(actor.risk_metrics.current_drawdown, pnl, portfolio_value);

    // Update actor state
    actor.risk_metrics.current_daily_loss = new_daily_loss;
    actor.risk_metrics.current_drawdown = new_drawdown;

    if pnl < 0.0 {
        info!(
            "📉 Updated daily loss: {:.2}% of portfolio (${:.2} loss, max limit: {:.1}%)",
            new_daily_loss * 100.0,
            pnl.abs(),
            actor.risk_metrics.max_daily_loss_limit * 100.0
        );
        info!(
            "📉 Updated drawdown: {:.2}% of portfolio (max limit: {:.1}%)",
            new_drawdown * 100.0,
            actor.risk_metrics.max_drawdown_limit * 100.0
        );
    }

    Ok(())
}

/// Reset daily metrics (typically called at start of new trading day)
/// Uses pure function from features layer
pub fn reset_daily_metrics(actor: &mut RiskManagerActor) {
    actor.risk_metrics.current_daily_loss = reset_daily_loss();
    info!("Reset daily loss metrics");
}

/// Calculate position size using simplified fixed sizing
pub async fn calculate_position_size(
    actor: &RiskManagerActor,
    token_id: &str,
    signal: &Signal,
    token_metrics: &TokenMetrics,
) -> Result<f64> {
    if signal.is_buy() {
        // Validate market data
        if !crate::core::risk::has_valid_market_data(token_metrics) {
            warn!(
                "Insufficient market data for {} (price: {}, volume: {})",
                token_metrics.symbol, token_metrics.price_usd, token_metrics.volume_24h
            );
            return Err(crate::application::errors::Error::Trading(
                "Insufficient market data".to_string(),
            ));
        }

        // Check volatility
        let max_volatility = actor.config.trading.max_volatility_24h;
        if !check_token_volatility(token_metrics, max_volatility) {
            return Err(crate::application::errors::Error::Trading(format!(
                "Token volatility too high: {:.1}%",
                token_metrics.price_change_24h.abs()
            )));
        }

        // Calculate portfolio risk factors
        let current_exposure = actor.get_current_total_value();
        let portfolio_factors = crate::core::risk::calculate_portfolio_risk_factors(
            actor.risk_metrics.current_daily_loss,
            actor.risk_metrics.max_daily_loss_limit,
            actor.risk_metrics.current_drawdown,
            actor.risk_metrics.max_drawdown_limit,
            current_exposure,
            actor.config.trading.max_total_exposure,
        );

        // Compute position size using fixed sizing
        let result = crate::core::risk::compute_position_size(
            actor.config.trading.max_position_size,
            actor.config.trading.min_position_size,
            &portfolio_factors,
        );

        debug!(
            "Position size for {}: {}",
            token_metrics.symbol, result.calculation_details
        );

        Ok(result.final_size_usd)
    } else {
        // For sell signals, use existing position size (in USD)
        match actor.position_repo.get_position_by_token_id(token_id).await {
            Ok(Some((_position_id, position))) => {
                // Calculate USD value: token_quantity * current_price
                let position_value_usd = position.size * token_metrics.price_usd;
                debug!(
                    "SELL signal for {}: {} tokens @ ${:.8} = ${:.2}",
                    token_id, position.size, token_metrics.price_usd, position_value_usd
                );
                Ok(position_value_usd)
            }
            Ok(None) => {
                info!("No position for {} to sell - ignoring signal", token_id);
                Err(crate::application::errors::Error::Trading(
                    "No position to sell".to_string(),
                ))
            }
            Err(e) => {
                error!("Failed to get position for {}: {}", token_id, e);
                Err(e)
            }
        }
    }
}

/// Apply constraints to position size (max trade risk, minimum size)
/// This is a thin wrapper around the domain function that adds logging
pub fn apply_position_size_constraints(
    actor: &RiskManagerActor,
    token_id: &str,
    position_size: f64,
    signal: &Signal,
) -> Result<f64> {
    match crate::core::risk::apply_position_size_constraints(
        position_size,
        signal,
        actor.config.trading.max_total_exposure,
        actor.config.trading.max_trade_risk_pct,
        actor.config.trading.max_position_size,
    ) {
        Ok(size) => Ok(size),
        Err(e) => {
            warn!("Position size constraint failed for {}: {}", token_id, e);
            Err(crate::application::errors::Error::Trading(e.to_string()))
        }
    }
}
