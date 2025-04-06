use crate::types::market::TokenMetrics;
use crate::trading::indicators::{
    PriceTimeSeries, IndicatorWeights, calculate_composite_momentum,
    calculate_rsi, calculate_macd, calculate_bollinger_bands, analyze_volume_trend
};
use std::fmt::Debug;
use chrono::{DateTime, Utc, Duration as ChronoDuration};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::fmt;
use log::{info, debug, trace, error, warn};
use crate::error::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum Signal {
    Buy,
    Sell,
    None,
}

#[derive(Debug, Clone)]
pub struct Position {
    pub token_id: String,
    pub coingecko_id: String,
    pub entry_price: f64,
    pub current_price: f64,
    pub highest_price: f64,
    pub size: f64,
    pub unrealized_pnl: f64,
    pub entry_time: DateTime<Utc>,
}

/// Trading strategy trait that all strategy implementations must implement
pub trait TradingStrategy: fmt::Display + Send + Sync + 'static {
    /// Get the name of the strategy
    fn name(&self) -> &str;
    
    /// Analyze a token and determine if there is a trading signal
    fn analyze(&self, token: &TokenMetrics) -> Signal;
    
    /// Check if a position should be exited
    fn should_exit(&self, position: &Position) -> bool;
    
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
    
    pub fn analyze(&self, token: &TokenMetrics) -> Signal {
        self.inner.analyze(token)
    }
    
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

/// Factory function to create a strategy by name with parameters
pub fn create_strategy(
    strategy_name: &str,
    threshold: f64,
    min_volume: f64,
    stop_loss_pct: f64,
    min_data_points: Option<usize>,
    risk_tolerance: Option<u8>,
) -> Result<Strategy, Error> {
    match strategy_name {
        "momentum" => {
            let mut strategy = MomentumStrategy::new(
                threshold,
                min_volume,
                stop_loss_pct,
            );
            
            // Apply min_data_points if provided
            if let Some(points) = min_data_points {
                strategy = strategy.with_min_data_points(points);
            }
            
            // Apply risk_tolerance if provided
            if let Some(level) = risk_tolerance {
                strategy = strategy.with_risk_tolerance(level);
            }
            
            Ok(Strategy::new(Box::new(strategy)))
        },
        // Add other strategies here as they are implemented
        // "rsi" => Ok(Strategy::new(Box::new(RSIStrategy::new(
        //     threshold, min_volume, stop_loss_pct
        // )))),
        _ => Err(Error::Config(format!("Unknown strategy: {}", strategy_name))),
    }
}

/// Now MomentumStrategy implements the TradingStrategy trait
#[derive(Clone)]
pub struct MomentumStrategy {
    threshold: f64,
    min_volume: f64,
    stop_loss_pct: f64,
    price_data: Arc<RwLock<HashMap<String, PriceTimeSeries>>>,
    indicator_weights: IndicatorWeights,
    recently_considered: Arc<RwLock<HashMap<String, DateTime<Utc>>>>,
    cooldown_period: ChronoDuration,
    min_data_points: usize,          // Minimum data points required for analysis
    risk_tolerance: Option<u8>,       // Risk tolerance level (0-5)
}

