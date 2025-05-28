use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::fmt;

use crate::core::models::market::TokenMetrics;

/// Represents a trading signal
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Signal {
    Buy,
    Sell,
    Hold,
    StrongBuy,
    StrongSell,
}

impl Signal {
    /// Check if the signal is a buy signal (Buy or StrongBuy)
    pub fn is_buy(&self) -> bool {
        matches!(self, Signal::Buy | Signal::StrongBuy)
    }

    /// Check if the signal is a sell signal (Sell or StrongSell)
    pub fn is_sell(&self) -> bool {
        matches!(self, Signal::Sell | Signal::StrongSell)
    }

    /// Check if the signal indicates holding (Hold)
    pub fn is_hold(&self) -> bool {
        matches!(self, Signal::Hold)
    }
}

impl fmt::Display for Signal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Signal::Buy => write!(f, "BUY"),
            Signal::Sell => write!(f, "SELL"),
            Signal::Hold => write!(f, "HOLD"),
            Signal::StrongBuy => write!(f, "STRONG BUY"),
            Signal::StrongSell => write!(f, "STRONG SELL"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub token_id: String,
    pub provider_id: String,
    pub entry_price: f64,
    pub current_price: f64,
    pub highest_price: f64,
    pub size: f64,
    pub unrealized_pnl: f64,
    pub entry_time: DateTime<Utc>,
}

impl Position {
    /// Calculate profit/loss for the position based on a given price
    pub fn calculate_pnl(&self, price: f64) -> f64 {
        (price - self.entry_price) * self.size
    }

    /// Calculate percentage profit/loss for the position
    pub fn calculate_pnl_pct(&self, price: f64) -> f64 {
        if self.entry_price == 0.0 {
            return 0.0;
        }
        ((price - self.entry_price) / self.entry_price) * 100.0
    }

    /// New constructor for Position that initializes coingecko_id from provider_id
    pub fn new(
        token_id: String,
        provider_id: String,
        entry_price: f64,
        size: f64,
        entry_time: DateTime<Utc>,
    ) -> Self {
        Self {
            token_id,
            provider_id: provider_id.clone(),
            entry_price,
            current_price: entry_price,
            highest_price: entry_price,
            size,
            unrealized_pnl: 0.0,
            entry_time,
        }
    }
}

/// Represents a reason for exiting a position
#[derive(Debug, Clone, PartialEq)]
pub struct ExitReason {
    pub reason: String,
    pub confidence: f64,
    pub is_risk_based: bool,
}

impl ExitReason {
    pub fn new(reason: &str, confidence: f64, is_risk_based: bool) -> Self {
        Self {
            reason: reason.to_string(),
            confidence,
            is_risk_based,
        }
    }

    pub fn risk_based(reason: &str) -> Self {
        Self::new(reason, 1.0, true)
    }

    pub fn strategy_based(reason: &str) -> Self {
        Self::new(reason, 0.8, false)
    }
}

/// Trading strategy trait that all strategy implementations must implement
pub trait TradingStrategy: fmt::Display + Send + Sync + Any + 'static {
    /// Get the name of the strategy
    fn name(&self) -> &str;

    /// Analyze a token for entry signals only (BUY)
    fn analyze_for_entry(&self, token: &TokenMetrics) -> bool;

    /// Analyze a token for exit signals, using latest market data
    ///
    /// Parameters:
    /// * token - The most recent market data for the token
    /// * position - The existing position data if available
    /// * risk_params - Optional risk parameters (take_profit, stop_loss, risk_tolerance)
    ///
    /// Returns Some(ExitReason) if position should be exited, None otherwise
    fn analyze_for_exit(
        &self,
        token: &TokenMetrics,
        position: Option<&Position>,
        risk_params: Option<(f64, f64, usize)>,
    ) -> Option<ExitReason>;

    /// Original analyze method kept for backwards compatibility
    /// In the new separation paradigm, this should only be used for entry analysis
    fn analyze(&self, token: &TokenMetrics) -> Signal {
        if self.analyze_for_entry(token) {
            Signal::Buy
        } else {
            Signal::Hold
        }
    }

    /// Original should_exit method kept for backwards compatibility
    fn should_exit(&self, _position: &Position) -> bool {
        // Since we need token metrics now, this is harder to implement in backwards compatible way
        // We'll use a default implementation that indicates no exit signal
        false
    }

    /// Update internal market data for the strategy
    fn update_market_data(&mut self, token: &TokenMetrics);

    /// Clone the strategy into a boxed trait object
    fn box_clone(&self) -> Box<dyn TradingStrategy>;

    /// Get a reference to the underlying Any type
    fn as_any(&self) -> &dyn Any;

    /// Get a mutable reference to the underlying Any type
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// Implement Clone for Box<dyn TradingStrategy>
impl Clone for Box<dyn TradingStrategy> {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

/// Strategy is now a wrapper around a Box<dyn TradingStrategy>
#[derive(Clone)]
pub struct Strategy {
    inner: Box<dyn TradingStrategy>,
}

impl Strategy {
    pub fn new(strategy: Box<dyn TradingStrategy>) -> Self {
        Self { inner: strategy }
    }

    pub fn name(&self) -> &str {
        self.inner.name()
    }

    pub fn analyze(&self, token: &TokenMetrics) -> Signal {
        self.inner.analyze(token)
    }

    pub fn analyze_for_entry(&self, token: &TokenMetrics) -> bool {
        self.inner.analyze_for_entry(token)
    }

    pub fn analyze_for_exit(
        &self,
        token: &TokenMetrics,
        position: Option<&Position>,
        risk_params: Option<(f64, f64, usize)>,
    ) -> Option<ExitReason> {
        self.inner.analyze_for_exit(token, position, risk_params)
    }

    pub fn should_exit(&self, _position: &Position) -> bool {
        self.inner.should_exit(_position)
    }

    pub fn update_market_data(&mut self, token: &TokenMetrics) {
        self.inner.update_market_data(token)
    }

    pub fn inner_mut(&mut self) -> &mut dyn TradingStrategy {
        &mut *self.inner
    }
}

impl fmt::Display for Strategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.fmt(f)
    }
}

impl fmt::Debug for Strategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Strategy({})", self.inner)
    }
}
