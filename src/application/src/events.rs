use chrono::{DateTime, Utc};

// Import domain types needed for events
use crate::infrastructure::dex::{TransactionPriority, TransactionStatus};

/// Events that can be passed between actors
#[derive(Debug, Clone)]
pub enum Event {
    Market(MarketEvent),
    Strategy(StrategyEvent),
    AIAdvisor(AIAdvisorEvent),
    Risk(RiskEvent),
    Execution(ExecutionEvent),
    DexTransaction(Box<DexTransactionEvent>),
}

/// AI Advisor events — emitted after Claude analyses a signal
#[derive(Debug, Clone)]
pub enum AIAdvisorEvent {
    SignalAnalysed {
        token_id: String,
        signal: crate::core::domain::trading::Signal,
        approved: bool,
        confidence: u8,
        reasoning: String,
        metadata: crate::core::domain::trading::SignalMetadata,
    },
}

/// Market-related events
#[derive(Debug, Clone)]
pub enum MarketEvent {
    PriceUpdate {
        token_id: String,
        price: f64,
        volume: Option<f64>,
        symbol: String,
        name: String,
        decimals: u8,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Pool discovery events for cache population
    PoolsDiscovered {
        pools: Vec<PoolDiscoveryData>,
        source: String,
        discovery_mode: String,
        timestamp: DateTime<Utc>,
    },
    MarketAnomalyDetected {
        token_id: String,
        anomaly_type: String,
        description: String,
        severity: String,
        timestamp: DateTime<Utc>,
    },
    MarketDataError(String),
}

/// Pool discovery data for event transmission
#[derive(Debug, Clone)]
pub struct PoolDiscoveryData {
    pub pool_address: String,
    pub token0_address: String,
    pub token0_symbol: String,
    pub token0_name: String,
    pub token0_decimals: String,
    pub token1_address: String,
    pub token1_symbol: String,
    pub token1_name: String,
    pub token1_decimals: String,
    pub fee_tier: String,
    pub liquidity: String,
    pub sqrt_price: String,
    pub tick: Option<String>,
    pub tvl_usd: String,
    pub volume_24h_usd: String,
}

/// Signal metadata - re-export from domain
pub use crate::core::domain::trading::SignalMetadata;

/// Strategy-related events
#[derive(Debug, Clone)]
pub enum StrategyEvent {
    Signal {
        token_id: String,
        signal: crate::core::domain::trading::Signal,
        timestamp: chrono::DateTime<chrono::Utc>,
        metadata: SignalMetadata,
    },
}

/// Risk-related events
#[derive(Debug, Clone)]
pub enum RiskEvent {
    TradeApproved {
        token_id: String,
        signal: crate::core::domain::trading::Signal,
        position_size: f64,
        timestamp: chrono::DateTime<chrono::Utc>,
        signal_metadata: SignalMetadata,
    },
    PositionCreated {
        token_id: String,
        position_id: String,
        amount: f64,
        price: f64,
        timestamp: DateTime<Utc>,
    },
    PositionClosed {
        token_id: String,
        pnl: f64,
        timestamp: chrono::DateTime<chrono::Utc>,
        entry_price: f64,
        exit_price: f64,
        size: f64,
        entry_time: chrono::DateTime<chrono::Utc>,
        delete_position: bool,
    },
    RiskLimitExceeded {
        limit_type: String,
        current: f64,
        max: f64,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    InvalidSignalReceived {
        token_id: String,
        reason: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    InsufficientFunds {
        token_id: String,
        current_balance_usd: f64,
        required_balance_usd: f64,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    TradeSizeAdjusted {
        token_id: String,
        original_size_usd: f64,
        adjusted_size_usd: f64,
        reason: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    TradingHalted {
        token_id: String,
        reason: String,
        timestamp: DateTime<Utc>,
    },
}

/// Execution-related events
#[derive(Debug, Clone)]
pub enum ExecutionEvent {
    OrderExecuted {
        token_id: String,
        provider_token_id: String,
        signal: crate::core::domain::trading::Signal,
        executed_value_usd: f64,
        token_quantity: f64,
        price_per_token: f64,
        timestamp: chrono::DateTime<chrono::Utc>,
        // Fields needed for SELL (position close):
        entry_price: Option<f64>, // Original entry price (for SELL)
        entry_time: Option<chrono::DateTime<chrono::Utc>>, // Original entry time (for SELL)
        actual_fees: Option<f64>, // Transaction fees (for SELL)
        // Correlation ID for reservation cleanup
        correlation_id: Option<String>, // Signal correlation ID (for releasing position slot reservations)
    },
    OrderFailed {
        token_id: String,
        order_id: Option<String>,
        reason: String,
        timestamp: DateTime<Utc>,
        signal: crate::core::domain::trading::Signal, // Signal type to determine if reservation cleanup needed
        correlation_id: Option<String>, // Signal correlation ID (for releasing position slot reservations)
    },
    // PositionUpdate removed - position updates now handled via MarketEvent::PriceUpdate for real-time updates
}

/// DEX Transaction related events
#[derive(Debug, Clone)]
pub enum DexTransactionEvent {
    Submitted {
        tx_id: String,
        submitted_details: Option<SubmittedTransactionInfo>,
        submission_time: DateTime<Utc>,
        priority: TransactionPriority,
    },
    StatusUpdated {
        status: TransactionStatus,
        /// Complete transaction details including blockchain data (when available)
        /// Present for Success/Failed states when transaction receipt is available
        details: Option<Box<crate::infrastructure::dex::TransactionDetails>>,
    },
}

/// Supporting struct for initial submission details
#[derive(Debug, Clone)]
pub struct SubmittedTransactionInfo {
    pub from_token_address: String,
    pub to_token_address: String,
    pub amount_in_f64: f64,
    pub price_limit: Option<f64>,
    pub dex_name: String,
}

/// Event types for subscription
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum EventType {
    Market,
    Strategy,
    AIAdvisor,
    Risk,
    Execution,
    DexTransaction,
}