impl MomentumStrategy {
    pub fn new(threshold: f64, min_volume: f64, stop_loss_pct: f64) -> Self {          
        Self {
            threshold,
            min_volume,
            stop_loss_pct,
            price_data: Arc::new(RwLock::new(HashMap::new())),
            indicator_weights: IndicatorWeights::default(),
            recently_considered: Arc::new(RwLock::new(HashMap::new())),
            cooldown_period: ChronoDuration::hours(24), // Don't consider same token for 24 hours
            min_data_points: 7,       // Require 7 days of data by default (reduced from 14)
            risk_tolerance: None,     // Default to conservative (standard analysis)
        }
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

    // Set risk tolerance level (0-5)
    pub fn with_risk_tolerance(mut self, level: u8) -> Self {
        self.risk_tolerance = Some(if level > 5 { 5 } else { level });
        self
    }

    fn is_in_cooldown(&self, token_id: &str) -> Result<bool, crate::error::Error> {
        let recently_considered = self.recently_considered.read()
            .map_err(|e| crate::error::Error::Concurrency(format!("Failed to lock recently_considered: {}", e)))?;
            
        let result = if let Some(last_considered) = recently_considered.get(token_id) {
            let now = Utc::now();
            now - *last_considered < self.cooldown_period
        } else {
            false
        };
        
        Ok(result)
    }

    fn mark_considered(&self, token_id: String) -> Result<(), crate::error::Error> {
        let mut recently_considered = self.recently_considered.write()
            .map_err(|e| crate::error::Error::Concurrency(format!("Failed to lock recently_considered for update: {}", e)))?;
            
        recently_considered.insert(token_id, Utc::now());
        
        // Cleanup old entries
        recently_considered.retain(|_, time| {
            Utc::now() - *time < self.cooldown_period
        });
        
        Ok(())
    }

    pub fn log_status(&self) -> Result<(), crate::error::Error> {
        let price_data = self.price_data.read()
            .map_err(|e| crate::error::Error::Concurrency(format!("Failed to lock price data: {}", e)))?;
        
        info!("Momentum Strategy Status:");
        info!("  Threshold: {:.2}%", self.threshold);
        info!("  Min Volume: ${:.2}M", self.min_volume / 1_000_000.0);
        info!("  Stop Loss: {:.2}%", self.stop_loss_pct);
        info!("  Tracked Tokens: {}", price_data.len());
        
        Ok(())
    }
    
    // Helper function to mark a token as considered
    // Separate function to avoid holding locks across function calls
    fn mark_token_as_considered(&self, token_id: String) -> Result<(), crate::error::Error> {
        let mut recently_considered = self.recently_considered.write()
            .map_err(|e| crate::error::Error::Concurrency(format!("Failed to lock recently_considered for update: {}", e)))?;
            
        recently_considered.insert(token_id, Utc::now());
        
        // Cleanup old entries
        recently_considered.retain(|_, time| {
            Utc::now() - *time < self.cooldown_period
        });
        
        Ok(())
    }

    // Update the analyze_relaxed method to handle risk levels 0-5
    fn analyze_relaxed(&self, token: &TokenMetrics, time_series: &PriceTimeSeries) -> Signal {
        let data_points = time_series.prices().len();
        // Get risk_tolerance or default to 0
        let risk_level = self.risk_tolerance.unwrap_or_default();
        
        info!("📊 Analyzing {} (${:.4}) with {} data points in RELAXED mode", 
              token.symbol, token.price_usd, data_points);
        
        // Configure threshold adjustments based on risk tolerance
        let min_percent_change = match risk_level {
            0 => self.threshold * 0.5, // Conservative
            1 => self.threshold * 0.4, // Moderate
            2 => self.threshold * 0.35, // Balanced
            3 => self.threshold * 0.33, // Growth
            4 => self.threshold * 0.30, // Aggressive
            _ => self.threshold * 0.25, // Very Aggressive
        };
        
        let require_volume_increase = risk_level < 3;
        let require_price_uptrend = risk_level < 4;
        let min_data_needed = match risk_level {
            5 => 1, // Very Aggressive can work with just 1 data point
            4 => 2, // Aggressive
            _ => 3, // Others require more data
        };
        
        info!("🔧 Risk level {} settings: min_change={:.2}%, volume_req={}, uptrend_req={}, min_data={}",
              risk_level, min_percent_change, require_volume_increase, require_price_uptrend, min_data_needed);
        
        // Check if we have enough data points
        if data_points < min_data_needed {
            info!("❌ {} has only {} data points (need {} for risk level {})", 
                   token.symbol, data_points, min_data_needed, risk_level);
            return Signal::None;
        }
        
        // If we have only 1 data point and maximum risk tolerance, generate a signal based on volume
        if data_points == 1 && risk_level == 5 {
            // Get the current price and volume
            let current_price = *time_series.prices().last().unwrap();
            let current_volume = *time_series.volumes().last().unwrap();
            
            // For max risk with single data point, rely on high volume as a signal
            if current_volume > 100_000_000.0 { // Over $100M volume
                info!("⚡ HIGH RISK SIGNAL: {} has high volume (${:.4}M) with single data point", 
                      token.symbol, current_volume / 1_000_000.0);
                
                return Signal::Buy;
            } else {
                info!("❌ {} has insufficient volume (${:.4}M) for single data point analysis", 
                      token.symbol, current_volume / 1_000_000.0);
                return Signal::None;
            }
        }
        
        // For multiple data points, analyze trends
        let current_price = *time_series.prices().last().unwrap();
        let previous_price = time_series.prices().get(time_series.prices().len() - 2).map(|p| *p);
        
        // Get current and previous volume if available
        let current_volume = *time_series.volumes().last().unwrap();
        let previous_volume = time_series.volumes().get(time_series.volumes().len() - 2).map(|v| *v);
        
        // Calculate short-term change
        if let Some(prev_price) = previous_price {
            let percent_change = ((current_price - prev_price) / prev_price) * 100.0;
            let volume_trend = previous_volume.map_or(false, |prev_vol| current_volume > prev_vol);
            
            // For very aggressive risk (level 5), be more lenient
            let short_term_change_significant = 
                if risk_level == 5 && percent_change > 0.5 {
                    info!("⚡ {} has shown {:.2}% price increase (aggressive threshold: 0.5%)", 
                         token.symbol, percent_change);
                    true
                } else if percent_change > min_percent_change {
                    info!("✅ {} has shown {:.2}% price increase (above min threshold: {:.2}%)", 
                         token.symbol, percent_change, min_percent_change);
                    true
                } else {
                    info!("❌ {} price change of {:.2}% below threshold {:.2}%", 
                         token.symbol, percent_change, min_percent_change);
                    false
                };
            
            let volume_increasing = if volume_trend {
                info!("📈 {} volume is increasing", token.symbol);
                true
            } else {
                info!("📉 {} volume is decreasing", token.symbol);
                false
            };
            
            // Generate buy signal if conditions are met
            let signal_conditions = match risk_level {
                5 => short_term_change_significant, // Very aggressive: Only require price change
                4 => short_term_change_significant, // Aggressive: Only require price change
                3 => short_term_change_significant && (volume_increasing || !require_volume_increase), // Growth: Price + optional volume
                _ => short_term_change_significant && (volume_increasing || !require_volume_increase) // Others: More strict
            };
            
            if signal_conditions {
                info!("🔔 BUY signal generated for {} at ${:.4}", 
                      token.symbol, current_price);
                
                // Mark token as considered
                if let Err(e) = self.mark_token_as_considered(token.id.clone()) {
                    error!("Failed to mark token as considered: {}", e);
                }
                
                // Log weights used for decision
                debug!("🧮 {} Decision weights: RSI={:.1}%, MACD={:.1}%, BBands={:.1}%, Volume={:.1}%", 
                    token.symbol,
                    self.indicator_weights.rsi * 100.0,
                    self.indicator_weights.macd * 100.0,
                    self.indicator_weights.bollinger_bands * 100.0,
                    self.indicator_weights.volume * 100.0
                );
                
                return Signal::Buy;
            } else {
                info!("❌ No signal for {}. Missing conditions: price change={}, volume trend={}", 
                     token.symbol, short_term_change_significant, volume_increasing);
            }
        } else {
            // This should never happen given our checks above, but added for safety
            error!("Unexpected error: Could not get previous price point for {}", token.symbol);
        }
        
        Signal::None
    }
}

impl TradingStrategy for MomentumStrategy {
    fn name(&self) -> &str {
        "enhanced_momentum"
    }
    
