use crate::core::error::{Error, Result}; // Standardized import
use chrono::{DateTime, Duration, Utc};
use log::{info, warn};
use serde::Serialize;
use std::sync::Arc;

// ADDED: Import MessageBus
use crate::infra::actors::MessageBus;

// Export the ethereum module for Ethereum and EVM-compatible chain support
pub mod ethereum;

// Re-export the EthereumDexClient for Ethereum and EVM-compatible chains
pub use ethereum::EthereumDexClient;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TransactionDetails {
    pub tx_id: String,
    pub amount_in: f64, // Amount of the input token
    pub token_in_address: String,
    pub amount_out: f64, // Amount of the output token received
    pub token_out_address: String,
    pub actual_price: f64,         // Effective price of the swap
    pub fees_paid: f64,            // Fees paid in native currency or quote token
    pub fee_currency: String,      // e.g. "ETH", "USDC"
    pub gas_used: Option<u64>,     // Gas used by the transaction
    pub gas_price: Option<f64>,    // Gas price in gwei
    pub block_number: Option<u64>, // Block number where tx was mined
    pub confirmation_time: Option<chrono::DateTime<chrono::Utc>>, // When tx was confirmed
}

// NEW: Supporting enum for TransactionPriority
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub enum TransactionPriority {
    Low,
    Standard,
    High,
    Urgent,
}

// NEW: Supporting struct for TransactionMetrics
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TransactionMetrics {
    pub gas_price_gwei: f64,
    pub gas_used: u64,
    pub block_time: Duration, // Consider if this is std::time::Duration or chrono::Duration
    pub confirmation_time: Duration, // Same as above
    pub network_congestion: f64, // e.g., a score or percentage
}

#[derive(Debug, Clone, PartialEq)]
pub enum TransactionStatus {
    Queued {
        tx_id: String,
        submission_time: DateTime<Utc>,
        priority: TransactionPriority, // UPDATED
    },
    Pending {
        tx_id: String,
        submission_time: DateTime<Utc>,
        last_checked: DateTime<Utc>,
        block_height: Option<u64>, // NEW
        retry_count: u32,          // NEW
    },
    Confirmed {
        details: TransactionDetails,
        confirmations: u64,
        required_confirmations: u64,
        finality_probability: f64, // NEW
    },
    Success {
        details: TransactionDetails,
        execution_time: Duration, // NEW - Using chrono::Duration
        gas_efficiency: f64,      // NEW (e.g., amount_out / gas_used)
    },
    Failed {
        tx_id: String,
        reason: String,
        error_code: Option<String>,
        gas_used: Option<u64>,
        revert_reason: Option<String>,
        recovery_suggestion: Option<String>, // NEW
    },
    Dropped {
        tx_id: String,
        reason: String,
        replacement_tx: Option<String>,
        gas_price_delta: Option<f64>,    // NEW
        network_congestion: Option<f64>, // NEW
    },
}

#[derive(Debug, Clone)]
pub struct NetworkFeeInfo {
    pub gas_price_gwei: Option<f64>, // Optional, might not apply to all networks/DEXs
    pub estimated_fee_usd: f64,      // Estimated fee in USD for a typical transaction
    pub fee_currency_symbol: String, // e.g. ETH, MATIC
}

#[derive(Clone)]
pub enum DexClient {
    Paper {
        simulated_eth_balance: f64,
        simulated_default_token_balance: f64,
        simulated_stablecoin_symbol: String,
        simulated_stablecoin_balance: f64,
    },
    Ethereum(Box<ethereum::EthereumDexClient>),
}

impl DexClient {
    pub fn new_paper_trading(config: &crate::config::Config) -> Result<Self> {
        Ok(DexClient::Paper {
            simulated_eth_balance: config.dex.paper_simulated_eth_balance,
            simulated_default_token_balance: config.dex.paper_simulated_default_token_balance,
            simulated_stablecoin_symbol: config.dex.paper_simulated_stablecoin_symbol.clone(),
            simulated_stablecoin_balance: config.dex.paper_simulated_stablecoin_balance,
        })
    }

    pub fn new_ethereum(
        config: &crate::config::Config,
        message_bus: Arc<MessageBus>,
    ) -> Result<Self> {
        let client = ethereum::EthereumDexClient::new(config, message_bus)?;
        Ok(DexClient::Ethereum(Box::new(client)))
    }

