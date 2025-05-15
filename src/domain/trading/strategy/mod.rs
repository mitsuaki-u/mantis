use crate::core::error::Error;
use crate::core::models::market::TokenMetrics;
use crate::domain::trading::indicators::{
    analyze_volume_trend, calculate_bollinger_bands, calculate_composite_momentum, calculate_macd,
    calculate_rsi, IndicatorWeights, PriceTimeSeries,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use dashmap::DashMap;
use log::{debug, error, info, trace, warn};
use rand;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::fmt::Debug;
use std::sync::{Arc, RwLock};

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
    #[serde(skip, default)]
    pub coingecko_id: String, // Alias to provider_id for database compatibility
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
            coingecko_id: provider_id,
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
    fn should_exit(&self, position: &Position) -> bool {
        // Since we need token metrics now, this is harder to implement in backwards compatible way
        // We'll use a default implementation that indicates no exit signal
        false
    }

    /// Update internal market data for the strategy
    fn update_market_data(&mut self, token: &TokenMetrics);

    /// Clone the strategy into a boxed trait object
    fn box_clone(&self) -> Box<dyn TradingStrategy>;
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

    /// Original analyze method kept for backwards compatibility
    pub fn analyze(&self, token: &TokenMetrics) -> Signal {
        self.inner.analyze(token)
    }

    /// New method for entry analysis only
    pub fn analyze_for_entry(&self, token: &TokenMetrics) -> bool {
        debug!(
            "STRATEGY_WRAPPER: Entered analyze_for_entry for {}",
            token.symbol
        );
        // Skip analysis for tokens with invalid prices
        if token.price_usd <= 0.0 {
            trace!(
                "Skipping entry analysis for {} - price is invalid: ${:.4}",
                token.symbol,
                token.price_usd
            );
            debug!(
                "STRATEGY_WRAPPER: Exiting analyze_for_entry for {} (invalid price)",
                token.symbol
            );
            return false;
        }
        debug!(
            "STRATEGY_WRAPPER: Price valid for {}. Calling inner.analyze_for_entry...",
            token.symbol
        );
        let result = self.inner.analyze_for_entry(token);
        debug!(
            "STRATEGY_WRAPPER: Inner analyze_for_entry for {} returned: {}. Exiting.",
            token.symbol, result
        );
        result
    }

    /// New method for exit analysis that considers the latest market data
    pub fn analyze_for_exit(
        &self,
        token: &TokenMetrics,
        position: Option<&Position>,
        risk_params: Option<(f64, f64, usize)>,
    ) -> Option<ExitReason> {
        self.inner.analyze_for_exit(token, position, risk_params)
    }

    /// Original should_exit method kept for backwards compatibility
    pub fn should_exit(&self, position: &Position) -> bool {
        self.inner.should_exit(position)
    }

    pub fn update_market_data(&mut self, token: &TokenMetrics) {
        self.inner.update_market_data(token)
    }
}

impl fmt::Display for Strategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner)
    }
}

impl fmt::Debug for Strategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Strategy({})", self.inner)
    }
}

/// Mock strategy that generates artificial trading signals for testing
#[derive(Clone)]
pub struct MockStrategy {
    // Configuration
    signal_interval: std::time::Duration,
    hold_duration: std::time::Duration,
    entry_probability: f64,
    exit_probability: f64,
    success_rate: f64,

    // State tracking - Replace RwLock<HashMap> with DashMap
    last_entry_signal: Arc<DashMap<String, DateTime<Utc>>>,
    position_entries: Arc<DashMap<String, DateTime<Utc>>>,
}

