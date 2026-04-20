/// Financial calculation utilities
pub mod financial;

/// Uniswap V3 mathematical calculations
pub mod uniswap_v3;

// Re-export commonly used functions for convenience
pub use financial::{
    calculate_initial_investment_decimal, calculate_initial_investment_f64,
    calculate_net_profit_decimal, calculate_net_profit_f64, calculate_percentage_change_decimal,
    calculate_percentage_change_f64, calculate_pnl_decimal, calculate_pnl_f64,
    calculate_position_value_decimal, calculate_position_value_f64,
    calculate_roi_from_profit_decimal, calculate_roi_from_profit_f64,
    calculate_roi_percentage_decimal, calculate_roi_percentage_f64,
    calculate_unrealized_pnl_decimal, calculate_unrealized_pnl_f64,
    calculate_unrealized_pnl_percentage_decimal, calculate_unrealized_pnl_percentage_f64,
};

// Re-export Uniswap V3 functions
pub use uniswap_v3::calculate_v3_price_from_sqrt;
