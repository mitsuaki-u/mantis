# Mantis Architecture

> Working document. Captures the current shape of the system, the critical components, and the known trouble spots. File references are `path:line` where relevant; treat line numbers as approximate — they drift with refactors.

## 1. Purpose & Scope

Mantis is a Rust-based automated trading bot for Uniswap V3 on Ethereum mainnet. It discovers tokens, runs technical-indicator strategies, validates signals against risk limits, and executes swaps (paper or live). Paper trading is the default and the currently recommended use.

## 2. Layered Architecture

The crate follows clean architecture with three layers:

```
src/
├── core/                  Domain logic. No infra/external deps. Pure functions where possible.
├── infrastructure/        External adapters: RPC, DB, cache, subgraph, DEX.
├── application/           Actors + event plumbing. Orchestrates core + infra.
└── bin/cli/               Clap-based CLI, config loading, bootstrap.
```

Dependency rule: `application` depends on `core` and `infrastructure`; `infrastructure` depends on `core`; `core` depends on nothing in this crate. `bin/cli` wires everything together.

## 3. Directory Map

### `src/core/` — domain
- `domain/trading.rs` — `Signal`, `Position`, `ExitReason`
- `domain/market.rs` — `TokenMetrics` (OHLCV + liquidity/volume)
- `domain/token.rs`, `domain/wallet.rs`, `domain/dex.rs`, `domain/strategy_params.rs`
- `strategies/traits.rs` — `TradingStrategy` trait + `Strategy` enum (Momentum | Rsi)
- `strategies/custom/momentum.rs`, `strategies/custom/rsi.rs` — concrete strategies
- `strategies/exit_conditions.rs` — stop-loss / take-profit / trailing stop logic
- `strategies/factory.rs` — constructs strategy from config
- `indicators/mod.rs` — `PriceTimeSeries`, RSI/MACD/Bollinger/Volume
- `risk/assessment.rs` — position sizing, portfolio risk factors
- `risk/limits.rs` — overall limits, halt checks, trading-allowed gate
- `risk/metrics.rs` — risk calculations
- `calculations/financial.rs` — PnL, ROI
- `calculations/uniswap_v3.rs` — V3 sqrtPrice → price math
- `utils/conversion.rs` — safe `Decimal` ↔ `f64`
- `utils/validation/price.rs` — subgraph vs on-chain price discrepancy check
- `utils/validation/orders.rs` — buy/sell order validation

### `src/infrastructure/`
- `dex/client.rs` — `DexClient` enum (`Paper` | `Live`), top-level swap interface
- `dex/ethereum/eth_client.rs` — wallet, RPC, WETH wrap/unwrap
- `dex/ethereum/rpc.rs` — `eth_getBalance`, `eth_call`, etc.
- `dex/ethereum/pool_pricing.rs` — `slot0()`, liquidity reads
- `dex/ethereum/config/addresses.rs` — WETH/USDC/router addresses per network
- `dex/ethereum/providers/uniswap_v3/` — full V3 provider: `provider.rs`, `execution.rs`, `pricing.rs`, `quoter.rs`, `pool_cache.rs`, `gas.rs`, `abi.rs`, `types.rs`
- `dex/ethereum/transactions/manager.rs` — tx status polling / mempool tracking
- `market/providers/alchemy/` — Uniswap V3 subgraph provider (`provider.rs`, `graphql.rs`, `pricing.rs`, `quality.rs`, `types.rs`)
- `market/providers/queries.rs` — GraphQL query strings
- `database/pool.rs` — deadpool-postgres connection pool
- `database/schema/tables.rs` — table definitions
- `database/repositories/` — `TokenRepository`, `PositionRepository`, `TradeRepository`, `TransactionRepository`
- `database/queries/` — raw SQL
- `database/queue.rs` — Redis-backed `PositionUpdateQueue` for batched writes
- `cache/` — Redis client, token cache, batch ops
- `logging/mod.rs` — tracing-subscriber init
- `network/`, `retry/` — HTTP helpers, retry-with-backoff

### `src/application/src/`
- `app.rs` — `TradingBotSystem` bootstrap, actor registration, routing config
- `events.rs` — all event types (see §5)
- `actors/system/` — `Actor` trait, lifecycle, `EventRouter`
- `actors/supervisor/` — `SupervisorActor` (lifecycle coordinator)
- `actors/market/` — `MarketDataActor`
- `actors/strategy/` — `StrategyActor`
- `actors/risk_manager/` — `RiskManagerActor` (+ `limits.rs`, `operations.rs`)
- `actors/execution/` — `ExecutionActor` (+ `orders.rs`, `positions.rs`, `tasks.rs`)
- `actors/database/` — `DatabaseActor` (+ `queuing.rs`, `tasks.rs`)