impl MockStrategy {
    /// Create a new mock strategy with the specified parameters
    pub fn new(threshold: f64, min_volume: f64, stop_loss_pct: f64) -> Self {
        // Convert threshold to probability (threshold is normally % for momentum)
        // Higher threshold = fewer signals
        let base_probability = (100.0 - threshold.min(95.0).max(5.0)) / 100.0;

        Self {
            // Default to generating signals every 30-60 seconds
            signal_interval: std::time::Duration::from_secs(30),
            // Default to holding positions for 2-5 minutes
            hold_duration: std::time::Duration::from_secs(60), // Reduced from 180 to 60 seconds for testing
            // Use threshold to determine entry probability (inverted)
            entry_probability: base_probability / 5.0, // 1-20% chance per check
            // Exit probability is much higher for testing
            exit_probability: 0.8, // Fixed high probability (80%) for testing sell signals
            // Success rate determines what % of mock trades "succeed"
            success_rate: 0.6, // 60% success by default

            // State tracking - Initialize DashMaps
            last_entry_signal: Arc::new(DashMap::new()),
            position_entries: Arc::new(DashMap::new()),
        }
    }

    // Configure signal interval (how often to generate entry signals)
    pub fn with_signal_interval(mut self, seconds: u64) -> Self {
        self.signal_interval = std::time::Duration::from_secs(seconds);
        self
    }

    // Configure hold duration (how long to hold positions before exit)
    pub fn with_hold_duration(mut self, seconds: u64) -> Self {
        self.hold_duration = std::time::Duration::from_secs(seconds);
        self
    }

    // Configure success rate (% of trades that are profitable)
    pub fn with_success_rate(mut self, rate: f64) -> Self {
        self.success_rate = rate.min(1.0).max(0.0);
        self
    }

    // Helper method to get a strategy name, used by the register_strategy macro
    pub fn strategy_name() -> &'static str {
        "mock"
    }

    // Check if enough time has passed since last signal for this token
    fn should_generate_entry_signal(&self, token_id: &str) -> bool {
        let now = Utc::now();
        // Use DashMap's .get() - returns Ref<K, V>
        if let Some(last_signal_ref) = self.last_entry_signal.get(token_id) {
            let last_time = *last_signal_ref.value(); // Dereference Ref to get DateTime<Utc>
            now.signed_duration_since(last_time)
                >= chrono::Duration::from_std(self.signal_interval).unwrap()
        } else {
            true // No entry means it's okay to signal
        }
    }

    // Update the last signal time for a token
    fn update_last_signal_time(&self, token_id: &str) {
        // Use DashMap's .insert() - handles locking internally
        self.last_entry_signal
            .insert(token_id.to_string(), Utc::now());
    }

    // Record when we entered a position
    fn record_position_entry(&self, token_id: &str) {
        let entry_time = Utc::now();
        // Use DashMap's .insert() - handles locking internally
        self.position_entries
            .insert(token_id.to_string(), entry_time);

        // Logging outside lock scope (DashMap insert is quick)
        let num_entries = self.position_entries.len();
        info!(
            "💼 MockStrategy: Recording position entry for {} at {}",
            token_id, entry_time
        );
        info!(
            "🧮 Current position entries tracked: {} positions",
            num_entries
        );
        // Cannot easily iterate DashMap here for debug logging without more complex patterns
    }

    // Check if a position has been held long enough to exit
    fn should_generate_exit_signal(&self, token_id: &str) -> bool {
        let now = Utc::now();
        // Use DashMap's .get()
        if let Some(entry_ref) = self.position_entries.get(token_id) {
            let entry_time = *entry_ref.value();
            let hold_duration = now.signed_duration_since(entry_time);
            let min_hold = chrono::Duration::from_std(self.hold_duration).unwrap();

            info!("🕒 Found position entry time: {}, current time: {}, held for: {}s, minimum hold: {}s",
                 entry_time, now, hold_duration.num_seconds(), min_hold.num_seconds());

            if hold_duration >= min_hold {
                let should_exit = rand::thread_rng().gen_bool(self.exit_probability);
                info!(
                    "✨ Minimum hold time passed for {}! Exit probability: {:.1}%, Result: {}",
                    token_id,
                    self.exit_probability * 100.0,
                    should_exit
                );
                should_exit
            } else {
                info!(
                    "⏳ Not enough time has passed for {} ({}s < {}s)",
                    token_id,
                    hold_duration.num_seconds(),
                    min_hold.num_seconds()
                );
                false
            }
        } else {
            info!("❌ No record of position entry time for {}", token_id);
            false
        }
    }

    // Clear position entry record after exit
    fn clear_position_entry(&self, token_id: &str) {
        // Use DashMap's .remove() - handles locking internally
        self.position_entries.remove(token_id);
    }
}

