use super::base::BaseStrategy;
use super::core::{ExitReason, Position, TradingStrategy};
use crate::core::models::market::TokenMetrics;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use log::{debug, info, trace};
use rand::Rng;
use std::any::Any;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct MockStrategy {
    signal_interval: Duration,
    hold_duration: Duration,
    entry_probability: f64,
    exit_probability: f64,
    success_rate: f64,
    last_entry_signal: Arc<DashMap<String, DateTime<Utc>>>,
    position_entries: Arc<DashMap<String, DateTime<Utc>>>,
}

impl MockStrategy {
    pub fn new(threshold: f64) -> Self {
        Self {
            signal_interval: Duration::from_secs(300),
            hold_duration: Duration::from_secs(1800),
            entry_probability: 0.1,
            exit_probability: 0.2,
            success_rate: threshold.max(0.1).min(0.9),
            last_entry_signal: Arc::new(DashMap::new()),
            position_entries: Arc::new(DashMap::new()),
        }
    }

    pub fn strategy_name() -> &'static str {
        "mock"
    }

    fn should_generate_entry_signal(&self, token_id: &str) -> bool {
        if let Some(last_signal) = self.last_entry_signal.get(token_id) {
            let elapsed = Utc::now()
                .signed_duration_since(*last_signal)
                .to_std()
                .unwrap_or(Duration::ZERO);
            if elapsed < self.signal_interval {
                return false;
            }
        }
        rand::thread_rng().gen::<f64>() < self.entry_probability
    }

    fn update_last_signal_time(&self, token_id: &str) {
        self.last_entry_signal
            .insert(token_id.to_string(), Utc::now());
    }

    fn record_position_entry(&self, token_id: &str) {
        self.position_entries
            .insert(token_id.to_string(), Utc::now());
        trace!("📝 Mock strategy recorded position entry for {}", token_id);
    }

    fn should_generate_exit_signal(&self, token_id: &str) -> bool {
        if let Some(entry_time) = self.position_entries.get(token_id) {
            let elapsed = Utc::now()
                .signed_duration_since(*entry_time)
                .to_std()
                .unwrap_or(Duration::ZERO);

            if elapsed >= self.hold_duration {
                debug!(
                    "🕐 Mock strategy forcing exit for {} after {:?}",
                    token_id, elapsed
                );
                return true;
            }

            if elapsed >= self.hold_duration / 2 {
                let should_exit = rand::thread_rng().gen::<f64>() < self.exit_probability;
                if should_exit {
                    debug!("🎲 Mock strategy random exit for {}", token_id);
                }
                return should_exit;
            }
        }
        false
    }

    fn clear_position_entry(&self, token_id: &str) {
        self.position_entries.remove(token_id);
        trace!("🗑️ Mock strategy cleared position entry for {}", token_id);
    }
}

impl TradingStrategy for MockStrategy {
    fn name(&self) -> &str {
        "mock_strategy"
    }

    fn analyze_for_entry(&self, token: &TokenMetrics) -> bool {
        if token.volume_24h < 100_000.0 {
            return false;
        }

        if self.should_generate_entry_signal(&token.id) {
            info!(
                "🎯 Mock strategy generating BUY signal for {} at ${:.4}",
                token.id, token.price
            );
            self.update_last_signal_time(&token.id);
            self.record_position_entry(&token.id);
            return true;
        }
        false
    }

    fn analyze_for_exit(
        &self,
        token: &TokenMetrics,
        position: Option<&Position>,
        _risk_params: Option<(f64, f64, usize)>,
    ) -> Option<ExitReason> {
        let _position = position?;

        if self.should_generate_exit_signal(&token.id) {
            let is_profitable = rand::thread_rng().gen::<f64>() < self.success_rate;
            let reason = if is_profitable {
                "Mock strategy: simulated profitable exit"
            } else {
                "Mock strategy: simulated stop loss"
            };

            info!(
                "🎯 Mock strategy generating EXIT signal for {} at ${:.4} - {}",
                token.id, token.price, reason
            );
            self.clear_position_entry(&token.id);

            return Some(if is_profitable {
                ExitReason::strategy_based(reason)
            } else {
                ExitReason::risk_based(reason)
            });
        }
        None
    }

    fn update_market_data(&mut self, _token: &TokenMetrics) {}

    fn should_exit(&self, position: &Position) -> bool {
        self.should_generate_exit_signal(&position.token_id)
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

impl BaseStrategy for MockStrategy {
    fn get_stop_loss_pct(&self) -> f64 {
        5.0
    }

    fn get_min_volume(&self) -> f64 {
        100_000.0
    }

    fn get_threshold(&self) -> f64 {
        self.success_rate * 100.0
    }
}

impl fmt::Display for MockStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MockStrategy(success_rate={:.1}%, interval={}s, hold={}s)",
            self.success_rate * 100.0,
            self.signal_interval.as_secs(),
            self.hold_duration.as_secs()
        )
    }
}
