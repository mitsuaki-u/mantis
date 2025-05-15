# HoneyBadger - Crypto Trading & Analysis Tool

HoneyBadger is a powerful command-line tool for cryptocurrency market analysis, DEX interaction, wallet tracking, and automated trading.

## Installation

```bash
cargo install honeybadger
```

## Configuration

HoneyBadger offers a robust configuration system with support for config files, environment variables, and command-line arguments.

### Basic Setup

Set up your API keys for various services:

```bash
honeybadger config set-key coingecko YOUR_API_KEY
honeybadger config set-key cryptocompare YOUR_API_KEY
honeybadger config set-key etherscan YOUR_API_KEY
```

View your current configuration:

```bash
honeybadger config show
```

For sensitive information display:

```bash
honeybadger config show --show-secrets
```

Get JSON output format:

```bash
honeybadger config show --json
```

### Advanced Configuration

View configuration file location:

```bash
honeybadger config path
```

Get specific setting value:

```bash
honeybadger config get trading.paper_trading
honeybadger config get api_keys.coingecko
```

Change individual settings:

```bash
honeybadger config set trading.paper_trading true
honeybadger config set trading.strategy.threshold 5.0
```

Configure trading settings in bulk:

```bash
honeybadger config set-trading --paper-trading true --max-position 200 --max-exposure 1000
```

Configure strategy parameters:

```bash
honeybadger config set-strategy --strategy-type momentum --threshold 6.0 --min-volume 20000
```

Set risk management parameters:

```bash
honeybadger config set-risk --stop-loss 3.0 --take-profit 9.0 --max-positions 5
```

Configure tokens to track for market data:

```bash
honeybadger config set trading.tokens_to_track '["bitcoin", "ethereum", "solana"]'
```

Reset to default configuration:

```bash
honeybadger config reset --force
```

For a complete configuration guide, see [CONFIG.md](CONFIG.md).

### Logging Configuration

HoneyBadger provides flexible logging options to help with monitoring and debugging.

Control log verbosity with different log levels:

```bash
honeybadger trading start --log-level debug
honeybadger trading start --log-level trace  # Most verbose
honeybadger trading start --log-level error  # Only show errors
```

Save logs to a file while still displaying in the console:

```bash
honeybadger trading start --log-file trading.log
```

Focus on specific modules for detailed debugging:

```bash
honeybadger trading start --log-modules "honeybadger::trading,honeybadger::api" --log-level debug
```

Test logging configuration:

```bash
honeybadger trading start --log-level debug --log-modules "honeybadger::trading" --log-file test.log
```

You can combine these flags with any command:

```bash
honeybadger market overview --log-level info --log-file market.log
honeybadger trading analyze --log-level debug --log-modules "honeybadger::trading::strategy" --log-file strategy.log
```

> **New Feature**: When you use `--log-level` or `--debug` without specifying a log file, HoneyBadger now automatically creates a timestamped log file in your configured logs directory (e.g., `trading_20240526_123045.log`).

#### Default Log File Locations

By default, log files are created in the following platform-specific directories:

- **Linux**: `~/.local/share/honeybadger/logs/`
- **macOS**: `~/Library/Application Support/honeybadger/logs/`
- **Windows**: `C:\Users\[username]\AppData\Roaming\honeybadger\logs\`

You can customize the logs directory using:

```bash
honeybadger config setlogs /path/to/custom/logs
```

When using the `--log-file` option:
- If you specify a relative path (e.g., `--log-file trading.log`), it will be created in the configured logs directory
- If you specify an absolute path (e.g., `--log-file /tmp/trading.log`), it will be created at that exact location

## Features

### Market Analysis

Get an overview of the crypto market:

```bash
honeybadger market overview --limit 10
```

Example output:
```
╭────────────────────────────────────────────────────────────────────╮
│                        Cryptocurrency Markets                       │
╰────────────────────────────────────────────────────────────────────╯
┌────────┬────────────┬───────────┬───────────┬─────────┬───────────┐
│ Symbol │ Name       │ Price     │ 24h Chg % │ 7d Chg% │ Mkt Cap   │
├────────┼────────────┼───────────┼───────────┼─────────┼───────────┤
│ BTC    │ Bitcoin    │ $51,207   │ +4.2%     │ +12.8%  │ $1.01T    │
│ ETH    │ Ethereum   │ $3,048    │ +6.3%     │ +20.5%  │ $366.1B   │
│ BNB    │ BNB        │ $389.70   │ +3.9%     │ +14.9%  │ $60.8B    │
│ SOL    │ Solana     │ $150.78   │ +10.6%    │ +46.2%  │ $60.3B    │
│ XRP    │ XRP        │ $0.48     │ +2.7%     │ +4.7%   │ $25.2B    │
└────────┴────────────┴───────────┴───────────┴─────────┴───────────┘
```

View trending cryptocurrencies:

```bash
honeybadger market trending --limit 20
```

Show top gainers:

```bash
honeybadger market gainers --limit 15
```

Show top losers:

```bash
honeybadger market losers --limit 15
```

Filter by market cap:

```bash
honeybadger market gainers --limit 15 --min-cap 1000000
```

Apply multiple filters:

```bash
honeybadger market gainers --limit 20 --min-cap 10000000 --max-cap 1000000000
```

### Wallet Analysis

Get detailed wallet information:

```bash
honeybadger wallet info 0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045 --chain ethereum
```

Default to Ethereum chain:

```bash
honeybadger wallet info 0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045
```

### DEX Interaction & Trading

HoneyBadger provides robust DEX (Decentralized Exchange) integration capabilities for both analysis and trading execution.

#### DEX Information Queries

View token/pair information on DEXs:

```bash
honeybadger dex pair 0x6b175474e89094c44da98b954eedeac495271d0f ethereum
```

Get DEX statistics:

```bash
honeybadger dex stats raydium solana
```

#### DEX Trading Modes

HoneyBadger supports three trading modes for DEX interaction:

1. **Paper Trading**: Simulates trades without executing real transactions
   ```bash
   honeybadger trading start --strategy momentum --paper
   ```

2. **Testnet Trading**: Executes real transactions on Ethereum testnets
   ```bash
   honeybadger trading start --strategy momentum --testnet
   ```

3. **Live Trading**: (Placeholder for future implementation)

#### Supported Testnets

When using testnet mode, HoneyBadger supports:
- **Goerli** (Ethereum testnet) - Default
- **Mumbai** (Polygon testnet)

#### DEX Wallet Configuration

For testnet or live trading, a wallet configuration is required:

```bash
# Using environment variable
export HONEYBADGER_DEX_WALLET_PRIVATE_KEY_ENV=MY_PRIVATE_KEY_VAR
export MY_PRIVATE_KEY_VAR=0xYourPrivateKeyHere

# Or using a file
export HONEYBADGER_DEX_WALLET_PRIVATE_KEY_FILE=/path/to/your/key.txt
```

#### DEX Network Configuration

```bash
# Enable testnet mode
export HONEYBADGER_DEX_TESTNET=true

# Optionally specify network (defaults to Goerli)
export HONEYBADGER_DEX_NETWORK=goerli  # or mumbai

# For better performance with testnets
export HONEYBADGER_INFURA_KEY=your_infura_key_here
```

#### Supported DEX Protocols

HoneyBadger is compatible with:
- Uniswap V2 (for both Goerli and Mumbai testnets)
- More protocols planned for future releases

#### Token Balance Checking

Get your wallet's token balance on the testnet:

```bash
honeybadger wallet balance --testnet
```

Get balance for a specific token:

```bash
honeybadger wallet balance --testnet --token 0x07865c6e87b9f70255377e024ace6630c1eaa37f
```

For additional details on DEX trading, see [TESTNET_TRADING.md](TESTNET_TRADING.md).

### Trading Bot

The trading bot allows automated trading based on various strategies with customizable parameters. HoneyBadger implements an actor-based architecture for robust, concurrent operation and improved error handling.

Basic usage with paper trading (no real trades):

```bash
honeybadger trading start --strategy momentum --paper
```

#### Testnet Trading

HoneyBadger now supports real trading on Ethereum testnets, allowing you to test with real blockchain transactions without risking real funds:

```bash
honeybadger trading start --strategy momentum --testnet
```

This executes real trades on Ethereum test networks (Goerli or Mumbai). For comprehensive DEX integration details, see the [DEX_SUPPORT.md](DEX_SUPPORT.md) guide alongside [TESTNET_TRADING.md](TESTNET_TRADING.md).

#### Wide Scan Mode

HoneyBadger supports "wide scan mode" for processing all tokens returned by market data APIs, not just those in the tracking list:

```bash
# Enable wide scan mode for the trading bot
honeybadger trading start --wide-scan

