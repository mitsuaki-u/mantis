use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Represents a trading signal
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Signal {
    Buy,
    Sell,
    Hold,
    NoAction, // For when no position exists and conditions don't warrant buying
}

impl Signal {
    /// Check if the signal is a buy signal
    pub fn is_buy(&self) -> bool {
        matches!(self, Signal::Buy)
    }

    /// Check if the signal is a sell signal
    pub fn is_sell(&self) -> bool {
        matches!(self, Signal::Sell)
    }

    /// Check if the signal indicates holding (Hold)
    pub fn is_hold(&self) -> bool {
        matches!(self, Signal::Hold)
    }

    pub fn is_no_action(&self) -> bool {
        matches!(self, Signal::NoAction)
    }
}

impl fmt::Display for Signal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Signal::Buy => write!(f, "BUY"),
            Signal::Sell => write!(f, "SELL"),
            Signal::Hold => write!(f, "HOLD"),
            Signal::NoAction => write!(f, "NO ACTION"),
        }
    }
}

/// Signal metadata captured at generation time
#[derive(Debug, Clone)]
pub struct SignalMetadata {
    pub correlation_id: String,
    pub signal_price: f64,
    pub signal_volume_24h: f64,
    pub strategy_name: String,
    pub market_conditions: String,
    // Indicator values for AI advisor context
    pub rsi: Option<f64>,
    pub bollinger_pct: Option<f64>,
    pub momentum_score: Option<f64>,
    pub volume_24h: Option<f64>,
    pub price_change_24h: Option<f64>,
}

impl SignalMetadata {
    /// Create new signal metadata with a unique correlation ID
    pub fn new(
        signal_price: f64,
        signal_volume_24h: f64,
        strategy_name: String,
        market_conditions: String,
    ) -> Self {
        Self {
            correlation_id: uuid::Uuid::new_v4().to_string(),
            signal_price,
            signal_volume_24h,
            strategy_name,
            market_conditions,
            rsi: None,
            bollinger_pct: None,
            momentum_score: None,
            volume_24h: None,
            price_change_24h: None,
        }
    }

    /// Attach an indicator snapshot + 24h market values to the metadata.
    ///
    /// Builder-style so the call site stays readable in `publish_signal`.
    pub fn with_indicators(
        mut self,
        snapshot: crate::core::indicators::IndicatorSnapshot,
        volume_24h: f64,
        price_change_24h: f64,
    ) -> Self {
        self.rsi = snapshot.rsi;
        self.bollinger_pct = snapshot.bollinger_pct;
        self.momentum_score = snapshot.momentum_score;
        self.volume_24h = Some(volume_24h);
        self.price_change_24h = Some(price_change_24h);
        self
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

    /// Create a new Position with the given parameters
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
