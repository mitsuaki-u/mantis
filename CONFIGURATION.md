# Configuration Reference

Every configuration option, what it does, and how to set it. Pairs with the high-level overview in [README.md](README.md) and the actor-system detail in [docs/architecture.md](docs/architecture.md).

## Quick navigation

- [File location](#file-location) · [Priority](#configuration-priority) · [Environment variables](#environment-variables)
- Sections: [`api_keys`](#api_keys) · [`anthropic_api_key`](#anthropic_api_key) · [`trading`](#trading) · [`database`](#database) · [`data_collection`](#data_collection) · [`logs`](#logs) · [`rpc`](#rpc) · [`dex`](#dex) · [`cache`](#cache) · [`solana`](#solana)
- [CLI commands](#cli-commands) · [Common setups](#common-setups)

## File location

Default path:

| Platform | Path |
|---|---|
| macOS | `~/Library/Application Support/mantis/config.json` |
| Linux | `~/.config/mantis/config.json` |
| Windows | `%APPDATA%\mantis\config.json` |

Run `mantis config path` to see the exact path on your system. The file is created automatically the first time you run `mantis config set ...`.

## Configuration priority

Sources are merged in this order (later overrides earlier):

1. Built-in defaults
2. Configuration file
3. Environment variables (only for fields that support env override — see below)
4. CLI flags passed to `mantis trading start`

For most fields, the config file is authoritative. Env vars are limited to a small set — see the env vars section.

## Environment variables

Only these fields support env-var override:

| Variable | Description | Maps to |
|---|---|---|
| `MANTIS_ALCHEMY_KEY` | Ethereum RPC API key | `api_keys.alchemy` |
| `MANTIS_INFURA_KEY` | Ethereum RPC API key (alternative) | `api_keys.infura` |
| `MANTIS_PRIVATE_KEY` | Ethereum wallet private key | Resolved via `dex.wallet.private_key_env` |
| `ANTHROPIC_API_KEY` | Claude API key for the AI advisor | `anthropic_api_key` |
| `REDIS_URL` | Redis connection URL | `cache.redis_url` |
| `RUST_LOG` | Log level | Standard `env_logger` filter |

Anthropic key precedence: if both `anthropic_api_key` (config) and `ANTHROPIC_API_KEY` (env) are set, **config wins**. Empty strings count as missing.

## Configuration file structure

Top-level JSON shape:

```json
{
  "api_keys":          { ... },
  "anthropic_api_key": "sk-ant-api03-...",
  "trading":           { ... },
  "database":          { ... },
  "data_collection":   { ... },
  "logs":              { ... },
  "rpc":               { ... },
  "dex":               { ... },
  "cache":             { ... },
  "solana":            { ... }
}
```

Every section is optional — defaults fill in anything you don't set.

---

## `api_keys`

Ethereum RPC provider keys. Both are optional; the bot uses whichever is present.

```json
"api_keys": {
  "infura":  null,
  "alchemy": "..."
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `infura` | string \| null | `null` | Infura API key |
| `alchemy` | string \| null | `null` | Alchemy API key |

**Set via CLI**:
```bash
mantis config set-key alchemy YOUR_ALCHEMY_KEY
mantis config set-key infura YOUR_INFURA_KEY
```

Or via environment: `MANTIS_ALCHEMY_KEY`, `MANTIS_INFURA_KEY`.

## `anthropic_api_key`

Top-level field (not nested). Required for the AI advisor; without it, signals pass through unmodified and a warning is logged at startup.

```json
"anthropic_api_key": "sk-ant-api03-..."
```

**Set via CLI**:
```bash
mantis config set anthropic_api_key sk-ant-api03-...
```

Or via environment:
```bash
export ANTHROPIC_API_KEY=sk-ant-api03-...
```

Get a key at [console.anthropic.com](https://console.anthropic.com). Note: API credits are billed separately from Claude.ai subscriptions — see the README for details.

---

## `trading`

The largest config section. Strategy choice, risk limits, indicator weights, gas protection.

### Mode and strategy

| Field | Type | Default | Description |
|---|---|---|---|
| `live_trading` | bool | `false` | When `true`, executes real swaps. Defaults to paper mode. |
| `strategy` | string | `"momentum"` | Strategy type. Supported: `"momentum"`, `"rsi"`. |
| `signal_confidence_threshold` | f64 (0.0-1.0) | `0.65` | Minimum strategy score to fire a BUY signal. |
| `indicator_profile` | string | `"day_trading"` | Indicator period preset. One of: `"scalping"`, `"day_trading"`, `"swing_trading"`, `"standard"`. |
| `market_data_provider` | string | `"dexscreener_solana"` | Token discovery and pricing provider. |

### Position sizing

| Field | Type | Default | Description |
|---|---|---|---|
| `max_position_size` | f64 (USD) | `50.0` | Maximum size of a single position. |
| `min_position_size` | f64 (USD) | `20.0` | Minimum size of a single position. |
| `max_total_exposure` | f64 (USD) | `1000.0` | Maximum total open exposure across all positions. |
| `max_positions` | usize | `5` | Maximum number of concurrent open positions. |

### Market filters

| Field | Type | Default | Description |
|---|---|---|---|
| `min_volume` | f64 (USD) | `1_000_000.0` | Drop tokens with 24h volume below this. |
| `min_liquidity` | f64 (USD) | `100_000.0` | Drop tokens with pool liquidity below this. |
| `min_pool_transaction_count` | u32 | `1000` | (Ethereum) drop pools with fewer than this many lifetime transactions. |
| `max_volatility_24h` | f64 (%) | `30.0` | Drop tokens whose 24h price change exceeds this. Set high to disable. |
| `max_tokens_to_scan` | usize | `150` | Max tokens evaluated per scan cycle (`0` = unlimited). |
| `tokens_to_track` | array of strings | `[]` | If non-empty, only track these specific tokens (mints/addresses). |

### Exit conditions

| Field | Type | Default | Description |
|---|---|---|---|
| `stop_loss` | f64 (%) | `5.0` | Close position if it drops more than this from entry. |
| `take_profit` | f64 (%) | `10.0` | Close position if it gains more than this from entry. |

### Risk management

| Field | Type | Default | Description |
|---|---|---|---|
| `max_daily_loss` | f64 (%) | `10.0` | Halt new BUY signals once daily loss exceeds 80% of this threshold. |
| `max_drawdown` | f64 (%) | `20.0` | Halt new BUY signals once drawdown from peak exceeds 80% of this. |
| `max_trade_risk_pct` | f64 (%) | `5.0` | Maximum percentage of wallet a single trade can risk. |
| `min_native_balance` | f64 (ETH or SOL) | `0.1` | Minimum native balance required to trade (alias: `min_eth_balance`). |
| `min_portfolio_risk_factor` | f64 (0.0-1.0) | `0.3` | Halt new trades when portfolio risk factor drops below this. |
| `max_execution_price_deviation` | f64 (0.0-1.0) | `0.05` | Reject trades if execution price differs from signal price by more than this fraction. |

### Indicator weights (momentum strategy)

Weights for the composite momentum score. Sum should equal 1.0; the bot doesn't enforce this but skewed weights tilt the signal.

| Field | Type | Default | Description |
|---|---|---|---|
| `rsi_weight` | f64 (0.0-1.0) | `0.3` | RSI contribution. |
| `macd_weight` | f64 (0.0-1.0) | `0.3` | MACD contribution. |
| `bollinger_weight` | f64 (0.0-1.0) | `0.2` | Bollinger Bands contribution. |
| `volume_weight` | f64 (0.0-1.0) | `0.2` | Volume trend contribution. |

### Gas / fee protection (Ethereum live only)

| Field | Type | Default | Description |
|---|---|---|---|
| `max_gas_cost_usd` | f64 (USD) | `4.0` | Reject trade if estimated gas exceeds this. |
| `max_gas_cost_percentage` | f64 (%) | `15.0` | Reject trade if estimated gas exceeds this percentage of trade size. |
| `transaction_priority` | string | `"Standard"` | Gas multiplier preset. One of: `"Low"` (0.9x), `"Medium"`, `"Standard"` (1.0x), `"High"` (1.2x), `"Urgent"` (1.5x). |

---

## `database`

PostgreSQL connection. Required.

```json
"database": {
  "host":          "localhost",
  "port":          5432,
  "user":          "mantis",
  "password":      null,
  "dbname":        "mantis",
  "pool_max_size": 10
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `host` | string | `"localhost"` | PostgreSQL host. |
| `port` | u16 (1-65535) | `5432` | PostgreSQL port. |
| `user` | string | `"mantis"` | PostgreSQL user (cannot be empty). |
| `password` | string \| null | `null` | PostgreSQL password (uses peer/trust auth if null). |
| `dbname` | string | `"mantis"` | Database name (cannot be empty). |
| `pool_max_size` | usize (≥1) | `10` | Max connections in the pool. |

Connection currently uses `NoTls`. Adding TLS requires the `tokio-postgres-rustls` crate — see the architecture doc's "Known Caveats."

---

## `data_collection`

Market polling cadence and history retention.

| Field | Type | Default | Description |
|---|---|---|---|
| `scan_interval_secs` | u64 (≥1) | `60` | How often to poll market data (in seconds). |
| `history_days` | u64 (1-365) | `30` | Days of historical price data to retain. |
| `auto_start` | bool | `true` | Start data collection immediately when the bot launches. |

Tuning hint: `scan_interval_secs` should match your `indicator_profile`. Scalping wants 5-30s; day_trading 60s; swing_trading 120s.

---

## `logs`

| Field | Type | Default | Description |
|---|---|---|---|
| `directory` | string | platform-default cache dir | Where rotating log files are written. |

Use `RUST_LOG` to control verbosity. Recommended:

```bash
RUST_LOG=mantis=info,tokio_postgres=warn ./target/release/mantis trading start
```

---

## `rpc`

Ethereum RPC provider settings. Used only when `dex.network` resolves to an EVM chain.

| Field | Type | Default | Description |
|---|---|---|---|
| `primary_provider` | string | `"alchemy"` | Primary RPC provider (`"alchemy"` or `"infura"`). |

The bot uses the matching key from `api_keys` for RPC calls.

---

## `dex`

DEX backend for execution. The default is Solana (paper-only); set `network: "ethereum"` for Ethereum live execution.

```json
"dex": {
  "network":                      "solana",
  "protocol":                     "jupiter",
  "custom_rpc_url":               null,
  "router_address":               null,
  "weth_address":                 null,
  "stablecoin_address":           null,
  "wallet":                       null,
  "paper_simulated_weth_balance": 10.0
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `network` | string \| null | `"solana"` | Target chain. `"ethereum"` and `"solana"` are supported; `"ethereum"` enables live execution. |
| `protocol` | string | `"jupiter"` | DEX protocol label. The CLI setter currently validates `"uniswap_v3"` only; the field is for display and forward-compat. |
| `custom_rpc_url` | string \| null | `null` | Override the RPC endpoint. |
| `router_address` | string \| null | `null` | Override the swap router contract (Ethereum). |
| `weth_address` | string \| null | `null` | Override the WETH contract (Ethereum). |
| `stablecoin_address` | string \| null | `null` | Override the stablecoin contract (Ethereum, USDC). |
| `wallet` | object \| null | `null` | Wallet config for live trading. See below. |
| `paper_simulated_weth_balance` | f64 | `10.0` | Starting WETH balance in Ethereum paper mode. |

### `dex.wallet` (Ethereum live trading only)

```json
"dex": {
  "wallet": {
    "private_key_env":  "MANTIS_PRIVATE_KEY",
    "private_key_file": null
  }
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `private_key_env` | string \| null | `null` | Name of the env var holding the hex-encoded private key. |
| `private_key_file` | string \| null | `null` | Path to a file containing the hex-encoded private key. |

The bot reads `private_key_env` first, then falls back to `private_key_file`. Never put a private key directly in the JSON — both options exist to keep secrets out of the config file.

---

## `cache`

Redis is optional. Used for batched DB writes and token metadata cache. The bot degrades gracefully when Redis is unavailable.

| Field | Type | Default | Description |
|---|---|---|---|
| `redis_url` | string | `"redis://127.0.0.1/"` | Redis connection URL. Override via `REDIS_URL` env var. |

---

## `solana`

Solana network config. Used by Solana paper trading today; required for live execution once Jupiter integration lands.

```json
"solana": {
  "rpc_url":      "https://mainnet.helius-rpc.com/?api-key=...",
  "network":      "mainnet",
  "keypair_path": null
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `rpc_url` | string \| null | `null` | Solana RPC endpoint (Helius recommended). Currently optional — paper mode uses DexScreener directly. |
| `network` | string | `"mainnet"` | Solana network. `"mainnet"` or `"devnet"`. |
| `keypair_path` | string \| null | `null` | Path to a Solana keypair JSON file (planned, for live execution). |

Get a free Helius RPC URL at [helius.dev](https://helius.dev) when Solana live execution is needed.

---

## CLI commands

```bash
# Show resolved config (config file + defaults; env vars are not shown here)
mantis config show

# Show the path to your config file
mantis config path

# Set any nested field
mantis config set <dotted.key> <value>
mantis config set trading.max_positions 3
mantis config set trading.indicator_profile day_trading
mantis config set anthropic_api_key sk-ant-api03-...
mantis config set dex.network ethereum
mantis config set solana.rpc_url https://mainnet.helius-rpc.com/?api-key=...

# Special command for API keys (Ethereum RPC providers)
mantis config set-key alchemy YOUR_ALCHEMY_KEY
mantis config set-key infura YOUR_INFURA_KEY

# Read a single field
mantis config get trading.max_positions

# Reset to defaults (be careful)
mantis config reset
```

---

## Common setups

### Solana paper mode (recommended default)

```bash
mantis config set anthropic_api_key sk-ant-api03-...
# That's it. DexScreener is keyless. Default network is already Solana.
mantis trading start --strategy momentum --interval 15 --indicator-profile scalping
```

### Ethereum paper mode

```bash
mantis config set dex.network ethereum
mantis config set-key alchemy YOUR_ALCHEMY_KEY
mantis config set anthropic_api_key sk-ant-api03-...
mantis trading start --strategy momentum --interval 60 --indicator-profile day_trading
```

### Ethereum live mode

> ⚠️ Live trading uses real funds. Use a dedicated wallet with only what you can lose.

```bash
mantis config set dex.network ethereum
mantis config set-key alchemy YOUR_ALCHEMY_KEY
mantis config set anthropic_api_key sk-ant-api03-...
mantis config set dex.wallet.private_key_env MANTIS_PRIVATE_KEY
mantis config set trading.live_trading true
mantis config set trading.max_position_size 20.0   # start small
mantis config set trading.max_positions 1          # one trade at a time

export MANTIS_PRIVATE_KEY=0xYOUR_PRIVATE_KEY
mantis trading start --strategy momentum --live
```

The wallet needs ~0.1 ETH to cover gas. The bot wraps ETH→WETH automatically before each buy.

### Disable AI advisor (run without Claude)

Just don't set `anthropic_api_key` and don't export `ANTHROPIC_API_KEY`. The bot logs:

```
AIAdvisorActor: no Anthropic API key — signals pass through without AI analysis
```

Strategy signals reach the risk layer unmodified, with confidence 75 and reasoning "AI advisor not configured."
