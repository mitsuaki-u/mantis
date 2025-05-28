use super::base::BaseStrategy;
use super::core::{ExitReason, Position, TradingStrategy};
use crate::core::models::market::TokenMetrics;
use crate::domain::trading::indicators::{calculate_rsi, PriceTimeSeries};
use chrono::Utc;
use dashmap::DashMap;
use log::{debug, info, trace};
use std::any::Any;
use std::fmt;
use std::sync::Arc;

#[derive(Clone)]
pub struct RSIStrategy {
    threshold: f64,
    min_volume: f64,
    stop_loss_pct: f64,
    price_data: Arc<DashMap<String, PriceTimeSeries>>,
    overbought_level: f64,
    oversold_level: f64,
    rsi_period: usize,
    trading_mode: crate::domain::trading::indicators::TradingMode,
}

impl RSIStrategy {
    /// Create a new RSI strategy
    pub fn new(threshold: f64, min_volume: f64, stop_loss_pct: f64) -> Self {
        Self {
            threshold,
            min_volume,
            stop_loss_pct,
            price_data: Arc::new(DashMap::new()),
            overbought_level: 70.0,
            oversold_level: 30.0,
            rsi_period: 14,
            trading_mode: crate::domain::trading::indicators::TradingMode::Production,
        }
    }

    pub fn strategy_name() -> &'static str {
        "rsi"
    }

    pub fn with_rsi_levels(mut self, oversold: f64, overbought: f64) -> Self {
        self.oversold_level = oversold;
        self.overbought_level = overbought;
        self
    }

    pub fn with_rsi_period(mut self, period: usize) -> Self {
        self.rsi_period = period;
        self
    }

    pub fn with_trading_mode(
        mut self,
        mode: crate::domain::trading::indicators::TradingMode,
    ) -> Self {
        self.trading_mode = mode;
        // Update existing price data to use new mode
        for mut entry in self.price_data.iter_mut() {
            *entry.value_mut() = PriceTimeSeries::new(mode);
        }
        self
    }

    fn get_price_time_series(&self, token: &TokenMetrics) -> Option<PriceTimeSeries> {
        self.price_data.get(&token.id).map(|entry| entry.clone())
    }
}

impl TradingStrategy for RSIStrategy {
    fn name(&self) -> &str {
        "rsi_strategy"
    }

    fn update_market_data(&mut self, token: &TokenMetrics) {
        // Get or create price time series for this token
        let mut time_series = self
            .price_data
            .entry(token.id.clone())
            .or_insert_with(|| PriceTimeSeries::new(self.trading_mode));

        // Add the new data point
        time_series.add_data_point(token.price_usd, token.volume_24h, Utc::now());

        trace!(
            "📊 Updated RSI price data for {} - Price: ${:.4}",
            token.id,
            token.price_usd
        );
    }

    fn analyze_for_entry(&self, token: &TokenMetrics) -> bool {
        // Basic volume check
        if !self.check_volume_requirements(token) {
            trace!("❌ {} failed volume check for RSI strategy", token.id);
            return false;
        }

        // Get price time series
        let time_series = match self.get_price_time_series(token) {
            Some(ts) => ts,
            None => {
                trace!("📊 No price data for {} in RSI strategy", token.id);
                return false;
            }
        };

        // Check if we have enough data points for RSI calculation
        let min_points = self.rsi_period + 1;
        if !time_series.has_enough_data(min_points) {
            trace!(
                "📊 Not enough data points for RSI calculation on {} (need {})",
                token.id,
                min_points
            );
            return false;
        }

        // Calculate RSI
        let prices = time_series.prices();
        let rsi = match calculate_rsi(&prices, self.rsi_period) {
            Some(rsi_value) => rsi_value,
            None => {
                trace!("📊 Failed to calculate RSI for {}", token.id);
                return false;
            }
        };

        // RSI strategy: Buy when oversold (RSI < oversold_level)
        let should_buy = rsi <= self.oversold_level;

        if should_buy {
            info!(
                "🎯 {} - RSI oversold signal! RSI: {:.2} <= {:.2}",
                token.id, rsi, self.oversold_level
            );
            return true;
        }

        trace!(
            "📊 {} RSI {:.2} not oversold (threshold: {:.2})",
            token.id,
            rsi,
            self.oversold_level
        );
        false
    }

    fn analyze_for_exit(
        &self,
        token: &TokenMetrics,
        position: Option<&Position>,
        risk_params: Option<(f64, f64, usize)>,
    ) -> Option<ExitReason> {
        // First check common exit conditions
        if let Some(exit_reason) = self.common_exit_analysis(token, position, risk_params) {
            return Some(exit_reason);
        }

        // RSI-specific exit logic
        let time_series = self.get_price_time_series(token)?;
        let prices = time_series.prices();

        if let Some(rsi) = calculate_rsi(&prices, self.rsi_period) {
            // Exit when overbought (RSI > overbought_level)
            if rsi >= self.overbought_level {
                let reason = format!(
                    "RSI overbought exit: {:.2} >= {:.2}",
                    rsi, self.overbought_level
                );
                info!("📈 {} - {}", token.id, reason);
                return Some(ExitReason::strategy_based(&reason));
            }

            debug!(
                "📊 {} RSI: {:.2} (oversold: {:.2}, overbought: {:.2})",
                token.id, rsi, self.oversold_level, self.overbought_level
            );
        }

        None
    }

    fn box_clone(&self) -> Box<dyn TradingStrategy> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl BaseStrategy for RSIStrategy {
    fn get_stop_loss_pct(&self) -> f64 {
        self.stop_loss_pct
    }

    fn get_min_volume(&self) -> f64 {
        self.min_volume
    }

    fn get_threshold(&self) -> f64 {
        self.threshold
    }
}

impl fmt::Display for RSIStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "RSIStrategy(oversold={:.0}, overbought={:.0}, period={}, stop_loss={:.2}%)",
            self.oversold_level, self.overbought_level, self.rsi_period, self.stop_loss_pct
        )
    }
}
