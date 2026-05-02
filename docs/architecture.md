# Mantis Architecture

> Engineering reference. Captures the current shape of the system, the critical components, and known gaps.

## 1. Purpose & Scope

Mantis is a Rust trading framework with a supervised actor architecture and an LLM advisor that audits every BUY decision. It supports two DEX backends: Ethereum (Uniswap V3, paper + live) and Solana (DexScreener for token discovery and price data, paper). Solana live execution is on the roadmap; paper mode is the recommended use today.

## 2. Layered Architecture

```
src/
├── core/                  Domain logic. No infra/external deps. Pure functions where possible.
├── infrastructure/        External adapters: RPC, DB, cache, market data, DEX, AI.
├── application/           Actors + event plumbing. Orchestrates core + infra.
└── bin/cli/               Clap-based CLI, config loading, bootstrap.
```

Dependency rule: `application` depends on `core` and `infrastructure`; `infrastructure` depends on `core`; `core` depends on nothing in this crate. `bin/cli` wires everything together.

The strategy, risk, and indicator layers in `core/` know nothing about which chain they run on. Adding a new DEX backend is a matter of implementing the `DexClient` interface and a `MarketDataProvider`.

## 3. Directory Map

### `src/core/` — domain logic
- `domain/trading.rs` — `Signal`, `Position`, `ExitReason`, `SignalMetadata` (with indicator snapshot)
- `domain/market.rs` — `TokenMetrics` (price, volume, liquidity, 24h change)
- `domain/token.rs`, `domain/wallet.rs`, `domain/dex.rs`, `domain/strategy_params.rs`
- `strategies/traits.rs` — `TradingStrategy` trait + `Strategy` enum (Momentum | Rsi)
- `strategies/custom/momentum.rs`, `strategies/custom/rsi.rs` — concrete strategies
- `strategies/exit_conditions.rs` — stop-loss / take-profit / trailing stop logic
- `strategies/factory.rs` — constructs strategy from config
- `indicators/mod.rs` — `PriceTimeSeries`, RSI/MACD/Bollinger/Volume, `IndicatorSnapshot`, `compute_snapshot`
- `risk/assessment.rs` — position sizing, portfolio risk factors
- `risk/limits.rs` — overall limits, halt checks, trading-allowed gate
- `calculations/financial.rs` — PnL, ROI
- `calculations/uniswap_v3.rs` — V3 sqrtPrice → price math
- `utils/conversion.rs` — safe `Decimal` ↔ `f64`
- `utils/validation/price.rs` — subgraph vs on-chain price discrepancy check (Ethereum-only)
- `utils/validation/orders.rs` — buy/sell order validation

### `src/infrastructure/`
- `dex/client.rs` — `DexClient` enum (`SolanaPaper` | `Paper` | `Live`), top-level swap interface
- `dex/ethereum/eth_client.rs` — wallet, RPC, WETH wrap/unwrap
- `dex/ethereum/rpc.rs` — `eth_getBalance`, `eth_call`, etc.
- `dex/ethereum/pool_pricing.rs` — `slot0()`, liquidity reads
- `dex/ethereum/config/addresses.rs` — WETH/USDC/router addresses per network
- `dex/ethereum/providers/uniswap_v3/` — full V3 provider: `provider.rs`, `execution.rs`, `pricing.rs`, `quoter.rs`, `pool_cache.rs`, `gas.rs`, `abi.rs`, `types.rs`
- `dex/ethereum/transactions/manager.rs` — tx status polling / mempool tracking
- `market/providers/alchemy/` — Uniswap V3 subgraph provider for Ethereum token discovery
- `market/providers/dexscreener/` — DexScreener provider for Solana trending tokens and price data
- `ai/claude.rs` — LLM HTTP client + response parser (`parse_decision`)
- `database/pool.rs` — deadpool-postgres connection pool
- `database/repositories/` — `TokenRepository`, `PositionRepository`, `TradeRepository`, `TransactionRepository`
- `database/queue.rs` — Redis-backed `PositionUpdateQueue` for batched writes
- `cache/` — Redis client, token cache, batch ops
- `network/`, `retry/` — HTTP helpers, retry-with-backoff

### `src/application/src/`
- `app.rs` — `TradingBotSystem` bootstrap, actor registration, routing config
- `events.rs` — all event types (see §5)
- `actors/system/` — `Actor` trait, lifecycle, `EventRouter`
- `actors/supervisor/` — `SupervisorActor` (lifecycle coordinator + health watcher)
- `actors/market/` — `MarketDataActor`
- `actors/strategy/` — `StrategyActor`
- `actors/ai_advisor/` — `AIAdvisorActor`
- `actors/risk_manager/` — `RiskManagerActor`
- `actors/execution/` — `ExecutionActor`
- `actors/database/` — `DatabaseActor`

