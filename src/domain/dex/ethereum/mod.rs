pub mod abi;
pub mod client;
pub mod network;
pub mod price;
pub mod protocols;
pub mod transaction;

// Re-export the main client and commonly used types
pub use client::EthereumDexClient;
pub use network::NetworkConfig;
pub use price::PriceOracle;
pub use protocols::{DexProtocol, SwapParams, UniswapV2Protocol};
pub use transaction::TransactionManager;
