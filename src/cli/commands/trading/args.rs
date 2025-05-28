use clap::{ArgAction, Subcommand};

#[derive(Subcommand, Debug)]
pub enum TradingArgs {
    /// Start the trading bot
    Start {
        /// Trading strategy to use (e.g., momentum)
        #[arg(short, long, default_value = "momentum")]
        strategy: String,
        /// Maximum position size in USD
        #[arg(short, long, default_value_t = 100.0)]
        max_position: f64,
        /// Maximum total exposure in USD
        #[arg(short = 'e', long, default_value_t = 500.0)]
        max_exposure: f64,
        /// Confidence threshold for strategy signals (0.0-1.0)
        #[arg(long, default_value_t = 5.0)]
        confidence_threshold: f64,
        /// Minimum 24h volume in USD
        #[arg(long, default_value_t = 100000.0)]
        min_volume: f64,
        /// Maximum loss per position (%)
        #[arg(long, default_value_t = 5.0)]
        stop_loss: f64,
        /// Run in paper trading mode (no real trades)
        #[arg(long, action = ArgAction::SetTrue)]
        paper: bool,
        /// Network to use (goerli, sepolia, mumbai, mainnet, polygon)
        #[arg(long, default_value = "goerli")]
        network: String,
        /// Market scan interval in seconds
        #[arg(short, long, default_value_t = 60)]
        interval: u64,
        /// Run in the background (daemon mode)
        #[arg(short, long, action = ArgAction::SetTrue)]
        background: bool,
        /// Enable wide scan mode to process all available tokens
        #[arg(long, action = ArgAction::SetTrue)]
        wide_scan: bool,
        /// Minimum data points required for analysis
        #[arg(short = 'p', long, default_value_t = 7)]
        min_data_points: u32,
        /// Risk tolerance level (0-5): 0=Conservative, 1=Conservative-Moderate, 2=Moderate, 3=Moderate-Aggressive, 4=Aggressive, 5=Very Aggressive
        #[arg(short = 'r', long, default_value_t = 0)]
        risk_tolerance: u8,
        /// RSI indicator weight (0-1)
        #[arg(long, default_value_t = 0.3)]
        rsi_weight: f64,
        /// MACD indicator weight (0-1)
        #[arg(long, default_value_t = 0.3)]
        macd_weight: f64,
        /// Bollinger Bands indicator weight (0-1)
        #[arg(long, default_value_t = 0.2)]
        bollinger_weight: f64,
        /// Volume trend indicator weight (0-1)
        #[arg(long, default_value_t = 0.2)]
        volume_weight: f64,
        /// Testing mode for faster signal generation (production, fast, ultra, mock)
        #[arg(long, default_value = "production")]
        testing_mode: String,
    },
    /// Show current bot status and positions
    Status,
    /// Get health report from the supervisor
    Health,
    /// Restart a specific actor
    Restart {
        /// Actor ID to restart (market, strategy, risk, execution, database)
        actor_id: String,
    },
    /// Stop the trading bot
    Stop,
    /// View trading history and performance
    History {
        /// Number of trades to display
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
        /// Show only paper trading history
        #[arg(long, action = ArgAction::SetTrue)]
        paper: bool,
        /// Show only live trading history
        #[arg(long, action = ArgAction::SetTrue)]
        live: bool,
    },
    /// View current open positions
    Positions {
        /// Show only paper trading positions
        #[arg(long, action = ArgAction::SetTrue)]
        paper: bool,
        /// Show only live trading positions
        #[arg(long, action = ArgAction::SetTrue)]
        live: bool,
    },
    /// Manually close an open position
    Close {
        /// Token ID of the position to close
        #[arg(short, long)]
        token: String,
        /// Exit price for the position
        #[arg(short, long)]
        price: f64,
        /// Close a paper trading position (default) or live position
        #[arg(long, action = ArgAction::SetTrue)]
        paper: bool,
        /// Close a live trading position
        #[arg(long, action = ArgAction::SetTrue)]
        live: bool,
    },
    /// Manually open a new position
    Open {
        /// Token ID to buy
        #[arg(short, long)]
        token: String,
        /// USD amount to spend on the position
        #[arg(short, long)]
        amount: f64,
        /// Maximum price per token (optional price limit)
        #[arg(short, long)]
        price: Option<f64>,
        /// Open a paper trading position (default) or live position
        #[arg(long, action = ArgAction::SetTrue)]
        paper: bool,
        /// Open a live trading position
        #[arg(long, action = ArgAction::SetTrue)]
        live: bool,
    },
}

/// Helper function to determine if a network is a testnet
pub fn is_testnet_network(network: &str) -> bool {
    matches!(
        network.to_lowercase().as_str(),
        "goerli" | "sepolia" | "mumbai"
    )
}
