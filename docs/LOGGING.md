# HoneyBadger Logging System

This document describes the logging system used in HoneyBadger.

## Overview

HoneyBadger uses a centralized logging system that provides:

- Structured logging (JSON format) for better parsing and analysis
- Consistent log levels with clear guidelines
- Automatic log file management
- Rate-limited logging to reduce noise
- Module-specific log level control

## Log Levels

The following log levels are used in the system:

- **ERROR**: Only for actual errors that indicate failure. These logs indicate something is wrong and requires attention.
- **WARN**: For concerning but non-fatal issues that need attention. These are potential problems that don't prevent the application from functioning.
- **INFO**: Important operational events, startup, shutdown, and significant state changes. These logs provide high-level insights into application behavior.
- **DEBUG**: Detailed information useful for debugging. These logs are more verbose and helpful for troubleshooting issues.
- **TRACE**: Very detailed diagnostics, typically disabled in production. These logs provide the most detailed view of application behavior.

## Configuration

Logging can be configured via:

1. Command line arguments:
   - `--debug`: Enable debug logging
   - `--log-level [level]`: Set log level (error, warn, info, debug, trace)
   - `--log-file [path]`: Write logs to a file
   - `--log-modules [modules]`: Filter logs by module (comma-separated)

2. Configuration file:
   - The `logs.directory` setting controls where log files are stored

## Log File Management

- Log files are automatically created for each command execution with a timestamp
- The format is: `{command}_{timestamp}.log`
- All logs are written to both stdout and the log file

## Rate Limiting

To reduce log volume, the system implements:

1. **Database logging rate limiting**: Only logs one in 20 database operations of the same type
2. **Retry operation rate limiting**: Limits repeating retry logs to avoid flooding

## Usage Guidelines

### When to use each log level:

- **ERROR**: Use for failures that prevent an operation from completing normally
- **WARN**: Use for potentially problematic situations that don't cause failure
- **INFO**: Use for significant operational events
- **DEBUG**: Use for detailed information useful for debugging
- **TRACE**: Use for the most detailed diagnostics

### Structured Logging

Logs are structured in JSON format with these fields:
- `timestamp`: ISO-8601 timestamp
- `level`: Log level (ERROR, WARN, etc.)
- `target`: Module path
- `message`: Log message

### Context in Log Messages

Include relevant context in log messages:
- For errors, include the operation being performed and what went wrong
- For state changes, include the before and after states
- For API operations, include request IDs or other correlation identifiers

## Implementation

The logging system is implemented in:
- `src/utils/logging/mod.rs`: Core logging implementation
- `src/bin/honeybadger.rs`: Main application logging setup
- `src/lib.rs`: Library logging initialization

## Logging Helpers

Several helper functions are available:

- `log_and_default(result, context, default)`: Log an error and return a default value
- `log_error(result, context)`: Log an error and return the error
- `generate_operation_id()`: Generate a unique ID for tracking related log messages 