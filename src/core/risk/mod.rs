//! Pure risk calculation functions
//!
//! This module contains stateless risk calculation functions that belong
//! in the core domain layer. The stateful RiskManager has been moved to
//! the application layer (application::services::RiskManager).

pub mod assessment;
pub mod limits;
pub mod metrics;

// Re-export key business logic
pub use assessment::{
    calculate_portfolio_risk_factors, check_token_volatility, compute_position_size,
    has_valid_market_data, PortfolioRiskFactors, PositionSizeResult,
};
pub use limits::{
    apply_position_size_constraints, check_overall_risk_limits, check_trading_allowed,
    get_halted_tokens, halt_token_trading, resume_token_trading, RiskLimitViolation,
    TradingLimitsContext,
};
pub use metrics::{
    calculate_portfolio_risk, calculate_portfolio_risk_factor, calculate_updated_daily_loss,
    calculate_updated_drawdown, reset_daily_loss,
};
