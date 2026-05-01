use chrono::{DateTime, Duration as ChronoDuration, Utc};
use dashmap::DashMap;
use log::trace;
use std::fmt;
use std::sync::Arc;

use crate::core::domain::market::TokenMetrics;
use crate::core::domain::trading::{ExitReason, Position};
use crate::core::indicators::{IndicatorWeights, PriceTimeSeries};

/// Trading strategy trait that all strategy implementations must implement
pub trait TradingStrategy: fmt::Display + Send + Sync + 'static {
    /// Get the name of the strategy
    fn name(&self) -> &str;

    /// Analyze a token for entry signals only (BUY)
    fn analyze_for_entry(&self, token: &TokenMetrics) -> bool;

    /// Analyze a token for exit signals, using latest market data
    ///
    /// Parameters:
    /// * token - The most recent market data for the token
    /// * position - The existing position data if available
    /// * risk_params - Optional risk parameters (take_profit, stop_loss)
    ///
    /// Returns Some(ExitReason) if position should be exited, None otherwise
    fn analyze_for_exit(
        &self,
        token: &TokenMetrics,
        position: Option<&Position>,
        risk_params: Option<(f64, f64)>,
    ) -> Option<ExitReason>;

    /// Get access to the price data map for market data updates
    fn get_price_data(&mut self) -> &mut Arc<DashMap<String, PriceTimeSeries>>;

    /// Read-only snapshot of a token's price time series, if tracked.
    ///
    /// Parallel to `get_price_data` but non-mutating — used by callers that
    /// need to inspect indicator state without holding a mutable borrow of
    /// the strategy (e.g. assembling `SignalMetadata` at signal publication).
    fn price_series_for(&self, token_id: &str) -> Option<PriceTimeSeries>;

    /// Indicator weights used by this strategy's composite momentum calculation.
    ///
    /// Default returns `IndicatorWeights::default()`; strategies with
    /// configurable weights (e.g. `MomentumStrategy`) should override so that
    /// downstream consumers compute the same composite score the strategy
    /// used for its decision.
    fn indicator_weights(&self) -> IndicatorWeights {
        IndicatorWeights::default()
    }

    /// Get the minimum volume threshold for this strategy
    fn min_volume(&self) -> f64;

    /// Get the stop loss percentage for this strategy
    fn stop_loss_pct(&self) -> f64;

    /// Get the cooldown period for this strategy
    fn cooldown_period(&self) -> ChronoDuration;

    /// Get access to the considered tokens map for cooldown tracking
    fn considered_tokens(&self) -> &Arc<DashMap<String, DateTime<Utc>>>;

    /// Get the indicator profile for this strategy
    fn indicator_profile(&self) -> crate::core::constants::IndicatorProfile;

    /// Check if token meets volume requirements
    fn check_volume_requirements(&self, token: &TokenMetrics) -> bool {
        token.volume_24h >= self.min_volume()
    }

    /// Mark a token as considered to implement cooldown behavior
    fn mark_token_as_considered(&self, token_id: String) -> Result<(), crate::core::errors::Error> {
        self.considered_tokens()
            .insert(token_id.clone(), Utc::now());
        trace!("Marked {} as considered", token_id);
        Ok(())
    }

    /// Check if a token is in cooldown period
    fn is_token_in_cooldown(&self, token_id: &str) -> bool {
        if let Some(last_considered) = self.considered_tokens().get(token_id) {
            let time_since_last = Utc::now() - *last_considered;
            time_since_last < self.cooldown_period()
        } else {
            false
        }
    }

    /// Update internal market data for the strategy (default implementation)
    fn update_market_data(&mut self, token: &TokenMetrics) {
        let profile = self.indicator_profile();

        {
            let mut time_series = self
                .get_price_data()
                .entry(token.id.clone())
                .or_insert_with(|| PriceTimeSeries::new(profile));

            time_series.add_data_point(token.price_usd, token.volume_24h, Utc::now());
        }

        trace!(
            "Updated price data for {} - Price: ${:.4}",
            token.id,
            token.price_usd
        );
    }
}

/// Enum wrapper for all strategy types to enable cloning and avoid trait object issues
#[derive(Clone)]
pub enum Strategy {
    Momentum(crate::core::strategies::custom::momentum::MomentumStrategy),
    Rsi(crate::core::strategies::custom::rsi::RsiStrategy),
}

impl TradingStrategy for Strategy {
    fn name(&self) -> &str {
        match self {
            Strategy::Momentum(s) => s.name(),
            Strategy::Rsi(s) => s.name(),
        }
    }

    fn analyze_for_entry(&self, token: &TokenMetrics) -> bool {
        match self {
            Strategy::Momentum(s) => s.analyze_for_entry(token),
            Strategy::Rsi(s) => s.analyze_for_entry(token),
        }
    }

    fn analyze_for_exit(
        &self,
        token: &TokenMetrics,
        position: Option<&Position>,
        risk_params: Option<(f64, f64)>,
    ) -> Option<ExitReason> {
        match self {
            Strategy::Momentum(s) => s.analyze_for_exit(token, position, risk_params),
            Strategy::Rsi(s) => s.analyze_for_exit(token, position, risk_params),
        }
    }

    fn get_price_data(&mut self) -> &mut Arc<DashMap<String, PriceTimeSeries>> {
        match self {
            Strategy::Momentum(s) => s.get_price_data(),
            Strategy::Rsi(s) => s.get_price_data(),
        }
    }

    fn price_series_for(&self, token_id: &str) -> Option<PriceTimeSeries> {
        match self {
            Strategy::Momentum(s) => s.price_series_for(token_id),
            Strategy::Rsi(s) => s.price_series_for(token_id),
        }
    }

    fn indicator_weights(&self) -> IndicatorWeights {
        match self {
            Strategy::Momentum(s) => s.indicator_weights(),
            Strategy::Rsi(s) => s.indicator_weights(),
        }
    }

    fn min_volume(&self) -> f64 {
        match self {
            Strategy::Momentum(s) => s.min_volume(),
            Strategy::Rsi(s) => s.min_volume(),
        }
    }

    fn stop_loss_pct(&self) -> f64 {
        match self {
            Strategy::Momentum(s) => s.stop_loss_pct(),
            Strategy::Rsi(s) => s.stop_loss_pct(),
        }
    }

    fn cooldown_period(&self) -> ChronoDuration {
        match self {
            Strategy::Momentum(s) => s.cooldown_period(),
            Strategy::Rsi(s) => s.cooldown_period(),
        }
    }

    fn considered_tokens(&self) -> &Arc<DashMap<String, DateTime<Utc>>> {
        match self {
            Strategy::Momentum(s) => s.considered_tokens(),
            Strategy::Rsi(s) => s.considered_tokens(),
        }
    }

    fn indicator_profile(&self) -> crate::core::constants::IndicatorProfile {
        match self {
            Strategy::Momentum(s) => s.indicator_profile(),
            Strategy::Rsi(s) => s.indicator_profile(),
        }
    }
}

impl fmt::Display for Strategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Strategy::Momentum(s) => s.fmt(f),
            Strategy::Rsi(s) => s.fmt(f),
        }
    }
}

impl fmt::Debug for Strategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Strategy::Momentum(s) => write!(f, "Strategy::Momentum({:?})", s),
            Strategy::Rsi(s) => write!(f, "Strategy::Rsi({:?})", s),
        }
    }
}
