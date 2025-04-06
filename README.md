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

### DEX Interaction

View token/pair information on DEXs:

```bash
honeybadger dex pair 0x6b175474e89094c44da98b954eedeac495271d0f ethereum
```

Get DEX statistics:

```bash
honeybadger dex stats raydium solana
```

### Trading Bot

The trading bot allows automated trading based on various strategies with customizable parameters. HoneyBadger implements an actor-based architecture for robust, concurrent operation and improved error handling.

Basic usage with paper trading (no real trades):

```bash
honeybadger trading start --strategy momentum --dry-run
```

Customize trading parameters:

```bash
honeybadger trading start \
  --strategy momentum \
  --threshold 5.0 \
  --max-position 100 \
  --max-exposure 500 \
  --min-volume 50000 \
  --stop-loss 5.0 \
  --interval 60 \
  --dry-run
```

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
honeybadger trading start --strategy momentum --threshold 4.0 --stop-loss 1.5 --dry-run
```

### Mid-Cap Hunter
Focus on moderate market cap coins with decent volume:

```bash
honeybadger market gainers --limit 20 --min-cap 1000000 --max-cap 100000000
honeybadger trading start --strategy momentum --threshold 8.0 --min-volume 500000 --max-position 200 --dry-run
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
  --dry-run
```

## Additional Features

### Actor-Based Architecture

HoneyBadger implements an actor-based system for enhanced reliability and concurrency:

- **MarketDataActor**: Handles market data collection and real-time price updates
- **StrategyActor**: Implements trading strategies and generates signals
- **RiskManagerActor**: Manages risk parameters and position sizing
- **ExecutionActor**: Handles order execution and interaction with exchanges
- **DatabaseActor**: Manages persistent storage of market data and trades

This architecture provides:
- Improved error isolation and recovery
- Better concurrent performance
- Clear separation of concerns
- More robust message passing

### Data Collection

The bot automatically collects market data to improve trading decisions. This can be configured using:

```bash
honeybadger config set data_collection.interval 600  # 10 minute intervals
honeybadger config set data_collection.auto_start false  # Disable auto-start
```

### Database Configuration

Configure the database settings:

```bash
honeybadger config set-database Path "/custom/path/to/database.db"
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
```

This feature leverages WebSocket connections to:
- Get real-time price updates without polling
- Reduce API rate limit issues
- Provide faster market data for trading decisions
- Support multiple data providers (CoinCap, Binance)

### Configuration

To use the WebSocket streaming features, add your API keys to the configuration file:

```json
{
  "api_keys": {
    "coincap": "your-coincap-api-key",
    "binance": "your-binance-api-key",
    "binance_secret": "your-binance-api-secret"
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

For detailed help on any command:

```bash
honeybadger <command> --help
honeybadger market --help
honeybadger trading start --help
```

## License

This project is licensed under the MIT License - see the LICENSE file for details.