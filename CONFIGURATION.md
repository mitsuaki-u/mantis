# Configuration Guide

This document describes all configuration options for the Mantis trading bot.

## Configuration File Location

- **Linux/macOS**: `~/.config/mantis/config.json`
- **Windows**: `%APPDATA%\mantis\config.json`

## Configuration Priority

Configuration values are loaded in the following priority order (highest to lowest):

1. **Environment variables** (highest priority)
2. **Configuration file**
3. **Default values** (lowest priority)

## Environment Variables

### API Keys

| Variable | Description | Example |
|----------|-------------|---------|
| `MANTIS_INFURA_KEY` | Infura RPC API key | `1234567890abcdef...` |
| `MANTIS_ALCHEMY_KEY` | Alchemy RPC API key | `abcdef1234567890...` |

### Cache

| Variable | Description | Example |
|----------|-------------|---------|
| `REDIS_URL` | Redis connection URL | `redis://localhost:6379` |

### Wallet (for live trading)

Configure wallet via the config file `dex.wallet` section (see below).

## Configuration File Structure

```json
{
  "api_keys": { ... },
  "trading": { ... },
  "database": { ... },
  "api": { ... },
  "data_collection": { ... },
  "logs": { ... },
  "rpc": { ... },
  "dex": { ... },
  "cache": { ... }
}
```

---

## API Keys Configuration

```json
{
  "api_keys": {
    "infura": "your_infura_api_key",
    "alchemy": "your_alchemy_api_key"
  }
}
```

### CLI Commands

```bash
# Set Alchemy API key
mantis config set-key alchemy YOUR_ALCHEMY_KEY

# Set Infura API key
mantis config set-key infura YOUR_INFURA_KEY
```

**Note**: At least one RPC provider key (Alchemy or Infura) is required.

---

## Trading Configuration

Controls bot trading behavior, risk management, and strategy parameters.

```json
{
  "trading": {
    "live_trading": false,
    "max_position_size": 100.0,
    "min_position_size": 10.0,
    "max_total_exposure": 1000.0,
    "strategy": "momentum",
    "signal_confidence_threshold": 0.65,
    "indicator_profile": "day_trading",
    "min_volume": 50000.0,
    "min_liquidity": 100000.0,
    "min_pool_transaction_count": 500,
    "stop_loss": 5.0,
    "take_profit": 10.0,
    "max_positions": 5,
    "max_volatility_24h": 30.0,
    "rsi_weight": 0.3,
    "macd_weight": 0.3,
    "bollinger_weight": 0.25,
    "volume_weight": 0.15,
    "max_tokens_to_scan": 150,
    "max_daily_loss": 10.0,
    "max_drawdown": 20.0,
    "max_trade_risk_pct": 2.0,
    "min_eth_balance": 0.1,
    "tokens_to_track": [],
    "market_data_provider": "alchemy_uniswap_v3",
    "max_gas_cost_usd": 10.0,
    "max_gas_cost_percentage": 5.0,
    "transaction_priority": "Standard",
    "max_execution_price_deviation": 0.05,
    "min_portfolio_risk_factor": 0.3
  }
}
```

### Trading Options

#### Mode & Position Sizing

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `live_trading` | bool | `false` | Enable live trading with real funds. Leave `false` for paper trading (default). |
| `max_position_size` | float | `100.0` | Maximum USD value per trade position |
| `min_position_size` | float | `10.0` | Minimum USD value per trade position |
| `max_total_exposure` | float | `1000.0` | Maximum total USD value across all positions |
| `max_positions` | int | `5` | Maximum number of concurrent positions |

#### Strategy Selection

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `strategy` | string | `"momentum"` | Trading strategy: `"momentum"` or `"rsi"` |
| `signal_confidence_threshold` | float | `0.65` | Minimum confidence (0.0-1.0) to trigger a trade |

#### Indicator Configuration (for momentum strategy)

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `indicator_profile` | string | `"day_trading"` | Preset that auto-configures indicator periods for your trading style |

**Indicator Profile Presets:**

The `indicator_profile` option automatically optimizes all indicator periods (RSI, MACD, Bollinger Bands, Volume) based on your trading timeframe:

- `"scalping"` - Ultra-fast (5min scan interval)
  - MACD: (5, 13, 4) - 40 min warmup
  - Best for: High-frequency trading, 1-5 minute candles

- `"day_trading"` - Balanced (1min scan interval) **[DEFAULT]**
  - MACD: (8, 17, 6) - 50 min warmup
  - Best for: Active trading, 1-minute candles, 60s scan intervals

- `"swing_trading"` - Conservative (1-5min scan intervals)
  - MACD: (10, 20, 7) - 57 min warmup
  - Best for: Position trading, 5-15 minute candles

- `"standard"` - Traditional settings (5-15min scan intervals)
  - MACD: (12, 26, 9) - 71 min warmup
  - Best for: Slower timeframes, traditional TA

**Recommendation**: Use `"day_trading"` with 60s scan intervals for optimal momentum trading.