    fn update_market_data(&mut self, token: &TokenMetrics) {
        let mut price_data = match self.price_data.write() {
            Ok(guard) => guard,
            Err(e) => {
                error!("Failed to lock price data: {}", e);
                return;
            }
        };
        
        // Get or create price time series for this token
        let time_series = price_data
            .entry(token.id.clone())
            .or_insert_with(|| PriceTimeSeries::new(30));  // Keep 30 data points
            
        // Add the latest price and volume
        time_series.add_data_point(token.price_usd, token.volume_24h);
    }
    
    fn analyze(&self, token: &TokenMetrics) -> Signal {
        trace!("Analyzing {} ({})", token.name, token.symbol);
        
        // Check minimum volume requirement first (no locks needed)
        if token.volume_24h < self.min_volume {
            trace!("❌ {} volume ${:.2}M below minimum ${:.2}M, skipping", 
                token.symbol, 
                token.volume_24h / 1_000_000.0,
                self.min_volume / 1_000_000.0
            );
            return Signal::None;
        }
        
        // Check if token is in cooldown
        // This requires the recently_considered lock
        let is_cooldown = match self.recently_considered.read() {
            Ok(recently_considered) => {
                if let Some(last_considered) = recently_considered.get(token.symbol.as_str()) {
                    let now = Utc::now();
                    now - *last_considered < self.cooldown_period
                } else {
                    false
                }
            },
            Err(e) => {
                error!("Failed to lock recently_considered for {}: {}", token.symbol, e);
                return Signal::None;
            }
        };
        
        if is_cooldown {
            debug!("Token {} is in cooldown period, skipping analysis", token.symbol);
            return Signal::None;
        }
        
        // Now acquire price_data lock for analysis
        let price_data = match self.price_data.read() {
            Ok(guard) => guard,
            Err(e) => {
                error!("Failed to lock price data for analysis: {}", e);
                return Signal::None;
            }
        };
        
        let time_series = match price_data.get(&token.id) {
            Some(ts) => {
                // Check if we should use relaxed analysis based on risk tolerance
                // Higher risk tolerance allows for less data
                if self.risk_tolerance.map_or(false, |level| level >= 3) || !ts.has_enough_data(self.min_data_points) {
                    let data_points = ts.prices().len();
                    
                    // Use relaxed analysis for any of these conditions:
                    // 1. Not enough data but at least 2 points (minimum for relaxed)
                    // 2. Risk tolerance is high (3-5)
                    if (data_points < self.min_data_points && data_points >= 2) || self.risk_tolerance.map_or(false, |level| level >= 3) {
                        info!("📊 Analyzing {} (${:.4}) with {} data points using RELAXED mode (risk level: {})", 
                            token.symbol, token.price_usd, data_points, self.risk_tolerance.map_or(0, |level| level));
                        return self.analyze_relaxed(token, ts);
                    } else if data_points < 2 {
                        debug!("📊 {} has insufficient price history ({} points, need at least 2)", 
                            token.symbol, data_points);
                        return Signal::None;
                    }
                }
                
                // If we reach here, use standard analysis
                if !ts.has_enough_data(self.min_data_points) {
                    debug!("📊 {} has insufficient price history ({} points, need {})", 
                        token.symbol, ts.prices().len(), self.min_data_points);
                    return Signal::None;
                }
                
                debug!("📊 Analyzing {} (${:.4}) with {} data points using STANDARD mode", 
                    token.symbol, token.price_usd, ts.prices().len());
                ts
            },
            None => {
                debug!("No price history found for {}", token.symbol);
                return Signal::None;
            }
        };
        
        // Log individual technical indicators for transparency
        let prices = time_series.prices();
        let volumes = time_series.volumes();
        
        // RSI
        if let Some(rsi) = calculate_rsi(&prices, 14) {
            debug!("📈 {} RSI: {:.2} (>70 overbought, <30 oversold)", 
                token.symbol, rsi);
        }
        
        // MACD
        if let Some((macd_line, signal_line, histogram)) = calculate_macd(&prices, 12, 26, 9) {
            debug!("📈 {} MACD: Line={:.6}, Signal={:.6}, Hist={:.6}", 
                token.symbol, macd_line, signal_line, histogram);
            
            let macd_signal = if macd_line > 0.0 && macd_line > signal_line {
                "Strong Bullish 🔥"
            } else if macd_line > 0.0 {
                "Bullish 📈"
            } else if macd_line < signal_line {
                "Strong Bearish 📉"
            } else {
                "Bearish 🔻"
            };
            
            debug!("📈 {} MACD Signal: {}", token.symbol, macd_signal);
        }
        
        // Bollinger Bands
        if let Some((upper, middle, lower)) = calculate_bollinger_bands(&prices, 20, 2.0) {
            let latest_price = *prices.last().unwrap();
            let band_position = (latest_price - lower) / (upper - lower);
            let band_width = (upper - lower) / middle;
            
            debug!("📈 {} Bollinger Bands: Current=${:.4}, Upper=${:.4}, Lower=${:.4}", 
                token.symbol, latest_price, upper, lower);
            debug!("📈 {} Position in band: {:.2}% (>80% near upper, <20% near lower)", 
                token.symbol, band_position * 100.0);
            debug!("📈 {} Band width: {:.2}% of price (higher = more volatile)", 
                token.symbol, band_width * 100.0);
        }
        
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
            
            debug!("📈 {} Volume Trend: {:.2} - {}", token.symbol, volume_trend, trend_msg);
        }
        
