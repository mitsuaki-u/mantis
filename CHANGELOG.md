# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.1.0]

Initial public release.

### Added
- Actor-based architecture: market data, strategy, AI advisor, risk manager, execution, database, supervisor.
- Typed event router with pub/sub routing between actors.
- Two trading strategies: momentum (composite RSI/MACD/Bollinger/volume) and RSI (oversold/overbought).
- Four indicator profiles (Scalping, DayTrading, SwingTrading, Standard) with per-profile period configuration.
- Risk management: stop-loss, take-profit, trailing stop, per-position and portfolio exposure limits, volatility filter, max drawdown, daily loss halt, gas/fee protection.
- Ethereum DEX integration: paper and live trading via Uniswap V3.
- Solana DEX integration: paper trading via DexScreener for token discovery and price data.
- AI advisor: an LLM reviews every BUY signal with technical indicators and portfolio state, returns APPROVE/REJECT with confidence and one-sentence reasoning.
- Indicator snapshot pipeline so the AI advisor sees the same indicator values the strategy used to decide.
- Prompt caching on the system prompt (~90% input cost reduction).
- Fail-open AI advisor: if the LLM API is unreachable, signals approve with confidence 50.
- 151 unit tests covering strategies, indicators, risk, financial calculations, validation, and AI response parsing.
- CI: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --lib`.
- Structured JSON logging.
- CLI: `mantis trading start/stop/status/positions/history`, `mantis config set/show/path`.