# Combine with other options
honeybadger trading start --strategy momentum --wide-scan --paper
```

You can also enable it through configuration:

```bash
# Enable via config command
honeybadger config set market_data.wide_scan_mode true

# Or with JSON configuration
honeybadger config set '{"market_data": {"wide_scan_mode": true}}'
```

When wide scan mode is enabled:
- All tokens from the API are processed (potentially hundreds)
- Trading signals are generated for all matching tokens
- Position tracking and risk management apply to all tokens
- Performance may be affected due to increased data processing

This is useful for:
- Discovering trading opportunities across the entire market
- Back-testing strategies with a broader range of tokens
- Analyzing market-wide patterns and correlations

Customize trading parameters:

```bash
# Start trading with custom parameters
honeybadger trading start --strategy momentum --max-position 100 --max-exposure 500 --threshold 5 --min-volume 100000 --stop-loss 5 --paper --interval 60 --testing-mode production
```

### Data Requirements

The trading bot uses several technical indicators that require different amounts of price data points:

- MACD: Requires the most data points (26 + 9 + buffer = ~40 points)
- RSI: Requires 14 data points plus 1 for price change
- Bollinger Bands: Requires 20 data points
- Volume Trend: Requires 14 data points

The bot will accumulate price data points over time. You'll see progress logs indicating how many points have been collected. Full technical analysis will begin once sufficient data is available.

You can adjust the minimum required points with:
```bash
honeybadger trading start --min-data-points <value>
```

Note that while you can set a lower value for faster signals, this may reduce accuracy. The bot will still wait for enough points for accurate MACD calculation in production mode.

### Trading Parameters

You can customize various trading parameters when starting the bot:

```bash
# Start trading with custom parameters
honeybadger trading start --strategy momentum --max-position 100 --max-exposure 500 --threshold 5 --min-volume 100000 --stop-loss 5 --paper --interval 60 --testing-mode production
```

### Testing Modes

The `--testing-mode` option allows you to run the trading system with different analysis speeds:

- `production`: Normal mode with full accuracy (default)
- `fast`: Faster testing with reduced analysis periods (half of production)
- `ultra`: Ultra-fast testing with minimal periods for rapid testing
- `mock`: Generates artificial buy/sell signals without doing any technical analysis

The mock testing mode is particularly useful for testing the entire trading system without waiting for actual market conditions to trigger signals. It randomly generates buy and sell signals based on configured probabilities and durations, allowing you to validate the full order execution flow.

#### Indicator Periods by Mode

Each testing mode uses different periods for technical indicators:

| Indicator | Production | Fast | Ultra | Mock |
|-----------|------------|------|-------|------|
| RSI | 14 | 7 | 3 | 0 |
| MACD Fast | 12 | 6 | 3 | 0 |
| MACD Slow | 26 | 13 | 7 | 0 |
| MACD Signal | 9 | 4 | 2 | 0 |
| Bollinger | 20 | 10 | 5 | 0 |
| Volume | 20 | 10 | 5 | 0 |

Note: The mock mode doesn't use technical indicators since it generates signals artificially.

### Risk Tolerance Levels

HoneyBadger supports 6 risk tolerance levels (0-5) that affect trading behavior:

- **0 (Conservative)**: Standard analysis with tight risk controls
- **1 (Conservative-Moderate)**: Some flexibility in entry/exit conditions
- **2 (Moderate)**: Balanced approach with more trading signals
- **3 (Moderate-Aggressive)**: More aggressive entry conditions
- **4 (Aggressive)**: Maximum trading signals with wider stops
- **5 (Very Aggressive)**: Maximum trading signals with widest stops

### Indicator Weights

The trading strategy uses a weighted combination of technical indicators:

- **RSI (Relative Strength Index)**: Measures overbought/oversold conditions
- **MACD (Moving Average Convergence Divergence)**: Identifies trend changes
- **Bollinger Bands**: Measures volatility and price levels
- **Volume**: Confirms price movements with trading activity

Each indicator's weight (0-1) determines its influence on trading decisions. The weights should sum to 1.0.

### Minimum Data Points

The `--min-data-points` parameter (default: 7) determines how many historical price points are required before making trading decisions. This helps ensure sufficient data for accurate analysis.

Check bot status:

```bash
honeybadger trading status
```

Stop the trading bot:

```bash
honeybadger trading stop
```

View trading history:

```bash
honeybadger trading history --limit 20
```

Check open positions:

```bash
honeybadger trading positions
```

View detailed analysis information:

```bash
honeybadger trading analyze --debug
```

## Strategy Examples

### Conservative Strategy
Low risk, small position sizes, tight stop-loss:

```bash
honeybadger trading start --strategy momentum --threshold 4.0 --stop-loss 1.5 --paper
```

### Mid-Cap Hunter
Focus on moderate market cap coins with decent volume:

```bash
honeybadger market gainers --limit 20 --min-cap 1000000 --max-cap 100000000
honeybadger trading start --strategy momentum --threshold 8.0 --min-volume 500000 --max-position 200 --paper
```

### Aggressive Strategy
Higher threshold for entry but larger position sizes and wider stops:

```bash
honeybadger trading start \
  --strategy momentum \
  --threshold 12.0 \
  --max-position 300 \
  --max-exposure 1500 \
  --stop-loss 10.0 \
  --interval 30 \
  --paper
