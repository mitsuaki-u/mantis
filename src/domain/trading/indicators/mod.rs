//! Technical indicators for trading strategies
//!
//! This module provides implementations of common technical indicators
//! used in trading algorithms, designed for efficiency and accuracy.

use std::collections::VecDeque;
use log::{info, debug, trace, error, warn};
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

/// Trading analysis mode to control indicator periods
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TradingMode {
    /// Production mode with full periods for accurate signals
    Production,
    /// Fast testing mode with reduced periods
    FastTest,
    /// Ultra-fast mode with minimum periods for rapid testing
    UltraFast,
    /// Mock mode for generating artificial buy/sell signals
    Mock,
}

/// Represents a time series of price data with a fixed length
#[derive(Debug, Clone)]
pub struct PriceTimeSeries {
    mode: TradingMode,
    prices: Vec<f64>,
    volumes: Vec<f64>,
    timestamps: Vec<DateTime<Utc>>,
}

impl PriceTimeSeries {
    /// Create a new price time series with the specified period
    pub fn new(mode: TradingMode) -> Self {
        if mode != TradingMode::Production {
            warn!("⚠️ Running in {:?} mode - signals will be generated with reduced accuracy!", mode);
        }
        
        let capacity = get_max_required_points(mode);
        debug!("Creating PriceTimeSeries with mode {:?}, capacity {}", mode, capacity);
        
        Self {
            mode,
            prices: Vec::with_capacity(capacity),
            volumes: Vec::with_capacity(capacity),
            timestamps: Vec::with_capacity(capacity),
        }
    }

    /// Add a new price data point, maintaining the fixed length
    pub fn add_data_point(&mut self, price: f64, volume: f64, timestamp: DateTime<Utc>) {
        debug!("Adding data point in {:?} mode", self.mode);
        self.prices.push(price);
        self.volumes.push(volume);
        self.timestamps.push(timestamp);

        let max_points = get_max_required_points(self.mode);
        while self.prices.len() > max_points {
            self.prices.remove(0);
            self.volumes.remove(0);
            self.timestamps.remove(0);
        }
    }

    /// Get a slice of the most recent prices
    pub fn prices(&self) -> Vec<f64> {
        self.prices.iter().copied().collect()
    }

    /// Get a slice of the most recent volumes
    pub fn volumes(&self) -> Vec<f64> {
        self.volumes.iter().copied().collect()
    }

    /// Check if we have enough data points for calculations
    pub fn has_enough_data(&self, min_points: usize) -> bool {
        self.prices.len() >= min_points
    }

    pub fn has_minimum_data_for_analysis(&self) -> bool {
        self.prices.len() >= get_max_required_points(self.mode)
    }
}

/// Constants for indicator periods
const RSI_PERIOD: usize = 14;
const MACD_FAST_PERIOD: usize = 12;
const MACD_SLOW_PERIOD: usize = 26;
const MACD_SIGNAL_PERIOD: usize = 9;
const BOLLINGER_PERIOD: usize = 20;
const VOLUME_TREND_PERIOD: usize = 14;

// Fast test mode periods
const TEST_RSI_PERIOD: usize = 7;
const TEST_MACD_FAST: usize = 6;
const TEST_MACD_SLOW: usize = 13;
const TEST_MACD_SIGNAL: usize = 4;
const TEST_BOLLINGER: usize = 10;
const TEST_VOLUME: usize = 7;

// Ultra-fast test mode periods
const ULTRA_RSI_PERIOD: usize = 4;
const ULTRA_MACD_FAST: usize = 3;
const ULTRA_MACD_SLOW: usize = 7;
const ULTRA_MACD_SIGNAL: usize = 2;
const ULTRA_BOLLINGER: usize = 5;
const ULTRA_VOLUME: usize = 4;

