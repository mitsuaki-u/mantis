//! Core layer constants
//!
//! Business logic constants for trading strategies, market data validation,
//! and risk management.

// ============================================================================
// POSITION & EXPOSURE DEFAULTS
// ============================================================================

/// Default maximum position size in USD
pub const DEFAULT_MAX_POSITION_SIZE: f64 = 50.0;

/// Default minimum position size in USD
pub const DEFAULT_MIN_POSITION_SIZE: f64 = 20.0;

/// Default maximum total exposure in USD
pub const DEFAULT_MAX_TOTAL_EXPOSURE: f64 = 1000.0;

/// Default maximum number of concurrent positions
pub const DEFAULT_MAX_POSITIONS: usize = 5;

// ============================================================================
// STRATEGY & CONFIDENCE
// ============================================================================

/// Default trading strategy type
pub const DEFAULT_STRATEGY_TYPE: &str = "momentum";

/// Default signal confidence threshold (0.0-1.0)
pub const DEFAULT_CONFIDENCE_THRESHOLD: f64 = 0.65;

// ============================================================================
// INDICATOR SETTINGS
// ============================================================================

/// Warmup periods needed for technical indicator calculations
pub const INDICATOR_WARMUP_PERIODS: usize = 10;

// ============================================================================
// INDICATOR WEIGHTS
// ============================================================================

/// Default RSI weight in momentum strategy
pub const DEFAULT_RSI_WEIGHT: f64 = 0.3;

/// Default MACD weight in momentum strategy
pub const DEFAULT_MACD_WEIGHT: f64 = 0.3;

/// Default Bollinger Bands weight in momentum strategy
pub const DEFAULT_BOLLINGER_WEIGHT: f64 = 0.2;

/// Default volume weight in momentum strategy
pub const DEFAULT_VOLUME_WEIGHT: f64 = 0.2;

// ============================================================================
// SCAN MODE LIMITS
// ============================================================================

/// Token limit for Targeted scan mode (tracked list only)
pub const SCAN_LIMIT_TARGETED: usize = 20;

/// Token limit for Limited scan mode
pub const SCAN_LIMIT_LIMITED: usize = 50;

/// Token limit for Medium scan mode (recommended)
pub const SCAN_LIMIT_MEDIUM: usize = 150;

/// Token limit for Wide scan mode
pub const SCAN_LIMIT_WIDE: usize = 250;

/// Fallback token limit for enhanced discovery
pub const SCAN_LIMIT_FALLBACK: usize = 100;

/// Minimum tokens to scan in targeted mode when no tokens configured
pub const MIN_TARGETED_SCAN_TOKENS: usize = 10;

// ============================================================================
// GAS SETTINGS
// ============================================================================

/// Default maximum gas cost in USD
pub const DEFAULT_MAX_GAS_COST_USD: f64 = 4.0;

/// Default maximum gas cost as percentage of trade size
pub const DEFAULT_MAX_GAS_COST_PCT: f64 = 15.0;

/// Minimum trade size to allow gas fees (USD)
pub const DEFAULT_MIN_TRADE_SIZE_FOR_GAS: f64 = 20.0;

/// Default gas limit for V3 swaps
pub const DEFAULT_V3_SWAP_GAS_LIMIT: u64 = 100_000;

/// Complex V3 swap gas limit (for complex transactions)
pub const COMPLEX_V3_SWAP_GAS_LIMIT: u64 = 160_000;

/// Gas estimate buffer percentage (20% safety margin)
pub const GAS_ESTIMATE_BUFFER_PCT: u64 = 20;

// ============================================================================
// SLIPPAGE & EXECUTION
// ============================================================================

/// Default slippage tolerance for swaps (2%)
pub const DEFAULT_SLIPPAGE_TOLERANCE: f64 = 0.02;

/// Maximum age of signal price data before considered stale (5 minutes)
pub const MAX_SIGNAL_AGE_SECS: u64 = 300;

// ============================================================================
// STOP LOSS & TAKE PROFIT
// ============================================================================

/// Default stop loss percentage
pub const DEFAULT_STOP_LOSS_PCT: f64 = 5.0;

/// Default take profit percentage
pub const DEFAULT_TAKE_PROFIT_PCT: f64 = 10.0;