```

## Additional Features

### Actor-Based Architecture

HoneyBadger implements an actor-based system for enhanced reliability and concurrency:

- **MarketDataActor**: Handles market data collection and real-time price updates
- **StrategyActor**: Implements trading strategies and generates signals, using configuration for indicator weights and risk tolerance
- **RiskManagerActor**: Manages risk parameters and position sizing based on configuration
- **ExecutionActor**: Handles order execution and interaction with exchanges, using configuration for paper/live trading mode
- **DatabaseActor**: Manages persistent storage of market data and trades

Each actor maintains its own configuration state, allowing for:
- Dynamic parameter updates during runtime
- Independent configuration for different trading modes
- Flexible strategy adjustments without restarting
- Consistent configuration across the system

This architecture provides:
- Improved error isolation and recovery
- Better concurrent performance
- Clear separation of concerns
- More robust message passing
- Configuration-driven behavior

### Supervisor System

HoneyBadger implements a robust Supervisor system that monitors and manages all actors within the trading bot:

- **Health Monitoring**: Continuous monitoring of all actor components with detailed metrics
- **Fault Tolerance**: Automatic detection and recovery from actor failures
- **Actor Lifecycle Management**: Centralized control for starting, stopping, and restarting actors
- **Health Reporting**: Comprehensive health and status information for the entire system
- **Event Subscription Management**: Tracks and manages event subscriptions between actors
- **Configuration Synchronization**: Ensures consistent configuration across all actors

The Supervisor system provides commands for monitoring and managing the trading system:
```bash
# View detailed health report for all actors
honeybadger trading health

# Restart a specific actor that may be in a problematic state
honeybadger trading restart market

# View event subscription status
honeybadger trading subscriptions