        // Calculate composite momentum score
        if let Some(momentum_score) = calculate_composite_momentum(time_series, &self.indicator_weights) {
            info!("🔍 {} Momentum Score: {:.2} (threshold: {:.2})", 
                token.symbol, momentum_score, self.threshold);
            
            if momentum_score >= self.threshold {
                info!("🚀 {} BUY SIGNAL! Momentum score {:.2} exceeds threshold {:.2}", 
                    token.symbol, momentum_score, self.threshold);
                
                // Mark this token as considered to prevent repeated signals
                // We dropped all locks before calling this, following lock hierarchy
                if let Err(e) = self.mark_token_as_considered(token.id.clone()) {
                    error!("Failed to mark token as considered: {}", e);
                    // Continue with the signal despite the error
                }
                
                // Log weights used for decision
                debug!("🧮 {} Decision weights: RSI={:.1}%, MACD={:.1}%, BBands={:.1}%, Volume={:.1}%", 
                    token.symbol,
                    self.indicator_weights.rsi * 100.0,
                    self.indicator_weights.macd * 100.0,
                    self.indicator_weights.bollinger_bands * 100.0,
                    self.indicator_weights.volume * 100.0
                );
                
                return Signal::Buy;
            } else {
                debug!("⏳ {} No signal. Score {:.2} below threshold {:.2}", 
                    token.symbol, momentum_score, self.threshold);
            }
        }
        