impl TradingStrategy for MockStrategy {
    fn name(&self) -> &str {
        "mock"
    }

    fn analyze_for_entry(&self, token: &TokenMetrics) -> bool {
        // Remove lock logging added earlier
        // debug!(\"MOCK_STRATEGY: Entered analyze_for_entry for symbol: {}, id: {}\\");

        // First validate the token price
        if token.price_usd <= 0.0 {
            trace!(
                "📉 {} Price is invalid: ${:.4} - skipping mock analysis",
                token.symbol,
                token.price_usd
            );
            return false;
        }

        // Skip tokens with too low volume to be interesting
        // This ensures we don't generate signals for every possible token
        if token.volume_24h < 10000.0 {
            // <-- RESTORING THIS FILTER
            return false;
        }

        // Check if enough time has passed since last signal
        if !self.should_generate_entry_signal(&token.id) {
            return false;
        }

        // Check if we already have an open position for this token
        if self.position_entries.contains_key(&token.id) {
            info!("🔒 MockStrategy: Skipping BUY signal for {} because an open position already exists", token.symbol);
            return false;
        }

        // Random chance to generate entry signal
        let should_enter = rand::thread_rng().gen_bool(self.entry_probability);

        if should_enter {
            info!(
                "🎲 Mock strategy generating BUY signal for {}",
                token.symbol
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
        // If we don't have a position, can't exit
        let position = match position {
            Some(p) => p,
            None => return None,
        };

        // Check if we should generate an exit signal
        if self.should_generate_exit_signal(&token.id) {
            // Determine if this will be a "successful" exit based on success rate
            let is_successful = rand::thread_rng().gen_bool(self.success_rate);

            // Clear the position entry record
            self.clear_position_entry(&token.id);

            if is_successful {
                info!("🧪 MOCK: Generated profitable SELL signal for {}", token.id);
                return Some(ExitReason::strategy_based("Mock successful exit"));
            } else {
                info!(
                    "🧪 MOCK: Generated unprofitable SELL signal for {}",
                    token.id
                );
                return Some(ExitReason::risk_based("Mock unsuccessful exit"));
            }
        }

        None
    }

    fn update_market_data(&mut self, _token: &TokenMetrics) {
        // No data to update in mock strategy
    }

    fn should_exit(&self, position: &Position) -> bool {
        // Use the new analyze_for_exit method instead
        self.should_generate_exit_signal(&position.token_id)
    }

    fn box_clone(&self) -> Box<dyn TradingStrategy> {
        Box::new(self.clone())
    }
}

impl fmt::Display for MockStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MockStrategy(signal_interval={}s, hold_duration={}s, success_rate={:.0}%)",
            self.signal_interval.as_secs(),
            self.hold_duration.as_secs(),
            self.success_rate * 100.0
        )
    }
}