// ============================================================================
// POSITION SIZING
// ============================================================================

/// Minimum position size as percentage of maximum
pub const MIN_POSITION_PCT_OF_MAX: f64 = 0.01;

/// Minimum size multiplier for risk calculations
pub const MIN_SIZE_MULTIPLIER: f64 = 0.1;

/// Maximum size multiplier for single position
pub const MAX_SIZE_MULTIPLIER: f64 = 0.8;

// ============================================================================
// STRATEGY-SPECIFIC CONSTANTS
// ============================================================================

pub mod rsi {
    pub const OVERSOLD_THRESHOLD: f64 = 30.0;
    pub const TRADITIONAL_OVERBOUGHT: f64 = 70.0;
    pub const DEFAULT_MIN_VOLUME: f64 = 1_000_000.0;
    pub const DEFAULT_STOP_LOSS_PCT: f64 = 5.0;
    pub const COOLDOWN_HOURS: i64 = 1;
    pub const STANDARD_PERIOD: usize = 14;
}

pub mod production {
    pub const STANDARD_PERIODS: (usize, usize, usize, usize, usize, usize) =
        (14, 12, 26, 9, 20, 20);
}

// ============================================================================
// INDICATOR PROFILES - Optimized period presets for different trading styles
// ============================================================================

/// Indicator profile presets optimized for different trading timeframes
///
/// Each profile defines periods for: (RSI, MACD_fast, MACD_slow, MACD_signal, Bollinger, Volume)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndicatorProfile {
    /// Ultra-fast settings for scalping (15-60s intervals)
    /// MACD (5, 13, 4) - 40 min warmup
    /// Catches rapid momentum shifts but generates more false signals
    Scalping,

    /// Balanced settings for day trading (60-180s intervals) - RECOMMENDED for 1-min candles
    /// MACD (8, 17, 6) - 50 min warmup
    /// Good balance between responsiveness and reliability
    #[default]
    DayTrading,

    /// Conservative settings for swing trading (300-900s intervals)
    /// MACD (10, 20, 7) - 57 min warmup
    /// More reliable signals, filters out most noise
    SwingTrading,

    /// Traditional/standard settings (900s+ intervals)
    /// MACD (12, 26, 9) - 71 min warmup: (2×26)+9+10
    Standard,
}

impl IndicatorProfile {
    /// Get indicator periods for this profile
    /// Returns: (rsi_period, macd_fast, macd_slow, macd_signal, bollinger_period, volume_period)
    pub fn periods(&self) -> (usize, usize, usize, usize, usize, usize) {
        match self {
            IndicatorProfile::Scalping => (14, 5, 13, 4, 20, 20),
            IndicatorProfile::DayTrading => (14, 8, 17, 6, 20, 20),
            IndicatorProfile::SwingTrading => (14, 10, 20, 7, 20, 20),
            IndicatorProfile::Standard => (14, 12, 26, 9, 20, 20),
        }
    }

    /// Get warmup time in minutes for this profile (at 60s scan interval)
    pub fn warmup_minutes(&self) -> usize {
        let (rsi, _, macd_slow, macd_signal, bollinger, volume) = self.periods();
        let macd_required = (2 * macd_slow) + macd_signal;
        let max_period = *[rsi, macd_required, bollinger, volume]
            .iter()
            .max()
            .unwrap();
        max_period + INDICATOR_WARMUP_PERIODS
    }

    /// Get recommended profile based on scan interval in seconds
    pub fn recommended_for_interval(scan_interval_secs: u64) -> Self {
        match scan_interval_secs {
            0..=60 => IndicatorProfile::DayTrading,     // 1-min candles
            61..=300 => IndicatorProfile::SwingTrading, // 5-min candles
            _ => IndicatorProfile::Standard,            // 15+ min candles
        }
    }

    /// Parse from string (case-insensitive)
    pub fn from_string(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "scalping" => Some(IndicatorProfile::Scalping),
            "day_trading" | "daytrading" => Some(IndicatorProfile::DayTrading),
            "swing_trading" | "swingtrading" => Some(IndicatorProfile::SwingTrading),
            "standard" => Some(IndicatorProfile::Standard),
            _ => None,
        }
    }

    /// Convert to string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            IndicatorProfile::Scalping => "scalping",
            IndicatorProfile::DayTrading => "day_trading",
            IndicatorProfile::SwingTrading => "swing_trading",
            IndicatorProfile::Standard => "standard",
        }
    }
}