    pub async fn connect_wallet(&mut self, private_key: &str) -> Result<()> {
        match self {
            DexClient::Ethereum(client) => client.connect_wallet(private_key).await,
            DexClient::Paper { .. } => {
                info!("Paper trading: Wallet connection simulated.");
                Ok(())
            }
        }
    }

    pub async fn get_native_balance(&self) -> Result<f64> {
        match self {
            DexClient::Paper {
                simulated_eth_balance,
                ..
            } => {
                info!(
                    "Paper trading: Returning simulated ETH balance: {}",
                    simulated_eth_balance
                );
                Ok(*simulated_eth_balance)
            }
            DexClient::Ethereum(client) => client.get_native_balance().await,
        }
    }

    pub async fn get_token_balance(&self, token_address: &str) -> Result<f64> {
        match self {
            DexClient::Paper {
                simulated_default_token_balance,
                simulated_stablecoin_symbol,
                simulated_stablecoin_balance,
                ..
            } => {
                let addr_lower = token_address.to_lowercase();
                // Example: Check against a known paper trading stablecoin address or symbol
                // This part depends on how paper trading identifies its stablecoin.
                // We'll use the configured paper_simulated_stablecoin_symbol.
                if addr_lower == "0xstablecoin_paper_address"
                    || addr_lower == simulated_stablecoin_symbol.to_lowercase()
                {
                    info!(
                        "Paper trading: Returning simulated stablecoin balance {} for {}",
                        simulated_stablecoin_balance, token_address
                    );
                    return Ok(*simulated_stablecoin_balance);
                }
                // For any other token in paper mode, return the default simulated token balance
                info!(
                    "Paper trading: Returning default simulated token balance {} for {}",
                    simulated_default_token_balance, token_address
                );
                Ok(*simulated_default_token_balance)
            }
            DexClient::Ethereum(client) => client.get_token_balance(token_address).await,
        }
    }

    // For Paper trading, this could query TokenRepository. For Ethereum, it'd query the DEX.
    pub async fn get_token_price_usd(&self, token_address: &str) -> Result<f64> {
        match self {
            DexClient::Paper {
                simulated_stablecoin_symbol,
                ..
            } => {
                let addr_lower = token_address.to_lowercase();
                // Check if it's the native currency (e.g., ETH for Ethereum mainnet)
                // This requires knowing the native symbol for the paper trading "network"
                // Assuming paper trading simulates Ethereum behavior for native currency price
                if addr_lower == "eth" || addr_lower == "ethereum" {
                    // Simple check for ETH
                    // For paper trading, ETH price is simulated or fetched if a mechanism exists
                    // Let's assume a fixed paper price for ETH for now, or error if not available.
                    // This part would ideally use a shared price source if paper trading needs real prices.
                    warn!("Paper trading: get_token_price_usd for native token (ETH) requested. This should ideally use a live oracle or be explicitly simulated.");
                    Ok(1600.0) // Placeholder: 1 ETH = $1600 USD for paper
                } else if addr_lower == simulated_stablecoin_symbol.to_lowercase()
                // Use field from Paper variant
                {
                    Ok(1.0) // Stablecoin is $1
                } else {
                    // For other tokens in paper mode, we need a mock price.
                    // This could come from a config, a simple hash-based price, or error out.
                    warn!(
                        "Paper trading: Price requested for unsimulated token {}. Returning placeholder or error.",
                        token_address
                    );
                    // Placeholder: hash token_address to get a pseudo-random price for testing
                    // Simple hash: sum of char values modulo 1000 + 1
                    let pseudo_price =
                        (token_address.chars().map(|c| c as u32).sum::<u32>() % 1000 + 1) as f64;
                    Ok(pseudo_price)
                }
            }
            DexClient::Ethereum(client) => client.get_token_price_usd(token_address).await,
        }
    }