### `src/bin/cli/src/`
- `mantis.rs` — entry point, command dispatch
- `bootstrap.rs` — builds DB pool, cache, DEX client, actors
- `overrides.rs` — apply CLI flags onto loaded config
- `config/` — defaults, file I/O, env vars, validation, API keys, networks
- `commands/trading/` — `start`, `stop`, `status`, `history`, `positions`, `transactions`
- `commands/config/` — `show`, `set`, `get`, `reset`, `set-key`
- `commands/database/` — DB maintenance

## 4. Actor System

All actors implement the `Actor` trait (`application/src/actors/system/actor.rs`) and communicate through a central `EventRouter` (`application/src/actors/system/router.rs`) using broadcast channels. Routing is wired in `application/src/app.rs`.

| Actor | Publishes | Consumes |
|---|---|---|
| `SupervisorActor` | lifecycle commands | — |
| `MarketDataActor` | `MarketEvent::PriceUpdate`, `MarketEvent::PoolsDiscovered` | — (poll-driven) |
| `StrategyActor` | `StrategyEvent::Signal` | `MarketEvent::PriceUpdate` |
| `RiskManagerActor` | `RiskEvent::{TradeApproved, TradeSizeAdjusted, RiskLimitExceeded, PositionCreated, PositionClosed}` | `StrategyEvent::Signal`, `RiskEvent::{PositionCreated, PositionClosed}` |
| `ExecutionActor` | `ExecutionEvent::{OrderExecuted, OrderFailed}`, `DexTransactionEvent::{Submitted, StatusUpdated}` | `RiskEvent::TradeApproved`, `RiskEvent::PositionClosed` |
| `DatabaseActor` | — (write-only) | all events |

Routing graph (from `app.rs`):
- `MarketEvent` → `StrategyActor`, `DatabaseActor`
- `StrategyEvent` → `RiskManagerActor`, `DatabaseActor`
- `RiskEvent` → `ExecutionActor`, `DatabaseActor`
- `ExecutionEvent` → `DatabaseActor`
- `DexTransactionEvent` → `DatabaseActor`

## 5. Event Model

Defined in `application/src/events.rs`. Top-level `Event` enum wraps:

- `MarketEvent::PriceUpdate { token_id, price_usd, volume_24h, liquidity, timestamp, … }`
- `MarketEvent::PoolsDiscovered { pools }`
- `StrategyEvent::Signal { token_id, signal: Buy|Sell|Hold|NoAction, correlation_id, signal_price, metadata }`
- `RiskEvent::TradeApproved { token_id, signal, position_size_usd, correlation_id }`
- `RiskEvent::TradeSizeAdjusted { original, adjusted, reason }`
- `RiskEvent::RiskLimitExceeded { limit_type, current_value, threshold }`
- `RiskEvent::PositionCreated { token_id, entry_price, size }`
- `RiskEvent::PositionClosed { token_id, exit_reason }`
- `ExecutionEvent::OrderExecuted { token_id, side, executed_value_usd, token_quantity, tx_hash, fees_usd, entry_price?, entry_time?, realized_pnl? }`
- `ExecutionEvent::OrderFailed { token_id, reason }`
- `DexTransactionEvent::{Submitted, StatusUpdated}`

Every signal carries a `correlation_id` (UUID) so a trade can be traced Signal → Approved → Executed.

## 6. Trading Pipeline (end-to-end)

### 6.1 Token discovery (`AlchemyUniswapV3Provider`)
1. GraphQL query to Satsuma-hosted Uniswap V3 subgraph. Filters: `tvl > min_liquidity`, pool age ≥ ~6 months, `txCount >= min_pool_transaction_count`.
2. Keep only WETH-paired pools.
3. Second query (`poolDayDatas`) enriches with 24h volume.
4. Drop pools with `volume < config.trading.min_volume`.
5. Build `TokenMetrics` (symbol, decimals, on-chain price from `sqrtPriceX96`, volume, liquidity, 24h change).

### 6.2 Market polling (`MarketDataActor`)
- Every `scan_interval_secs`, call `MarketDataProvider::fetch_tokens_and_metrics()`.
- For each token: update strategy's `PriceTimeSeries`, publish `PriceUpdate`.

### 6.3 Signal generation (`StrategyActor`)
- Volume gate: reject if `volume_24h < strategy.min_volume()`.
- Cooldown: reject if token recently signaled.
- Indicators computed from the selected **indicator profile** (scalping / day_trading / swing_trading / standard).
- `analyze_for_entry(token)` → bool; if an open position exists, `analyze_for_exit(token, position, risk_params)` → `Option<ExitReason>`.
- **On-chain price validation** (before BUY, added in commit `8dcffbf`): reject if `|subgraph_price − slot0_price| / slot0_price > 5%`.
- Publish `StrategyEvent::Signal`.