impl serde::Serialize for IndicatorProfile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for IndicatorProfile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        IndicatorProfile::from_string(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("Invalid indicator profile: '{}'", s)))
    }
}

// ============================================================================
// TECHNICAL INDICATORS
// ============================================================================

/// Standard deviation multiplier for Bollinger Bands (typically 2.0)
pub const BOLLINGER_STD_DEV_MULTIPLIER: f64 = 2.0;

/// Default lookback period for volume trend analysis
pub const DEFAULT_VOLUME_LOOKBACK: usize = 10;

// ============================================================================
// UNISWAP V3 SPECIFICS
// ============================================================================

/// Fee tier to decimal divisor (Uniswap V3 uses millionths: 1,000,000 = 100%)
/// Example: 3000 / 1,000,000 = 0.003 (0.3% fee)
pub const V3_FEE_TIER_DIVISOR: f64 = 1000000.0;

/// Standard Uniswap V3 fee tier for simulations (0.3% = 3000 in millionths)
pub const V3_STANDARD_FEE_TIER_BPS: f64 = 3000.0;

/// Standard Uniswap V3 fee percentage (0.3%)
pub const V3_STANDARD_FEE_PCT: f64 = 0.003;

// ============================================================================
// EXIT CONDITIONS
// ============================================================================

/// Trailing stop loss multiplier (50% of regular stop loss)
pub const TRAILING_STOP_MULTIPLIER: f64 = 0.5;

/// Default maximum hold time for positions in days
pub const DEFAULT_MAX_HOLD_TIME_DAYS: i64 = 7;

/// Risk tolerance multiplier increment per level (10% per level)
pub const RISK_TOLERANCE_MULTIPLIER_INCREMENT: f64 = 0.1;

/// Default momentum threshold percentage
pub const DEFAULT_MOMENTUM_THRESHOLD_PCT: f64 = 5.0;

// ============================================================================
// PRICE VALIDATION
// ============================================================================

/// Minimum token price filter in USD (filters out microcap scams)
pub const MIN_TOKEN_PRICE_USD: f64 = 0.10;

/// Low price warning threshold in USD
pub const LOW_PRICE_WARNING_USD: f64 = 1.0;

/// Maximum token price ceiling in USD (likely data errors above this)
pub const MAX_TOKEN_PRICE_USD: f64 = 100_000.0;

/// High price warning threshold in USD
pub const HIGH_PRICE_WARNING_USD: f64 = 100_000.0;

/// Maximum price change percentage allowed (1000% = 10x)
pub const MAX_PRICE_CHANGE_PCT: f64 = 1000.0;

/// Large price change warning threshold (100% = 2x)
pub const LARGE_PRICE_CHANGE_WARNING_PCT: f64 = 100.0;

/// Maximum market cap in USD
pub const MAX_MARKET_CAP_USD: f64 = 1_000_000_000_000.0;

// ============================================================================
// VOLUME & LIQUIDITY
// ============================================================================

/// Minimum daily volume requirement (USD)
pub const DEFAULT_MIN_VOLUME: f64 = 1_000_000.0;

/// Minimum liquidity requirement for trading pairs (USD)
pub const DEFAULT_MIN_LIQUIDITY: f64 = 100_000.0;

/// Volume threshold for full confidence in volume-based analysis
pub const FULL_CONFIDENCE_VOLUME_THRESHOLD: f64 = 1_000_000.0;

/// Minimum volume confidence when no data available
pub const MIN_VOLUME_CONFIDENCE: f64 = 0.1;

/// Minimum pool transaction count requirement
pub const DEFAULT_MIN_POOL_TRANSACTION_COUNT: u32 = 1000;

// ============================================================================
// LIQUIDITY CONFIDENCE SCORING
// ============================================================================

