//! Technical indicators for trading strategies
//!
//! This module provides implementations of common technical indicators
//! used in trading algorithms, designed for efficiency and accuracy.

use chrono::{DateTime, Utc};
use log::{debug, info, trace};

/// Represents a time series of price data with a fixed length
#[derive(Debug, Clone)]
pub struct PriceTimeSeries {
    prices: Vec<f64>,
    volumes: Vec<f64>,
    timestamps: Vec<DateTime<Utc>>,
    capacity: usize,
    profile: crate::core::constants::IndicatorProfile,
}

impl Default for PriceTimeSeries {
    fn default() -> Self {
        Self::new(crate::core::constants::IndicatorProfile::default())
    }
}

impl PriceTimeSeries {
    /// Create a new price time series with the specified capacity
    /// Uses the selected indicator profile to determine required capacity
    pub fn new(profile: crate::core::constants::IndicatorProfile) -> Self {
        let capacity = get_max_required_points(profile);
        debug!(
            "Creating PriceTimeSeries with capacity {} for {:?} profile",
            capacity, profile
        );

        Self {
            prices: Vec::with_capacity(capacity),
            volumes: Vec::with_capacity(capacity),
            timestamps: Vec::with_capacity(capacity),
            capacity,
            profile,
        }
    }

    /// Add a new price data point, maintaining the fixed length
    pub fn add_data_point(&mut self, price: f64, volume: f64, timestamp: DateTime<Utc>) {
        self.prices.push(price);
        self.volumes.push(volume);
        self.timestamps.push(timestamp);

        while self.prices.len() > self.capacity {
            self.prices.remove(0);
            self.volumes.remove(0);
            self.timestamps.remove(0);
        }
    }

    /// Get a slice of the most recent prices
    pub fn prices(&self) -> Vec<f64> {
        self.prices.to_vec()
    }

    /// Get a slice of the most recent volumes
    pub fn volumes(&self) -> Vec<f64> {
        self.volumes.to_vec()
    }

    /// Get the current number of data points in the time series
    pub fn len(&self) -> usize {
        self.prices.len()
    }

    /// Returns true if the time series has no data points
    pub fn is_empty(&self) -> bool {
        self.prices.is_empty()
    }

    /// Get the maximum capacity of the time series
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Check if we have enough data points for calculations
    pub fn has_enough_data(&self, min_points: usize) -> bool {
        self.prices.len() >= min_points
    }

    pub fn has_minimum_data_for_analysis(&self) -> bool {
        self.prices.len() >= get_max_required_points(self.profile)
    }
}

/// Returns a tuple of periods for various indicators based on the selected profile
/// (rsi_period, macd_fast_period, macd_slow_period, macd_signal_period, bollinger_period, vol_trend_period)
pub fn get_indicator_periods(
    profile: crate::core::constants::IndicatorProfile,
) -> (usize, usize, usize, usize, usize, usize) {
    profile.periods()
}