/// Returns a tuple of periods for various indicators based on the trading mode
/// (rsi_period, macd_fast_period, macd_slow_period, macd_signal_period, bollinger_period, vol_trend_period)
pub fn get_indicator_periods(mode: TradingMode) -> (usize, usize, usize, usize, usize, usize) {
    match mode {
        TradingMode::Production => {
            // Production: Default periods for most accurate signals
            (14, 12, 26, 9, 20, 20)
        }
        TradingMode::FastTest => {
            // Fast Test: Half periods for quicker signal generation
            (7, 6, 13, 4, 10, 10)
        }
        TradingMode::UltraFast => {
            // Ultra Fast: Minimum viable periods for extremely quick testing
            (3, 3, 7, 2, 5, 5)
        }
        TradingMode::Mock => {
            // Mock: Zero periods since we'll be generating artificial signals
            (0, 0, 0, 0, 0, 0)
        }
    }
}

/// Returns the maximum number of data points required for indicator calculations
/// based on the trading mode.
/// 
/// This is calculated by taking the maximum period from all indicators and
/// adding a buffer to ensure enough data for accurate calculations.
pub fn get_max_required_points(mode: TradingMode) -> usize {
    let (rsi, macd_fast, macd_slow, macd_signal, volume, bollinger) = get_indicator_periods(mode);
    
    // Find max period needed
    let max_period = *[rsi, macd_fast, macd_slow, macd_signal, volume, bollinger]
        .iter()
        .max()
        .unwrap();
        
    // Add buffer for calculations
    max_period + 10
}

/// Calculate the Relative Strength Index (RSI)
/// 
/// RSI measures the magnitude of recent price changes to evaluate
/// overbought or oversold conditions.
/// 
/// # Parameters
/// * `prices` - A slice of price values
/// * `period` - The period for RSI calculation (typically 14)
/// 
/// # Returns
/// The RSI value between 0 and 100
pub fn calculate_rsi(prices: &[f64], period: usize) -> Option<f64> {
    if prices.len() < period + 1 {
        trace!("RSI calculation failed: need {} points, have {}", period + 1, prices.len());
        return None;
    }

    let mut gains = 0.0;
    let mut losses = 0.0;

    // Calculate initial average gain and loss
    for i in 1..=period {
        let difference = prices[i] - prices[i - 1];
        if difference >= 0.0 {
            gains += difference;
        } else {
            losses += difference.abs();
        }
    }

    let mut avg_gain = gains / period as f64;
    let mut avg_loss = losses / period as f64;

    // Calculate RSI using the smoothed method
    for i in period + 1..prices.len() {
        let difference = prices[i] - prices[i - 1];
        
        let (current_gain, current_loss) = if difference >= 0.0 {
            (difference, 0.0)
        } else {
            (0.0, difference.abs())
        };

        avg_gain = (avg_gain * (period as f64 - 1.0) + current_gain) / period as f64;
        avg_loss = (avg_loss * (period as f64 - 1.0) + current_loss) / period as f64;
    }

    if avg_loss == 0.0 {
        return Some(100.0);
    }

    let rs = avg_gain / avg_loss;
    let rsi = 100.0 - (100.0 / (1.0 + rs));
    
    Some(rsi)
}

/// Calculate Moving Average Convergence Divergence (MACD)
/// 
/// MACD is a trend-following momentum indicator that shows the
/// relationship between two moving averages of a security's price.
/// 
/// # Parameters
/// * `prices` - A slice of price values
/// * `mode` - The trading mode to determine periods
/// 
/// # Returns
/// A tuple of (MACD Line, Signal Line, Histogram)
pub fn calculate_macd(prices: &[f64], mode: TradingMode) -> Option<(f64, f64, f64)> {
    let (_, fast_period, slow_period, signal_period, _, _) = get_indicator_periods(mode);
    
    if prices.len() < (2 * slow_period) + signal_period {
        debug!("Not enough data points for MACD calculation in {:?} mode. Need {}, have {}", 
               mode, (2 * slow_period) + signal_period, prices.len());
        return None;
    }

    // Calculate EMAs
    let fast_ema = calculate_ema(prices, fast_period)?;
    let slow_ema = calculate_ema(prices, slow_period)?;

    // MACD Line = Fast EMA - Slow EMA
    let macd_line = fast_ema - slow_ema;

    // Calculate Signal Line (EMA of MACD Line)
    let signal_line = calculate_ema(&[macd_line], signal_period)?;

    // Calculate Histogram
    let histogram = macd_line - signal_line;

    debug!("MACD calculated in {:?} mode - Line: {:.4}, Signal: {:.4}, Hist: {:.4}", 
           mode, macd_line, signal_line, histogram);

    Some((macd_line, signal_line, histogram))
}