/// Factory function to create a strategy by name with parameters
pub fn create_strategy(
    strategy_name: &str,
    threshold: f64,
    min_volume: f64,
    stop_loss_pct: f64,
    min_data_points: Option<usize>,
    risk_tolerance: Option<i32>,
    testing_mode: Option<String>,
) -> Result<Strategy, Error> {
    match strategy_name {
        "momentum" => {
            let mut strategy = MomentumStrategy::new(threshold, min_volume, stop_loss_pct);

            // Apply min_data_points if provided
            if let Some(points) = min_data_points {
                strategy = strategy.with_min_data_points(points);
            }

            // Apply risk_tolerance if provided
            if let Some(level) = risk_tolerance {
                strategy = strategy.with_risk_tolerance(level as f64);
            }

            // Apply testing mode if provided
            if let Some(mode) = testing_mode {
                let trading_mode = match mode.to_lowercase().as_str() {
                    "fast" => crate::trading::indicators::TradingMode::FastTest,
                    "ultra" => crate::trading::indicators::TradingMode::UltraFast,
                    "mock" => crate::trading::indicators::TradingMode::Mock,
                    _ => crate::trading::indicators::TradingMode::Production,
                };
                strategy = strategy.with_trading_mode(trading_mode);
            }

            Ok(Strategy::new(Box::new(strategy)))
        }
        "mock" => {
            let mut strategy = MockStrategy::new(threshold, min_volume, stop_loss_pct);

            // Configure mock strategy parameters
            if let Some(level) = risk_tolerance {
                // Use risk tolerance to influence hold duration and success rate
                // Higher risk tolerance = shorter hold times and higher variability
                let hold_duration = match level {
                    0 => 300, // Conservative: 5 minutes
                    1 => 240, // Conservative-Moderate: 4 minutes
                    2 => 180, // Moderate: 3 minutes
                    3 => 120, // Moderate-Aggressive: 2 minutes
                    4 => 60,  // Aggressive: 1 minute
                    _ => 30,  // Very Aggressive: 30 seconds
                };
                strategy = strategy.with_hold_duration(hold_duration);

                // Higher risk tolerance = lower success rate but more frequent trades
                let success_rate = match level {
                    0 => 0.7,  // Conservative: 70% success
                    1 => 0.65, // Conservative-Moderate: 65% success
                    2 => 0.6,  // Moderate: 60% success
                    3 => 0.55, // Moderate-Aggressive: 55% success
                    4 => 0.5,  // Aggressive: 50% success
                    _ => 0.45, // Very Aggressive: 45% success
                };
                strategy = strategy.with_success_rate(success_rate);
            }

            Ok(Strategy::new(Box::new(strategy)))
        }
        // Add other strategies here as they are implemented
        // "rsi" => Ok(Strategy::new(Box::new(RSIStrategy::new(
        //     threshold, min_volume, stop_loss_pct
        // )))),
        _ => Err(Error::Config(format!(
            "Unknown strategy: {}",
            strategy_name
        ))),
    }
}

/// Now MomentumStrategy implements the TradingStrategy trait
#[derive(Clone)]
pub struct MomentumStrategy {
    threshold: f64,
    min_volume: f64,
    stop_loss_pct: f64,
    // Use DashMap instead of RwLock<HashMap>
    price_data: Arc<DashMap<String, PriceTimeSeries>>,
    considered_tokens: Arc<DashMap<String, DateTime<Utc>>>,
    indicator_weights: IndicatorWeights,
    cooldown_period: ChronoDuration,
    min_data_points: usize,
    risk_tolerance: f64,
    trading_mode: crate::trading::indicators::TradingMode,
}

impl MomentumStrategy {
    /// Create a new momentum strategy with the specified parameters
    pub fn new(threshold: f64, min_volume: f64, stop_loss_pct: f64) -> Self {
        Self {
            threshold: 0.5,
            min_volume,
            stop_loss_pct,
            // Initialize DashMaps
            price_data: Arc::new(DashMap::new()),
            considered_tokens: Arc::new(DashMap::new()),
            indicator_weights: IndicatorWeights::default(),
            cooldown_period: ChronoDuration::hours(24),
            min_data_points: 7,
            risk_tolerance: 0.02,
            trading_mode: crate::trading::indicators::TradingMode::Production,
        }
    }

    pub fn with_trading_mode(mut self, mode: crate::trading::indicators::TradingMode) -> Self {
        self.trading_mode = mode;
        if mode != crate::trading::indicators::TradingMode::Production {
            warn!("⚠️ Strategy running in {:?} mode - signals will be generated with reduced accuracy!", mode);
        }
        self
    }

