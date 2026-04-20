use crate::core::constants::{
    DRAWDOWN_IMPACT_WEIGHT, LOSS_DRAWDOWN_REDUCTION_THRESHOLD, LOSS_IMPACT_WEIGHT,
};
use std::collections::HashMap;

/// Pure functional risk metrics calculations
/// All functions now take individual parameters instead of mutable state
///
/// Calculate updated daily loss as percentage of portfolio (pure function)
///
/// # Arguments
/// * `current_loss_pct` - Current daily loss as percentage (0.05 = 5%)
/// * `pnl_usd` - Profit/loss in USD (negative for losses)
/// * `portfolio_value` - Total portfolio value in USD (max_total_exposure)
///
/// # Returns
/// Updated daily loss as percentage
pub fn calculate_updated_daily_loss(
    current_loss_pct: f64,
    pnl_usd: f64,
    portfolio_value: f64,
) -> f64 {
    if pnl_usd < 0.0 && portfolio_value > 0.0 {
        // Convert absolute loss to percentage and add to current loss
        let loss_pct = pnl_usd.abs() / portfolio_value;
        current_loss_pct + loss_pct
    } else {
        current_loss_pct
    }
}

/// Calculate updated drawdown as percentage of portfolio (pure function)
///
/// Drawdown represents the peak-to-trough decline, similar to daily loss but
/// typically tracks the largest cumulative loss from a peak.
///
/// # Arguments
/// * `current_drawdown_pct` - Current drawdown as percentage (0.10 = 10%)
/// * `pnl_usd` - Profit/loss in USD (negative for losses)
/// * `portfolio_value` - Total portfolio value in USD (max_total_exposure)
///
/// # Returns
/// Updated drawdown as percentage
pub fn calculate_updated_drawdown(
    current_drawdown_pct: f64,
    pnl_usd: f64,
    portfolio_value: f64,
) -> f64 {
    if pnl_usd < 0.0 && portfolio_value > 0.0 {
        // Convert absolute loss to percentage and add to current drawdown
        let loss_pct = pnl_usd.abs() / portfolio_value;
        current_drawdown_pct + loss_pct
    } else {
        current_drawdown_pct
    }
}

/// Clamp risk score to valid range (pure function)
pub fn clamp_risk_score(risk_score: f64) -> f64 {
    risk_score.clamp(0.0, 1.0)
}

/// Reset daily loss to zero (pure function - returns new value)
pub fn reset_daily_loss() -> f64 {
    0.0
}

/// Calculate portfolio-wide risk metrics (pure function)
pub fn calculate_portfolio_risk(
    risk_scores: &HashMap<String, f64>,
    current_daily_loss: f64,
    max_daily_loss_limit: f64,
    current_drawdown: f64,
    max_drawdown_limit: f64,
) -> f64 {
    if risk_scores.is_empty() {
        return 0.5; // Default medium risk
    }

    let total_risk: f64 = risk_scores.values().sum();
    let average_risk = total_risk / risk_scores.len() as f64;

    // Factor in current losses
    let loss_factor = if max_daily_loss_limit > 0.0 {
        current_daily_loss / max_daily_loss_limit
    } else {
        0.0
    };

    let drawdown_factor = if max_drawdown_limit > 0.0 {
        current_drawdown / max_drawdown_limit
    } else {
        0.0
    };

    // Increase risk score based on current losses
    let adjusted_risk = average_risk
        + (loss_factor * LOSS_IMPACT_WEIGHT)
        + (drawdown_factor * DRAWDOWN_IMPACT_WEIGHT);
    adjusted_risk.clamp(0.0, 1.0)
}

/// Calculate portfolio-wide risk factor that affects all position sizes (pure function)
pub fn calculate_portfolio_risk_factor(
    current_daily_loss: f64,
    max_daily_loss_limit: f64,
    current_drawdown: f64,
    max_drawdown_limit: f64,
) -> f64 {
    let mut risk_factor = 1.0;

    // Reduce position sizes if we're approaching daily loss limit
    if max_daily_loss_limit > 0.0 {
        let loss_ratio = current_daily_loss / max_daily_loss_limit;
        if loss_ratio > LOSS_DRAWDOWN_REDUCTION_THRESHOLD {
            risk_factor *= 1.0 - (loss_ratio - LOSS_DRAWDOWN_REDUCTION_THRESHOLD);
            // Reduce by up to 50%
        }
    }

    // Reduce position sizes if we're approaching drawdown limit
    if max_drawdown_limit > 0.0 {
        let drawdown_ratio = current_drawdown / max_drawdown_limit;
        if drawdown_ratio > LOSS_DRAWDOWN_REDUCTION_THRESHOLD {
            risk_factor *= 1.0 - (drawdown_ratio - LOSS_DRAWDOWN_REDUCTION_THRESHOLD);
            // Reduce by up to 50%
        }
    }

    // Ensure we don't go below minimum risk factor
    risk_factor.clamp(0.1, 1.0)
}
