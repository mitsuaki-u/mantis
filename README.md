# Mantis Trading Bot

[![CI](https://github.com/yourusername/mantis/actions/workflows/ci.yml/badge.svg)](https://github.com/yourusername/mantis/actions/workflows/ci.yml)

An automated Uniswap V3 trading bot built in Rust. Discovers tokens via on-chain data, runs technical-indicator strategies, enforces risk limits, and executes swaps — in paper or live mode.

## 🎯 Project Status

**Paper trading**: fully functional
**Live trading**: implemented and tested at small scale — use with caution and at your own risk

## ✨ Features

### Strategies
- **Momentum** — composite signal from RSI, MACD, Bollinger Bands, and volume
- **RSI** — oversold/overbought entries and exits
- Configurable indicator weights and profiles per timeframe

### Risk Management
- Stop-loss, take-profit, and trailing stop
- Per-position and portfolio-level exposure limits
- Volatility filter, max drawdown, and daily loss halts
- Gas cost protection (absolute USD + percentage of trade)
- On-chain price validation before every trade (rejects stale subgraph data)

### Technical
- Actor model architecture (concurrent scanning, strategy, risk, execution)
- PostgreSQL for trade/position persistence
- Paper trading simulates fees and slippage realistically
- Structured JSON logs, CI with zero warnings

## ⚙️ How It Works

1. **Discover** — queries the Uniswap V3 subgraph for pools filtered by TVL, volume, liquidity, and age
2. **Analyse** — builds indicator time series (RSI, MACD, Bollinger, volume trend); no signals until warmup completes
3. **Validate** — risk manager checks position limits, exposure, drawdown, gas cost, and on-chain price vs subgraph price
4. **Execute** — paper: simulates swap and records position; live: wraps ETH→WETH, executes Uniswap V3 swap, polls for receipt
5. **Manage** — monitors open positions every cycle; exits on stop-loss, take-profit, trailing stop, or max hold time

## 🚀 Quick Start

### Prerequisites

- Rust 1.70+ (`rustup install stable`)
- PostgreSQL 12+
- Alchemy API key — [alchemy.com](https://www.alchemy.com) (free tier works)
- Satsuma subgraph URL — [app.satsuma.xyz](https://app.satsuma.xyz) (free — deploy the Uniswap V3 subgraph)

### Installation

**1. Clone and build:**
```bash
git clone https://github.com/yourusername/mantis.git
cd mantis
cargo build --release
```

> Add `./target/release` to your `PATH` to use `mantis` directly instead of `./target/release/mantis`.

**2. Set up PostgreSQL:**
```bash
psql postgres -c "CREATE ROLE mantis WITH LOGIN;"
psql postgres -c "CREATE DATABASE mantis OWNER mantis;"
```

**3. Set your API key** (creates the config file on first run):
```bash
./target/release/mantis config set-key alchemy YOUR_ALCHEMY_KEY
```

**4. Set your subgraph URL:**
```bash
./target/release/mantis config set dex.subgraph_url YOUR_SATSUMA_URL
```

**5. Verify:**
```bash
./target/release/mantis config show
./target/release/mantis config path   # exact config file location on your system
```

### Paper Trading

```bash
./target/release/mantis trading start --strategy momentum --interval 60 --indicator-profile day_trading
```

> ⏳ **Warmup**: the momentum strategy collects ~50 minutes of price data before generating signals. The bot is working — it's just building its dataset.

```bash
# In a separate terminal
./target/release/mantis trading status
./target/release/mantis trading positions
./target/release/mantis trading history

# Stop the bot
./target/release/mantis trading stop
```

### Live Trading

> ⚠️ Use a **dedicated wallet** funded with only what you're willing to lose. Never your main wallet.

**1. Export your wallet private key** (never put this in the config file):
```bash
export MANTIS_PRIVATE_KEY=0xYOUR_PRIVATE_KEY
```

**2. Point the config to that env var:**
```bash
./target/release/mantis config set dex.wallet.private_key_env MANTIS_PRIVATE_KEY
```

**3. Set conservative limits:**
```bash
./target/release/mantis config set trading.max_positions 1
./target/release/mantis config set trading.max_position_size 50.0
./target/release/mantis config set trading.max_total_exposure 50.0
```

**4. Run:**
```bash
MANTIS_PRIVATE_KEY=0x... ./target/release/mantis trading start --live
```

Your wallet needs ~0.1 ETH to cover gas. The bot wraps ETH→WETH automatically before each buy.

## 📊 Configuration

Config file location (run `mantis config path` to confirm):
- **macOS**: `~/Library/Application Support/mantis/config.json`
- **Linux**: `~/.config/mantis/config.json`
- **Windows**: `%APPDATA%\mantis\config.json`

For all options see [CONFIGURATION.md](CONFIGURATION.md).

### Indicator Profiles

| Profile | Scan interval | Warmup | Best for |
|---|---|---|---|
| `scalping` | 5–30s | 40 min | High-frequency |
| `day_trading` | 60s | 50 min | **Recommended default** |
| `swing_trading` | 120s | 57 min | Position trading |
| `standard` | 300s+ | 71 min | Traditional TA |

```bash
mantis config set trading.indicator_profile day_trading
mantis config set data_collection.scan_interval_secs 60
```

### Logging

```bash
# Recommended — info only, suppress DB noise
RUST_LOG=mantis=info,tokio_postgres=warn ./target/release/mantis trading start

# Verbose
RUST_LOG=mantis=debug,tokio_postgres=warn ./target/release/mantis trading start
```

## 🧪 Testing

```bash
cargo test --lib                      # run all 137 unit tests
cargo test --lib position_sizing      # run tests matching a name
```

**Coverage**: position sizing, portfolio risk limits, stop-loss/take-profit/trailing stop, gas validation, RSI/momentum signal logic, on-chain price discrepancy detection.

## 🐛 Known Limitations

- **Warmup required** — no trades fire until enough candles are collected (40–71 min depending on profile)
- **Ethereum mainnet only** — Uniswap V3, WETH as base token
- **One strategy at a time** — one strategy type per bot instance
- **No backtesting** — no built-in historical simulation
- **Postgres TLS** — currently `NoTls`; requires `tokio-postgres-rustls` to enable

## 📚 Documentation

- [CONFIGURATION.md](CONFIGURATION.md) — full config reference
- [docs/architecture.md](docs/architecture.md) — actor model, trading pipeline, component detail

## 🛠️ Development

```bash
cargo fmt                  # format
cargo clippy -- -D warnings  # lint (zero warnings enforced in CI)
cargo test --lib           # test
cargo check --all-targets  # type check
```

## 🤝 Contributing

Feedback, bug reports, and PRs are welcome. Some areas that would benefit from contributions:
- Backtesting framework
- Additional strategies
- Multi-chain support (Arbitrum, Base)
- Web dashboard

## ⚠️ Disclaimer

This software is provided for educational purposes. Trading bots can and do lose money. Gas fees can exceed profits. Smart contracts carry risk. You are solely responsible for any funds used with this software.

---

**Built with Rust** 🦀 | **Uniswap V3** 🦄 | **Alchemy** ⚗️