    // Helper method to get a strategy name, used by the register_strategy macro
    pub fn strategy_name() -> &'static str {
        "momentum"
    }

    // Set minimum data points required (defaults to 7, must be at least 3)
    pub fn with_min_data_points(mut self, points: usize) -> Self {
        self.min_data_points = if points < 3 { 3 } else { points };
        self
    }

    // Builder method for setting the minimum volume required
    pub fn with_minimum_volume(mut self, min_volume: f64) -> Self {
        self.min_volume = min_volume;
        self
    }

    // Set risk tolerance level (0-1)
    pub fn with_risk_tolerance(mut self, level: f64) -> Self {
        self.risk_tolerance = level;
        self
    }

    pub fn with_indicator_weights(mut self, weights: IndicatorWeights) -> Self {
        self.indicator_weights = weights;
        self
    }

    pub fn log_status(&self) -> Result<(), crate::error::Error> {
        info!("Momentum Strategy Status:");
        info!("  Threshold: {:.2}%", self.threshold);
        info!("  Min Volume: ${:.2}M", self.min_volume / 1_000_000.0);
        info!("  Stop Loss: {:.2}%", self.stop_loss_pct);
        info!("  Tracked Tokens (Price Data): {}", self.price_data.len());
        info!(
            "  Considered Tokens (Cooldown): {}",
            self.considered_tokens.len()
        );
        Ok(())
    }

    // Helper function to mark a token as considered
    fn mark_token_as_considered(&self, token_id: String) -> Result<(), crate::error::Error> {
        // Use DashMap insert
        self.considered_tokens.insert(token_id, Utc::now());
        // TODO: Implement periodic cleanup for considered_tokens DashMap, as retain is not available.
        // This could be done in a separate task or less frequently. For now, it will grow indefinitely.
        Ok(())
    }

    /// Helper method to get the price time series for a token
    fn get_price_time_series(&self, token: &TokenMetrics) -> Option<PriceTimeSeries> {
        // Use DashMap get, may need to clone the PriceTimeSeries if needed outside the Ref scope.
        // PriceTimeSeries needs to be Clone. Assuming it is.
        self.price_data
            .get(&token.id)
            .map(|ts_ref| ts_ref.value().clone())
    }

    pub fn update_market_data(&mut self, token: &TokenMetrics) {
        // Use DashMap's entry API - handles locking internally
        let mut time_series = self.price_data.entry(token.id.clone()).or_insert_with(|| {
            debug!(
                "Creating new PriceTimeSeries with mode: {:?}",
                self.trading_mode
            );
            PriceTimeSeries::new(self.trading_mode)
        });

        // Add the new data point - time_series is a RefMut here
        time_series
            .value_mut()
            .add_data_point(token.price_usd, token.volume_24h, Utc::now());
    }
}

impl TradingStrategy for MomentumStrategy {
    fn name(&self) -> &str {
        "momentum"
    }

    fn update_market_data(&mut self, token: &TokenMetrics) {
        self.update_market_data(token);
    }