/// Calculate Exponential Moving Average (EMA)
/// 
/// # Parameters
/// * `prices` - A slice of price values
/// * `period` - The period for EMA calculation
/// 
/// # Returns
/// The EMA value
fn calculate_ema(prices: &[f64], period: usize) -> Option<f64> {
    if prices.len() < period {
        return None;
    }
    
    // Start with a simple moving average
    let mut ema = prices[0..period].iter().sum::<f64>() / period as f64;
    
    // Multiplier: (2 / (period + 1))
    let multiplier = 2.0 / (period as f64 + 1.0);
    
    // Calculate EMA
    for price in prices.iter().skip(period) {
        ema = price * multiplier + ema * (1.0 - multiplier);
    }
    
    Some(ema)
}

/// Calculate Bollinger Bands
/// 
/// Bollinger Bands are volatility bands placed above and below a moving average.
/// 
/// # Parameters
/// * `prices` - A slice of price values
/// * `period` - The period for the moving average (typically 20)
/// * `std_dev_multiplier` - The standard deviation multiplier (typically 2.0)
/// 
/// # Returns
/// A tuple of (Upper Band, Middle Band, Lower Band)
pub fn calculate_bollinger_bands(
    prices: &[f64], 
    period: usize, 
    std_dev_multiplier: f64
) -> Option<(f64, f64, f64)> {
    if prices.len() < period {
        trace!("Bollinger Bands calculation failed: need {} points, have {}", period, prices.len());
        return None;
    }
    
    // Calculate simple moving average (SMA)
    let slice = &prices[prices.len() - period..];
    let sma = slice.iter().sum::<f64>() / period as f64;
    
    // Calculate standard deviation
    let variance = slice.iter()
        .map(|&x| {
            let diff = x - sma;
            diff * diff
        })
        .sum::<f64>() / period as f64;
    
    let std_dev = variance.sqrt();
    
    // Calculate bands
    let upper_band = sma + (std_dev_multiplier * std_dev);
    let lower_band = sma - (std_dev_multiplier * std_dev);
    
    Some((upper_band, sma, lower_band))
}

/// Analyze volume trend
/// 
/// Evaluates volume trend to confirm price movements
/// 
/// # Parameters
/// * `prices` - A slice of price values
/// * `volumes` - A slice of volume values
/// * `period` - The period for volume analysis
/// 
/// # Returns
/// A score between -1.0 and 1.0 where:
/// * > 0 indicates volume confirms uptrend
/// * < 0 indicates volume confirms downtrend
/// * 0 indicates volume doesn't confirm trend
pub fn analyze_volume_trend(prices: &[f64], volumes: &[f64], period: usize) -> Option<f64> {
    if prices.len() < period || volumes.len() < period {
        trace!("Volume trend calculation failed: need {} points, have prices={} volumes={}", 
               period, prices.len(), volumes.len());
        return None;
    }
    
    let slice_prices = &prices[prices.len() - period..];
    let slice_volumes = &volumes[volumes.len() - period..];
    
    // Calculate price change direction
    let price_direction = if slice_prices.last()? > slice_prices.first()? { 1.0 } else { -1.0 };
    
    // Calculate volume trend
    let avg_volume = slice_volumes.iter().sum::<f64>() / period as f64;
    let recent_volume = slice_volumes.iter().skip(period / 2).sum::<f64>() / (period as f64 / 2.0);
    
    // Normalize volume difference
    let volume_ratio = recent_volume / avg_volume;
    let volume_score = (volume_ratio - 1.0).min(1.0).max(-1.0);
    
    // Return score combining direction and volume
    Some(price_direction * volume_score)
}

