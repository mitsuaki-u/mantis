//! Technical indicators for trading strategies
//!
//! This module provides implementations of common technical indicators
//! used in trading algorithms, designed for efficiency and accuracy.

use std::collections::VecDeque;

/// Represents a time series of price data with a fixed length
#[derive(Debug, Clone)]
pub struct PriceTimeSeries {
    period: usize,
    prices: VecDeque<f64>,
    volumes: VecDeque<f64>,
}

impl PriceTimeSeries {
    /// Create a new price time series with the specified period
    pub fn new(period: usize) -> Self {
        Self {
            period,
            prices: VecDeque::with_capacity(period),
            volumes: VecDeque::with_capacity(period),
        }
    }

    /// Add a new price data point, maintaining the fixed length
    pub fn add_data_point(&mut self, price: f64, volume: f64) {
        if self.prices.len() >= self.period {
            self.prices.pop_front();
            self.volumes.pop_front();
        }
        self.prices.push_back(price);
        self.volumes.push_back(volume);
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
/// * `fast_period` - The period for the fast EMA (typically 12)
/// * `slow_period` - The period for the slow EMA (typically 26)
/// * `signal_period` - The period for the signal line (typically 9)
/// 
/// # Returns
/// A tuple of (MACD Line, Signal Line, Histogram)
pub fn calculate_macd(
    prices: &[f64], 
    fast_period: usize, 
    slow_period: usize, 
    signal_period: usize
) -> Option<(f64, f64, f64)> {
    if prices.len() < slow_period + signal_period {
        return None;
    }

    // Calculate fast and slow EMAs
    let fast_ema = calculate_ema(prices, fast_period)?;
    let slow_ema = calculate_ema(prices, slow_period)?;
    
    // MACD Line = Fast EMA - Slow EMA
    let macd_line = fast_ema - slow_ema;
    
    // Get historical MACD values for signal line calculation
    let mut historical_macd = Vec::with_capacity(prices.len());
    for i in 0..prices.len() - slow_period + 1 {
        let slice = &prices[i..i + slow_period];
        if let (Some(fast), Some(slow)) = (
            calculate_ema(slice, fast_period),
            calculate_ema(slice, slow_period)
        ) {
            historical_macd.push(fast - slow);
        }
    }
    
    // Calculate Signal Line (EMA of MACD Line)
    let signal_line = calculate_ema(&historical_macd, signal_period)?;
    
    // MACD Histogram = MACD Line - Signal Line
    let histogram = macd_line - signal_line;
    
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
    if !time_series.has_enough_data(30) {  // Need at least 30 data points for reliable indicators
        return None;
    }
    
    let prices = time_series.prices();
    let volumes = time_series.volumes();
    
    // Calculate individual indicators
    let rsi = calculate_rsi(&prices, 14)?;
    let (macd_line, signal_line, _) = calculate_macd(&prices, 12, 26, 9)?;
    let (upper, middle, lower) = calculate_bollinger_bands(&prices, 20, 2.0)?;
    let volume_trend = analyze_volume_trend(&prices, &volumes, 14)?;
    
    // Normalize indicators to a -100 to 100 scale
    let rsi_score = (rsi - 50.0) * 2.0;  // Convert 0-100 to -100 to 100
    
    let macd_score = if macd_line > 0.0 && macd_line > signal_line {
        100.0  // Strong bullish
    } else if macd_line > 0.0 {
        50.0   // Bullish
    } else if macd_line < signal_line {
        -100.0 // Strong bearish
    } else {
        -50.0  // Bearish
    };
    
    // Band width indicates volatility, position within bands indicates momentum
    let latest_price = *prices.last()?;
    let band_width = (upper - lower) / middle;
    let band_position = (latest_price - lower) / (upper - lower);
    let bb_score = ((band_position * 2.0 - 1.0) * 100.0) * band_width;
    
    let volume_score = volume_trend * 100.0;
    
    // Calculate weighted score
    let composite_score = 
        (rsi_score * weights.rsi) +
        (macd_score * weights.macd) +
        (bb_score * weights.bollinger_bands) +
        (volume_score * weights.volume);
    
    // Normalize to -100 to 100 range
    let total_weight = weights.rsi + weights.macd + weights.bollinger_bands + weights.volume;
    Some(composite_score / total_weight)
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