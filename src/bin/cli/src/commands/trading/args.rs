use clap::{ArgAction, Args, Subcommand};

/// Arguments for the `trading start` command
#[derive(Args, Debug)]
pub struct StartArgs {
    /// Trading strategy to use (e.g., momentum)
    #[arg(short, long, default_value = "momentum")]
    pub strategy: String,
    /// Maximum position size in USD
    #[arg(short, long)]
    pub max_position: Option<f64>,
    /// Maximum total exposure in USD
    #[arg(short = 'e', long)]
    pub max_exposure: Option<f64>,
    /// Actor confidence threshold for signal filtering (0.0-1.0)
    #[arg(long)]
    pub confidence_threshold: Option<f64>,
    /// Momentum strategy: momentum score threshold (0.0-100.0)
    #[arg(long)]
    pub momentum_threshold: Option<f64>,
    /// Minimum 24h volume in USD
    #[arg(long)]
    pub min_volume: Option<f64>,
    /// Minimum liquidity required for trading pairs in USD
    #[arg(long)]
    pub min_liquidity: Option<f64>,
    /// Minimum transaction count required for V3 pools
    #[arg(long)]
    pub min_pool_transaction_count: Option<u32>,
    /// Maximum loss per position (%)
    #[arg(long)]
    pub stop_loss: Option<f64>,
    /// Run in live trading mode (real money - default is paper trading for safety)
    #[arg(long, action = ArgAction::SetTrue)]
    pub live: bool,
    /// Network to use (amoy, mainnet, polygon, optimism, arbitrum)
    #[arg(long, value_name = "NETWORK")]
    pub network: Option<String>,
    /// Market scan interval in seconds
    #[arg(short, long)]
    pub interval: Option<u64>,
    /// Run in the background (daemon mode)
    #[arg(short, long, action = ArgAction::SetTrue)]
    pub background: bool,
    /// Maximum number of tokens to scan (0 = unlimited, recommended: 100-200)
    #[arg(long)]
    pub max_tokens_to_scan: Option<usize>,
    /// Indicator profile preset: optimizes indicator periods for your scan interval
    /// Options: scalping, day_trading (recommended for 60s intervals), swing_trading, standard
    #[arg(long, value_parser = ["scalping", "day_trading", "swing_trading", "standard"])]
    pub indicator_profile: Option<String>,
    /// RSI indicator weight (0-1)
    #[arg(long)]
    pub rsi_weight: Option<f64>,
    /// MACD indicator weight (0-1)
    #[arg(long)]
    pub macd_weight: Option<f64>,
    /// Bollinger Bands indicator weight (0-1)
    #[arg(long)]
    pub bollinger_weight: Option<f64>,
    /// Volume trend indicator weight (0-1)
    #[arg(long)]
    pub volume_weight: Option<f64>,
    /// Market data provider (dexscreener_solana)
    #[arg(long, value_parser = ["dexscreener_solana"])]
    pub market_data_provider: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum TradingArgs {
    /// Start the trading bot with specified strategy and parameters
    ///
    /// Examples:
    ///   mantis trading start --strategy momentum
    ///   mantis trading start --max-position 500 --max-exposure 2000
    ///   mantis trading start --live (live trading mode with real money)
    ///   mantis trading start --background (run as daemon in paper trading mode)
    Start(Box<StartArgs>),
    /// Show current bot status, active positions, and performance metrics
    ///
    /// Example: mantis trading status
    Status,

    /// Get detailed health report from all system actors
    ///
    /// Example: mantis trading health
    Health,

    /// Restart a specific system actor (for recovery from errors)
    ///
    /// Example: mantis trading restart market
    Restart {
        /// Actor ID to restart (market, strategy, risk, execution, database)
        actor_id: String,
    },

    /// Stop the trading bot gracefully
    ///
    /// Example: mantis trading stop
    Stop,

    /// View trading history with performance statistics
    ///
    /// Examples:
    ///   mantis trading history (shows paper trades by default)
    ///   mantis trading history --limit 20
    ///   mantis trading history --live (live trades only)
    History {
        /// Number of trades to display
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
        /// Show paper trading history (default, can be omitted)
        #[arg(long, action = ArgAction::SetTrue)]
        paper: bool,
        /// Show live trading history instead of paper
        #[arg(long, action = ArgAction::SetTrue)]
        live: bool,
    },
    /// View current open positions (defaults to paper trading)
    Positions {
        /// Show paper trading positions (default, can be omitted)
        #[arg(long, action = ArgAction::SetTrue)]
        paper: bool,
        /// Show live trading positions instead of paper
        #[arg(long, action = ArgAction::SetTrue)]
        live: bool,
        /// Number of closed positions to display (default: 10)
        #[arg(long, default_value_t = 10)]
        closed_limit: usize,
    },
    /// View DEX transaction logs and sync with blockchain
    Transactions {
        /// Number of transactions to display
        #[arg(short, long, default_value_t = 20)]
        limit: usize,
        /// Check blockchain status and update database
        #[arg(long, action = ArgAction::SetTrue)]
        sync: bool,
        /// Show only failed transactions
        #[arg(long, action = ArgAction::SetTrue)]
        failed: bool,
        /// Show only pending transactions
        #[arg(long, action = ArgAction::SetTrue)]
        pending: bool,
        /// Show only confirmed transactions
        #[arg(long, action = ArgAction::SetTrue)]
        confirmed: bool,
    },
}
