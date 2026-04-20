use crate::core::domain::market::TokenMetrics;
use crate::core::domain::trading::{ExitReason, Position};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use log::{debug, info};

/// Common exit condition utilities for all strategies
pub mod checks {
    use super::*;

    /// Check if position should be exited based on risk parameters
    pub fn check_risk_exit(
        token: &TokenMetrics,
        position: &Position,
        risk_params: Option<(f64, f64)>,
    ) -> Option<ExitReason> {
        let current_price = token.price_usd;
        let price_change_pct = position.calculate_pnl_pct(current_price);

        if let Some((take_profit, stop_loss)) = risk_params {
            if price_change_pct >= take_profit {
                let reason = format!(
                    "Take profit: {:.2}% >= {:.2}%",
                    price_change_pct, take_profit
                );
                info!("💰 {} - {}", token.id, reason);
                return Some(ExitReason::risk_based(&reason));
            }

            let stop_loss_threshold = -stop_loss;
            if price_change_pct <= stop_loss_threshold {
                let reason = format!(
                    "Stop loss: {:.2}% <= {:.2}%",
                    price_change_pct, stop_loss_threshold
                );
                info!("🛑 {} - {}", token.id, reason);
                return Some(ExitReason::risk_based(&reason));
            }
        }

        None
    }

    /// Check trailing stop loss
    pub fn check_trailing_stop(
        position: &Position,
        current_price: f64,
        trailing_stop_pct: f64,
    ) -> Option<ExitReason> {
        if position.highest_price > position.entry_price {
            let drop_from_high_pct =
                ((position.highest_price - current_price) / position.highest_price) * 100.0;

            if drop_from_high_pct >= trailing_stop_pct {
                let price_change_pct = position.calculate_pnl_pct(current_price);
                let reason = format!(
                    "Trailing stop triggered: {:.2}% drop from high ${:.4}, still in profit: {:.2}%",
                    drop_from_high_pct, position.highest_price, price_change_pct
                );
                info!("🔽 {}", reason);
                return Some(ExitReason::strategy_based(&reason));
            }
        }
        None
    }

    /// Check maximum hold time
    pub fn check_max_hold_time(
        position: &Position,
        current_price: f64,
        max_hold_duration: ChronoDuration,
        min_movement_threshold: f64,
    ) -> Option<ExitReason> {
        let now = Utc::now();
        let hold_duration = now - position.entry_time;
        let price_change_pct = position.calculate_pnl_pct(current_price);

        if hold_duration > max_hold_duration && price_change_pct.abs() < min_movement_threshold {
            let reason = format!(
                "Max hold time reached with minimal movement ({:.2}%)",
                price_change_pct
            );
            info!("⏱️ {}", reason);
            return Some(ExitReason::strategy_based(&reason));
        }
        None
    }

    /// Check strategy-specific stop loss
    pub fn check_strategy_stop_loss(
        token: &TokenMetrics,
        position: &Position,
        stop_loss_pct: f64,
    ) -> Option<ExitReason> {
        let current_price = token.price_usd;
        let price_change_pct = position.calculate_pnl_pct(current_price);

        if price_change_pct <= -stop_loss_pct {
            let reason = format!(
                "Strategy stop loss triggered: {:.2}% <= {:.2}%",
                price_change_pct, -stop_loss_pct
            );
            info!("🛑 {}", reason);
            return Some(ExitReason::strategy_based(&reason));
        }
        None
    }

    /// Common exit analysis that can be used by all strategies
    pub fn common_exit_analysis(
        token: &TokenMetrics,
        position: Option<&Position>,
        risk_params: Option<(f64, f64)>,
        stop_loss_pct: f64,
        momentum_threshold: Option<f64>,
    ) -> Option<ExitReason> {
        let position = position?;
        let current_price = token.price_usd;

        if let Some(exit_reason) = check_risk_exit(token, position, risk_params) {
            return Some(exit_reason);
        }

        if let Some(exit_reason) = check_strategy_stop_loss(token, position, stop_loss_pct) {
            return Some(exit_reason);
        }

        // Trailing stop uses 50% of regular stop loss
        let trailing_stop_pct = stop_loss_pct * 0.5;
        if let Some(exit_reason) = check_trailing_stop(position, current_price, trailing_stop_pct) {
            return Some(exit_reason);
        }

        let max_hold_time = ChronoDuration::days(7);
        let min_movement = momentum_threshold.unwrap_or(5.0) / 2.0;
        if let Some(exit_reason) =
            check_max_hold_time(position, current_price, max_hold_time, min_movement)
        {
            return Some(exit_reason);
        }

        let price_change_pct = position.calculate_pnl_pct(current_price);
        debug!(
            "Maintaining position for {} at ${:.4}, P&L: {:.2}%",
            token.id, current_price, price_change_pct
        );

        None
    }
}

/// Utility functions for strategy implementations
pub mod utils {
    use super::*;
    use dashmap::DashMap;
    use std::sync::Arc;

    /// Mark a token as considered with a timestamp
    pub fn mark_token_considered(
        considered_tokens: &Arc<DashMap<String, DateTime<Utc>>>,
        token_id: String,
    ) {
        considered_tokens.insert(token_id, Utc::now());
    }

    /// Check if enough time has passed since last consideration
    pub fn check_cooldown_period(
        considered_tokens: &Arc<DashMap<String, DateTime<Utc>>>,
        token_id: &str,
        cooldown_period: ChronoDuration,
    ) -> bool {
        if let Some(last_considered) = considered_tokens.get(token_id) {
            let time_since_last = Utc::now() - *last_considered;
            time_since_last >= cooldown_period
        } else {
            true // Never considered before
        }
    }