    fn analyze_for_entry(&self, token: &TokenMetrics) -> bool {
        // Immediate check for invalid price
        if token.price_usd <= 0.0 {
            trace!(
                "📉 {} Price is invalid: ${:.4} - skipping momentum analysis",
                token.symbol,
                token.price_usd
            );
            return false;
        }

        // Check if volume meets minimum requirement
        if token.volume_24h < self.min_volume {
            trace!(
                "📉 {} Volume too low: ${:.2}M < ${:.2}M",
                token.symbol,
                token.volume_24h / 1_000_000.0,
                self.min_volume / 1_000_000.0
            );
            return false;
        }

        // Check if token is in cooldown using DashMap
        let is_cooldown =
            if let Some(last_considered_ref) = self.considered_tokens.get(&token.symbol) {
                let last_considered = *last_considered_ref.value();
                let now = Utc::now();
                let duration = now - last_considered;
                if duration < self.cooldown_period {
                    trace!(
                        "⏳ {} in cooldown period ({} hours remaining)",
                        token.symbol,
                        (self.cooldown_period - duration).num_hours()
                    );
                    true
                } else {
                    false
                }
            } else {
                false
            };

        if is_cooldown {
            trace!("⏳ Skipping {} analysis - in cooldown period", token.symbol);
            return false;
        }

        // Get prices for this token
        let time_series = match self.get_price_time_series(token) {
            Some(ts) => ts,
            None => {
                debug!(
                    "⚠️ {} No price history available for analysis",
                    token.symbol
                );
                return false;
            }
        };

        // Log how many data points we have
        debug!(
            "📊 {} Analyzing with {} price data points",
            token.symbol,
            time_series.prices().len()
        );

        // Log basic price stats
        let prices = time_series.prices();
        let volumes = time_series.volumes();

        if prices.len() < self.min_data_points {
            debug!(
                "⚠️ {} Insufficient data points ({} < {})",
                token.symbol,
                prices.len(),
                self.min_data_points
            );
            return false;
        }

        // Enhanced logging - Adding RSI, MACD, and Bollinger Bands calculation
        debug!("➡️ {}: Calculating RSI...", token.symbol);
        // RSI Analysis
        if let Some(rsi) = calculate_rsi(&prices, 14) {
            let rsi_analysis = if rsi < 30.0 {
                "Oversold 📉"
            } else if rsi > 70.0 {
                "Overbought 📈"
            } else {
                "Neutral ↔️"
            };

            debug!("📈 {} RSI: {:.2} - {}", token.symbol, rsi, rsi_analysis);
        }
        debug!("⬅️ {}: RSI calculation finished.", token.symbol);

        debug!("➡️ {}: Calculating MACD...", token.symbol);
        // MACD Analysis
        if let Some((macd_line, signal_line, histogram)) =
            calculate_macd(&prices, self.trading_mode)
        {
            let macd_cross = if macd_line > signal_line && histogram > 0.0 {
                "Bullish cross ⬆️"
            } else if macd_line < signal_line && histogram < 0.0 {
                "Bearish cross ⬇️"
            } else {
                "No cross ↔️"
            };

            debug!(
                "📈 {} MACD: Line={:.6}, Signal={:.6}, Hist={:.6} - {}",
                token.symbol, macd_line, signal_line, histogram, macd_cross
            );
        }
        debug!("⬅️ {}: MACD calculation finished.", token.symbol);

        debug!("➡️ {}: Calculating Bollinger Bands...", token.symbol);
        // Bollinger Bands Analysis
        if let Some((upper, middle, lower)) = calculate_bollinger_bands(&prices, 20, 2.0) {
            let latest_price = *prices.last().unwrap();
            let band_width = (upper - lower) / middle; // Potential division by zero if middle is 0
            let band_position = if (upper - lower).abs() < f64::EPSILON {
                0.5
            } else {
                (latest_price - lower) / (upper - lower)
            };

            debug!(
                "📈 {} Price ${:.4} relative to bands: {:.1}% ({:.1}% = lower, {:.1}% = upper)",
                token.symbol,
                latest_price,
                band_position * 100.0,
                0.0,
                100.0
            );
            debug!(
                "📈 {} Band position: {:.1}% from bottom of range",
                token.symbol,
                band_position * 100.0
            );
            debug!(
                "📈 {} Band width: {:.2}% of price (higher = more volatile)",
                token.symbol,
                band_width * 100.0
            );
        }
        debug!("⬅️ {}: Bollinger Bands calculation finished.", token.symbol);

        debug!("➡️ {}: Analyzing volume trend...", token.symbol);
        // Volume Trend
        if let Some(volume_trend) = analyze_volume_trend(&prices, &volumes, 14) {
            let trend_msg = if volume_trend > 0.5 {
                "Strong volume confirming uptrend 🔝"
            } else if volume_trend > 0.0 {
                "Volume supporting uptrend ↗️"
            } else if volume_trend > -0.5 {
                "Volume supporting downtrend ↘️"
            } else {
                "Strong volume confirming downtrend 🔽"
            };

            debug!(
                "📈 {} Volume Trend: {:.2} - {}",
                token.symbol, volume_trend, trend_msg
            );
        }
        debug!("⬅️ {}: Volume trend analysis finished.", token.symbol);

        debug!("➡️ {}: Calculating composite momentum...", token.symbol);
        // Calculate composite momentum score
        if let Some(momentum_score) =
            calculate_composite_momentum(&time_series, &self.indicator_weights)
        {
            debug!(
                "⬅️ {}: Composite momentum calculation finished. Score: {:.2}",
                token.symbol, momentum_score
            );
            info!(
                "🔍 {} Momentum Score: {:.2} (threshold: {:.2})",
                token.symbol, momentum_score, self.threshold
            );

            // Use a very lenient threshold for testing
            if momentum_score >= self.threshold {
                // Almost any token with decent data will generate a signal
                info!(
                    "🚀 {} BUY SIGNAL! Momentum score {:.2} exceeds threshold {:.2}",
                    token.symbol, momentum_score, self.threshold
                );

                debug!("➡️ {}: Marking token as considered...", token.symbol);
                // Mark this token as considered to prevent repeated signals
                // We dropped all locks before calling this, following lock hierarchy
                if let Err(e) = self.mark_token_as_considered(token.id.clone()) {
                    error!("Failed to mark token as considered: {}", e);
                    // Continue with the signal despite the error
                }
                debug!("⬅️ {}: Marking token as considered finished.", token.symbol);

                // Log weights used for decision
                debug!(
                    "🧮 {} Decision weights: RSI={:.1}%, MACD={:.1}%, BBands={:.1}%, Volume={:.1}%",
                    token.symbol,
                    self.indicator_weights.rsi * 100.0,
                    self.indicator_weights.macd * 100.0,
                    self.indicator_weights.bollinger_bands * 100.0,
                    self.indicator_weights.volume * 100.0
                );

                return true;
            } else {
                debug!(
                    "⏳ {} No signal. Score {:.2} below threshold {:.2}",
                    token.symbol, momentum_score, self.threshold
                );
            }
        } else {
            debug!(
                "⬅️ {}: Composite momentum calculation finished. Could not calculate score.",
                token.symbol
            );
            info!(
                "⚠️ {} Could not calculate momentum score - insufficient data",
                token.symbol
            );
        }

        false
    }