### 6.4 Risk validation (`RiskManagerActor`)
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

### 6.5 Execution (`ExecutionActor`)

**Buy** (`actors/execution/orders.rs`):
1. Receive `TradeApproved`.
2. Fetch `TokenData` from DB; assert `token_id` matches (guards against wrong-token execution).
3. Validate order (min size, slippage, price range) via `core/utils/validation/orders.rs`.
4. **USD → WETH conversion** (critical, commit `fd294b1`): `weth_amount = position_size_usd / eth_price_usd`.
5. `DexClient::ensure_weth_balance(required_weth, …)` — wraps ETH if live; no-op for paper.
6. `DexClient::execute_swap(WETH → token, weth_amount, slippage)` → `TransactionDetails`.
7. `PositionRepository::create_position(…)`.
8. Publish `ExecutionEvent::OrderExecuted`.

**Sell**:
1. Verify position exists (silent skip if not).
2. Execute swap token → WETH.
3. Compute realized PnL and ROI.
4. `PositionRepository::record_close(…)`.
5. Publish `ExecutionEvent::OrderExecuted` with entry/exit fields.

### 6.6 Position tracking
- RiskManager marks-to-market on every `PriceUpdate` to update unrealized PnL and portfolio total.
- Exit conditions (`core/strategies/exit_conditions.rs`): take-profit %, stop-loss %, trailing-stop %, strategy-based exits, max-hold time.

### 6.7 Transaction monitoring
`ExecutionActor::tasks.rs` polls `DexClient::get_transaction_status(tx_hash)` on a timer, emits `DexTransactionEvent::StatusUpdated`; `DatabaseActor` persists final state.

## 7. Critical Components

### Risk management
- `core/risk/assessment.rs`: `compute_position_size`, `calculate_portfolio_risk_factors`, `check_token_volatility`, `has_valid_market_data`.
- `core/risk/limits.rs`: `check_overall_risk_limits`, `check_trading_allowed`, halt/resume token trading.
- `application/src/actors/risk_manager/operations.rs`: `check_token_risk`, `update_risk_metrics`, `reset_daily_metrics`.
- State held inline in `RiskManagerActor`: `HashMap<token_id, PositionDetails>`, cumulative daily PnL, peak equity for drawdown.

### DEX client (Uniswap V3)
- `DexClient` enum — `Paper { simulated_balance }` | `Live { ethereum_client }`.
- `EthereumDexClient`: wallet, RPC endpoints, `ensure_weth_balance`, `get_native_balance`.
- `UniswapV3ProtocolProvider`:
  - `pool_cache.rs` — pool discovery cache populated from `PoolsDiscovered`.
  - `quoter.rs` / `pricing.rs` — QuoterV2 integration, direct + multi-hop routing through WETH/USDC.
  - `execution.rs` — builds `exactInputSingle` / `exactInputMultihop` router calls.
  - `gas.rs` — pre-trade gas estimate, checked against `max_gas_cost_usd` and `max_gas_cost_percentage`.

### Price validation
- `core/utils/validation/price.rs` — compares subgraph price vs on-chain `slot0` price; rejects if discrepancy > 5%. Adds ~1 RPC per potential trade.

### Position sizing & USD ↔ WETH
- `core/utils/conversion.rs` — validated `Decimal` ↔ `f64`.
- Sizing lives in `core/risk/assessment.rs`; the USD→WETH conversion is in `ExecutionActor`'s buy path. Both were points of critical bugs (commits `fd294b1`, `403061a`) — keep tests green here.

### Persistence (Postgres)
Tables: `tokens`, `positions`, `trades`, `prices`, `transactions`. `PositionRepository` carries an `is_paper_trade` flag so paper and live coexist without bleeding into each other. `PositionUpdateQueue` (Redis list) batches updates; `database::task_handler` drains to Postgres.

### Cache (Redis)
Optional. Used for (a) token metadata / TokenMetrics cache, (b) position-update queue. Graceful degradation if unavailable.

### Indicator profiles
Selected via `trading.indicator_profile`. Each profile sets periods for RSI, MACD (fast/slow/signal), Bollinger, volume. Drives warmup duration and indicator sensitivity. Profiles currently: `scalping` (fastest), `day_trading` (recommended default), `swing_trading`, `standard`.

