# Honeybadger Codebase Restructuring Plan

## Current Structure Analysis

The current Honeybadger project structure has these main components:

```
honeybadger/
├── src/
│   ├── actors/         # Actor-based concurrency system
│   ├── api/            # External API integrations
│   ├── bin/            # Additional binaries
│   ├── cache/          # Caching implementation
│   ├── commands/       # CLI command handling
│   ├── data/           # Data models and processing
│   ├── db/             # Database operations
│   ├── dex/            # DEX (decentralized exchange) functionality
│   ├── display/        # Output formatting and UI
│   ├── repositories/   # Data access layer
│   ├── trading/        # Trading logic and strategies
│   ├── types/          # Common type definitions
│   ├── utils/          # Helper utilities
│   ├── config.rs       # Configuration management
│   ├── error.rs        # Error handling
│   ├── lib.rs          # Library entry point
│   └── main.rs         # CLI entry point
├── Cargo.toml          # Project dependencies
├── Cargo.lock          # Locked dependencies
└── Various .md files   # Documentation
```

## Proposed Rust-Idiomatic Structure

Based on Rust ecosystem best practices and to improve maintainability, we propose the following structure:

```
honeybadger/
├── src/                      # Main library code
│   ├── bin/                  # Binary entry points
│   │   └── honeybadger.rs    # Main CLI binary (was main.rs)
│   │   └── test_dex.rs       # Test DEX binary
│   ├── cli/                  # Command-line interface
│   │   ├── commands/         # Command definitions & handlers
│   │   ├── display/          # Terminal output formatting
│   │   └── mod.rs            # CLI module entry
│   ├── core/                 # Core business logic
│   │   ├── config/           # Configuration management
│   │   ├── error/            # Error types & handling
│   │   ├── models/           # Core domain models
│   │   └── mod.rs            # Core module entry
│   ├── domain/               # Domain-specific modules
│   │   ├── market/           # Market analysis
│   │   ├── trading/          # Trading functionality
│   │   │   ├── strategy/     # Trading strategies
│   │   │   ├── execution/    # Trade execution
│   │   │   ├── indicators/   # Technical indicators
│   │   │   ├── risk/         # Risk management
│   │   │   └── mod.rs        # Trading module entry
│   │   ├── dex/              # DEX interaction
│   │   ├── wallet/           # Wallet management
│   │   └── mod.rs            # Domain module entry
│   ├── infra/                # Infrastructure
│   │   ├── api/              # External API clients
│   │   ├── db/               # Database layer
│   │   │   ├── models/       # Database models
│   │   │   ├── repositories/ # Data access patterns
│   │   │   ├── migrations/   # Schema migrations
│   │   │   └── mod.rs        # DB module entry
│   │   ├── cache/            # Caching layer
│   │   ├── actors/           # Actor system
│   │   └── mod.rs            # Infrastructure module entry
│   ├── utils/                # Common utilities
│   │   ├── concurrency/      # Concurrency utilities
│   │   ├── logging/          # Logging utilities
│   │   └── mod.rs            # Utils module entry
│   └── lib.rs                # Library entry point
├── examples/                 # Example usage
├── benches/                  # Performance benchmarks
├── tests/                    # Integration tests
├── docs/                     # Documentation
│   ├── CONFIG.md             # Configuration documentation
│   ├── DEX_SUPPORT.md        # DEX support documentation
│   └── etc.                  # Other documentation
├── Cargo.toml                # Project dependencies
├── README.md                 # Project readme
└── .gitignore                # Git ignore file
```

## File Mapping: Current → Proposed

Below is a mapping of existing files to their new locations:

### Binaries & Entry Points
- `src/main.rs` → `src/bin/honeybadger.rs`
- `src/lib.rs` → `src/lib.rs` (updated with new module structure)
- `src/bin/test_dex.rs` → `src/bin/test_dex.rs` (unchanged location)

### Core Components
- `src/error.rs` → `src/core/error/mod.rs`
- `src/config.rs` → `src/core/config/mod.rs`
- `src/types/` → `src/core/models/`

### CLI & Display Components
- `src/commands/` → `src/cli/commands/`
- `src/display/` → `src/cli/display/`

### Domain Logic
- `src/trading/strategy.rs` → `src/domain/trading/strategy/mod.rs`
- `src/trading/indicators.rs` → `src/domain/trading/indicators/mod.rs`
- `src/trading/bot.rs` → `src/domain/trading/execution/bot.rs`
- `src/trading/risk.rs` → `src/domain/trading/risk/mod.rs`
- `src/trading/analysis.rs` → `src/domain/trading/analysis.rs`
- `src/trading/execution.rs` → `src/domain/trading/execution/mod.rs`
- `src/trading/mod.rs` → `src/domain/trading/mod.rs`

### Infrastructure
- `src/api/` → `src/infra/api/`
- `src/db/` → `src/infra/db/`
- `src/db/schema/` → `src/infra/db/schema/` (database schema definitions)
- `src/repositories/` → `src/infra/db/repositories/`
- `src/cache/` → `src/infra/cache/`
- `src/actors/` → `src/infra/actors/`
- `src/data/collector.rs` → `src/infra/datacollector/collector.rs` (data collection service)
- `src/dex/` → `src/domain/dex/`

### Other
- `src/utils/` → `src/utils/` (maintain structure but reorganize internals)

## Documentation Files
- All .md files except README.md → `docs/` directory
- README.md → Remains at root

## Migration Strategy

To implement this restructuring:

1. Create the new directory structure
2. Move files according to the mapping above, updating their module paths
3. Update import statements in all files to reflect new paths
4. Update Cargo.toml to reflect new binary locations
5. Test extensively to ensure no functionality is broken

## Benefits of the New Structure

1. **Clearer Separation of Concerns**:
   - Core business logic is separated from infrastructure
   - Domain logic is organized by business domain
   - Infrastructure concerns are isolated

2. **Improved Modularity**:
   - Each module has a clear responsibility
   - Easier to understand and maintain
   - Better encapsulation of implementation details

3. **Better Testability**:
   - Core logic separated from external dependencies
   - Easier to mock dependencies for testing
   - Clearer boundaries between components

4. **More Rust-Idiomatic**:
   - Follows common patterns from the Rust ecosystem
   - Better use of modules and visibility
   - Clearer entry points for binaries and libraries

5. **Better Developer Experience**:
   - Easier to find code related to specific features
   - More intuitive organization for new contributors
   - Documentation is better organized

## Implementation Notes

1. This restructuring focuses on organization without changing functionality
2. Some modules may need to be split into smaller, more focused components
3. Public APIs should remain stable during migration
4. Internal module paths will need to be updated throughout the codebase

## Future Enhancements

After restructuring, consider these follow-up improvements:

1. Add proper error types per domain instead of a single global error type
2. Implement proper feature flags in Cargo.toml for optional components
3. Improve test coverage with the new structure
4. Consider workspace structure for larger components if they grow 