    fn analyze_for_exit(
        &self,
        token: &TokenMetrics,
        position: Option<&Position>,
        risk_params: Option<(f64, f64, usize)>,
    ) -> Option<ExitReason> {
        // If we don't have a position for this token, we can't exit
        let position = match position {
            Some(pos) => pos,
            None => return None,
        };

        // Get price time series using the helper that now uses DashMap
        // We currently don't use the time_series directly in the exit logic below,
        // but we might want to log if it's missing.
        if self.get_price_time_series(token).is_none() {
            warn!("No price history found for {} during exit analysis. Strategy exit checks might be limited.", token.id);
            // Proceed with checks that don't require history
        };

        // Use the fresh price from token metrics
        let current_price = token.price_usd;

        // Calculate current profit/loss percentage
        let price_change_pct = (current_price / position.entry_price - 1.0) * 100.0;

        // First check risk-based exits if parameters provided
        if let Some((take_profit, stop_loss, risk_tolerance)) = risk_params {
            // Calculate risk multiplier based on risk tolerance
            let risk_multiplier = match risk_tolerance {
                1..=3 => 0.8,  // Conservative
                4..=6 => 1.0,  // Moderate
                7..=10 => 1.2, // Aggressive
                _ => 1.0,      // Default to moderate
            };

            // Check take profit
            let take_profit_threshold = take_profit * risk_multiplier;
            if price_change_pct >= take_profit_threshold {
                let reason = format!(
                    "Take profit: {:.2}% >= {:.2}%",
                    price_change_pct, take_profit_threshold
                );
                info!("💰 {} - {}", token.id, reason);
                return Some(ExitReason::risk_based(&reason));
            }

            // Check stop loss
            let stop_loss_threshold = -1.0 * stop_loss;
            if price_change_pct <= stop_loss_threshold {
                let reason = format!(
                    "Stop loss: {:.2}% <= {:.2}%",
                    price_change_pct, stop_loss_threshold
                );
                info!("🛑 {} - {}", token.id, reason);
                return Some(ExitReason::risk_based(&reason));
            }
        }

        // Check strategy-specific stop loss
        if price_change_pct <= -self.stop_loss_pct {
            let reason = format!(
                "Strategy stop loss triggered: {:.2}% <= {:.2}%",
                price_change_pct, -self.stop_loss_pct
            );
            info!("🛑 {}", reason);
            return Some(ExitReason::strategy_based(&reason));
        }

        // Check if price has dropped significantly from highest point (trailing stop)
        if position.highest_price > position.entry_price {
            // Calculate the percentage drop from highest price (as a positive percentage)
            let drop_from_high_pct =
                ((position.highest_price - current_price) / position.highest_price) * 100.0;
            // Use a tighter trailing stop for profits (50% of regular stop loss)
            let trailing_stop_pct = self.stop_loss_pct * 0.5;

            if drop_from_high_pct >= trailing_stop_pct {
                let reason = format!("Trailing stop triggered: {:.2}% drop from high ${:.4}, still in profit: {:.2}%", 
                                   drop_from_high_pct, position.highest_price, price_change_pct);
                info!("🔽 {}", reason);
                return Some(ExitReason::strategy_based(&reason));
            }
        }

        // Check if we've been holding for too long with minimal movement
        let now = Utc::now();
        let hold_duration = now - position.entry_time;
        let max_hold_time = ChronoDuration::days(7); // Max 7 days for momentum strategy

        if hold_duration > max_hold_time && price_change_pct.abs() < self.threshold / 2.0 {
            let reason = format!(
                "Max hold time reached with minimal movement ({:.2}%)",
                price_change_pct
            );
            info!("⏱️ {}", reason);
            return Some(ExitReason::strategy_based(&reason));
        }

        // By default, maintain the position
        debug!(
            "Maintaining position for {} at ${:.4}, P&L: {:.2}%",
            token.id, current_price, price_change_pct
        );
        None
    }