### `src/bin/cli/src/`
- `mantis.rs` — entry point, command dispatch
- `bootstrap.rs` — builds DB pool, cache, DEX client, actors
- `overrides.rs` — apply CLI flags onto loaded config
- `config/` — defaults, file I/O, env vars, validation, API keys, networks
- `commands/trading/` — `start`, `stop`, `status`, `history`, `positions`, `transactions`
- `commands/config/` — `show`, `set`, `get`, `reset`, `set-key`

## 4. Actor System

All actors implement the `Actor` trait (`application/src/actors/system/actor.rs`) and communicate through a central `EventRouter` (`application/src/actors/system/router.rs`). A `SupervisorActor` watches every actor and can restart any one independently without bringing the rest down.

| Actor | Publishes | Consumes |
|---|---|---|
| `SupervisorActor` | lifecycle commands | — |
| `MarketDataActor` | `MarketEvent::PriceUpdate`, `MarketEvent::PoolsDiscovered` | — (poll-driven) |
| `StrategyActor` | `StrategyEvent::Signal` (with `IndicatorSnapshot`) | `MarketEvent::PriceUpdate` |
| `AIAdvisorActor` | `AIAdvisorEvent::SignalAnalysed` | `StrategyEvent::Signal` |
| `RiskManagerActor` | `RiskEvent::{TradeApproved, TradeSizeAdjusted, RiskLimitExceeded, PositionCreated, PositionClosed}` | `AIAdvisorEvent::SignalAnalysed`, `RiskEvent::{PositionCreated, PositionClosed}`, `MarketEvent::PriceUpdate` |
| `ExecutionActor` | `ExecutionEvent::{OrderExecuted, OrderFailed}`, `DexTransactionEvent::{Submitted, StatusUpdated}` | `RiskEvent::TradeApproved` |
| `DatabaseActor` | — (write-only) | all events |

Routing graph (from `app.rs`):
- `MarketEvent` → `StrategyActor`, `RiskManagerActor`, `DatabaseActor`, `ExecutionActor`
- `StrategyEvent` → `AIAdvisorActor`, `DatabaseActor`
- `AIAdvisorEvent` → `RiskManagerActor`, `DatabaseActor`
- `RiskEvent` → `ExecutionActor`, `DatabaseActor`
- `ExecutionEvent` → `DatabaseActor`, `RiskManagerActor` (for position tracking)

The AI advisor sits between strategy and risk so every BUY signal goes through the LLM before reaching the risk layer. SELL signals are forwarded by the AI advisor without consulting the LLM — exits should never wait on an external API.

## 5. Event Model

Defined in `application/src/events.rs`. Top-level `Event` enum wraps:

- `MarketEvent::PriceUpdate { token_id, price, volume, timestamp, ... }`
- `MarketEvent::PoolsDiscovered { pools }`
- `StrategyEvent::Signal { token_id, signal: Buy|Sell|Hold|NoAction, timestamp, metadata: SignalMetadata }`
- `AIAdvisorEvent::SignalAnalysed { token_id, signal, approved, confidence, reasoning, metadata }`
- `RiskEvent::TradeApproved { token_id, signal, position_size_usd, correlation_id, ... }`
- `RiskEvent::TradeSizeAdjusted { original, adjusted, reason }`
- `RiskEvent::RiskLimitExceeded { limit_type, current_value, threshold }`
- `RiskEvent::PositionCreated { token_id, entry_price, size }`
- `RiskEvent::PositionClosed { token_id, exit_reason }`
- `ExecutionEvent::OrderExecuted { token_id, side, executed_value_usd, token_quantity, tx_hash, fees_usd, entry_price?, entry_time?, realized_pnl? }`
- `ExecutionEvent::OrderFailed { token_id, reason }`
- `DexTransactionEvent::{Submitted, StatusUpdated}`

`SignalMetadata` carries:
- `correlation_id` (UUID — every signal traceable through Approved → Executed)
- `signal_price`, `signal_volume_24h`
- `strategy_name`, `market_conditions`
- The full `IndicatorSnapshot`: `rsi`, `bollinger_pct`, `momentum_score`
- `volume_24h`, `price_change_24h`

