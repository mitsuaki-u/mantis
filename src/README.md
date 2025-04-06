# HoneyBadger Actor Architecture Integration

This directory contains the implementation of HoneyBadger trading bot with actor-based architecture. The main components are:

## Actor Architecture

The actor architecture provides a concurrent, event-driven programming model that avoids many common concurrency pitfalls like deadlocks, race conditions, and callback hell. Key components:

- `actors/mod.rs` - Core actor definitions including Actor trait, MessageBus, and actor references
- `actors/market.rs` - MarketDataActor for handling market data and WebSocket connections
- `actors/strategy.rs` - StrategyActor for analyzing market data and generating trading signals
- `actors/risk.rs` - RiskManagerActor for validating trade decisions against risk parameters
- `actors/execution.rs` - ExecutionActor for executing trades via DEX or paper trading
- `actors/database.rs` - DatabaseActor for persisting trading data and positions
- `actors/supervisor.rs` - SupervisorActor for managing the lifecycle of other actors

## Integration with Trading Bot

The actor-based architecture has been integrated with the existing trading bot:

- `trading/bot.rs` - TradingBotSystem class that implements the actor-based trading bot
- `commands/trading.rs` - Updated command handlers to use the actor-based trading bot

## Event Types and Communication

Communication between actors happens via strongly-typed events and commands:

- Market events: price updates, volume changes, etc.
- Strategy events: signals, pattern detections
- Risk events: validations, limit changes
- Execution events: order executions, fills, rejections
- Database events: persistence confirmations

## Benefits of the Actor Approach

1. **Concurrency** - Each actor runs independently and concurrently, improving performance
2. **Fault isolation** - Failures in one actor don't cascade to others
3. **Flexibility** - Easy to add, remove, or swap actors without affecting others
4. **Scalability** - Actors can be distributed across threads, cores, or even machines
5. **Simplified reasoning** - Each actor focuses on a specific task, making code more maintainable

## Real-time Data Integration

The actor architecture is well-suited for handling real-time data streams:

- WebSocket connections are managed by the MarketDataActor
- Events flow through the system asynchronously
- Multiple data sources can be easily integrated (CoinCap, Binance, etc.)

## Running the Actor-Based Trading Bot

```bash
# Start the trading bot with momentum strategy
honeybadger trading start --strategy momentum --threshold 5.0 --dry-run

# Check the status
honeybadger trading status

# Stop the bot
honeybadger trading stop
``` 