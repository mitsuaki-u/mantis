use super::super::traits::TradingStrategy;
use crate::core::constants::rsi::{
    COOLDOWN_HOURS, DEFAULT_MIN_VOLUME, DEFAULT_STOP_LOSS_PCT, OVERSOLD_THRESHOLD,
};
use crate::core::constants::rsi::{STANDARD_PERIOD as RSI_PERIOD, TRADITIONAL_OVERBOUGHT};
use crate::core::domain::market::TokenMetrics;
use crate::core::domain::trading::{ExitReason, Position};
use crate::core::indicators::{calculate_rsi, PriceTimeSeries};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use dashmap::DashMap;
use log::{debug, info, trace, warn};
use std::fmt;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct RsiStrategy {
    pub oversold_threshold: f64,
    pub overbought_threshold: f64,
    pub min_volume: f64,
    pub stop_loss_pct: f64,
    price_data: Arc<DashMap<String, PriceTimeSeries>>,
    considered_tokens: Arc<DashMap<String, DateTime<Utc>>>,
    pub indicator_profile: crate::core::constants::IndicatorProfile,
    cooldown_period: ChronoDuration,
}

impl Default for RsiStrategy {
    fn default() -> Self {
        Self::new(
            OVERSOLD_THRESHOLD,
            TRADITIONAL_OVERBOUGHT,
            DEFAULT_MIN_VOLUME,
            DEFAULT_STOP_LOSS_PCT,
        )
    }
}

impl RsiStrategy {
    pub fn new(oversold: f64, overbought: f64, min_volume: f64, stop_loss_pct: f64) -> Self {
        Self {
            oversold_threshold: oversold,
            overbought_threshold: overbought,
            min_volume,
            stop_loss_pct,
            price_data: Arc::new(DashMap::new()),
            considered_tokens: Arc::new(DashMap::new()),
            indicator_profile: crate::core::constants::IndicatorProfile::default(),
            cooldown_period: ChronoDuration::hours(COOLDOWN_HOURS),
        }
    }

    pub fn strategy_name() -> &'static str {
        "rsi"
    }

    fn get_price_time_series(&self, token: &TokenMetrics) -> Option<PriceTimeSeries> {
        self.price_data.get(&token.id).map(|entry| entry.clone())
    }
}

impl TradingStrategy for RsiStrategy {
    fn name(&self) -> &str {
        "rsi_strategy"
    }

    fn get_price_data(&mut self) -> &mut Arc<DashMap<String, PriceTimeSeries>> {
        &mut self.price_data
    }

