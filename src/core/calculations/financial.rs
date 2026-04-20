use rust_decimal::Decimal;

/// Financial calculation utilities for trading operations
///
/// This module provides centralized, tested financial calculations
/// to ensure consistency across the entire codebase.
///
/// Calculate profit and loss for a position
///
/// # Arguments
/// * `entry_price` - The price at which the position was entered
/// * `exit_price` - The price at which the position was/will be exited
/// * `size` - The size of the position
///
/// # Returns
/// The profit or loss (negative for loss)
pub fn calculate_pnl_f64(entry_price: f64, exit_price: f64, size: f64) -> f64 {
    size * (exit_price - entry_price)
}

/// Calculate profit and loss for a position using Decimal precision
pub fn calculate_pnl_decimal(entry_price: Decimal, exit_price: Decimal, size: Decimal) -> Decimal {
    size * (exit_price - entry_price)
}

/// Calculate unrealized PnL based on current price
pub fn calculate_unrealized_pnl_f64(entry_price: f64, current_price: f64, size: f64) -> f64 {
    calculate_pnl_f64(entry_price, current_price, size)
}

/// Calculate unrealized PnL using Decimal precision
pub fn calculate_unrealized_pnl_decimal(
    entry_price: Decimal,
    current_price: Decimal,
    size: Decimal,
) -> Decimal {
    calculate_pnl_decimal(entry_price, current_price, size)
}

/// Calculate Return on Investment (ROI) as a percentage
///
/// # Arguments
/// * `entry_price` - The price at which the position was entered
/// * `exit_price` - The price at which the position was/will be exited
///
/// # Returns
/// ROI as a percentage (e.g., 10.0 for 10%)
pub fn calculate_roi_percentage_f64(entry_price: f64, exit_price: f64) -> f64 {
    if entry_price <= 0.0 {
        return 0.0;
    }
    ((exit_price - entry_price) / entry_price) * 100.0
}

/// Calculate ROI percentage using Decimal precision
pub fn calculate_roi_percentage_decimal(entry_price: Decimal, exit_price: Decimal) -> Decimal {
    if entry_price <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    ((exit_price - entry_price) / entry_price) * Decimal::new(100, 0)
}

/// Calculate ROI based on net profit and initial investment
///
/// # Arguments
/// * `net_profit` - The net profit after fees
/// * `initial_investment` - The initial investment amount
///
/// # Returns
/// ROI as a percentage
pub fn calculate_roi_from_profit_f64(net_profit: f64, initial_investment: f64) -> f64 {
    if initial_investment <= 0.0 {
        return 0.0;
    }
    (net_profit / initial_investment) * 100.0
}

/// Calculate ROI from profit using Decimal precision
pub fn calculate_roi_from_profit_decimal(
    net_profit: Decimal,
    initial_investment: Decimal,
) -> Decimal {
    if initial_investment <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    (net_profit / initial_investment) * Decimal::new(100, 0)
}

/// Calculate net profit after fees
///
/// # Arguments
/// * `gross_profit` - The gross profit before fees
/// * `total_fees` - The total fees paid
///
/// # Returns
/// Net profit after fees
pub fn calculate_net_profit_f64(gross_profit: f64, total_fees: f64) -> f64 {
    gross_profit - total_fees
}

/// Calculate net profit using Decimal precision
pub fn calculate_net_profit_decimal(gross_profit: Decimal, total_fees: Decimal) -> Decimal {
    gross_profit - total_fees
}

/// Calculate initial investment including entry fees
///
/// # Arguments
/// * `size` - Position size
/// * `entry_price` - Entry price
/// * `entry_fees` - Optional entry fees (defaults to 0)
///
/// # Returns
/// Total initial investment
pub fn calculate_initial_investment_f64(
    size: f64,
    entry_price: f64,
    entry_fees: Option<f64>,
) -> f64 {
    let base_investment = size * entry_price;
    base_investment + entry_fees.unwrap_or(0.0)
}

/// Calculate initial investment using Decimal precision
pub fn calculate_initial_investment_decimal(
    size: Decimal,
    entry_price: Decimal,
    entry_fees: Option<Decimal>,
) -> Decimal {
    let base_investment = size * entry_price;
    base_investment + entry_fees.unwrap_or(Decimal::ZERO)
}

