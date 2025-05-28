use super::base::BaseStrategy;
use super::core::{ExitReason, Position, TradingStrategy};
use crate::core::models::market::TokenMetrics;
use crate::domain::trading::indicators::{
    analyze_volume_trend, calculate_bollinger_bands, calculate_composite_momentum, calculate_macd,
    calculate_rsi, IndicatorWeights, PriceTimeSeries,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use dashmap::DashMap;
use log::{debug, info, trace, warn};
use std::any::Any;
use std::fmt;
use std::sync::Arc;

#[derive(Clone)]
pub struct MomentumStrategy {
    pub threshold: f64,
    pub min_volume: f64,
    pub stop_loss_pct: f64,
    // Use DashMap instead of RwLock<HashMap>
    price_data: Arc<DashMap<String, PriceTimeSeries>>,
    considered_tokens: Arc<DashMap<String, DateTime<Utc>>>,
    indicator_weights: IndicatorWeights,
    cooldown_period: ChronoDuration,
    min_data_points: usize,
    risk_tolerance: f64,
    trading_mode: crate::domain::trading::indicators::TradingMode,
}

impl MomentumStrategy {
    /// Create a new momentum strategy
    pub fn new(threshold: f64, min_volume: f64, stop_loss_pct: f64) -> Self {
        Self {
            threshold,
            min_volume,
            stop_loss_pct,
            price_data: Arc::new(DashMap::new()),
            considered_tokens: Arc::new(DashMap::new()),
            indicator_weights: IndicatorWeights::default(),
            cooldown_period: ChronoDuration::hours(1),
            min_data_points: 20,
            risk_tolerance: 0.5,
            trading_mode: crate::domain::trading::indicators::TradingMode::Production,
        }
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

    pub fn strategy_name() -> &'static str {
        "momentum"
    }

    pub fn with_min_data_points(mut self, points: usize) -> Self {
        self.min_data_points = points;
        self
    }

    pub fn with_minimum_volume(mut self, min_volume: f64) -> Self {
        self.min_volume = min_volume;
        self
    }

    pub fn with_risk_tolerance(mut self, level: f64) -> Self {
        self.risk_tolerance = level;
        self
    }

    pub fn with_indicator_weights(mut self, weights: IndicatorWeights) -> Self {
        self.indicator_weights = weights;
        self
    }

    pub fn log_status(&self) -> Result<(), crate::core::error::Error> {
        info!(
            "📊 Momentum Strategy Status - Threshold: {:.2}%, Min Volume: ${:.2}M, Stop Loss: {:.2}%",
            self.threshold,
            self.min_volume / 1_000_000.0,
            self.stop_loss_pct
        );
        info!("📈 Tracking {} tokens", self.price_data.len());
        Ok(())
    }

    fn mark_token_as_considered(&self, token_id: String) -> Result<(), crate::core::error::Error> {
        self.considered_tokens.insert(token_id.clone(), Utc::now());
        trace!("📝 Marked {} as considered", token_id);
        Ok(())
    }

    fn get_price_time_series(&self, token: &TokenMetrics) -> Option<PriceTimeSeries> {
        self.price_data.get(&token.id).map(|entry| entry.clone())
    }

    pub fn update_market_data(&mut self, token: &TokenMetrics) {
        // Get or create price time series for this token
        let mut time_series = self
            .price_data
            .entry(token.id.clone())
            .or_insert_with(|| PriceTimeSeries::new(self.trading_mode));

        // Add the new data point
        time_series.add_data_point(token.price_usd, token.volume_24h, Utc::now());

        trace!(
            "📊 Updated price data for {} - Price: ${:.4}, Volume: ${:.2}M",
            token.id,
            token.price_usd,
            token.volume_24h / 1_000_000.0
        );
    }
}

impl TradingStrategy for MomentumStrategy {
    fn name(&self) -> &str {
        "momentum_strategy"
    }

    fn update_market_data(&mut self, token: &TokenMetrics) {
        self.update_market_data(token);
    }

    fn analyze_for_entry(&self, token: &TokenMetrics) -> bool {
        // Basic volume check
        if !self.check_volume_requirements(token) {
            trace!(
                "❌ {} failed volume check: ${:.2}M < ${:.2}M",
                token.id,
                token.volume_24h / 1_000_000.0,
                self.min_volume / 1_000_000.0
            );
            return false;
        }

        // Check cooldown period
        if let Some(last_considered) = self.considered_tokens.get(&token.id) {
            let time_since_last = Utc::now() - *last_considered;
            if time_since_last < self.cooldown_period {
                trace!("⏳ {} in cooldown period", token.id);
                return false;
            }
        }

        // Get price time series
        let time_series = match self.get_price_time_series(token) {
            Some(ts) => ts,
            None => {
                trace!("📊 No price data for {}", token.id);
                return false;
            }
        };

        // Check if we have enough data points
        if !time_series.has_enough_data(self.min_data_points) {
            trace!(
                "📊 Not enough data points for {} (need {})",
                token.id,
                self.min_data_points
            );
            return false;
        }

        // Calculate composite momentum
        let momentum_score =
            match calculate_composite_momentum(&time_series, &self.indicator_weights) {
                Some(score) => score,
                None => {
                    trace!("📊 Failed to calculate momentum for {}", token.id);
                    return false;
                }
            };

        // Check if momentum exceeds threshold
        let should_buy = momentum_score >= self.threshold;

        if should_buy {
            info!(
                "🚀 {} - Strong momentum detected! Score: {:.2}% (threshold: {:.2}%)",
                token.id, momentum_score, self.threshold
            );

            // Mark token as considered
            if let Err(e) = self.mark_token_as_considered(token.id.clone()) {
                warn!("Failed to mark token as considered: {}", e);
            }

            // Log detailed analysis
            let prices = time_series.prices();
            if let Some(rsi) = calculate_rsi(&prices, 14) {
                debug!("📊 {} RSI: {:.2}", token.id, rsi);
            }

            if let Some((macd, signal, histogram)) = calculate_macd(&prices, self.trading_mode) {
                debug!(
                    "📊 {} MACD: {:.4}, Signal: {:.4}, Histogram: {:.4}",
                    token.id, macd, signal, histogram
                );
            }

            if let Some((upper, middle, lower)) = calculate_bollinger_bands(&prices, 20, 2.0) {
                let bb_position = (token.price_usd - lower) / (upper - lower) * 100.0;
                debug!("📊 {} Bollinger Position: {:.1}%", token.id, bb_position);
            }

            let volumes = time_series.volumes();
            if let Some(volume_trend) = analyze_volume_trend(&prices, &volumes, 10) {
                debug!("📊 {} Volume Trend: {:.2}", token.id, volume_trend);
            }

            return true;
        }

        trace!(
            "📊 {} momentum score {:.2}% below threshold {:.2}%",
            token.id,
            momentum_score,
            self.threshold
        );
        false
    }

    fn analyze_for_exit(
        &self,
        token: &TokenMetrics,
        position: Option<&Position>,
        risk_params: Option<(f64, f64, usize)>,
    ) -> Option<ExitReason> {
        // Use common exit analysis from BaseStrategy
        self.common_exit_analysis(token, position, risk_params)
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

impl BaseStrategy for MomentumStrategy {
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

impl fmt::Display for MomentumStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MomentumStrategy(threshold={:.2}%, min_vol=${:.2}M, stop_loss={:.2}%)",
            self.threshold,
            self.min_volume / 1_000_000.0,
            self.stop_loss_pct
        )
    }
}