# Check actor configuration
honeybadger trading config
```

The supervisor maintains detailed metrics including:
- Actor uptime and status
- Message processing rates
- Error rates and types
- Event subscription counts
- Configuration state

For comprehensive details on the Supervisor system, see [SUPERVISOR_SYSTEM.md](SUPERVISOR_SYSTEM.md).

### Data Collection

The bot automatically collects market data to improve trading decisions. This can be configured using:

```bash
honeybadger config set data_collection.interval 600  # 10 minute intervals
honeybadger config set data_collection.auto_start false  # Disable auto-start
```

### Database Configuration

Configure the database settings:

```bash
honeybadger config set-database path "/custom/path/to/database.db"
honeybadger config set-database Logging true  # Enable SQL query logging
```

## Custom Trading Strategies

HoneyBadger has a flexible, trait-based strategy pattern that allows you to create and implement your own trading strategies.

### Strategy Interface

All strategies implement the `TradingStrategy` trait:

```rust
pub trait TradingStrategy: fmt::Display + Send + Sync + 'static {
    fn name(&self) -> &str;
    fn analyze(&self, token: &TokenMetrics) -> Signal;
    fn should_exit(&self, position: &Position) -> bool;
    fn update_market_data(&mut self, token: &TokenMetrics);
    fn box_clone(&self) -> Box<dyn TradingStrategy>;
}
```

### Available Strategies

The bot comes with these built-in strategies:

1. **Momentum Strategy**: Analyzes price momentum using RSI, MACD, Bollinger Bands, and volume indicators

### Creating Your Own Strategy

To implement a custom strategy:

1. Create a new struct that holds your strategy's state
2. Implement the `TradingStrategy` trait for your struct
3. Add your strategy to the `create_strategy` factory function

Example for an RSI-based strategy:

```rust
#[derive(Clone)]
pub struct RSIStrategy {
    threshold: f64,
    min_volume: f64, 
    stop_loss_pct: f64,
    price_data: Arc<Mutex<HashMap<String, PriceTimeSeries>>>,
    overbought_level: f64,
    oversold_level: f64,
}

impl TradingStrategy for RSIStrategy {
    fn name(&self) -> &str {
        "rsi_reversal"
    }
    
    fn analyze(&self, token: &TokenMetrics) -> Signal {
        // Your RSI-based analysis logic here
        Signal::None
    }
    
    // Implement other required methods...
    
    fn box_clone(&self) -> Box<dyn TradingStrategy> {
        Box::new(self.clone())
    }
}
```

Then add it to the strategy factory:

```rust
pub fn create_strategy(strategy_name: &str, ...) -> Result<Strategy, Error> {
    match strategy_name {
        "momentum" => Ok(Strategy::new(Box::new(MomentumStrategy::new(...)))),
        "rsi" => Ok(Strategy::new(Box::new(RSIStrategy::new(...)))),
        _ => Err(Error::Config(format!("Unknown strategy: {}", strategy_name))),
    }
}
```

## Error Handling

HoneyBadger implements a comprehensive error handling system:

- **Task Errors**: For errors related to async task execution and message passing
- **API Errors**: For errors related to external API calls
- **Database Errors**: For errors related to data storage and retrieval
- **Config Errors**: For errors related to configuration loading and parsing

The system provides detailed error context and recovery mechanisms to ensure the trading bot can continue operating even when individual components encounter issues.

## Concurrency and Deadlock Prevention

HoneyBadger implements a strict lock hierarchy to prevent deadlocks in concurrent operations. This is especially important for trading bots running with multiple strategies and managing shared data.

Key principles:

1. Locks are always acquired in a specific order
2. Time-limited lock acquisition prevents hanging
3. Lock duration is minimized to reduce contention

For detailed guidelines on concurrent programming in the bot, see [DEADLOCK_PREVENTION.md](DEADLOCK_PREVENTION.md).

## Performance Considerations

HoneyBadger is designed to handle concurrent operations efficiently, but there are important performance considerations:

1. **Lock contention** - Heavy reliance on mutexes can impact performance under load
2. **Read vs. write access** - Many operations are read-heavy and can benefit from RwLock
3. **Batch processing** - Grouping operations reduces lock acquisition frequency

See [PERFORMANCE_OPTIMIZATIONS.md](PERFORMANCE_OPTIMIZATIONS.md) for detailed guidance on improving performance, including:
- Converting Mutex to RwLock for read-heavy operations
- Using lock-free data structures where appropriate
- Implementing parallel processing for CPU-intensive tasks
- Reducing lock scope to minimize contention

## Notes

- The bot runs continuously until stopped with `honeybadger trading stop`
- All trades are logged to a SQLite database for later analysis
- Configuration settings are saved automatically across sessions
- Global logging options available for all commands:
  - `--debug` for quick debug level logging
  - `--log-level [error|warn|info|debug|trace]` for specific log verbosity
  - `--log-file PATH` to save logs to a file
  - `--log-modules "module1,module2"` to focus logging on specific modules

# HoneyBadger Trading Bot

## Database

HoneyBadger stores trading data in a SQLite database for analysis and historical tracking. This guide explains how to access and query this data.

### Database Location

The SQLite database is stored in your user's data directory, which varies by operating system:
- **Linux**: `~/.local/share/honeybadger/trading_history.db`
- **macOS**: `~/Library/Application Support/com.honeybadger.honeybadger/trading_history.db`
- **Windows**: `%APPDATA%\honeybadger\trading_history.db`

You can set a custom location using:
```bash
honeybadger config set-database Path "/path/to/custom/database.db"
```

## Real-Time Market Data

HoneyBadger now supports real-time price streaming using WebSockets, which provides continuous market data without rate limiting issues.

```bash
# Stream real-time prices for bitcoin and ethereum using CoinCap WebSockets
honeybadger market stream --tokens bitcoin,ethereum

