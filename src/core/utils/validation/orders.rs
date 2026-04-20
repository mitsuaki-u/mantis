use crate::core::domain::strategy_params::StrategyParams;
use crate::core::domain::token::TokenData;
use crate::core::domain::trading::Signal;
use crate::core::utils::validation::price::validate_price;
use log::{error, info, warn};
use rust_decimal::Decimal;

/// Order validation result
#[derive(Debug, Clone)]
pub struct OrderValidation {
    pub is_valid: bool,
    pub reason: Option<String>,
}

impl OrderValidation {
    pub fn valid() -> Self {
        Self {
            is_valid: true,
            reason: None,
        }
    }

    pub fn invalid(reason: String) -> Self {
        Self {
            is_valid: false,
            reason: Some(reason),
        }
    }
}

/// Order execution parameters
#[derive(Debug, Clone)]
pub struct OrderParams {
    pub token_id: String,
    pub signal: Signal,
    pub position_size: f64,
    pub current_price: Decimal,
    pub symbol: String,
}

/// Validate a buy order before execution
pub fn validate_buy_order(
    token_data: &TokenData,
    position_size: f64,
    params: &StrategyParams,
) -> OrderValidation {
    let token_id = &token_data.id;
    let symbol = &token_data.symbol;
    let current_price = token_data.price_usd;

    // Validate price using shared utility
    if let Err(e) = validate_price(
        current_price,
        &format!("BUY order current price for {}", token_id),
    ) {
        error!(
            "❌ CRITICAL: Refusing to execute BUY order for {}: {}",
            token_id, e
        );
        return OrderValidation::invalid(format!("Invalid price: {}", e));
    }

    // Check minimum position size
    let min_position_size = params.min_position_size;
    if position_size < min_position_size {
        warn!(
            "❌ Position size ${:.2} for {} is below minimum ${:.2}",
            position_size, symbol, min_position_size
        );
        return OrderValidation::invalid(format!(
            "Position size below minimum of ${:.2}",
            min_position_size
        ));
    }

    // Check maximum position size
    let max_position_size = params.max_position_size;
    if position_size > max_position_size {
        warn!(
            "❌ Position size ${:.2} for {} exceeds maximum ${:.2}",
            position_size, symbol, max_position_size
        );
        return OrderValidation::invalid(format!(
            "Position size exceeds maximum of ${:.2}",
            max_position_size
        ));
    }

    OrderValidation::valid()
}

/// Validate a sell order before execution
pub fn validate_sell_order(token_data: &TokenData, current_position_size: f64) -> OrderValidation {
    let token_id = &token_data.id;
    let symbol = &token_data.symbol;
    let current_price = token_data.price_usd;

    // Validate price using shared utility
    if let Err(e) = validate_price(
        current_price,
        &format!("SELL order current price for {}", token_id),
    ) {
        error!(
            "❌ CRITICAL: Refusing to execute SELL order for {}: {}",
            token_id, e
        );
        return OrderValidation::invalid(format!("Invalid price: {}", e));
    }

    // Check if we have a position to sell
    if current_position_size <= 0.0 {
        warn!(
            "❌ Cannot sell {}: no position exists (size: {:.6})",
            symbol, current_position_size
        );
        return OrderValidation::invalid("No position to sell".to_string());
    }

    OrderValidation::valid()
}

