use super::super::traits::TradingStrategy;
use crate::core::domain::market::TokenMetrics;
use crate::core::domain::trading::{ExitReason, Position};
use crate::core::indicators::{
    analyze_volume_trend, calculate_bollinger_bands, calculate_composite_momentum, calculate_macd,
    calculate_rsi, get_max_required_points, IndicatorWeights, PriceTimeSeries,
};

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use dashmap::DashMap;
use log::{debug, info, trace, warn};
use std::fmt;
use std::sync::Arc;

/// Momentum-based trading strategy
#[derive(Clone, Debug)]
pub struct MomentumStrategy {
    pub momentum_entry_threshold: f64,
    pub min_volume: f64,
    pub stop_loss_pct: f64,
    price_data: Arc<DashMap<String, PriceTimeSeries>>,
    considered_tokens: Arc<DashMap<String, DateTime<Utc>>>,
    pub indicator_weights: IndicatorWeights,
    pub indicator_profile: crate::core::constants::IndicatorProfile,
    cooldown_period: ChronoDuration,
}

impl Default for MomentumStrategy {
    fn default() -> Self {
        Self::new(0.6, 1000000.0, 5.0)
    }
}

impl MomentumStrategy {
    /// Create a new momentum strategy
    pub fn new(momentum_entry_threshold: f64, min_volume: f64, stop_loss_pct: f64) -> Self {
        Self {
            momentum_entry_threshold,
            min_volume,
            stop_loss_pct,
            price_data: Arc::new(DashMap::new()),
            considered_tokens: Arc::new(DashMap::new()),
            indicator_weights: IndicatorWeights::default(),
            indicator_profile: crate::core::constants::IndicatorProfile::default(),
            cooldown_period: ChronoDuration::hours(1),
        }
    }

    pub fn strategy_name() -> &'static str {
        "momentum"
    }

    pub fn log_status(&self) -> Result<(), crate::core::errors::Error> {
        info!(
            "📊 Momentum Strategy Status - Momentum Entry Threshold: {:.2}%, Min Volume: ${:.2}M, Stop Loss: {:.2}%",
            self.momentum_entry_threshold,
            self.min_volume / 1_000_000.0,
            self.stop_loss_pct
        );
        info!("📈 Tracking {} tokens", self.price_data.len());
        Ok(())
    }

    /// Get the minimum volume threshold for this strategy
    pub fn get_min_volume(&self) -> f64 {
        self.min_volume
    }

    fn get_price_time_series(&self, token: &TokenMetrics) -> Option<PriceTimeSeries> {
        self.price_data.get(&token.id).map(|entry| entry.clone())
    }
}

impl TradingStrategy for MomentumStrategy {
    fn name(&self) -> &str {
        "momentum_strategy"
    }

    fn get_price_data(&mut self) -> &mut Arc<DashMap<String, PriceTimeSeries>> {
        &mut self.price_data
    }

