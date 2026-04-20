use crate::core::constants::{
    EXPOSURE_REDUCTION_THRESHOLD, LOSS_DRAWDOWN_REDUCTION_THRESHOLD, MAX_EXPOSURE_REDUCTION,
    MAX_LOSS_DRAWDOWN_REDUCTION,
};
use crate::core::domain::market::TokenMetrics;
use log::debug;

// Simplified Risk Assessment System
//
// This module provides simple, transparent risk management:
// 1. Fixed position sizing (max_position_size × portfolio_factors)
// 2. Simple volatility check to filter risky tokens
// 3. Portfolio-wide risk factors (daily loss, drawdown, exposure)

/// Portfolio risk factors that affect all positions
#[derive(Debug, Clone)]
pub struct PortfolioRiskFactors {
    pub daily_loss_factor: f64,
    pub drawdown_factor: f64,
    pub exposure_factor: f64,
}

/// Position sizing calculation result
#[derive(Debug, Clone)]
pub struct PositionSizeResult {
    pub final_size_usd: f64,
    pub base_size_usd: f64,
    pub portfolio_factor: f64,
    pub calculation_details: String,
}

/// Check if token volatility is within acceptable limits
/// Returns true if token passes volatility check, false if too volatile
pub fn check_token_volatility(token_stats: &TokenMetrics, max_volatility_24h: f64) -> bool {
    let volatility = token_stats.price_change_24h.abs();

    if volatility > max_volatility_24h {
        debug!(
            "Token {} rejected due to high volatility: {:.1}% (max: {:.1}%)",
            token_stats.symbol, volatility, max_volatility_24h
        );
        return false;
    }

    true
}

/// Validate market data for position sizing
/// Returns true if token has sufficient data for risk assessment
pub fn has_valid_market_data(token: &TokenMetrics) -> bool {
    token.volume_24h > 0.0 && token.price_usd > 0.0 && !token.price_change_24h.is_nan()
}

/// Compute final position size using fixed sizing with portfolio factors
///
/// Parameters:
/// - max_position_size: Maximum position size in USD
/// - min_position_size: Minimum position size in USD
/// - portfolio_factors: Portfolio-wide risk factors (daily loss, drawdown, exposure)
pub fn compute_position_size(
    max_position_size: f64,
    min_position_size: f64,
    portfolio_factors: &PortfolioRiskFactors,
) -> PositionSizeResult {
    // Calculate portfolio-wide factor
    let portfolio_factor = portfolio_factors.daily_loss_factor
        * portfolio_factors.drawdown_factor
        * portfolio_factors.exposure_factor;

    // Apply portfolio factors to max position size
    let position_size = max_position_size * portfolio_factor;

    // Clamp to configured min/max bounds
    let final_size = position_size.clamp(min_position_size, max_position_size);

    let calculation_details = format!(
        "max_position={:.2} * portfolio_factor={:.3} = {:.2} (clamped to {:.2})",
        max_position_size, portfolio_factor, position_size, final_size
    );

    debug!("Position size calculation: {}", calculation_details);

    PositionSizeResult {
        final_size_usd: final_size,
        base_size_usd: max_position_size,
        portfolio_factor,
        calculation_details,
    }
}