/// Validate gas cost is acceptable for the trade
pub fn validate_gas_cost(
    gas_cost_usd: f64,
    position_size: f64,
    max_gas_cost_absolute: f64,
    max_gas_percentage: f64,
    symbol: &str,
    correlation_id: &str,
) -> OrderValidation {
    // Check absolute maximum
    if gas_cost_usd > max_gas_cost_absolute {
        error!(
            "[{}] REJECTING TRADE: Gas cost ${:.2} exceeds absolute maximum ${:.2} for {}",
            &correlation_id[..8],
            gas_cost_usd,
            max_gas_cost_absolute,
            symbol
        );
        return OrderValidation::invalid(format!(
            "Gas cost ${:.2} exceeds maximum ${:.2}",
            gas_cost_usd, max_gas_cost_absolute
        ));
    }

    // Check percentage of trade
    let actual_gas_percentage = (gas_cost_usd / position_size) * 100.0;
    if actual_gas_percentage > max_gas_percentage {
        error!(
            "[{}] REJECTING TRADE: Gas cost ${:.2} ({:.1}% of ${:.2} trade) exceeds max {:.1}% for {}",
            &correlation_id[..8], gas_cost_usd, actual_gas_percentage, position_size, max_gas_percentage, symbol
        );
        return OrderValidation::invalid(format!(
            "Gas cost {:.1}% of trade exceeds maximum {:.1}%",
            actual_gas_percentage, max_gas_percentage
        ));
    }

    info!(
        "[{}] Gas protection check passed for {}: Trade ${:.2}, gas ${:.2} ({:.1}% of trade)",
        &correlation_id[..8],
        symbol,
        position_size,
        gas_cost_usd,
        actual_gas_percentage
    );

    OrderValidation::valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::domain::token::TokenData;
    use rust_decimal::Decimal;

    #[test]
    fn test_validate_gas_cost_passes_when_within_limits() {
        let result = validate_gas_cost(
            5.0,   // gas_cost_usd
            100.0, // position_size
            10.0,  // max_gas_cost_absolute
            10.0,  // max_gas_percentage (5% actual < 10% max)
            "WETH",
            "test-correlation-id-12345",
        );
        assert!(result.is_valid);
        assert!(result.reason.is_none());
    }

    #[test]
    fn test_validate_gas_cost_fails_when_exceeds_absolute_limit() {
        let result = validate_gas_cost(
            15.0,  // gas_cost_usd (exceeds max_gas_cost_absolute)
            100.0, // position_size
            10.0,  // max_gas_cost_absolute
            10.0,  // max_gas_percentage
            "WETH",
            "test-correlation-id-12345",
        );
        assert!(!result.is_valid);
        assert!(result.reason.is_some());
        assert!(result.reason.unwrap().contains("exceeds maximum $10.00"));
    }

    #[test]
    fn test_validate_gas_cost_fails_when_exceeds_percentage_limit() {
        let result = validate_gas_cost(
            15.0,  // gas_cost_usd (15% of 100 exceeds 10% max)
            100.0, // position_size
            20.0,  // max_gas_cost_absolute (won't trigger)
            10.0,  // max_gas_percentage
            "WETH",
            "test-correlation-id-12345",
        );
        assert!(!result.is_valid);
        assert!(result.reason.is_some());
        assert!(result.reason.unwrap().contains("exceeds maximum 10.0%"));
    }

    #[test]
    fn test_validate_gas_cost_edge_case_exactly_at_limit() {
        let result = validate_gas_cost(
            10.0,  // gas_cost_usd (exactly at absolute limit)
            100.0, // position_size
            10.0,  // max_gas_cost_absolute
            10.0,  // max_gas_percentage
            "WETH",
            "test-correlation-id-12345",
        );
        assert!(result.is_valid); // Should pass when exactly at limit
    }

    #[test]
    fn test_validate_sell_order_fails_with_zero_position() {
        let token_data = TokenData::new("test-token", "TEST", "Test Token", Decimal::from(100));
        let result = validate_sell_order(&token_data, 0.0);
        assert!(!result.is_valid);
        assert!(result.reason.unwrap().contains("No position to sell"));
    }

    #[test]
    fn test_validate_sell_order_passes_with_valid_position() {
        let token_data = TokenData::new("test-token", "TEST", "Test Token", Decimal::from(100));
        let result = validate_sell_order(&token_data, 10.0);
        assert!(result.is_valid);
    }

    #[test]
    fn test_validate_buy_order_fails_below_min_position_size() {
        let token_data = TokenData::new("test-token", "TEST", "Test Token", Decimal::from(100));
        let params = StrategyParams::default()
            .with_min_position_size(50.0)
            .with_max_position_size(1000.0);

        let result = validate_buy_order(&token_data, 25.0, &params);
        assert!(!result.is_valid);
        assert!(result.reason.unwrap().contains("below minimum"));
    }

    #[test]
    fn test_validate_buy_order_fails_above_max_position_size() {
        let token_data = TokenData::new("test-token", "TEST", "Test Token", Decimal::from(100));
        let params = StrategyParams::default()
            .with_min_position_size(50.0)
            .with_max_position_size(1000.0);

        let result = validate_buy_order(&token_data, 1500.0, &params);
        assert!(!result.is_valid);
        assert!(result.reason.unwrap().contains("exceeds maximum"));
    }

    #[test]
    fn test_validate_buy_order_passes_within_limits() {
        let token_data = TokenData::new("test-token", "TEST", "Test Token", Decimal::from(100));
        let params = StrategyParams::default()
            .with_min_position_size(50.0)
            .with_max_position_size(1000.0);

        let result = validate_buy_order(&token_data, 500.0, &params);
        assert!(result.is_valid);
    }
}