#### Indicator Weights (for momentum strategy)

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `rsi_weight` | float | `0.3` | RSI indicator weight (0.0-1.0) |
| `macd_weight` | float | `0.3` | MACD indicator weight (0.0-1.0) |
| `bollinger_weight` | float | `0.25` | Bollinger Bands weight (0.0-1.0) |
| `volume_weight` | float | `0.15` | Volume indicator weight (0.0-1.0) |

**Note**: Weights should sum to 1.0 for best results.

#### Market Filters

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `min_volume` | float | `50000.0` | Minimum 24h USD volume to consider a token |
| `min_liquidity` | float | `100000.0` | Minimum USD liquidity in trading pairs |
| `min_pool_transaction_count` | int | `500` | Minimum transaction count for Uniswap V3 pools |
| `max_tokens_to_scan` | int | `150` | Maximum tokens to scan per market update (0 = unlimited) |
| `tokens_to_track` | array | `[]` | Specific token addresses to track (empty = use defaults) |

#### Risk Management

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `stop_loss` | float | `5.0` | Stop loss percentage (e.g., `5.0` = -5%) |
| `take_profit` | float | `10.0` | Take profit percentage (e.g., `10.0` = +10%) |
| `max_volatility_24h` | float | `30.0` | Maximum allowed 24-hour price volatility percentage (0.0-100.0). Tokens exceeding this volatility will be skipped |
| `max_daily_loss` | float | `10.0` | Maximum daily loss percentage before halting |
| `max_drawdown` | float | `20.0` | Maximum drawdown percentage before halting |
| `max_trade_risk_pct` | float | `2.0` | Maximum percentage of wallet to risk per trade |
| `min_portfolio_risk_factor` | float | `0.3` | Minimum portfolio risk factor (0.0-1.0) before halting new trades |

#### Gas Protection

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `max_gas_cost_usd` | float | `10.0` | Maximum USD to spend on gas per transaction |
| `max_gas_cost_percentage` | float | `5.0` | Maximum gas cost as % of trade size |
| `transaction_priority` | string | `"Standard"` | Gas priority: `"Low"`, `"Standard"`, `"High"`, `"Urgent"` |

**Priority Multipliers**:
- `"Low"`: 0.9x base gas price
- `"Standard"`: 1.0x base gas price
- `"High"`: 1.2x base gas price
- `"Urgent"`: 1.5x base gas price

#### Price Validation

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `max_execution_price_deviation` | float | `0.05` | Maximum price deviation from signal to execution (e.g., `0.05` = 5%) |

#### Live Trading Requirements

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `min_eth_balance` | float | `0.1` | Minimum ETH balance required for gas fees |

#### Market Data Provider

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `market_data_provider` | string | `"alchemy_uniswap_v3"` | Data provider: `"alchemy_uniswap_v3"` |

---

## Database Configuration

PostgreSQL database connection settings.

```json
{
  "database": {
    "host": "localhost",
    "port": 5432,
    "user": "mantis",
    "password": "your_password",
    "dbname": "mantis",
    "pool_max_size": 10
  }
}
```

### Database Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `host` | string | `"localhost"` | PostgreSQL server hostname |
| `port` | int | `5432` | PostgreSQL server port |
| `user` | string | `"mantis"` | Database username |
| `password` | string | `null` | Database password (optional for local dev) |
| `dbname` | string | `"mantis"` | Database name |
| `pool_max_size` | int | `10` | Maximum database connection pool size |

### Database Setup

```bash
# Create database role and database
psql postgres -c "CREATE ROLE mantis WITH LOGIN PASSWORD 'your_password';"
psql postgres -c "CREATE DATABASE mantis OWNER mantis;"
```

---

## Data Collection Configuration

Controls market data scanning and history collection.

```json
{
  "data_collection": {
    "scan_interval_secs": 60,
    "history_days": 30,
    "auto_start": true
  }
}
```

### Data Collection Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `scan_interval_secs` | int | `60` | Market scan interval in seconds (e.g., `60` = 1 minute, optimized for momentum strategy) |
| `history_days` | int | `30` | Days of historical data to maintain (1-365) |
| `auto_start` | bool | `true` | Automatically start data collection on bot startup |

---

## Logs Configuration

```json
{
  "logs": {
    "directory": "logs"
  }
}
```

### Logs Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `directory` | string | `"logs"` | Directory path for log files |

---

## RPC Configuration

Ethereum RPC provider settings.

```json
{
  "rpc": {
    "primary_provider": "alchemy"
  }
}
```

### RPC Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `primary_provider` | string | `"alchemy"` | Primary RPC provider: `"alchemy"` or `"infura"` |

**Note**: The bot automatically falls back to the secondary provider if the primary fails.

---

## DEX Configuration

Decentralized exchange and blockchain network settings.

```json
{
  "dex": {
    "network": "ethereum",
    "protocol": "uniswap_v3",
    "subgraph_url": "https://subgraph.satsuma-prod.com/YOUR_KEY/YOUR_TEAM/uniswap-v3-mainnet/version/0.0.1/api",
    "custom_rpc_url": null,
    "router_address": null,
    "weth_address": null,
    "stablecoin_address": null,
    "wallet": {
      "private_key_env": "MANTIS_PRIVATE_KEY",
      "private_key_file": null
    },
    "paper_simulated_weth_balance": 10.0
  }
}
```