    fn analyze_for_entry(&self, token: &TokenMetrics) -> bool {
        if token.symbol.is_empty() {
            debug!(
                "❌ Skipping token {} - empty symbol (data quality issue)",
                &token.id[..10]
            );
            return false;
        }

        if !self.check_volume_requirements(token) {
            debug!(
                "❌ {} ({}) failed volume check: ${:.2}M < ${:.2}M (required)",
                token.symbol,
                &token.id[..10],
                token.volume_24h / 1_000_000.0,
                self.min_volume / 1_000_000.0
            );
            return false;
        }

        debug!(
            "✅ {} ({}) passed volume check: ${:.2}M >= ${:.2}M (required)",
            token.symbol,
            &token.id[..10],
            token.volume_24h / 1_000_000.0,
            self.min_volume / 1_000_000.0
        );

        if self.is_token_in_cooldown(&token.id) {
            trace!("{} in cooldown period", token.symbol);
            return false;
        }

        let time_series = match self.get_price_time_series(token) {
            Some(ts) => ts,
            None => {
                trace!("No price data for {}", token.symbol);
                return false;
            }
        };

        debug!(
            "📊 {} ({}) has {} price data points (need {} for full analysis)",
            token.symbol,
            &token.id[..10],
            time_series.prices().len(),
            get_max_required_points(self.indicator_profile)
        );

        let momentum_score = match calculate_composite_momentum(
            &time_series,
            &self.indicator_weights,
            &token.symbol,
            &token.id,
        ) {
            Some(score) => score,
            None => {
                debug!(
                    "📊 Failed to calculate momentum for {} ({}) - insufficient data",
                    token.symbol,
                    &token.id[..10]
                );
                return false;
            }
        };

        let should_buy = momentum_score >= self.momentum_entry_threshold;

        info!(
            "🎯 {} ({}) momentum analysis: Score={:.3}, Threshold={:.3}, Should Buy={}",
            token.symbol,
            &token.id[..10],
            momentum_score,
            self.momentum_entry_threshold,
            should_buy
        );

        if should_buy {
            info!(
                "🚀 {} ({}) - Strong momentum detected! Score: {:.2}% (threshold: {:.2}%)",
                token.symbol,
                &token.id[..10],
                momentum_score,
                self.momentum_entry_threshold
            );

            if let Err(e) = self.mark_token_as_considered(token.id.clone()) {
                warn!("Failed to mark token as considered: {}", e);
            }

            let prices = time_series.prices();
            if let Some(rsi) = calculate_rsi(&prices, 14) {
                debug!("📊 {} ({}) RSI: {:.2}", token.symbol, &token.id[..10], rsi);
            }

            let (_rsi, fast, slow, signal_period, _bb, _vol) = self.indicator_profile.periods();
            if let Some((macd, signal, histogram)) =
                calculate_macd(&prices, fast, slow, signal_period)
            {
                debug!(
                    "📊 {} ({}) MACD: {:.4}, Signal: {:.4}, Histogram: {:.4}",
                    token.symbol,
                    &token.id[..10],
                    macd,
                    signal,
                    histogram
                );
            }

            if let Some((upper, _middle, lower)) = calculate_bollinger_bands(
                &prices,
                20,
                crate::core::constants::BOLLINGER_STD_DEV_MULTIPLIER,
            ) {
                let bb_position = (token.price_usd - lower) / (upper - lower) * 100.0;
                debug!(
                    "📊 {} ({}) Bollinger Position: {:.1}%",
                    token.symbol,
                    &token.id[..10],
                    bb_position
                );
            }

            let volumes = time_series.volumes();
            if let Some(volume_trend) = analyze_volume_trend(
                &prices,
                &volumes,
                crate::core::constants::DEFAULT_VOLUME_LOOKBACK,
            ) {
                debug!(
                    "📊 {} ({}) Volume Trend: {:.2}",
                    token.symbol,
                    &token.id[..10],
                    volume_trend
                );
            }

            return true;
        }

        debug!(
            "📊 {} ({}) momentum score {:.2}% below threshold {:.2}%",
            token.symbol,
            &token.id[..10],
            momentum_score,
            self.momentum_entry_threshold
        );
        false
    }

    fn analyze_for_exit(
        &self,
        token: &TokenMetrics,
        position: Option<&Position>,
        risk_params: Option<(f64, f64)>,
    ) -> Option<ExitReason> {
        use super::super::exit_conditions::checks;
        checks::common_exit_analysis(
            token,
            position,
            risk_params,
            self.stop_loss_pct,
            Some(self.momentum_entry_threshold),
        )
    }

    fn min_volume(&self) -> f64 {
        self.min_volume
    }

    fn stop_loss_pct(&self) -> f64 {
        self.stop_loss_pct
    }

    fn considered_tokens(&self) -> &Arc<DashMap<String, DateTime<Utc>>> {
        &self.considered_tokens
    }

    fn cooldown_period(&self) -> ChronoDuration {
        self.cooldown_period
    }

    fn indicator_profile(&self) -> crate::core::constants::IndicatorProfile {
        self.indicator_profile
    }
}

impl fmt::Display for MomentumStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MomentumStrategy(momentum_entry_threshold={:.2}%, min_vol=${:.2}M, stop_loss={:.2}%)",
            self.momentum_entry_threshold,
            self.min_volume / 1_000_000.0,
            self.stop_loss_pct
        )
    }
}