    /// Clean up old entries from consideration map
    pub fn cleanup_old_considerations(
        considered_tokens: &Arc<DashMap<String, DateTime<Utc>>>,
        max_age: ChronoDuration,
    ) {
        let cutoff_time = Utc::now() - max_age;
        considered_tokens.retain(|_, &mut timestamp| timestamp > cutoff_time);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::domain::market::TokenMetrics;
    use crate::core::domain::trading::Position;
    use chrono::{Duration, Utc};

    fn create_test_position(entry_price: f64, current_price: f64, highest_price: f64) -> Position {
        Position {
            token_id: "test-token".to_string(),
            provider_id: "test-provider".to_string(),
            entry_price,
            current_price,
            highest_price,
            size: 100.0,
            unrealized_pnl: 0.0,
            entry_time: Utc::now(),
        }
    }

    fn create_test_token(price: f64) -> TokenMetrics {
        TokenMetrics {
            id: "test-token".to_string(),
            symbol: "TEST".to_string(),
            name: "Test Token".to_string(),
            price_usd: price,
            price_change_24h: 0.0,
            volume_24h: 10000.0,
            decimals: 18,
            chain: Some("ethereum".to_string()),
            last_updated: Utc::now(),
        }
    }

    #[test]
    fn test_check_strategy_stop_loss_triggers_when_below_threshold() {
        let position = create_test_position(100.0, 100.0, 100.0);
        let token = create_test_token(90.0); // 10% drop

        let result = checks::check_strategy_stop_loss(&token, &position, 5.0);
        assert!(result.is_some());
        let exit_reason = result.unwrap();
        assert!(exit_reason.reason.contains("Strategy stop loss triggered"));
    }

    #[test]
    fn test_check_strategy_stop_loss_does_not_trigger_above_threshold() {
        let position = create_test_position(100.0, 100.0, 100.0);
        let token = create_test_token(98.0); // 2% drop (< 5% threshold)

        let result = checks::check_strategy_stop_loss(&token, &position, 5.0);
        assert!(result.is_none());
    }

    #[test]
    fn test_check_trailing_stop_triggers_on_drop_from_high() {
        let position = create_test_position(100.0, 100.0, 120.0); // Went up to 120, now at 100
        let current_price = 110.0; // Dropped from 120 to 110 (8.3% drop)

        let result = checks::check_trailing_stop(&position, current_price, 5.0);
        assert!(result.is_some());
        let exit_reason = result.unwrap();
        assert!(exit_reason.reason.contains("Trailing stop triggered"));
    }

    #[test]
    fn test_check_trailing_stop_does_not_trigger_before_profit() {
        let position = create_test_position(100.0, 100.0, 100.0); // No profit yet
        let current_price = 95.0; // Price dropped but we never had profit

        let result = checks::check_trailing_stop(&position, current_price, 5.0);
        assert!(result.is_none()); // Should not trigger because highest_price == entry_price
    }

    #[test]
    fn test_check_risk_exit_take_profit_triggers() {
        let mut position = create_test_position(100.0, 100.0, 100.0);
        position.current_price = 115.0;
        let token = create_test_token(115.0); // 15% profit

        let risk_params = Some((10.0, 5.0)); // take_profit=10%, stop_loss=5%
        let result = checks::check_risk_exit(&token, &position, risk_params);

        assert!(result.is_some());
        let exit_reason = result.unwrap();
        assert!(exit_reason.reason.contains("Take profit"));
        assert!(exit_reason.is_risk_based);
    }

    #[test]
    fn test_check_risk_exit_stop_loss_triggers() {
        let position = create_test_position(100.0, 100.0, 100.0);
        let token = create_test_token(92.0); // 8% loss

        let risk_params = Some((10.0, 5.0)); // take_profit=10%, stop_loss=5%
        let result = checks::check_risk_exit(&token, &position, risk_params);

        assert!(result.is_some());
        let exit_reason = result.unwrap();
        assert!(exit_reason.reason.contains("Stop loss"));
        assert!(exit_reason.is_risk_based);
    }

    #[test]
    fn test_check_risk_exit_does_not_trigger_within_range() {
        let position = create_test_position(100.0, 100.0, 100.0);
        let token = create_test_token(105.0); // 5% profit (below take_profit threshold with risk tolerance)

        let risk_params = Some((10.0, 5.0)); // take_profit=10%, stop_loss=5%
        let result = checks::check_risk_exit(&token, &position, risk_params);

        assert!(result.is_none());
    }

    #[test]
    fn test_check_max_hold_time_triggers_with_minimal_movement() {
        let mut position = create_test_position(100.0, 100.0, 100.0);
        position.entry_time = Utc::now() - Duration::days(8); // 8 days ago
        let current_price = 101.0; // Only 1% movement

        let max_hold_duration = Duration::days(7);
        let min_movement_threshold = 2.5; // Require at least 2.5% movement

        let result = checks::check_max_hold_time(
            &position,
            current_price,
            max_hold_duration,
            min_movement_threshold,
        );

        assert!(result.is_some());
        let exit_reason = result.unwrap();
        assert!(exit_reason.reason.contains("Max hold time"));
    }

    #[test]
    fn test_check_max_hold_time_does_not_trigger_with_significant_movement() {
        let mut position = create_test_position(100.0, 100.0, 100.0);
        position.entry_time = Utc::now() - Duration::days(8); // 8 days ago
        let current_price = 110.0; // 10% movement (significant)

        let max_hold_duration = Duration::days(7);
        let min_movement_threshold = 2.5;

        let result = checks::check_max_hold_time(
            &position,
            current_price,
            max_hold_duration,
            min_movement_threshold,
        );

        assert!(result.is_none()); // Should not exit despite time elapsed
    }
}