/// Calculate percentage change between two values
///
/// # Arguments
/// * `old_value` - The original value
/// * `new_value` - The new value
///
/// # Returns
/// Percentage change (e.g., 10.0 for 10% increase)
pub fn calculate_percentage_change_f64(old_value: f64, new_value: f64) -> f64 {
    if old_value <= 0.0 {
        return 0.0;
    }
    ((new_value - old_value) / old_value) * 100.0
}

/// Calculate percentage change using Decimal precision
pub fn calculate_percentage_change_decimal(old_value: Decimal, new_value: Decimal) -> Decimal {
    if old_value <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    ((new_value - old_value) / old_value) * Decimal::new(100, 0)
}

/// Calculate position value at current price
pub fn calculate_position_value_f64(size: f64, current_price: f64) -> f64 {
    size * current_price
}

/// Calculate position value using Decimal precision
pub fn calculate_position_value_decimal(size: Decimal, current_price: Decimal) -> Decimal {
    size * current_price
}

/// Calculate unrealized PnL percentage based on initial investment
pub fn calculate_unrealized_pnl_percentage_f64(
    unrealized_pnl: f64,
    initial_investment: f64,
) -> f64 {
    if initial_investment <= 0.0 {
        return 0.0;
    }
    (unrealized_pnl / initial_investment) * 100.0
}

/// Calculate unrealized PnL percentage using Decimal precision
pub fn calculate_unrealized_pnl_percentage_decimal(
    unrealized_pnl: Decimal,
    initial_investment: Decimal,
) -> Decimal {
    if initial_investment <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    (unrealized_pnl / initial_investment) * Decimal::new(100, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    #[test]
    fn test_calculate_pnl_f64() {
        // Profit scenario
        assert_eq!(calculate_pnl_f64(100.0, 110.0, 10.0), 100.0);
        // Loss scenario
        assert_eq!(calculate_pnl_f64(100.0, 90.0, 10.0), -100.0);
        // No change
        assert_eq!(calculate_pnl_f64(100.0, 100.0, 10.0), 0.0);
    }

    #[test]
    fn test_calculate_roi_percentage_f64() {
        // 10% gain
        assert_eq!(calculate_roi_percentage_f64(100.0, 110.0), 10.0);
        // 10% loss
        assert_eq!(calculate_roi_percentage_f64(100.0, 90.0), -10.0);
        // No change
        assert_eq!(calculate_roi_percentage_f64(100.0, 100.0), 0.0);
        // Zero entry price
        assert_eq!(calculate_roi_percentage_f64(0.0, 110.0), 0.0);
    }

    #[test]
    fn test_calculate_net_profit_f64() {
        assert_eq!(calculate_net_profit_f64(100.0, 5.0), 95.0);
        assert_eq!(calculate_net_profit_f64(100.0, 0.0), 100.0);
        assert_eq!(calculate_net_profit_f64(-50.0, 5.0), -55.0);
    }

    #[test]
    fn test_calculate_initial_investment_f64() {
        // Without fees
        assert_eq!(calculate_initial_investment_f64(10.0, 100.0, None), 1000.0);
        // With fees
        assert_eq!(
            calculate_initial_investment_f64(10.0, 100.0, Some(25.0)),
            1025.0
        );
    }

    #[test]
    fn test_calculate_percentage_change_f64() {
        // 50% increase
        assert_eq!(calculate_percentage_change_f64(100.0, 150.0), 50.0);
        // 25% decrease
        assert_eq!(calculate_percentage_change_f64(100.0, 75.0), -25.0);
        // Zero old value
        assert_eq!(calculate_percentage_change_f64(0.0, 100.0), 0.0);
    }

    #[test]
    fn test_decimal_calculations() {
        let entry = Decimal::new(100, 0);
        let exit = Decimal::new(110, 0);
        let size = Decimal::new(10, 0);

        assert_eq!(
            calculate_pnl_decimal(entry, exit, size),
            Decimal::new(100, 0)
        );
        assert_eq!(
            calculate_roi_percentage_decimal(entry, exit),
            Decimal::new(10, 0)
        );
    }
}