### Getting a Subgraph URL

Token discovery requires a Uniswap V3 subgraph endpoint. Get one free at [app.satsuma.xyz](https://app.satsuma.xyz):

1. Sign up and create a new subgraph
2. Search for "Uniswap V3" and deploy for Ethereum mainnet
3. Copy the query URL and set it:

```bash
mantis config set dex.subgraph_url https://subgraph.satsuma-prod.com/YOUR_KEY/YOUR_TEAM/uniswap-v3-mainnet/version/0.0.1/api
```

The bot will panic on startup with a clear message if this is not configured.

### DEX Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `subgraph_url` | string | required | Uniswap V3 subgraph query URL (see above) |
| `network` | string | `"ethereum"` | Blockchain network: `"ethereum"`, `"sepolia"`, etc. |
| `protocol` | string | `"uniswap_v3"` | DEX protocol to use |
| `custom_rpc_url` | string | `null` | Optional custom RPC endpoint URL |
| `router_address` | string | `null` | Optional custom router contract address |
| `weth_address` | string | `null` | Optional custom WETH contract address |
| `stablecoin_address` | string | `null` | Optional custom stablecoin contract address |

### Paper Trading Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `paper_simulated_weth_balance` | float | `10.0` | Starting WETH balance for paper trading |

### Wallet Configuration (Live Trading Only)

⚠️ **WARNING**: Only configure a wallet if you intend to do **live trading with real funds**.

```json
{
  "wallet": {
    "private_key_env": "MANTIS_PRIVATE_KEY",
    "private_key_file": null
  }
}
```

**Option 1: Environment Variable (Recommended)**

Set `private_key_env` to the name of an environment variable containing your private key:

```bash
export MANTIS_PRIVATE_KEY="0x1234567890abcdef..."
```

**Option 2: File**

Set `private_key_file` to a file path containing your private key:

```json
{
  "wallet": {
    "private_key_file": "/secure/path/to/private_key.txt"
  }
}
```

🔒 **Security Best Practices**:
- Never commit private keys to version control
- Use environment variables for production
- Restrict file permissions: `chmod 600 private_key.txt`
- Use a dedicated trading wallet, not your main wallet

---

## Cache Configuration

Optional Redis caching for improved performance (not required for paper trading).

```json
{
  "cache": {
    "enabled": false,
    "redis_url": "redis://127.0.0.1/"
  }
}
```

### Cache Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `enabled` | bool | `false` | Enable Redis caching |
| `redis_url` | string | `"redis://127.0.0.1/"` | Redis connection URL |

**Note**: Caching is primarily beneficial for high-frequency trading or monitoring many tokens. For paper trading and low-frequency scans, it's unnecessary overhead.

---

## Example Configurations

### Minimal Paper Trading Setup

```json
{
  "api_keys": {
    "alchemy": "YOUR_ALCHEMY_KEY"
  },
  "trading": {
    "live_trading": false,
    "max_position_size": 100.0,
    "strategy": "momentum",
    "indicator_profile": "day_trading"
  },
  "database": {
    "host": "localhost",
    "port": 5432,
    "user": "mantis",
    "dbname": "mantis"
  },
  "data_collection": {
    "scan_interval_secs": 60
  }
}
```

### Conservative Risk Profile

```json
{
  "trading": {
    "live_trading": false,
    "max_position_size": 50.0,
    "indicator_profile": "swing_trading",
    "stop_loss": 3.0,
    "take_profit": 8.0,
    "max_volatility_24h": 20.0,
    "min_volume": 100000.0,
    "min_liquidity": 500000.0,
    "max_positions": 3
  },
  "data_collection": {
    "scan_interval_secs": 120
  }
}
```

### Aggressive Risk Profile (Paper Trading Recommended)

```json
{
  "trading": {
    "live_trading": false,
    "max_position_size": 200.0,
    "indicator_profile": "scalping",
    "stop_loss": 8.0,
    "take_profit": 15.0,
    "max_volatility_24h": 50.0,
    "min_volume": 25000.0,
    "min_liquidity": 50000.0,
    "max_positions": 10
  },
  "data_collection": {
    "scan_interval_secs": 30
  }
}
```

---

## CLI Configuration Management

### View Configuration

```bash
# View entire config
mantis config show

# View specific section
mantis config get trading.strategy
```

### Update Configuration

```bash
# Set a value
mantis config set trading.max_position_size 150.0
mantis config set trading.strategy rsi

# Set API keys
mantis config set-key alchemy YOUR_KEY
mantis config set-key infura YOUR_KEY
```

### Configuration Files

```bash
# Show config file location
mantis config path
```

---

## Validation

The bot validates all configuration values on startup:

- Numeric ranges (e.g., percentages must be 0-100)
- Required fields
- Logical consistency (e.g., min < max)
- API key format
- Database connectivity

If validation fails, the bot will show clear error messages indicating which values need correction.

---

## Getting Help

For more information:

```bash
# General help
mantis --help

# Config command help
mantis config --help

# Trading command help
mantis trading --help
```