    pub async fn execute_swap(
        &self,
        from_token_address: &str,
        to_token_address: &str,
        amount_in: f64,
        slippage_tolerance: f64,
        price_limit: Option<f64>,
        priority: TransactionPriority,
    ) -> Result<TransactionDetails> {
        match self {
            DexClient::Paper { .. } => {
                info!(
                    "Paper trading: Simulating swap from {} to {} of {} (slippage: {}, price_limit: {:?}, priority: {:?})",
                    from_token_address, to_token_address, amount_in, slippage_tolerance, price_limit, priority
                );
                // Simulate a successful swap with some basic slippage
                // This is highly simplified.
                let mut amount_out = amount_in;
                let price_from = self
                    .get_token_price_usd(from_token_address)
                    .await
                    .unwrap_or(1.0);
                let price_to = self
                    .get_token_price_usd(to_token_address)
                    .await
                    .unwrap_or(1.0);

                if price_to > 0.0 {
                    amount_out = (amount_in * price_from) / price_to;
                }

                // Apply simple slippage (e.g. reduce output by 0.1%)
                amount_out *= 1.0 - 0.001;

                if let Some(limit) = price_limit {
                    let simulated_price = (amount_in * price_from) / amount_out; // effective price
                                                                                 // if buying (to_token is not stable, from_token is stable)
                    if price_from == 1.0 && price_to > 1.0 {
                        // Assuming from_token is stablecoin like USDC
                        if simulated_price > limit {
                            return Err(Error::Dex(format!(
                                "Paper Swap Failed: Price limit {} exceeded, simulated price {}",
                                limit, simulated_price
                            )));
                        }
                    }
                    // if selling (from_token is not stable, to_token is stable)
                    else if price_to == 1.0 && price_from > 1.0 {
                        // Assuming to_token is stablecoin
                        if simulated_price < limit {
                            return Err(Error::Dex(format!(
                                "Paper Swap Failed: Price limit {} not met, simulated price {}",
                                limit, simulated_price
                            )));
                        }
                    }
                }

                let details = TransactionDetails {
                    tx_id: format!(
                        "paper_tx_{}",
                        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
                    ),
                    amount_in,
                    token_in_address: from_token_address.to_string(),
                    amount_out,
                    token_out_address: to_token_address.to_string(),
                    actual_price: if amount_out > 0.0 {
                        (amount_in * price_from) / amount_out
                    } else {
                        0.0
                    },
                    fees_paid: amount_in * 0.003, // Simulate 0.3% fee on input amount
                    fee_currency: from_token_address.to_string(), // Assume fee is paid in input token for simplicity
                    gas_used: None,
                    gas_price: None,
                    block_number: None,
                    confirmation_time: None,
                };
                Ok(details)
            }
            DexClient::Ethereum(client) => {
                client
                    .execute_swap(
                        from_token_address,
                        to_token_address,
                        amount_in,
                        slippage_tolerance,
                        price_limit,
                        priority,
                    )
                    .await
            }
        }
    }

    pub async fn get_transaction_status(&self, tx_id: &str) -> Result<TransactionStatus> {
        match self {
            DexClient::Paper { .. } => {
                info!(
                    "Paper trading: Simulating get_transaction_status for tx_id: {}",
                    tx_id
                );
                // For paper trading, assume immediate success for any tx_id starting with "paper_tx_"
                if tx_id.starts_with("paper_tx_") {
                    // Create plausible dummy details
                    let details = TransactionDetails {
                        tx_id: tx_id.to_string(),
                        amount_in: 100.0,                      // dummy
                        token_in_address: "USDC".to_string(),  // dummy
                        amount_out: 0.05,                      // dummy
                        token_out_address: "WETH".to_string(), // dummy
                        actual_price: 2000.0,                  // dummy
                        fees_paid: 0.3,                        // dummy
                        fee_currency: "USDC".to_string(),      //dummy
                        gas_used: None,
                        gas_price: None,
                        block_number: None,
                        confirmation_time: None,
                    };
                    Ok(TransactionStatus::Success {
                        details,
                        execution_time: chrono::Duration::seconds(0), // ADDED
                        gas_efficiency: 0.0,                          // ADDED
                    })
                } else {
                    Ok(TransactionStatus::Failed {
                        tx_id: tx_id.to_string(),
                        reason: "Unknown paper transaction ID".to_string(),
                        error_code: None,
                        gas_used: None,
                        revert_reason: None,
                        recovery_suggestion: None, // ADDED
                    })
                }
            }
            DexClient::Ethereum(client) => client.get_transaction_status(tx_id).await,
        }
    }

    pub async fn get_network_fees(&self) -> Result<NetworkFeeInfo> {
        match self {
            DexClient::Paper { .. } => {
                info!("Paper trading: Simulating get_network_fees");
                Ok(NetworkFeeInfo {
                    gas_price_gwei: Some(10.0), // Simulated gwei
                    estimated_fee_usd: 0.50,    // Simulated $0.50 fee
                    fee_currency_symbol: "ETH".to_string(),
                })
            }
            DexClient::Ethereum(client) => client.get_network_fees().await,
        }
    }
}
