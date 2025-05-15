# HoneyBadger Trading Bot - Supervisor System

The HoneyBadger trading bot implements a robust actor supervision system, providing enhanced reliability, monitoring, and fault tolerance. This document explains the Supervisor system's architecture, features, and usage.

## Overview

The Supervisor system implements the Actor Supervision pattern, providing centralized monitoring and management for all trading system actors. This pattern enhances reliability by enabling:

- Centralized actor lifecycle management
- Health monitoring of all system components
- Automatic recovery from transient failures
- Comprehensive health reporting
- Individual actor restart capability

## Architecture

The Supervisor system uses a hierarchical structure:

```
                    ┌─────────────────┐
                    │  TradingSystem  │
                    └────────┬────────┘
                             │
                             ▼
                    ┌─────────────────┐
                    │   Supervisor    │
                    └────────┬────────┘
                             │
            ┌────────────────┼────────────────┐
            ▼                ▼                ▼
   ┌─────────────┐  ┌─────────────┐  ┌─────────────┐
   │    Market   │  │   Strategy  │  │     Risk    │  ...
   │    Actor    │  │    Actor    │  │    Actor    │
   └─────────────┘  └─────────────┘  └─────────────┘
```

The `SupervisorActor` sits between the main `TradingBotSystem` and the individual actors, managing their lifecycle and monitoring their health.

## Features

### 1. Actor Lifecycle Management

The Supervisor provides centralized control for:

- Starting all actors in a coordinated manner
- Graceful shutdown of all actors
- Restarting individual actors that may be in a failed state
- Tracking actor state (running/stopped)

### 2. Health Monitoring

The Supervisor continuously monitors the health of all actors:

- Periodic health checks at configurable intervals
- Tracking of actor failures
- Categorized health status: Good, Degraded, Critical
- Automatic recovery attempts for critical actors
- Time-based failure tracking and trending

### 3. Health Reporting

Comprehensive health information is available:

- Overall system health status
- Individual actor status and metrics
- Failure counts and history
- Uptime and performance metrics

### 4. Fault Tolerance

The Supervisor implements fault tolerance through:

- Isolated actor failures (one actor's failure doesn't crash the system)
- Automatic recovery attempts for critical actors
- Failure tracking to identify problematic components
- Manual restart capability for persistent issues

## Command Line Interface

HoneyBadger provides command-line access to the Supervisor system:

### View System Status

```bash
honeybadger trading status
```

This command shows the general status of the trading system, including which actors are running.

### Health Report

```bash
honeybadger trading health
```

The health command displays detailed information about each actor:
- Running status
- Health status (Good, Degraded, Critical)
- Failure count
- System metrics

Example output:
```
📊 Supervisor Health Report
──────────────────────────

Actor Health Status:
  MARKET: Running | Health: Good | Failures: 0
  STRATEGY: Running | Health: Good | Failures: 0
  RISK: Running | Health: Degraded | Failures: 2
  EXECUTION: Running | Health: Good | Failures: 0
  DATABASE: Running | Health: Good | Failures: 0

System Health:
  Uptime: 3 hours, 45 minutes, 12 seconds
  Memory Usage: 124.32 MB
  Overall Health: Good
```

### Restart Actor

```bash
honeybadger trading restart <actor_id>
```

This command allows restarting an individual actor that may be in a problematic state. Valid actor IDs are:
- market
- strategy
- risk
- execution
- database

Example:
```bash
honeybadger trading restart risk
```

## Implementation Details

### SupervisorActor

The `SupervisorActor` class is implemented in `src/actors/supervisor.rs` and provides:

- Actor registration and management
- Health monitoring through periodic checks
- Failure tracking and recovery
- Health metrics collection

Key methods:
- `register_actor()` - Adds an actor to the supervision system
- `start_all_actors()` - Starts all registered actors
- `stop_all_actors()` - Stops all registered actors
- `restart_actor()` - Restarts a specific actor
- `watch_actors()` - Starts the health monitoring background task
- `get_health_report()` - Generates a detailed health status report

### Health Monitoring

The Supervisor monitors actor health through:

1. Periodic status queries to each actor
2. Tracking response times and failures
3. Categorizing health status based on failure patterns:
   - **Good**: No recent failures
   - **Degraded**: Some failures but not critical
   - **Critical**: Multiple recent failures or non-responsive

### Health Status Calculation

Actor health status is determined by:
- Number of recent failures
- Time since last failure
- Response time trends
- Critical function availability

### TradingBotSystem Integration

The `TradingBotSystem` in `src/trading/bot.rs` integrates with the Supervisor by:
- Creating the Supervisor during initialization
- Registering all actors with the Supervisor
- Using the Supervisor for actor lifecycle operations
- Exposing Supervisor functions through its API

## Use Cases

### Scenario 1: System Startup

When starting the trading system:
1. The Supervisor is initialized first
2. Individual actors are created and registered
3. The Supervisor starts all actors in the correct order
4. The watch_actors() method begins health monitoring

### Scenario 2: System Shutdown

During system shutdown:
1. The Supervisor receives the stop command
2. All actors are stopped in an orderly fashion
3. Resources are properly released

### Scenario 3: Actor Failure

If an actor fails:
1. The Supervisor detects the failure during health checks
2. The actor's health status is updated to reflect the failure
3. For critical actors, automatic recovery may be attempted
4. The failure is logged and reported in health status

### Scenario 4: Manual Intervention

For persistent issues:
1. The user runs the health command to identify problematic actors
2. The user can restart specific actors that are in a degraded state
3. Health status is updated to reflect the manual intervention

## Best Practices

1. **Regular Health Checks**: Run `honeybadger trading health` periodically to ensure all components are working properly.

2. **Proactive Restarts**: If you notice an actor in a "Degraded" status, consider restarting it before it becomes critical.

3. **Monitoring During Development**: When implementing custom strategies, regularly check the health status to ensure your code isn't causing instability.

4. **Log Analysis**: Combine health reports with log analysis for comprehensive troubleshooting.

## Future Enhancements

Planned improvements to the Supervisor system:

1. Hierarchical supervision (supervisors can supervise other supervisors)
2. More sophisticated recovery strategies
3. Detailed performance metrics per actor
4. Resource usage tracking and limiting
5. Configurable recovery policies per actor

## Conclusion

The Supervisor system significantly enhances the reliability and operability of the HoneyBadger trading bot by providing robust actor management, health monitoring, and fault tolerance capabilities. By leveraging this system, both users and developers can ensure more reliable trading operations and easier troubleshooting when issues arise. 