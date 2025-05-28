use super::core::{ExitReason, Position, TradingStrategy};
use crate::core::models::market::TokenMetrics;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use log::{debug, info};

/// Common risk management utilities for all strategies
pub struct RiskManager;

impl RiskManager {
    /// Check if position should be exited based on risk parameters
    pub fn check_risk_exit(
        token: &TokenMetrics,
        position: &Position,
        risk_params: Option<(f64, f64, usize)>,
    ) -> Option<ExitReason> {
        let current_price = token.price_usd;
        let price_change_pct = position.calculate_pnl_pct(current_price);

        // Check risk-based exit conditions if risk parameters are provided
        if let Some((take_profit, stop_loss, risk_tolerance)) = risk_params {
            // Apply risk tolerance multiplier
            let risk_multiplier = 1.0 + (risk_tolerance as f64 * 0.1);

            // Check take profit
            let take_profit_threshold = take_profit * risk_multiplier;
            if price_change_pct >= take_profit_threshold {
                let reason = format!(
                    "Take profit: {:.2}% >= {:.2}%",
                    price_change_pct, take_profit_threshold
                );
                info!("💰 {} - {}", token.id, reason);
                return Some(ExitReason::risk_based(&reason));
            }

            // Check stop loss
            let stop_loss_threshold = -1.0 * stop_loss;
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
            // Calculate the percentage drop from highest price (as a positive percentage)
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
}

/// Base trait for strategies that provides common functionality
pub trait BaseStrategy: TradingStrategy {
    /// Get the strategy's stop loss percentage
    fn get_stop_loss_pct(&self) -> f64;

    /// Get the strategy's minimum volume threshold
    fn get_min_volume(&self) -> f64;

    /// Get the strategy's confidence threshold
    fn get_threshold(&self) -> f64;

    /// Check basic volume requirements
    fn check_volume_requirements(&self, token: &TokenMetrics) -> bool {
        token.volume_24h >= self.get_min_volume()
    }

    /// Check strategy-specific stop loss
    fn check_strategy_stop_loss(
        &self,
        token: &TokenMetrics,
        position: &Position,
    ) -> Option<ExitReason> {
        let current_price = token.price_usd;
        let price_change_pct = position.calculate_pnl_pct(current_price);

        if price_change_pct <= -self.get_stop_loss_pct() {
            let reason = format!(
                "Strategy stop loss triggered: {:.2}% <= {:.2}%",
                price_change_pct,
                -self.get_stop_loss_pct()
            );
            info!("🛑 {}", reason);
            return Some(ExitReason::strategy_based(&reason));
        }
        None
    }

    /// Common exit analysis that can be used by all strategies
    fn common_exit_analysis(
        &self,
        token: &TokenMetrics,
        position: Option<&Position>,
        risk_params: Option<(f64, f64, usize)>,
    ) -> Option<ExitReason> {
        let position = position?;
        let current_price = token.price_usd;

        // Check risk-based exits first
        if let Some(exit_reason) = RiskManager::check_risk_exit(token, position, risk_params) {
            return Some(exit_reason);
        }

        // Check strategy-specific stop loss
        if let Some(exit_reason) = self.check_strategy_stop_loss(token, position) {
            return Some(exit_reason);
        }

        // Check trailing stop (use 50% of regular stop loss)
        let trailing_stop_pct = self.get_stop_loss_pct() * 0.5;
        if let Some(exit_reason) =
            RiskManager::check_trailing_stop(position, current_price, trailing_stop_pct)
        {
            return Some(exit_reason);
        }

        // Check maximum hold time (default 7 days)
        let max_hold_time = ChronoDuration::days(7);
        let min_movement = self.get_threshold() / 2.0;
        if let Some(exit_reason) =
            RiskManager::check_max_hold_time(position, current_price, max_hold_time, min_movement)
        {
            return Some(exit_reason);
        }

        // Log position maintenance
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