### Strategy plug-ins
`TradingStrategy` trait + `Strategy` enum dispatch (`Momentum` | `Rsi`). Enum wrapping avoids `Box<dyn Trait>` + `Clone` issues. `strategies/factory.rs` constructs the strategy from config.

### Config loading
Hierarchy (later overrides earlier):
1. `config/defaults.rs`
2. JSON config file (`~/.config/mantis/config.json` on macOS/Linux)
3. Env vars (`config/env.rs`)
4. CLI flags (`overrides.rs`)

Validation runs after each merge (`config/validation.rs`).

### Logging
`tracing` + `tracing-subscriber` with env-filter. `RUST_LOG` tuned per-module; `tokio_postgres=info` is the common knob to hide query-prep noise. Console + rotating file sinks.

## 8. External Integrations

- **Alchemy / Satsuma**: Uniswap V3 community subgraph over GraphQL. No API key needed for subgraph reads; `api_keys.alchemy` is used for the RPC endpoint.
- **Ethereum RPC**: via `ethers`, connected through Alchemy. Used for `eth_call` (slot0, quoter), `eth_getBalance`, tx submission, receipt polling.
- **Uniswap V3**: SwapRouter02, QuoterV2, pool slot0 reads. Addresses per-network in `dex/ethereum/config/addresses.rs`.
- **Postgres**: via `tokio-postgres` + `deadpool-postgres`. Schema initialized on first run.
- **Redis**: via `redis` + `deadpool-redis`. Optional.

## 9. Tests

- Unit tests are inline `#[cfg(test)]` across `core/` (conversion, validation, formatting, financial math, exit conditions, V3 math, strategy params).
- CLI config: tests for file I/O, env loading, validation, defaults, networks, API keys.
- Infra: tests for DB pool, Redis queue, network helpers, V3 pool pricing.
- Integration: `tests/common/mod.rs` contains shared fixtures; limited end-to-end coverage.
- **Gaps**: no actor integration tests; no mocked on-chain execution tests. Both are high-value adds.

## 10. Known Caveats

Carried forward from `TODO.md` and recent commit history:

- **Market anomaly detection not implemented** (TODO). Placeholder event type exists; `RiskManager` would halt on high-severity anomalies, but `AnomalyDetector` isn't written. See `TODO.md`.
- **`infrastructure/execution/types.rs`** — flagged as possibly unnecessary (TODO).
- **`mod.rs` style** — repo still uses `mod.rs`; TODO to migrate to the newer Rust style.
- **Subgraph staleness** — mitigated by on-chain price validation, but a persistent discrepancy halts trading on a pair rather than falling back cleanly.
- **Single-chain** — Ethereum mainnet only. Network config layer exists but Alchemy provider and addresses are mainnet-tuned.
- **No backtesting harness**.
- **Error recovery** — some swap/tx edge cases currently require manual intervention.
- **Postgres TLS** — connection uses `NoTls` hardcoded. Adding TLS requires the `tokio-postgres-rustls` crate and a `tls_mode` field in `DatabaseConfig`. Not yet implemented.
- **Cleanup items** (TODO.md): unify trading-start logging, auto-create DB if missing, verify `update_position_metrics` usage, extract GraphQL queries and contract addresses, audit fallback logic, ensure `decimals` field used correctly, add Postgres TLS, audit `Cargo.toml` for unused crates.

## 11. Recent Trouble Spots (from git log)

Worth knowing before touching these areas:

- `fd294b1` — USD was treated directly as WETH amount; fixed by converting via live ETH price in `estimate_swap_output()`. Added balance validation.
- `8dcffbf` — on-chain price validation vs subgraph; rejects BUYs on >5% discrepancy. Also removed dead `check_risk_limits()` placeholder.
- `00846f5` — position-reservation race: more positions opened than `max_positions`. Fixed in `RiskManager`.
- `403061a` — `V3_FEE_TIER_DIVISOR` was 100× too small, corrupting fee math.
- `3c5ace1` — risk-management config wasn't reaching `StrategyActor` exit conditions.
- `65ee8b0` — multiple `Decimal`/`f64` conversion bugs in `RiskManager`.
- `adcb8ce` — base token switched from ETH to WETH; ETH is now wrapped before swaps.
- `893f762` — paper trading is now default; `--live` opts into live.

## 12. Open Questions / Next Steps

- Anomaly detection (TODO.md, ~250 LOC). Clear place to land next.
- Persistent cache warmup / subgraph retry policy isn't documented end-to-end.
- No metrics/telemetry layer beyond logs; a Prometheus exporter could replace some of the hand-rolled polling tasks.
- Secrets handling: `mainnet_config.json` in repo root contains what appears to be a live Alchemy key and is untracked. If real, rotate and gitignore.
