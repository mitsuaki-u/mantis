pub mod config;
pub mod eth_client;
pub mod pool_pricing;
pub mod providers;
pub mod rpc;
pub mod tokens;
pub mod transactions;

// Re-export the main client and commonly used types
pub use config::{get_network_addresses, validate_addresses, NetworkAddresses, NetworkConfig};
pub use eth_client::EthereumDexClient;
pub use providers::UniswapV3ProtocolProvider;
pub use tokens::{TokenRegistry, TokenRegistryService};
pub use transactions::{
    NetworkFeeInfo, SwapDirection, TransactionDetails, TransactionManager, TransactionPriority,
    TransactionStatus,
};
