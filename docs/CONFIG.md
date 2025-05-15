# Configuration System

## Overview

The HoneyBadger trading bot includes a comprehensive configuration system that supports multiple configuration sources with clear priority rules. This allows for flexible operation in different environments without modifying code.

## Configuration Sources

The system uses the following sources in descending order of priority:

1. **Command-Line Arguments** (highest priority)
2. **Environment Variables**
3. **Configuration File** (JSON)
4. **Default Values** (lowest priority)

## Configuration File

The default location for the configuration file is:

- **Linux**: `~/.config/honeybadger/config.json`
- **macOS**: `~/Library/Application Support/com.honeybadger.honeybadger/config.json`
- **Windows**: `%APPDATA%\honeybadger\config\config.json`

You can check your specific configuration file location with:

```bash
honeybadger config path
```

### Example Configuration File

```json
{
  "api_keys": {
    "cryptocompare": "your-api-key",
    "coingecko": "your-api-key",
    "etherscan": "your-api-key"
  },
  "trading": {
    "paper_trading": true,
    "max_position_size": 100.0,
    "max_total_exposure": 1000.0,
    "strategy": "momentum",
    "threshold": 0.5,
    "min_volume": 10000.0,
    "stop_loss": 5.0,
    "take_profit": 10.0,
    "max_positions": 5
  },
  "database": {
    "custom_path": null,
    "query_logging": false
  },
  "api": {
    "coingecko_url": "https://api.coingecko.com/api/v3",
    "request_timeout": 10,
    "max_retries": 3
  },
  "data_collection": {
    "interval": 300,
    "history_days": 30,
    "auto_start": true
  }
}
```

## Configuration Commands

HoneyBadger provides various commands for managing configuration:

### View Configuration

Show current configuration with sensitive values masked:
```bash
honeybadger config show
```

Show configuration including sensitive values:
```bash
honeybadger config show --show-secrets
```

Output configuration as JSON:
```bash
honeybadger config show --json
```

Show configuration file location:
```bash
honeybadger config path
```

### Get and Set Values

Get specific configuration value:
```bash
honeybadger config get trading.paper_trading
honeybadger config get api_keys.coingecko
```

Set specific configuration value:
```bash
honeybadger config set trading.paper_trading true
honeybadger config set trading.threshold 5.0
```

Set API keys:
```bash
honeybadger config set-key coingecko YOUR_API_KEY
```

### Bulk Configuration

Set multiple trading parameters at once:
```bash
honeybadger config set-trading --paper-trading true --max-position 200
```

Set multiple strategy parameters:
```bash
honeybadger config set-strategy --strategy-type momentum --threshold 6.0
```

Set multiple risk parameters:
```bash
honeybadger config set-risk --stop-loss 3.0 --take-profit 9.0
```

Set database configuration:
```bash
honeybadger config set-database Path "/custom/path/to/database.db"
honeybadger config set-database Logging true
```

Reset to defaults:
```bash
honeybadger config reset --force
```

## Environment Variables

All configuration options can be set via environment variables with the `HONEYBADGER_` prefix:

### API Keys
- `HONEYBADGER_COINGECKO_KEY` - CoinGecko API key
- `HONEYBADGER_CRYPTOCOMPARE_KEY` - CryptoCompare API key
- `HONEYBADGER_ETHERSCAN_KEY` - Etherscan API key

### Trading Configuration
- `HONEYBADGER_PAPER_TRADING` - Enable paper trading (true/false)
- `HONEYBADGER_SCAN_INTERVAL` - Market scan interval in seconds (sets data_collection.interval)
- `HONEYBADGER_MAX_POSITION` - Maximum position size in USD
- `HONEYBADGER_MAX_EXPOSURE` - Maximum total exposure in USD
- `HONEYBADGER_STRATEGY` - Strategy type (momentum, rsi, etc.)
- `HONEYBADGER_THRESHOLD` - Signal threshold
- `HONEYBADGER_MIN_VOLUME` - Minimum volume required for trading
- `HONEYBADGER_STOP_LOSS` - Stop loss percentage
- `HONEYBADGER_TAKE_PROFIT` - Take profit percentage
- `HONEYBADGER_MAX_POSITIONS` - Maximum number of positions

### Database Configuration
- `HONEYBADGER_DB_PATH` - Custom database path
- `HONEYBADGER_DB_LOGGING` - Enable SQL query logging (true/false)

### API Configuration
- `HONEYBADGER_COINGECKO_URL` - CoinGecko API base URL
- `HONEYBADGER_API_TIMEOUT` - API request timeout in seconds
- `HONEYBADGER_API_RETRIES` - Maximum API retry attempts

### Data Collection
- `HONEYBADGER_COLLECTION_INTERVAL` - Data collection interval in seconds
- `HONEYBADGER_HISTORY_DAYS` - Maximum history to maintain in days
- `HONEYBADGER_AUTO_COLLECT` - Automatically start data collection (true/false)

## Command-Line Arguments

The following global command-line arguments can be used to override configuration options:

```
--paper-trading           Enable paper trading (simulate trades)
--scan-interval <SECONDS> Market scan interval in seconds
--max-position <USD>      Maximum position size in USD
--max-exposure <USD>      Maximum total exposure in USD
--strategy <TYPE>         Strategy type (momentum, rsi, macd)
--threshold <VALUE>       Strategy signal threshold
--min-volume <USD>        Minimum volume required for trading
--stop-loss <PERCENT>     Stop loss percentage
--take-profit <PERCENT>   Take profit percentage
--max-positions <COUNT>   Maximum number of positions
--coingecko-key <KEY>     CoinGecko API key
--debug                   Enable debug logging
```

These arguments can be used with any command and will override any existing configuration for the current run.

## Default Values

The system uses the following default values if no other configuration is provided:

- **Paper Trading**: `true` (safety first)
- **Scan Interval**: `300` seconds (5 minutes)
- **Max Position Size**: `$100`
- **Max Total Exposure**: `$1,000`
- **Strategy Type**: `momentum`
- **Signal Threshold**: `0.5`
- **Minimum Volume**: `$10,000`
- **Stop Loss**: `5%`
- **Take Profit**: `10%`
- **Max Positions**: `5`
- **API Timeout**: `10` seconds
- **API Retries**: `3`
- **Data Collection Interval**: `300` seconds (5 minutes)
- **History Days**: `30`
- **Auto-start Collection**: `true`

## Using in Code

To use the configuration in your code:

```rust
use crate::config::Config;

fn my_function() -> Result<()> {
    // Load configuration from all sources
    let config = Config::load()?;
    
    // Access configuration values
    let is_paper_trading = config.trading.paper_trading;
    let scan_interval = config.data_collection.interval;
    
    // Use API keys
    if let Some(api_key) = &config.api_keys.coingecko {
        // Use the API key
    }
    
    // Get database path
    let db_path = config.db_path()?;
    
    Ok(())
}
```

## Implementation Details

The configuration system is implemented in `src/config.rs` with the following key components:

1. **Config Struct**: The main configuration structure that holds all settings
2. **Load Method**: Loads configuration from all sources in priority order
3. **Environment Variables**: Handled in the `load_from_env` method
4. **Command-Line Arguments**: Processed in `main.rs` using the `apply_cli_config` function
5. **File Storage**: Configuration is saved to and loaded from a JSON file

Configuration is applied in this order, with each level overriding the previous:
1. Default values
2. Configuration file
3. Environment variables
4. Command-line arguments 