    fn box_clone(&self) -> Box<dyn TradingStrategy> {
        Box::new(self.clone())
    }
}

// Add Display implementation for MomentumStrategy
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

/// Helper macro to easily register a new strategy implementation
#[macro_export]
macro_rules! register_strategy {
    ($strategy_name:expr, $strategy_type:ty, $threshold:expr, $min_volume:expr, $stop_loss:expr) => {
        match $strategy_name {
            name if name == <$strategy_type>::strategy_name() => {
                let strategy = <$strategy_type>::new($threshold, $min_volume, $stop_loss);
                Ok(Strategy::new(Box::new(strategy)))
            }
            _ => continue, // Move to next match arm
        }
    };
}

/*
// Example of how to add a new strategy:

// RSI Strategy Implementation
#[derive(Clone)]
pub struct RSIStrategy {
    threshold: f64,
    min_volume: f64,
    stop_loss_pct: f64,
    price_data: Arc<Mutex<HashMap<String, PriceTimeSeries>>>,
    overbought_level: f64,
    oversold_level: f64,
}

impl RSIStrategy {
    pub fn new(threshold: f64, min_volume: f64, stop_loss_pct: f64) -> Self {
        Self {
            threshold,
            min_volume,
            stop_loss_pct,
            price_data: Arc::new(Mutex::new(HashMap::new())),
            overbought_level: 70.0,
            oversold_level: 30.0,
        }
    }

    pub fn strategy_name() -> &'static str {
        "rsi"
    }
}

impl TradingStrategy for RSIStrategy {
    fn name(&self) -> &str {
        "rsi_reversal"
    }

    fn update_market_data(&mut self, token: &TokenMetrics) {
        // Similar to MomentumStrategy implementation
    }

    fn analyze(&self, token: &TokenMetrics) -> Signal {
        // RSI strategy implementation
        Signal::None
    }

    fn should_exit(&self, position: &Position) -> bool {
        // Exit criteria for RSI strategy
        false
    }

    fn box_clone(&self) -> Box<dyn TradingStrategy> {
        Box::new(self.clone())
    }
}

impl fmt::Display for RSIStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "RSIStrategy(oversold={:.0}, overbought={:.0}, stop_loss={:.2}%)",
            self.oversold_level,
            self.overbought_level,
            self.stop_loss_pct
        )
    }
}

// Then in create_strategy:
pub fn create_strategy(
    strategy_name: &str,
    threshold: f64,
    min_volume: f64,
    stop_loss_pct: f64,
) -> Result<Strategy, Error> {
    match strategy_name {
        "momentum" => Ok(Strategy::new(Box::new(MomentumStrategy::new(
            threshold,
            min_volume,
            stop_loss_pct,
        )))),
        "rsi" => Ok(Strategy::new(Box::new(RSIStrategy::new(
            threshold,
            min_volume,
            stop_loss_pct
        )))),
        _ => Err(Error::Config(format!("Unknown strategy: {}", strategy_name))),
    }
}
*/