/// Liquidity thresholds for confidence scoring
pub const EXCELLENT_LIQUIDITY: f64 = 10_000_000.0; // $10M+ = 100% confidence
pub const VERY_HIGH_LIQUIDITY: f64 = 5_000_000.0; // $5M+ = 95% confidence
pub const HIGH_LIQUIDITY: f64 = 1_000_000.0; // $1M+ = 85% confidence
pub const GOOD_LIQUIDITY: f64 = 500_000.0; // $500k+ = 75% confidence
pub const ADEQUATE_LIQUIDITY: f64 = 250_000.0; // $250k+ = 65% confidence
pub const MINIMUM_LIQUIDITY: f64 = 100_000.0; // $100k+ = 55% confidence

/// Confidence scores corresponding to liquidity levels
pub const EXCELLENT_CONFIDENCE: f64 = 1.0;
pub const VERY_HIGH_CONFIDENCE: f64 = 0.95;
pub const HIGH_CONFIDENCE: f64 = 0.85;
pub const GOOD_CONFIDENCE: f64 = 0.75;
pub const ADEQUATE_CONFIDENCE: f64 = 0.65;
pub const MINIMUM_CONFIDENCE: f64 = 0.55;
pub const BELOW_MINIMUM_CONFIDENCE: f64 = 0.1;

// ============================================================================
// MARKET DATA FILTERS
// ============================================================================

/// Minimum pool age in seconds (6 months)
pub const MIN_POOL_AGE_SECS: u64 = 6 * 30 * 86400;

/// Minimum token symbol length
pub const MIN_TOKEN_SYMBOL_LENGTH: usize = 2;

/// Maximum token symbol length
pub const MAX_TOKEN_SYMBOL_LENGTH: usize = 8;

/// Maximum volume/TVL ratio for wash trading detection
pub const MAX_VOLUME_TVL_RATIO: f64 = 50.0;

/// Maximum aggregated volume/liquidity ratio
pub const MAX_AGGREGATE_VOL_LIQ_RATIO: f64 = 10.0;

// ============================================================================
// PRICE VALIDATION & EXECUTION
// ============================================================================

/// Maximum allowed price discrepancy between external API and blockchain prices (5%)
pub const MAX_PRICE_DISCREPANCY_THRESHOLD: f64 = 0.05;

/// Default maximum allowed price deviation from signal to execution (5%)
pub const DEFAULT_MAX_EXECUTION_PRICE_DEVIATION: f64 = 0.05;

/// Enable pre-execution price cross-check validation
pub const ENABLE_PRICE_CROSS_CHECK: bool = true;

// ============================================================================
// GRAPHQL QUERY LIMITS
// ============================================================================

/// GraphQL query limit for full token scans
pub const GRAPHQL_QUERY_LIMIT_FULL: usize = 500;

/// Initial fetch limit for pool queries
pub const GRAPHQL_INITIAL_FETCH_LIMIT: usize = 5;

/// Limit for detailed token information queries
pub const GRAPHQL_TOKEN_DETAIL_LIMIT: usize = 10;

// ============================================================================
// POOL FILTERS
// ============================================================================

/// Relaxed minimum pool transaction count for lower quality scans
pub const MIN_POOL_TX_COUNT_RELAXED: u32 = 500;

// ============================================================================
// LIQUIDITY MULTIPLIERS
// ============================================================================

/// Base liquidity multiplier (accounts for 2-token split + quality factor)
pub const LIQUIDITY_QUALITY_MULTIPLIER: i32 = 4;

/// Liquidity multiplier for small token scans (high quality)
pub const HIGH_QUALITY_SCAN_LIQ_MULTIPLIER: i32 = 8;

/// Liquidity multiplier for medium token scans (balanced quality)
pub const MEDIUM_QUALITY_SCAN_LIQ_MULTIPLIER: i32 = 4;

/// Liquidity multiplier for large token scans (lower quality threshold)
pub const LOW_QUALITY_SCAN_LIQ_MULTIPLIER: i32 = 2;

// ============================================================================
// TOKEN DISCOVERY LIMITS
// ============================================================================

/// Default maximum number of tokens to scan in auto-discovery mode
pub const DEFAULT_MAX_TOKENS_TO_SCAN: usize = 150;

/// Small token scan threshold (high quality pools only)
pub const SMALL_SCAN_THRESHOLD: usize = 50;

/// Medium token scan threshold (balanced quality)
pub const MEDIUM_SCAN_THRESHOLD: usize = 150;