This is what the AI advisor receives. The snapshot is computed at signal-publication time (see §7) so the advisor sees the same indicator values the strategy used to decide.

## 6. Trading Pipeline (end-to-end)

### 6.1 Token discovery

Two providers:

**Ethereum** (`AlchemyUniswapV3Provider`):
1. GraphQL query to Satsuma-hosted Uniswap V3 subgraph. Filters: `tvl > min_liquidity`, pool age ≥ ~6 months, `txCount >= min_pool_transaction_count`.
2. Keep only WETH-paired pools.
3. Second query (`poolDayDatas`) enriches with 24h volume.
4. Drop pools with `volume < config.trading.min_volume`.
5. Build `TokenMetrics` from on-chain price (`sqrtPriceX96`).

**Solana** (`DexScreenerProvider`):
1. GET `api.dexscreener.com/latest/dex/search?q=solana` for trending pairs.
2. Filter by `volume_24h > config.trading.min_volume`, `liquidity > config.trading.min_liquidity`.
3. Normalise SOL/TOKEN pairs so the real token is always the base.
4. Filter stablecoins.
5. Build `TokenMetrics` from DexScreener pair data.

### 6.2 Market polling (`MarketDataActor`)
Every `scan_interval_secs`, call `MarketDataProvider::fetch_tokens_and_metrics()`. For each token: update strategy's `PriceTimeSeries`, publish `PriceUpdate`.

### 6.3 Signal generation (`StrategyActor`)
1. Volume gate: reject if `volume_24h < strategy.min_volume()`.
2. Cooldown: reject if token recently signaled.
3. Indicators computed from the selected **indicator profile** (scalping / day_trading / swing_trading / standard).
4. `analyze_for_entry(token)` → bool; if an open position exists, `analyze_for_exit(token, position, risk_params)` → `Option<ExitReason>`.
5. **On-chain price validation** (Ethereum only — Solana skips this): reject BUYs if `|subgraph_price − slot0_price| / slot0_price > 5%`.
6. Compute `IndicatorSnapshot` from the strategy's current `PriceTimeSeries` using `compute_snapshot()` (see §7).
7. Publish `StrategyEvent::Signal` with the snapshot embedded in `SignalMetadata`.

### 6.4 AI advisor (`AIAdvisorActor`)
For BUY signals only:
1. Construct user prompt with `IndicatorSnapshot` values + portfolio state (open positions, daily P&L).
2. POST to the LLM API (`claude-haiku-4-5-20251001`) with cached system prompt.
3. Parse response (`infrastructure/ai/claude.rs::parse_decision`) → `AIDecision { approve, confidence, reasoning }`.
4. Publish `AIAdvisorEvent::SignalAnalysed { approved, confidence, reasoning, ... }`.

For SELL/HOLD signals: forward immediately as approved with `confidence: 100` and reasoning `"SELL signal — bypasses AI advisor"`. No LLM call.

**Fail-open**: if the LLM errors or times out (3s), the signal is approved with `confidence: 50` and a diagnostic note. The bot does not stall on AI downtime.

### 6.5 Risk validation (`RiskManagerActor`)
Listens on `AIAdvisorEvent::SignalAnalysed`. Skips rejected signals.

Checks in order (`core/risk/limits.rs`):
1. Halted-token check.
2. Max-positions check (BUY only).
3. Portfolio risk factor ≥ 0.3 (BUY only).
4. Daily loss < 80% of `max_daily_loss` (BUY only).
5. Drawdown < 80% of `max_drawdown` (BUY only).

Position sizing (`core/risk/assessment.rs`):
- `portfolio_factor = daily_loss_factor × drawdown_factor × exposure_factor`
- `size_usd = clamp(max_position_size × portfolio_factor, min_position_size, max_position_size)`

Publish `RiskEvent::TradeApproved` or `RiskLimitExceeded` / size-adjusted variant.

### 6.6 Execution (`ExecutionActor`)

**Buy** (`actors/execution/orders.rs`):
1. Receive `TradeApproved`.
2. Fetch `TokenData` from DB; assert `token_id` matches (guards against wrong-token execution).
3. Validate order (min size, slippage, price range) via `core/utils/validation/orders.rs`.
4. Native-currency conversion: USD → WETH for Ethereum, simulated for Solana paper.
5. `DexClient::ensure_weth_balance(required_weth, …)` — wraps ETH if live; no-op for paper.
6. `DexClient::execute_swap(...)` → `TransactionDetails`. SolanaPaper variant simulates.
7. `PositionRepository::create_position(…)`.
8. Publish `ExecutionEvent::OrderExecuted`.