/// Calculate portfolio risk factors that affect all positions
pub fn calculate_portfolio_risk_factors(
    current_daily_loss: f64,
    max_daily_loss_limit: f64,
    current_drawdown: f64,
    max_drawdown_limit: f64,
    current_exposure: f64,
    max_exposure_limit: f64,
) -> PortfolioRiskFactors {
    // Daily loss factor (reduce position sizes as we approach daily loss limit)
    let daily_loss_factor = if max_daily_loss_limit > 0.0 {
        let loss_ratio = (current_daily_loss / max_daily_loss_limit).abs();
        if loss_ratio > LOSS_DRAWDOWN_REDUCTION_THRESHOLD {
            1.0 - (loss_ratio - LOSS_DRAWDOWN_REDUCTION_THRESHOLD).min(MAX_LOSS_DRAWDOWN_REDUCTION)
        } else {
            1.0
        }
    } else {
        1.0
    };

    // Drawdown factor (reduce position sizes as we approach drawdown limit)
    let drawdown_factor = if max_drawdown_limit > 0.0 {
        let drawdown_ratio = (current_drawdown / max_drawdown_limit).abs();
        if drawdown_ratio > LOSS_DRAWDOWN_REDUCTION_THRESHOLD {
            1.0 - (drawdown_ratio - LOSS_DRAWDOWN_REDUCTION_THRESHOLD)
                .min(MAX_LOSS_DRAWDOWN_REDUCTION)
        } else {
            1.0
        }
    } else {
        1.0
    };

    // Exposure factor (reduce position sizes as we approach exposure limit)
    let exposure_factor = if max_exposure_limit > 0.0 {
        let exposure_ratio = current_exposure / max_exposure_limit;
        if exposure_ratio > EXPOSURE_REDUCTION_THRESHOLD {
            1.0 - (exposure_ratio - EXPOSURE_REDUCTION_THRESHOLD).min(MAX_EXPOSURE_REDUCTION)
        } else {
            1.0
        }
    } else {
        1.0
    };

    PortfolioRiskFactors {
        daily_loss_factor: daily_loss_factor.clamp(0.1, 1.0),
        drawdown_factor: drawdown_factor.clamp(0.1, 1.0),
        exposure_factor: exposure_factor.clamp(0.1, 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::domain::market::TokenMetrics;
    use chrono::Utc;

    fn token(price: f64, volume: f64, change_24h: f64) -> TokenMetrics {
        TokenMetrics {
            id: "test-token".to_string(),
            symbol: "TEST".to_string(),
            name: "Test Token".to_string(),
            decimals: 18,
            price_usd: price,
            price_change_24h: change_24h,
            volume_24h: volume,
            chain: None,
            last_updated: Utc::now(),
        }
    }

    // ---- check_token_volatility ----

    #[test]
    fn volatility_within_limit_passes() {
        assert!(check_token_volatility(&token(1.0, 1000.0, 15.0), 30.0));
    }

    #[test]
    fn volatility_at_limit_passes() {
        assert!(check_token_volatility(&token(1.0, 1000.0, 30.0), 30.0));
    }

    #[test]
    fn volatility_above_limit_rejected() {
        assert!(!check_token_volatility(&token(1.0, 1000.0, 30.1), 30.0));
    }

    #[test]
    fn negative_price_change_uses_absolute_value() {
        // -35% swing should be rejected just like +35%
        assert!(!check_token_volatility(&token(1.0, 1000.0, -35.0), 30.0));
    }

    // ---- has_valid_market_data ----

    #[test]
    fn valid_market_data_passes() {
        assert!(has_valid_market_data(&token(1.0, 50000.0, 2.5)));
    }

    #[test]
    fn zero_price_rejected() {
        assert!(!has_valid_market_data(&token(0.0, 50000.0, 2.5)));
    }

    #[test]
    fn zero_volume_rejected() {
        assert!(!has_valid_market_data(&token(1.0, 0.0, 2.5)));
    }

    #[test]
    fn nan_price_change_rejected() {
        assert!(!has_valid_market_data(&token(1.0, 50000.0, f64::NAN)));
    }

    // ---- compute_position_size ----

    #[test]
    fn full_factors_yields_max_size() {
        let factors = PortfolioRiskFactors {
            daily_loss_factor: 1.0,
            drawdown_factor: 1.0,
            exposure_factor: 1.0,
        };
        let result = compute_position_size(100.0, 10.0, &factors);
        assert_eq!(result.final_size_usd, 100.0);
        assert_eq!(result.portfolio_factor, 1.0);
    }

    #[test]
    fn halved_factors_reduce_size() {
        let factors = PortfolioRiskFactors {
            daily_loss_factor: 0.5,
            drawdown_factor: 1.0,
            exposure_factor: 1.0,
        };
        let result = compute_position_size(100.0, 10.0, &factors);
        assert_eq!(result.final_size_usd, 50.0);
    }

    #[test]
    fn very_low_factor_clamped_to_minimum() {
        // portfolio_factor = 0.1 * 0.1 * 0.1 = 0.001 → 0.1 USD → below min (10.0) → error? No, clamp.
        // compute_position_size clamps to min_position_size
        let factors = PortfolioRiskFactors {
            daily_loss_factor: 0.1,
            drawdown_factor: 0.1,
            exposure_factor: 0.1,
        };
        let result = compute_position_size(100.0, 10.0, &factors);
        assert_eq!(result.final_size_usd, 10.0); // clamped to min
    }

    // ---- calculate_portfolio_risk_factors ----

    #[test]
    fn no_losses_yields_all_ones() {
        let f = calculate_portfolio_risk_factors(0.0, 10.0, 0.0, 20.0, 0.0, 1000.0);
        assert_eq!(f.daily_loss_factor, 1.0);
        assert_eq!(f.drawdown_factor, 1.0);
        assert_eq!(f.exposure_factor, 1.0);
    }

    #[test]
    fn loss_below_threshold_no_reduction() {
        // 40% of daily loss limit → ratio = 0.4 < LOSS_DRAWDOWN_REDUCTION_THRESHOLD (0.5)
        let f = calculate_portfolio_risk_factors(4.0, 10.0, 0.0, 20.0, 0.0, 1000.0);
        assert_eq!(f.daily_loss_factor, 1.0);
    }

    #[test]
    fn loss_above_threshold_reduces_factor() {
        // 80% of daily loss limit → ratio = 0.8 > threshold (0.5) → reduction applies
        let f = calculate_portfolio_risk_factors(8.0, 10.0, 0.0, 20.0, 0.0, 1000.0);
        assert!(f.daily_loss_factor < 1.0);
        assert!(f.daily_loss_factor >= 0.1); // clamped floor
    }

    #[test]
    fn high_exposure_reduces_factor() {
        // 90% exposure → ratio = 0.9 > EXPOSURE_REDUCTION_THRESHOLD (0.8) → reduction
        let f = calculate_portfolio_risk_factors(0.0, 10.0, 0.0, 20.0, 900.0, 1000.0);
        assert!(f.exposure_factor < 1.0);
    }
}