/// Returns the maximum number of data points required for indicator calculations.
///
/// This is calculated by taking the maximum period from all indicators and
/// adding a buffer to ensure enough data for accurate calculations.
///
/// For MACD, the requirement is (2 × slow_period) + signal_period for proper EMA stabilization.
pub fn get_max_required_points(profile: crate::core::constants::IndicatorProfile) -> usize {
    let (rsi, _macd_fast, macd_slow, macd_signal, volume, bollinger) = profile.periods();

    // MACD needs (2 × slow_period) + signal_period for reliable calculation
    let macd_required = (2 * macd_slow) + macd_signal;

    let max_period = *[rsi, macd_required, volume, bollinger]
        .iter()
        .max()
        .expect("Indicator periods array is non-empty");

    max_period + crate::core::constants::INDICATOR_WARMUP_PERIODS
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
        trace!(
            "RSI calculation failed: need {} points, have {}",
            period + 1,
            prices.len()
        );
        return None;
    }

    let mut gains = 0.0;
    let mut losses = 0.0;

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
/// * `fast_period` - Fast EMA period (e.g., 8 or 12)
/// * `slow_period` - Slow EMA period (e.g., 17 or 26)
/// * `signal_period` - Signal line EMA period (e.g., 6 or 9)
///
/// # Returns
/// A tuple of (MACD Line, Signal Line, Histogram)
pub fn calculate_macd(
    prices: &[f64],
    fast_period: usize,
    slow_period: usize,
    signal_period: usize,
) -> Option<(f64, f64, f64)> {
    if prices.len() < (2 * slow_period) + signal_period {
        debug!(
            "Not enough data points for MACD calculation. Need {}, have {}",
            (2 * slow_period) + signal_period,
            prices.len()
        );
        return None;
    }

    let fast_ema = calculate_ema(prices, fast_period)?;
    let slow_ema = calculate_ema(prices, slow_period)?;

    let macd_line = fast_ema - slow_ema;
    let signal_line = calculate_ema(&[macd_line], signal_period)?;
    let histogram = macd_line - signal_line;

    debug!(
        "MACD calculated - Line: {:.4}, Signal: {:.4}, Hist: {:.4}",
        macd_line, signal_line, histogram
    );

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

    let mut ema = prices[0..period].iter().sum::<f64>() / period as f64;
    let multiplier = 2.0 / (period as f64 + 1.0);

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
    std_dev_multiplier: f64,
) -> Option<(f64, f64, f64)> {
    if prices.len() < period {
        trace!(
            "Bollinger Bands calculation failed: need {} points, have {}",
            period,
            prices.len()
        );
        return None;
    }

    let slice = &prices[prices.len() - period..];
    let sma = slice.iter().sum::<f64>() / period as f64;

    let variance = slice
        .iter()
        .map(|&x| {
            let diff = x - sma;
            diff * diff
        })
        .sum::<f64>()
        / period as f64;

    let std_dev = variance.sqrt();

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
        trace!(
            "Volume trend calculation failed: need {} points, have prices={} volumes={}",
            period,
            prices.len(),
            volumes.len()
        );
        return None;
    }

    let slice_prices = &prices[prices.len() - period..];
    let slice_volumes = &volumes[volumes.len() - period..];

    let price_direction = if slice_prices.last()? > slice_prices.first()? {
        1.0
    } else {
        -1.0
    };

    let avg_volume = slice_volumes.iter().sum::<f64>() / period as f64;
    let recent_volume = slice_volumes.iter().skip(period / 2).sum::<f64>() / (period as f64 / 2.0);

    let volume_ratio = recent_volume / avg_volume;
    let volume_score = (volume_ratio - 1.0).clamp(-1.0, 1.0);

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
    weights: &IndicatorWeights,
    token_symbol: &str,
    token_id: &str,
) -> Option<f64> {
    let available_points = time_series.prices().len();
    let required_points = get_max_required_points(time_series.profile);

    if available_points < required_points {
        let progress_pct = (available_points as f64 / required_points as f64) * 100.0;

        let display_symbol = if token_symbol.is_empty()
            || (token_symbol.len() > 20 && token_symbol.starts_with("0x"))
        {
            if token_id.len() >= 10 {
                format!(
                    "{}...{}",
                    &token_id[..8],
                    if token_id.len() > 12 {
                        &token_id[token_id.len() - 4..]
                    } else {
                        ""
                    }
                )
            } else {
                token_id.to_string()
            }
        } else {
            token_symbol.to_string()
        };

        info!(
            "Building price history for {}: {}/{} points ({:.0}% complete)",
            display_symbol, available_points, required_points, progress_pct
        );

        return None;
    }

    debug!(
        "Price history complete for {} ({}/{} points) - analysis ready",
        token_symbol, available_points, required_points
    );

    let (rsi_period, fast_period, slow_period, signal_period, bollinger_period, volume_period) =
        get_indicator_periods(time_series.profile);

    let prices = &time_series.prices;
    let volumes = &time_series.volumes;

    debug!("Calculating indicators with {} data points", prices.len());
    debug!(
        "Using periods - RSI: {}, MACD Fast: {}, Slow: {}, Signal: {}, Bollinger: {}, Volume: {}",
        rsi_period, fast_period, slow_period, signal_period, bollinger_period, volume_period
    );

    let rsi = calculate_rsi(prices, rsi_period);
    let bollinger = calculate_bollinger_bands(prices, bollinger_period, 2.0);
    let volume_trend = analyze_volume_trend(prices, volumes, volume_period);

    let macd = if prices.len() >= (2 * slow_period) + signal_period {
        calculate_macd(prices, fast_period, slow_period, signal_period)
    } else {
        debug!(
            "Skipping MACD calculation (have {} points, need {})",
            prices.len(),
            (2 * slow_period) + signal_period
        );
        None
    };

    debug!("Indicator status for latest data point:");
    debug!(
        "  - RSI ({} period): {}",
        rsi_period,
        rsi.map_or("Not calculated".to_string(), |v| format!("{:.2}", v))
    );
    debug!(
        "  - MACD ({}/{}/{} periods): {}",
        fast_period,
        slow_period,
        signal_period,
        macd.map_or("Not calculated".to_string(), |(m, s, h)| format!(
            "MACD={:.4}, Signal={:.4}, Hist={:.4}",
            m, s, h
        ))
    );
    debug!(
        "  - Bollinger ({} period): {}",
        bollinger_period,
        bollinger.map_or("Not calculated".to_string(), |(u, m, l)| format!(
            "U={:.2}, M={:.2}, L={:.2}",
            u, m, l
        ))
    );
    debug!(
        "  - Volume trend ({} period): {}",
        volume_period,
        volume_trend.map_or("Not calculated".to_string(), |v| format!("{:.2}", v))
    );

    let (rsi_val, (upper, middle, lower), vol_trend) = match (rsi, bollinger, volume_trend) {
        (Some(r), Some(b), Some(v)) => (r, b, v),
        _ => {
            info!("One or more required indicators failed to calculate");
            info!(
                "RSI: {:?}, Bollinger: {:?}, Volume: {:?}",
                rsi.is_some(),
                bollinger.is_some(),
                volume_trend.is_some()
            );
            return None;
        }
    };

    info!("Raw Indicators:");
    info!("RSI: {:.2}", rsi_val);
    if let Some((macd_line, signal_line, hist)) = macd {
        info!(
            "MACD: Line={:.6}, Signal={:.6}, Hist={:.6}",
            macd_line, signal_line, hist
        );
    }
    info!(
        "Bollinger: Upper={:.4}, Middle={:.4}, Lower={:.4}",
        upper, middle, lower
    );
    info!("Volume Trend: {:.2}", vol_trend);

    let rsi_score = if rsi_val > 70.0 {
        -((rsi_val - 70.0) / 30.0).min(1.0)
    } else if rsi_val < 30.0 {
        ((30.0 - rsi_val) / 30.0).min(1.0)
    } else {
        (rsi_val - 50.0) / 40.0
    };
    info!("Normalized RSI score: {:.2}", rsi_score);

    let macd_score = macd.map(|(macd_line, signal_line, hist)| {
        let signal_cross = if macd_line > signal_line { 1.0 } else { -1.0 };
        let hist_strength = (hist / macd_line.abs().max(0.0001)).clamp(-1.0, 1.0);
        let trend_strength = (macd_line / signal_line.abs().max(0.0001)).clamp(-1.0, 1.0);

        let normalized = (signal_cross + hist_strength + trend_strength) / 3.0;
        info!("Normalized MACD score: {:.2}", normalized);
        normalized
    });

    // Get latest price - guaranteed to exist since we validated data length at function start
    // If this fails, it indicates a programming bug (data was modified after validation)
    let latest_price = *prices
        .last()
        .expect("Price data validated non-empty at function start but is now empty");

    let bollinger_score = {
        let band_position = (latest_price - lower) / (upper - lower);
        let normalized = (band_position - 0.5) * 2.0;
        info!("Normalized Bollinger score: {:.2}", normalized);
        normalized
    };

    let volume_score = vol_trend;
    info!("Normalized Volume score: {:.2}", volume_score);

    // Adjust weights if MACD is not available
    let mut adjusted_weights = *weights;
    if macd_score.is_none() {
        // Redistribute MACD weight to other indicators
        let macd_weight = weights.macd / 3.0;
        adjusted_weights.rsi += macd_weight;
        adjusted_weights.bollinger_bands += macd_weight;
        adjusted_weights.volume += macd_weight;
        adjusted_weights.macd = 0.0;

        info!(
            "Adjusted weights (no MACD): RSI={:.2}, BBands={:.2}, Volume={:.2}",
            adjusted_weights.rsi, adjusted_weights.bollinger_bands, adjusted_weights.volume
        );
    }

    // Calculate weighted composite score
    let composite_score = rsi_score * adjusted_weights.rsi
        + macd_score.unwrap_or(0.0) * adjusted_weights.macd
        + bollinger_score * adjusted_weights.bollinger_bands
        + volume_score * adjusted_weights.volume;

    info!(
        "Weighted component scores - RSI: {:.2}%, MACD: {:.2}%, BBands: {:.2}%, Volume: {:.2}%",
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

/// Snapshot of indicator values at signal-generation time.
///
/// Populated from a token's `PriceTimeSeries` and attached to `SignalMetadata`
/// so downstream consumers (e.g. the AI advisor) receive real indicator context
/// instead of fallback defaults.
#[derive(Debug, Clone, Copy, Default)]
pub struct IndicatorSnapshot {
    pub rsi: Option<f64>,
    /// Position of the latest price within the Bollinger bands, 0-100%.
    pub bollinger_pct: Option<f64>,
    /// Composite momentum score, -100 to 100. `None` when insufficient data.
    pub momentum_score: Option<f64>,
}

/// Compute an `IndicatorSnapshot` from a price time series.
///
/// Each indicator is computed independently and contributes `None` when the
/// series does not yet have enough data. Uses the profile's configured
/// periods; does not fail the whole snapshot if one indicator lacks data.
pub fn compute_snapshot(
    time_series: &PriceTimeSeries,
    weights: &IndicatorWeights,
    token_symbol: &str,
    token_id: &str,
) -> IndicatorSnapshot {
    let (rsi_period, _fast, _slow, _signal, bollinger_period, _vol) =
        get_indicator_periods(time_series.profile);

    let prices = time_series.prices();
    let latest_price = prices.last().copied();

    let rsi = calculate_rsi(&prices, rsi_period);

    let bollinger_pct = match (
        calculate_bollinger_bands(
            &prices,
            bollinger_period,
            crate::core::constants::BOLLINGER_STD_DEV_MULTIPLIER,
        ),
        latest_price,
    ) {
        (Some((upper, _middle, lower)), Some(price)) if upper > lower => {
            Some((price - lower) / (upper - lower) * 100.0)
        }
        _ => None,
    };

    let momentum_score = calculate_composite_momentum(time_series, weights, token_symbol, token_id);

    IndicatorSnapshot {
        rsi,
        bollinger_pct,
        momentum_score,
    }
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
        Self::new_const()
    }
}

impl IndicatorWeights {
    const fn new_const() -> Self {
        Self {
            rsi: 0.3,
            macd: 0.3,
            bollinger_bands: 0.2,
            volume: 0.2,
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::constants::IndicatorProfile;

    fn series_with(prices: &[f64]) -> PriceTimeSeries {
        let mut ts = PriceTimeSeries::new(IndicatorProfile::default());
        for &p in prices {
            ts.add_data_point(p, 1_000_000.0, Utc::now());
        }
        ts
    }

    #[test]
    fn snapshot_empty_series_returns_all_none() {
        let ts = PriceTimeSeries::new(IndicatorProfile::default());
        let snap = compute_snapshot(&ts, &IndicatorWeights::default(), "TOK", "tok");
        assert!(snap.rsi.is_none());
        assert!(snap.bollinger_pct.is_none());
        assert!(snap.momentum_score.is_none());
    }

    #[test]
    fn snapshot_populates_rsi_and_bollinger_with_enough_data() {
        // 30 oscillating prices — enough for RSI(14) and Bollinger(20).
        let prices: Vec<f64> = (0..30)
            .map(|i| 100.0 + if i % 2 == 0 { 1.0 } else { -0.5 })
            .collect();
        let ts = series_with(&prices);

        let snap = compute_snapshot(&ts, &IndicatorWeights::default(), "TOK", "tok");
        assert!(snap.rsi.is_some(), "RSI should populate with 30 points");
        assert!(
            snap.bollinger_pct.is_some(),
            "Bollinger % should populate with 30 points"
        );
        let bb = snap.bollinger_pct.unwrap();
        assert!(
            (0.0..=100.0).contains(&bb) || bb.is_finite(),
            "Bollinger % should be finite, got {}",
            bb
        );
    }

    #[test]
    fn snapshot_momentum_requires_full_profile_data() {
        // Only 30 points — Standard profile needs (2×26)+9 = 61 for MACD, so
        // composite momentum should be None here.
        let prices: Vec<f64> = (0..30).map(|i| 100.0 + i as f64).collect();
        let ts = series_with(&prices);
        let snap = compute_snapshot(&ts, &IndicatorWeights::default(), "TOK", "tok");
        assert!(
            snap.momentum_score.is_none(),
            "Momentum needs full profile warmup"
        );
    }
}
