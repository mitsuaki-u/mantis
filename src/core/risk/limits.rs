use crate::core::constants::{MIN_POSITION_PCT_OF_MAX, WARNING_THRESHOLD_PCT};
use crate::core::domain::trading::Signal;
use crate::core::errors::{Error, Result};
use log::{debug, info, warn};
use std::collections::HashSet;

/// Risk limit violation result
#[derive(Debug, Clone)]
pub struct RiskLimitViolation {
    pub limit_type: String,
    pub current_value: f64,
    pub max_value: f64,
    pub should_halt_trading: bool,
}

/// Check overall portfolio risk limits
pub fn check_overall_risk_limits(
    current_daily_loss: f64,
    max_daily_loss_limit: f64,
    current_drawdown: f64,
    max_drawdown_limit: f64,
) -> Vec<RiskLimitViolation> {
    let mut violations = Vec::new();
    info!("Checking overall risk limits");

    // Check overall daily loss
    if current_daily_loss >= max_daily_loss_limit {
        warn!(
            "🚨 Overall daily loss limit exceeded: {:.2}% >= {:.1}% max",
            current_daily_loss * 100.0,
            max_daily_loss_limit * 100.0
        );

        violations.push(RiskLimitViolation {
            limit_type: "overall_daily_loss".to_string(),
            current_value: current_daily_loss,
            max_value: max_daily_loss_limit,
            should_halt_trading: true,
        });
    }

    // Check overall drawdown
    if current_drawdown >= max_drawdown_limit {
        warn!(
            "🚨 Overall drawdown limit exceeded: {:.2}% >= {:.1}% max",
            current_drawdown * 100.0,
            max_drawdown_limit * 100.0
        );

        violations.push(RiskLimitViolation {
            limit_type: "overall_drawdown".to_string(),
            current_value: current_drawdown,
            max_value: max_drawdown_limit,
            should_halt_trading: true,
        });
    }

    violations
}

/// Portfolio risk context for trading allowance checks
pub struct TradingLimitsContext {
    pub current_daily_loss: f64,
    pub max_daily_loss_limit: f64,
    pub current_drawdown: f64,
    pub max_drawdown_limit: f64,
    pub max_positions: usize,
    pub current_positions: usize,
}

/// Check if trading is allowed for a specific token
pub fn check_trading_allowed(
    token_id: &str,
    signal: &Signal,
    halted_tokens: &HashSet<String>,
    ctx: &TradingLimitsContext,
) -> bool {
    let current_daily_loss = ctx.current_daily_loss;
    let max_daily_loss_limit = ctx.max_daily_loss_limit;
    let current_drawdown = ctx.current_drawdown;
    let max_drawdown_limit = ctx.max_drawdown_limit;
    let max_positions = ctx.max_positions;
    let current_positions = ctx.current_positions;
    debug!(
        "🔍 check_trading_allowed() called for token: {} with signal: {}",
        token_id, signal
    );

    // Check if the token is in the halted list
    if halted_tokens.contains(token_id) {
        info!("Trading halted for token: {}", token_id);
        return false;
    }

    // Portfolio risk checks should only restrict BUY signals (opening new positions)
    // SELL signals should always be allowed as they reduce exposure and help recover from losses
    if signal.is_buy() {
        // Check overall portfolio risk levels
        let portfolio_risk_factor = super::metrics::calculate_portfolio_risk_factor(
            current_daily_loss,
            max_daily_loss_limit,
            current_drawdown,
            max_drawdown_limit,
        );
        if portfolio_risk_factor < 0.3 {
            info!(
                "Portfolio risk too high ({:.2}), restricting new BUY orders",
                portfolio_risk_factor
            );
            return false;
        }

        // Check daily loss limits
        if current_daily_loss >= max_daily_loss_limit * WARNING_THRESHOLD_PCT {
            info!(
                "⚠️  Approaching daily loss limit: {:.2}% / {:.1}% max, restricting new BUY orders",
                current_daily_loss * 100.0,
                max_daily_loss_limit * 100.0
            );
            return false;
        }

        // Check drawdown limits
        if current_drawdown >= max_drawdown_limit * WARNING_THRESHOLD_PCT {
            info!(
                "⚠️  Approaching drawdown limit: {:.2}% / {:.1}% max, restricting new BUY orders",
                current_drawdown * 100.0,
                max_drawdown_limit * 100.0
            );
            return false;
        }
    } else if signal.is_sell() {
        debug!(
            "🟢 SELL signal for {} bypasses portfolio risk checks (selling reduces exposure)",
            token_id
        );
    }

    // Position limits should only apply to BUY signals (opening new positions)
    // SELL signals close existing positions and should be allowed regardless of position limits
    if signal.is_buy() {
        debug!(
            "🔢 Position limit check for {} (BUY signal): current={}, max={}",
            token_id, current_positions, max_positions,
        );

        if current_positions >= max_positions {
            info!(
                "❌ Too many open positions ({}/{}), restricting new BUY orders for {}",
                current_positions, max_positions, token_id
            );
            return false;
        } else {
            debug!(
                "✅ Position limit OK for {} (BUY signal): {}/{} positions",
                token_id, current_positions, max_positions
            );
        }
    } else if signal.is_sell() {
        debug!(
            "🟢 SELL signal for {} bypasses position limit checks (closing existing position)",
            token_id
        );
    }

    true
}