/// Maximum limit for unlimited scans (prevents API abuse)
pub const UNLIMITED_SCAN_MAX_LIMIT: usize = 500;

/// Minimum tokens for health check connectivity test
pub const HEALTH_CHECK_TOKEN_LIMIT: usize = 1;

// ============================================================================
// CALCULATION WEIGHTS
// ============================================================================

/// Weight of loss impact in risk calculations
pub const LOSS_IMPACT_WEIGHT: f64 = 0.3;

/// Weight of drawdown impact in risk calculations
pub const DRAWDOWN_IMPACT_WEIGHT: f64 = 0.3;

// ============================================================================
// PORTFOLIO FACTORS
// ============================================================================

/// Threshold for loss/drawdown ratio before reducing position size
pub const LOSS_DRAWDOWN_REDUCTION_THRESHOLD: f64 = 0.5;

/// Maximum reduction factor for loss/drawdown impact
pub const MAX_LOSS_DRAWDOWN_REDUCTION: f64 = 0.5;

/// Threshold for exposure ratio before reducing position size
pub const EXPOSURE_REDUCTION_THRESHOLD: f64 = 0.8;

/// Maximum reduction factor for exposure impact
pub const MAX_EXPOSURE_REDUCTION: f64 = 0.2;

// ============================================================================
// RISK THRESHOLDS
// ============================================================================

/// High risk threshold for risk assessment
pub const HIGH_RISK_THRESHOLD: f64 = 0.7;

/// Warning threshold percentage for daily loss and drawdown (90%)
pub const WARNING_THRESHOLD_PCT: f64 = 0.9;

/// Default maximum daily loss percentage
pub const DEFAULT_MAX_DAILY_LOSS: f64 = 10.0;

/// Default maximum drawdown percentage
pub const DEFAULT_MAX_DRAWDOWN: f64 = 20.0;

/// Minimum risk adjusted confidence threshold
pub const MIN_RISK_ADJUSTED_CONFIDENCE: f64 = 0.4;

// ============================================================================
// POSITION RISK
// ============================================================================

/// Default maximum single trade risk as percentage of wallet
pub const DEFAULT_MAX_TRADE_RISK_PCT: f64 = 5.0;

/// Default minimum required ETH balance for trading
pub const DEFAULT_MIN_ETH_BALANCE: f64 = 0.1;

// ============================================================================
// RISK SCORES
// ============================================================================

/// Default risk score for unknown tokens
pub const DEFAULT_UNKNOWN_TOKEN: f64 = 0.5;

/// Maximum risk score value
pub const MAX_RISK_SCORE: f64 = 1.0;

/// Minimum confidence threshold for risk assessment
pub const MIN_CONFIDENCE_THRESHOLD: f64 = 0.1;

pub const HIGH_SEVERITY_INCREMENT: f64 = 0.3;
pub const MEDIUM_SEVERITY_INCREMENT: f64 = 0.15;
pub const LOW_SEVERITY_INCREMENT: f64 = 0.05;
pub const GENERAL_RISK_INCREASE: f64 = 0.2;
pub const EXECUTION_FAILURE_INCREMENT: f64 = 0.25;

// ============================================================================
// TOLERANCE LEVELS - Very Conservative (Level 0)
// ============================================================================

pub const VERY_CONSERVATIVE_BASE_RISK: f64 = 0.7;
pub const VERY_CONSERVATIVE_RISK_THRESHOLD: f64 = 0.7;
pub const VERY_CONSERVATIVE_VOLATILITY_THRESHOLD: f64 = 0.8;
pub const VERY_CONSERVATIVE_DEFAULT_RISK_UNKNOWN: f64 = 0.95;

// ============================================================================
// TOLERANCE LEVELS - Conservative (Level 1)
// ============================================================================

pub const CONSERVATIVE_BASE_RISK: f64 = 0.6;
pub const CONSERVATIVE_RISK_THRESHOLD: f64 = 0.75;
pub const CONSERVATIVE_VOLATILITY_THRESHOLD: f64 = 0.85;
pub const CONSERVATIVE_DEFAULT_RISK_UNKNOWN: f64 = 0.9;