**Sell**:
1. Verify position exists (silent skip if not).
2. Execute swap.
3. Compute realized PnL and ROI.
4. `PositionRepository::record_close(…)`.
5. Publish `ExecutionEvent::OrderExecuted` with entry/exit fields.

### 6.7 Position tracking
- `RiskManagerActor` marks-to-market on every `PriceUpdate` to update unrealized PnL and portfolio total.
- Exit conditions (`core/strategies/exit_conditions.rs`): take-profit %, stop-loss %, trailing-stop %, strategy-based exits, max-hold time.

### 6.8 Transaction monitoring
`ExecutionActor::tasks.rs` polls `DexClient::get_transaction_status(tx_hash)` on a timer, emits `DexTransactionEvent::StatusUpdated`; `DatabaseActor` persists final state.

## 7. Critical Components

### Indicator snapshot pipeline
The AI advisor needs the same indicator values the strategy used to decide a BUY. The snapshot pipeline decouples computation from publication:

1. Strategy actor calls `update_market_data(&token_metrics)` on the price update — this appends to the strategy's `PriceTimeSeries`.
2. Strategy decides BUY/SELL via `analyze_for_entry` / `analyze_for_exit`.
3. **Before publishing the signal**, `StrategyActor::indicator_snapshot_for(token_id, symbol)` reads the strategy's current time series via the read-only `TradingStrategy::price_series_for` accessor and computes `IndicatorSnapshot { rsi, bollinger_pct, momentum_score }` using `compute_snapshot()`.
4. Snapshot is attached to `SignalMetadata` via `with_indicators(...)`.
5. Signal is published. AI advisor reads `metadata.rsi/bollinger_pct/momentum_score` directly — no fallbacks, no recomputation.

The strategy's own `indicator_weights()` is used so the composite momentum score in the snapshot matches the score the strategy used to make its decision.

### AI advisor (`infrastructure/ai/claude.rs`)
- Model: `claude-haiku-4-5-20251001`
- Prompt caching on the system prompt (~90% input cost reduction)
- 10s HTTP timeout; configurable
- `parse_decision` is pure logic with full unit-test coverage — handles the standard format, lowercase keywords, missing/malformed fields, and a keyword-count fallback for completely off-format responses
- Fail-open behaviour: errors don't propagate; signal is approved with `confidence: 50` and a diagnostic reasoning string

### Risk management
- `core/risk/assessment.rs`: `compute_position_size`, `calculate_portfolio_risk_factors`, `check_token_volatility`, `has_valid_market_data`.
- `core/risk/limits.rs`: `check_overall_risk_limits`, `check_trading_allowed`, halt/resume token trading.
- `application/src/actors/risk_manager/operations.rs`: `check_token_risk`, `update_risk_metrics`, `reset_daily_metrics`.
- State held inline in `RiskManagerActor`: `HashMap<token_id, PositionDetails>`, cumulative daily PnL, peak equity for drawdown.

### DEX client
- `DexClient::SolanaPaper { simulated_sol_balance, simulated_base_token_balance }` — Solana paper trading.
- `DexClient::Paper { ethereum_client, simulated_eth_balance, simulated_weth_balance }` — Ethereum paper.
- `DexClient::Live { ethereum_client }` — Ethereum live (Uniswap V3).
- The enum dispatches at the top level. Each method handles its variants explicitly so adding a new chain is a pattern-match exercise rather than a trait-object refactor.

#### Ethereum live (`UniswapV3ProtocolProvider`)
- `pool_cache.rs` — pool discovery cache populated from `PoolsDiscovered`.
- `quoter.rs` / `pricing.rs` — QuoterV2 integration, direct + multi-hop routing through WETH/USDC.
- `execution.rs` — builds `exactInputSingle` / `exactInputMultihop` router calls.
- `gas.rs` — pre-trade gas estimate, checked against `max_gas_cost_usd` and `max_gas_cost_percentage`.

### Price validation
- `core/utils/validation/price.rs` — Ethereum-only. Compares subgraph price vs on-chain `slot0` price; rejects if discrepancy > 5%. Adds ~1 RPC per potential trade. Skipped for Solana (different price-source model).

### Persistence (Postgres)
Tables: `tokens`, `positions`, `trades`, `prices`, `transactions`. `PositionRepository` carries an `is_paper_trade` flag so paper and live coexist without bleeding into each other. `PositionUpdateQueue` (Redis list) batches updates; `database::task_handler` drains to Postgres.

