//! Uniswap V3 Protocol Provider
//!
//! Refactored modular implementation:
//! - types.rs: Data structures
//! - abi.rs: Contract ABIs
//! - pool_cache.rs: Pool caching and selection
//! - quoter.rs: Quote operations
//! - gas.rs: Gas estimation and validation
//! - pricing.rs: Token/ETH price queries
//! - execution.rs: Swap execution logic
//! - provider.rs: Main provider implementation

// Refactored module declarations
mod abi;
mod execution;
mod gas;
mod pool_cache;
mod pricing;
mod provider;
mod quoter;
mod types;

// Public exports
pub use abi::load_weth_abi;
pub use gas::get_priority_multiplier;
pub use provider::UniswapV3ProtocolProvider;
pub use types::{GasEstimate, PoolInfo, SwapParams, V3FeeTier};