# Stream using Binance WebSockets with compact output
honeybadger market stream --tokens btc,eth --provider binance --compact

# Stream for 5 minutes and then exit
honeybadger market stream --tokens bitcoin,ethereum --duration 300

# Stream all available tokens with wide scan mode
honeybadger market stream --wide-scan
```

This feature leverages WebSocket connections to:
- Get real-time price updates without polling
- Reduce API rate limit issues
- Provide faster market data for trading decisions
- Support multiple data providers (CoinCap, Binance)
- Process all available tokens with wide scan mode

### WebSocket Trading Bot

To run the trading bot with WebSocket support for real-time data:

1. First, add your API keys to the configuration file:

```bash
honeybadger config set-key coincap YOUR_COINCAP_API_KEY
```

2. Start the trading bot with the same command you normally use. WebSockets will be automatically enabled if an API key for a supporting provider (like CoinCap) is configured:

```bash
# Start trading bot with WebSocket support
honeybadger trading start --paper --strategy momentum --wide-scan
```

The bot will:
- Automatically detect your CoinCap API key
- Establish a WebSocket connection for real-time price updates
- Maintain the connection and handle reconnections if needed
- Display WebSocket status in logs with 🔌 and 🟢 indicators

You can verify WebSocket is active by looking for these log messages:
```
[INFO] Setting up WebSocket connection for real-time market data
[INFO] 🟢 Successfully connected to WebSocket for real-time market data
```

### Configuration

To use the WebSocket streaming features, add your API keys to the configuration file:

```json
{
  "api_keys": {
    "coincap": "your-coincap-api-key",
    "binance": "your-binance-api-key",
    "binance_secret": "your-binance-api-secret"
  },
  "market_data": {
    "wide_scan_mode": true  // Enable processing all tokens returned by the API
  }
}
```

## Command Reference

HoneyBadger offers the following command categories:

```bash
honeybadger market    # Market analysis commands
honeybadger dex       # DEX interaction commands
honeybadger wallet    # Wallet analysis commands
honeybadger config    # Configuration management commands
honeybadger trading   # Trading bot commands
```

For DEX-specific commands and detailed integration information, see [DEX_SUPPORT.md](DEX_SUPPORT.md).

For detailed help on any command:

```bash
honeybadger <command> --help
honeybadger market --help
honeybadger trading start --help
```

## License

This project is licensed under the MIT License - see the LICENSE file for details.