/// Halt trading for a specific token
pub fn halt_token_trading(halted_tokens: &mut HashSet<String>, token_id: &str, reason: &str) {
    halted_tokens.insert(token_id.to_string());
    warn!("Halted trading for token {}: {}", token_id, reason);
}

/// Resume trading for a halted token (manual override)
pub fn resume_token_trading(halted_tokens: &mut HashSet<String>, token_id: &str) -> bool {
    if halted_tokens.remove(token_id) {
        info!("Resumed trading for token: {}", token_id);
        true
    } else {
        false
    }
}

/// Get list of currently halted tokens
pub fn get_halted_tokens(halted_tokens: &HashSet<String>) -> Vec<String> {
    halted_tokens.iter().cloned().collect()
}

/// Apply position size constraints (max trade risk, minimum size)
///
/// This is a pure business rule that enforces:
/// - Max trade risk cap (only for buy signals)
/// - Minimum position size requirement
pub fn apply_position_size_constraints(
    position_size: f64,
    signal: &Signal,
    max_total_exposure: f64,
    max_trade_risk_pct: f64,
    max_position_size: f64,
) -> Result<f64> {
    let mut capped_size = position_size;

    // Apply max trade risk cap for buy signals
    if signal.is_buy() {
        let max_risk_usd = max_total_exposure * (max_trade_risk_pct / 100.0);

        if position_size > max_risk_usd {
            debug!(
                "Position size ${:.2} exceeds max trade risk ({}% = ${:.2}) - capping",
                position_size, max_trade_risk_pct, max_risk_usd
            );
            capped_size = max_risk_usd;
        }
    }

    // Check minimum size requirement
    let min_size = max_position_size * MIN_POSITION_PCT_OF_MAX;
    if capped_size < min_size {
        return Err(Error::InvalidInput(format!(
            "Position size ${:.2} below minimum ${:.2}",
            capped_size, min_size
        )));
    }

    Ok(capped_size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::domain::trading::Signal;

    fn empty_halted() -> HashSet<String> {
        HashSet::new()
    }

    fn ctx(
        current_daily_loss: f64,
        max_daily_loss_limit: f64,
        current_drawdown: f64,
        max_drawdown_limit: f64,
        max_positions: usize,
        current_positions: usize,
    ) -> TradingLimitsContext {
        TradingLimitsContext {
            current_daily_loss,
            max_daily_loss_limit,
            current_drawdown,
            max_drawdown_limit,
            max_positions,
            current_positions,
        }
    }

    // ---- check_trading_allowed ----

    #[test]
    fn buy_allowed_when_all_clear() {
        assert!(check_trading_allowed(
            "token-a",
            &Signal::Buy,
            &empty_halted(),
            &ctx(0.0, 10.0, 0.0, 20.0, 5, 2),
        ));
    }

    #[test]
    fn buy_blocked_when_max_positions_reached() {
        assert!(!check_trading_allowed(
            "token-a",
            &Signal::Buy,
            &empty_halted(),
            &ctx(0.0, 10.0, 0.0, 20.0, 3, 3),
        ));
    }

    #[test]
    fn sell_bypasses_position_limit() {
        // SELL should go through even when positions are maxed
        assert!(check_trading_allowed(
            "token-a",
            &Signal::Sell,
            &empty_halted(),
            &ctx(0.0, 10.0, 0.0, 20.0, 3, 3),
        ));
    }

    #[test]
    fn buy_blocked_for_halted_token() {
        let mut halted = empty_halted();
        halted.insert("token-a".to_string());
        assert!(!check_trading_allowed(
            "token-a",
            &Signal::Buy,
            &halted,
            &ctx(0.0, 10.0, 0.0, 20.0, 5, 0),
        ));
    }

    #[test]
    fn sell_blocked_for_halted_token() {
        // Even SELL is blocked on a halted token (halt means something is wrong)
        let mut halted = empty_halted();
        halted.insert("token-a".to_string());
        assert!(!check_trading_allowed(
            "token-a",
            &Signal::Sell,
            &halted,
            &ctx(0.0, 10.0, 0.0, 20.0, 5, 0),
        ));
    }

    #[test]
    fn buy_blocked_approaching_daily_loss_limit() {
        // 95% of max daily loss → above WARNING_THRESHOLD_PCT (0.9)
        assert!(!check_trading_allowed(
            "token-a",
            &Signal::Buy,
            &empty_halted(),
            &ctx(9.5, 10.0, 0.0, 20.0, 5, 0),
        ));
    }

    #[test]
    fn sell_allowed_at_high_daily_loss() {
        // SELL bypasses daily loss check — should always be allowed
        assert!(check_trading_allowed(
            "token-a",
            &Signal::Sell,
            &empty_halted(),
            &ctx(9.5, 10.0, 0.0, 20.0, 5, 0),
        ));
    }

    // ---- check_overall_risk_limits ----

    #[test]
    fn no_violations_when_under_limits() {
        let violations = check_overall_risk_limits(0.05, 0.10, 0.10, 0.20);
        assert!(violations.is_empty());
    }

    #[test]
    fn daily_loss_violation_triggers_halt() {
        let violations = check_overall_risk_limits(0.11, 0.10, 0.0, 0.20);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].limit_type, "overall_daily_loss");
        assert!(violations[0].should_halt_trading);
    }

    #[test]
    fn drawdown_violation_triggers_halt() {
        let violations = check_overall_risk_limits(0.0, 0.10, 0.21, 0.20);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].limit_type, "overall_drawdown");
        assert!(violations[0].should_halt_trading);
    }

    #[test]
    fn both_limits_exceeded_returns_two_violations() {
        let violations = check_overall_risk_limits(0.15, 0.10, 0.25, 0.20);
        assert_eq!(violations.len(), 2);
    }

    // ---- apply_position_size_constraints ----

    #[test]
    fn size_capped_by_max_trade_risk() {
        // max_trade_risk = 2% of $1000 exposure = $20 cap
        let result = apply_position_size_constraints(100.0, &Signal::Buy, 1000.0, 2.0, 200.0);
        assert_eq!(result.unwrap(), 20.0);
    }

    #[test]
    fn size_within_risk_cap_unchanged() {
        // $15 is within the $20 cap
        let result = apply_position_size_constraints(15.0, &Signal::Buy, 1000.0, 2.0, 200.0);
        assert_eq!(result.unwrap(), 15.0);
    }

    #[test]
    fn sell_ignores_max_trade_risk_cap() {
        // SELL should not be capped by the trade risk percentage
        let result = apply_position_size_constraints(500.0, &Signal::Sell, 1000.0, 2.0, 200.0);
        assert_eq!(result.unwrap(), 500.0);
    }

    #[test]
    fn size_below_minimum_returns_error() {
        // max_position = $100 → min = 1% = $1. Passing $0.5 should error.
        let result = apply_position_size_constraints(0.5, &Signal::Buy, 1000.0, 2.0, 100.0);
        assert!(result.is_err());
    }
}