// ============================================================================
// TOLERANCE LEVELS - Moderate (Level 2)
// ============================================================================

pub const MODERATE_BASE_RISK: f64 = 0.5;
pub const MODERATE_RISK_THRESHOLD: f64 = 0.8;
pub const MODERATE_VOLATILITY_THRESHOLD: f64 = 0.9;
pub const MODERATE_DEFAULT_RISK_UNKNOWN: f64 = 0.8;

// ============================================================================
// TOLERANCE LEVELS - Moderate Aggressive (Level 3)
// ============================================================================

pub const MODERATE_AGGRESSIVE_BASE_RISK: f64 = 0.4;
pub const MODERATE_AGGRESSIVE_RISK_THRESHOLD: f64 = 0.85;
pub const MODERATE_AGGRESSIVE_VOLATILITY_THRESHOLD: f64 = 0.93;
pub const MODERATE_AGGRESSIVE_DEFAULT_RISK_UNKNOWN: f64 = 0.7;

// ============================================================================
// TOLERANCE LEVELS - Aggressive (Level 4)
// ============================================================================

pub const AGGRESSIVE_BASE_RISK: f64 = 0.3;
pub const AGGRESSIVE_RISK_THRESHOLD: f64 = 0.9;
pub const AGGRESSIVE_VOLATILITY_THRESHOLD: f64 = 0.96;
pub const AGGRESSIVE_DEFAULT_RISK_UNKNOWN: f64 = 0.6;

// ============================================================================
// TOLERANCE LEVELS - Very Aggressive (Level 5)
// ============================================================================

pub const VERY_AGGRESSIVE_BASE_RISK: f64 = 0.2;
pub const VERY_AGGRESSIVE_RISK_THRESHOLD: f64 = 0.95;
pub const VERY_AGGRESSIVE_VOLATILITY_THRESHOLD: f64 = 0.98;
pub const VERY_AGGRESSIVE_DEFAULT_RISK_UNKNOWN: f64 = 0.5;

// ============================================================================
// HEALTH STATUS
// ============================================================================

/// Critical threshold for health status
pub const CRITICAL_THRESHOLD: f64 = 0.8;

/// Maximum error count before critical status
pub const MAX_ERROR_COUNT: usize = 10;

/// Warning threshold for error count before degraded status
pub const WARNING_ERROR_COUNT: usize = 5;

// ============================================================================
// RISK ASSESSMENT DEFAULTS
// ============================================================================

/// Default medium risk when no data available
pub const DEFAULT_PORTFOLIO_RISK: f64 = 0.5;

/// Minimum risk factor clamp (10% of normal)
pub const MIN_RISK_FACTOR: f64 = 0.1;

/// Default minimum portfolio risk factor before halting new trades (30%)
/// When losses reduce the risk factor below this threshold, no new trades are opened
/// Risk factor: 1.0 = no losses, 0.5 = moderate losses, 0.3 = significant losses
pub const DEFAULT_MIN_PORTFOLIO_RISK_FACTOR_THRESHOLD: f64 = 0.3;

/// Minimum portfolio risk factor clamp (absolute floor for calculations)
pub const MIN_PORTFOLIO_RISK_FACTOR: f64 = 0.1;

/// Maximum portfolio risk factor clamp
pub const MAX_PORTFOLIO_RISK_FACTOR: f64 = 1.0;

// ============================================================================
// MARKET ASSESSMENT - Volatility
// ============================================================================

pub const MEDIUM_VOLATILITY: f64 = 10.0; // 10%
pub const HIGH_VOLATILITY: f64 = 20.0; // 20%

pub const HIGH_VOLATILITY_RISK: f64 = 0.2;
pub const MEDIUM_VOLATILITY_RISK: f64 = 0.1;
pub const LOW_VOLATILITY_RISK: f64 = 0.05;

// ============================================================================
// MARKET ASSESSMENT - Volume
// ============================================================================

pub const LOW_VOLUME: f64 = 100_000.0; // $100k
pub const MEDIUM_VOLUME: f64 = 1_000_000.0; // $1M

pub const LOW_VOLUME_RISK: f64 = 0.2;
pub const MEDIUM_VOLUME_RISK: f64 = 0.1;
pub const HIGH_VOLUME_RISK: f64 = 0.05;