### Cache (Redis)
Optional. Used for (a) token metadata / TokenMetrics cache, (b) position-update queue. Graceful degradation if unavailable.

### Indicator profiles
Selected via `trading.indicator_profile`. Each profile sets periods for RSI, MACD (fast/slow/signal), Bollinger, volume. Drives warmup duration and indicator sensitivity. Profiles: `scalping` (fastest), `day_trading` (recommended default), `swing_trading`, `standard`.

### Strategy plug-ins
`TradingStrategy` trait + `Strategy` enum dispatch (`Momentum` | `Rsi`). Enum wrapping avoids `Box<dyn Trait>` + `Clone` issues. `strategies/factory.rs` constructs the strategy from config. Adding a new strategy is one file plus a factory match arm.

### Config loading
Hierarchy (later overrides earlier):
1. `config/defaults.rs`
2. JSON config file (`~/.config/mantis/config.json` on macOS/Linux)
3. Env vars (`config/env.rs`) — including `ANTHROPIC_API_KEY` for the AI advisor
4. CLI flags (`overrides.rs`)

Validation runs after each merge (`config/validation.rs`).

### Logging
Structured JSON via `log` + `env_logger`. `RUST_LOG` tuned per-module; `tokio_postgres=info` is the common knob to hide query-prep noise. Console + rotating file sinks.

## 8. External Integrations

- **Anthropic API**: AI advisor uses `claude-haiku-4-5-20251001` with prompt caching. Required for AI features; bot runs without it but signals pass through unmodified.
- **DexScreener**: Solana token discovery, trending feed, and price data (per-pair). No API key required.
- **Helius / Solana RPC** (planned, not yet integrated): Solana mainnet RPC for balance reads and swap submission once Solana live execution lands.
- **Jupiter** (planned, not yet integrated): Solana swap routing via `quote-api.jup.ag` for live execution. No API key required.
- **Alchemy / Satsuma**: Uniswap V3 community subgraph over GraphQL for Ethereum token discovery. `api_keys.alchemy` is used for the RPC endpoint.
- **Ethereum RPC**: via `ethers`. Used for `eth_call` (slot0, quoter), `eth_getBalance`, tx submission, receipt polling.
- **Uniswap V3**: SwapRouter02, QuoterV2, pool slot0 reads.
- **Postgres**: via `tokio-postgres` + `deadpool-postgres`. Schema initialized on first run.
- **Redis**: via `redis` + `deadpool-redis`. Optional.

## 9. Tests

- 151 unit tests inline `#[cfg(test)]` across `core/` (conversion, validation, formatting, financial math, exit conditions, V3 math, strategy params, indicator snapshot), CLI config, infra (DB pool, Redis queue, network helpers, V3 pool pricing), and `infrastructure/ai/claude.rs::parse_decision`.
- CI (`.github/workflows/ci.yml`):
  - `cargo fmt --check`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo test --lib`
- **Gaps**: no actor integration tests. The actor pipeline is exercised via end-to-end manual runs but lacks automated coverage.

## 10. Known Caveats

- **Solana live execution not implemented.** Paper mode works (DexScreener for discovery and prices + simulated swap). Jupiter swap integration is on the roadmap.
- **Subgraph staleness** — mitigated by on-chain price validation on Ethereum, but a persistent discrepancy halts trading on a pair rather than falling back cleanly.
- **No backtesting harness** — paper trading is the only validation path.
- **Single-EVM** — Ethereum mainnet only on the EVM side. Network config layer has hooks for Polygon/Optimism/Arbitrum but they aren't wired into live execution.
- **Postgres TLS** — connection uses `NoTls` hardcoded. Adding TLS requires the `tokio-postgres-rustls` crate and a `tls_mode` field in `DatabaseConfig`.
- **Anomaly detection not implemented.** Placeholder event type exists; `RiskManager` would halt on high-severity anomalies, but the detector isn't written.
- **Bot runtime state** (`.mantis_state` PID file) lives in the working directory rather than an XDG state path.

## 11. Open Questions / Next Steps

- Solana live execution via Jupiter `/quote` + `/swap` endpoints. Largest scope item on the roadmap.
- Actor integration tests using mocked DEX/RPC clients. Highest-value test addition.
- Backtesting framework over historical price data — would let strategies be validated without running paper sessions.
- Anomaly detection (placeholder).
- Metrics/telemetry layer beyond logs (e.g., Prometheus exporter).
- Multi-EVM support: Polygon, Optimism, Arbitrum live execution.