        Signal::None
    }
    
    fn should_exit(&self, position: &Position) -> bool {
        let price_data = match self.price_data.read() {
            Ok(guard) => guard,
            Err(e) => {
                error!("Failed to lock price data for exit analysis: {}", e);
                return false; // Conservative approach: don't exit on error
            }
        };
        
        // If we don't have any price data for this token, continue holding
        if !price_data.contains_key(&position.coingecko_id) {
            return false;
        }
        
        // Calculate current profit/loss percentage
        let price_change_pct = (position.current_price / position.entry_price - 1.0) * 100.0;
        
        // Check stop loss
        if price_change_pct <= -self.stop_loss_pct {
            info!("🛑 Stop loss triggered for {} position. Current: ${:.4}, Entry: ${:.4}, Loss: {:.2}%", 
                position.token_id,
                position.current_price,
                position.entry_price,
                price_change_pct
            );
            return true;
        }
        
        // Check if price has dropped significantly from highest point (trailing stop)
        if position.highest_price > position.entry_price {
            let drop_from_high_pct = (position.current_price / position.highest_price - 1.0) * 100.0;
            // Use a tighter trailing stop for profits (50% of regular stop loss)
            let trailing_stop_pct = self.stop_loss_pct * 0.5;
            
            if drop_from_high_pct <= -trailing_stop_pct {
                info!("🔽 Trailing stop triggered for {} position. Current: ${:.4}, Highest: ${:.4}, Drop: {:.2}%, Still in profit: {:.2}%", 
                    position.token_id,
                    position.current_price,
                    position.highest_price,
                    drop_from_high_pct,
                    price_change_pct
                );
                return true;
            }
        }
        
        // Check if we've been holding for too long with minimal movement
        let now = Utc::now();
        let hold_duration = now - position.entry_time;
        let max_hold_time = ChronoDuration::days(7); // Max 7 days for momentum strategy
        
        if hold_duration > max_hold_time && price_change_pct.abs() < self.threshold / 2.0 {
            info!("⏱️ Max hold time reached for {} position with minimal movement ({:.2}%). Exiting.", 
                position.token_id, price_change_pct);
            return true;
        }
        
        false // Continue holding by default
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
