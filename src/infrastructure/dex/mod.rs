pub mod client;
pub mod ethereum;

// Re-export key types
pub use client::DexClient;
pub use ethereum::{
    EthereumDexClient, NetworkFeeInfo, SwapDirection, TransactionDetails, TransactionPriority,
    TransactionStatus, UniswapV3ProtocolProvider,
};

/// Parameters for executing a token swap
pub struct SwapParams<'a> {
    pub token_in: &'a str,
    pub token_out: &'a str,
    pub amount_in: f64,
    pub slippage_tolerance: f64,
    pub price_limit: Option<f64>,
    pub priority: TransactionPriority,
    pub direction: SwapDirection,
}