    fn analyze_for_entry(&self, token: &TokenMetrics) -> bool {
        if token.symbol.is_empty() {
            debug!(
                "❌ Skipping token {} - empty symbol (subgraph data quality issue)",
                &token.id[..10]
            );
            return false;
        }

        if self.is_token_in_cooldown(&token.id) {
            trace!("{} in cooldown period", token.symbol);
            return false;
        }

        if !self.check_volume_requirements(token) {
            trace!("{} failed volume check for RSI strategy", token.id);
            return false;
        }

        let time_series = match self.get_price_time_series(token) {
            Some(ts) => ts,
            None => {
                trace!("No price data for {} in RSI strategy", token.id);
                return false;
            }
        };

        let rsi_period = RSI_PERIOD;
        let oversold_level = self.oversold_threshold;
        let min_points = rsi_period + 1;
        if !time_series.has_enough_data(min_points) {
            trace!(
                "Not enough data points for RSI calculation on {} (need {})",
                token.id,
                min_points
            );
            return false;
        }

        let prices = time_series.prices();
        let rsi = match calculate_rsi(&prices, rsi_period) {
            Some(rsi_value) => rsi_value,
            None => {
                trace!("Failed to calculate RSI for {}", token.id);
                return false;
            }
        };

        let should_buy = rsi <= oversold_level;

        if should_buy {
            info!(
                "🎯 {} - RSI oversold signal! RSI: {:.2} <= {:.2}",
                token.id, rsi, oversold_level
            );

            if let Err(e) = self.mark_token_as_considered(token.id.clone()) {
                warn!("Failed to mark token as considered: {}", e);
            }

            return true;
        }

        trace!(
            "{} RSI {:.2} not oversold (threshold: {:.2})",
            token.id,
            rsi,
            oversold_level
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
        if let Some(exit_reason) = checks::common_exit_analysis(
            token,
            position,
            risk_params,
            self.stop_loss_pct,
            Some(self.oversold_threshold),
        ) {
            return Some(exit_reason);
        }

        let time_series = self.get_price_time_series(token)?;
        let prices = time_series.prices();

        let rsi_period = RSI_PERIOD;
        let overbought_level = self.overbought_threshold;
        let oversold_level = self.oversold_threshold;

        if let Some(rsi) = calculate_rsi(&prices, rsi_period) {
            if rsi >= overbought_level {
                let reason = format!("RSI overbought exit: {:.2} >= {:.2}", rsi, overbought_level);
                info!("📈 {} - {}", token.id, reason);
                return Some(ExitReason::strategy_based(&reason));
            }

            debug!(
                "📊 {} RSI: {:.2} (oversold: {:.2}, overbought: {:.2})",
                token.id, rsi, oversold_level, overbought_level
            );
        }

        None
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

impl fmt::Display for RsiStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "RSIStrategy(oversold={}, overbought={}, period={}, stop_loss={:.2}%)",
            self.oversold_threshold, self.overbought_threshold, RSI_PERIOD, self.stop_loss_pct
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::indicators::PriceTimeSeries;

    fn strategy_with(oversold: f64, overbought: f64) -> RsiStrategy {
        RsiStrategy::new(oversold, overbought, 1000.0, 5.0)
    }

    fn strategy_with_prices(oversold: f64, overbought: f64, prices: &[f64]) -> RsiStrategy {
        let s = strategy_with(oversold, overbought);
        let mut ts = PriceTimeSeries::default();
        for &p in prices {
            ts.add_data_point(p, 0.0, chrono::Utc::now());
        }
        s.price_data.insert("tok".to_string(), ts);
        s
    }

    fn token(id: &str) -> crate::core::domain::market::TokenMetrics {
        crate::core::domain::market::TokenMetrics {
            id: id.to_string(),
            symbol: "TOK".to_string(),
            name: "Token".to_string(),
            decimals: 18,
            price_usd: 1.0,
            price_change_24h: 0.5,
            volume_24h: 5_000_000.0,
            chain: None,
            last_updated: chrono::Utc::now(),
        }
    }

    #[test]
    fn constructor_stores_both_thresholds() {
        let s = strategy_with(25.0, 75.0);
        assert_eq!(s.oversold_threshold, 25.0);
        assert_eq!(s.overbought_threshold, 75.0);
    }

    #[test]
    fn display_shows_configured_values_not_constants() {
        let s = strategy_with(20.0, 80.0);
        let display = format!("{}", s);
        assert!(
            display.contains("oversold=20"),
            "Expected oversold=20 in: {}",
            display
        );
        assert!(
            display.contains("overbought=80"),
            "Expected overbought=80 in: {}",
            display
        );
    }

    #[test]
    fn entry_uses_oversold_threshold_from_config() {
        let prices: Vec<f64> = (0..30)
            .map(|i| 100.0 + if i % 2 == 0 { 1.0 } else { -0.5 })
            .collect();
        let tok = token("tok");

        let s_always = strategy_with_prices(100.0, 80.0, &prices);
        assert!(
            s_always.analyze_for_entry(&tok),
            "oversold=100 should always trigger"
        );

        let tok2 = token("tok");
        let s_never = strategy_with_prices(1.0, 80.0, &prices);
        assert!(
            !s_never.analyze_for_entry(&tok2),
            "oversold=1 should never trigger on stable prices"
        );
    }
}