/// Calculates a composite momentum score based on multiple indicators
/// 
/// # Parameters
/// * `prices` - Price time series
/// * `weights` - Weights for each indicator
/// 
/// # Returns
/// A momentum score between -100 and 100 where higher values indicate stronger upward momentum
pub fn calculate_composite_momentum(
    time_series: &PriceTimeSeries,
    weights: &IndicatorWeights
) -> Option<f64> {
    let available_points = time_series.prices().len();
    let required_points = get_max_required_points(time_series.mode);
    
    if available_points < required_points {
        info!("Building price history: {}/{} points in {:?} mode ({:.1}% complete)", 
              available_points, required_points, time_series.mode,
              (available_points as f64 / required_points as f64) * 100.0);
        return None;
    }

    let (rsi_period, fast_period, slow_period, signal_period, bollinger_period, volume_period) = 
        get_indicator_periods(time_series.mode);

    let prices = &time_series.prices;
    let volumes = &time_series.volumes;

    debug!("Calculating indicators in {:?} mode with {} data points", time_series.mode, prices.len());
    debug!("Using periods - RSI: {}, MACD Fast: {}, Slow: {}, Signal: {}, Bollinger: {}, Volume: {}", 
           rsi_period, fast_period, slow_period, signal_period, bollinger_period, volume_period);

    // Calculate RSI, Bollinger, and Volume trend first as they need fewer points
    let rsi = calculate_rsi(prices, rsi_period);
    let bollinger = calculate_bollinger_bands(prices, bollinger_period, 2.0);
    let volume_trend = analyze_volume_trend(prices, volumes, volume_period);

    // Calculate MACD based on mode and available points
    let macd = if prices.len() >= (2 * slow_period) + signal_period {
        calculate_macd(prices, time_series.mode)
    } else {
        debug!("Skipping MACD calculation in {:?} mode (have {} points, need {})", 
               time_series.mode, prices.len(), (2 * slow_period) + signal_period);
        None
    };

    // Log indicator status
    debug!("Indicator status for latest data point:");
    debug!("  - RSI ({} period): {}", rsi_period, rsi.map_or("Not calculated".to_string(), |v| format!("{:.2}", v)));
    debug!("  - MACD ({}/{}/{} periods): {}", 
        fast_period, slow_period, signal_period,
        macd.map_or("Not calculated".to_string(), |(m,s,h)| format!("MACD={:.4}, Signal={:.4}, Hist={:.4}", m, s, h)));
    debug!("  - Bollinger ({} period): {}", 
        bollinger_period,
        bollinger.map_or("Not calculated".to_string(), |(u,m,l)| format!("U={:.2}, M={:.2}, L={:.2}", u, m, l)));
    debug!("  - Volume trend ({} period): {}", 
        volume_period,
        volume_trend.map_or("Not calculated".to_string(), |v| format!("{:.2}", v)));

    // If RSI, Bollinger, or Volume trend failed, return None
    if rsi.is_none() || bollinger.is_none() || volume_trend.is_none() {
        info!("One or more required indicators failed to calculate");
        info!("RSI: {:?}, Bollinger: {:?}, Volume: {:?}", 
            rsi.is_some(), bollinger.is_some(), volume_trend.is_some());
        return None;
    }

    // Log raw indicator values
    let rsi_val = rsi.unwrap();
    let (upper, middle, lower) = bollinger.unwrap();
    let vol_trend = volume_trend.unwrap();

    info!("Raw Indicators:");
    info!("RSI: {:.2}", rsi_val);
    if let Some((macd_line, signal_line, hist)) = macd {
        info!("MACD: Line={:.6}, Signal={:.6}, Hist={:.6}", macd_line, signal_line, hist);
    }
    info!("Bollinger: Upper={:.4}, Middle={:.4}, Lower={:.4}", upper, middle, lower);
    info!("Volume Trend: {:.2}", vol_trend);

    // Calculate normalized scores
    let rsi_score = if rsi_val > 70.0 {
        -((rsi_val - 70.0) / 30.0).min(1.0)
    } else if rsi_val < 30.0 {
        ((30.0 - rsi_val) / 30.0).min(1.0)
    } else {
        (rsi_val - 50.0) / 40.0
    };
    info!("Normalized RSI score: {:.2}", rsi_score);

    // Calculate MACD score if available
    let macd_score = macd.map(|(macd_line, signal_line, hist)| {
        let signal_cross = if macd_line > signal_line { 1.0 } else { -1.0 };
        let hist_strength = (hist / macd_line.abs().max(0.0001)).min(1.0).max(-1.0);
        let trend_strength = (macd_line / signal_line.abs().max(0.0001)).min(1.0).max(-1.0);
        
        let normalized = (signal_cross + hist_strength + trend_strength) / 3.0;
        info!("Normalized MACD score: {:.2}", normalized);
        normalized
    });

    let latest_price = *prices.last().unwrap();
    let bollinger_score = {
        let band_position = (latest_price - lower) / (upper - lower);
        let normalized = (band_position - 0.5) * 2.0;
        info!("Normalized Bollinger score: {:.2}", normalized);
        normalized
    };

    let volume_score = vol_trend;
    info!("Normalized Volume score: {:.2}", volume_score);

    // Adjust weights if MACD is not available
    let mut adjusted_weights = weights.clone();
    if macd_score.is_none() {
        // Redistribute MACD weight to other indicators
        let macd_weight = weights.macd / 3.0;
        adjusted_weights.rsi += macd_weight;
        adjusted_weights.bollinger_bands += macd_weight;
        adjusted_weights.volume += macd_weight;
        adjusted_weights.macd = 0.0;
        
        info!("Adjusted weights (no MACD): RSI={:.2}, BBands={:.2}, Volume={:.2}", 
            adjusted_weights.rsi, adjusted_weights.bollinger_bands, adjusted_weights.volume);
    }

    // Calculate weighted composite score
    let composite_score = 
        rsi_score * adjusted_weights.rsi +
        macd_score.unwrap_or(0.0) * adjusted_weights.macd +
        bollinger_score * adjusted_weights.bollinger_bands +
        volume_score * adjusted_weights.volume;

    info!("Weighted component scores - RSI: {:.2}%, MACD: {:.2}%, BBands: {:.2}%, Volume: {:.2}%",
        rsi_score * adjusted_weights.rsi * 100.0,
        macd_score.unwrap_or(0.0) * adjusted_weights.macd * 100.0,
        bollinger_score * adjusted_weights.bollinger_bands * 100.0,
        volume_score * adjusted_weights.volume * 100.0
    );

    // Normalize to -100 to 100 range
    let normalized_score = composite_score * 100.0;
    info!("Final composite momentum score: {:.2}", normalized_score);

    Some(normalized_score)
}

/// Weights for each indicator in the composite momentum calculation
#[derive(Debug, Clone, Copy)]
pub struct IndicatorWeights {
    pub rsi: f64,
    pub macd: f64,
    pub bollinger_bands: f64,
    pub volume: f64,
}

impl Default for IndicatorWeights {
    fn default() -> Self {
        Self {
            rsi: 0.3,
            macd: 0.3,
            bollinger_bands: 0.2,
            volume: 0.2,
        }
    }
}

impl IndicatorWeights {
    /// Create a new set of weights
    pub fn new(rsi: f64, macd: f64, bollinger_bands: f64, volume: f64) -> Self {
        Self {
            rsi,
            macd,
            bollinger_bands,
            volume,
        }
    }
    
    /// Dynamically adjust weights based on market conditions
    pub fn adjust_for_market_conditions(&mut self, _volatility: f64, _trend_strength: f64) {
        // Advanced adaptive logic would go here
        // For now, using default weights
    }
} 