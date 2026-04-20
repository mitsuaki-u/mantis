use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use ethers::prelude::*;
use ethers::providers::{Http, Provider};
use ethers::types::{Address, Log, H256, U256};
use ethers::utils::format_units;
use log::{debug, error, info, warn};
use rust_decimal::prelude::*;
use rust_decimal::Decimal;

use crate::infrastructure::constants::{
    ETHEREUM_STANDARD_GAS_LIMIT, GAS_EFFICIENCY_MULTIPLIER, REQUIRED_BLOCK_CONFIRMATIONS,
};
use crate::infrastructure::dex::ethereum::config::NetworkConfig;
use crate::infrastructure::dex::ethereum::providers::uniswap_v3::UniswapV3ProtocolProvider;
use crate::infrastructure::dex::{NetworkFeeInfo, TransactionDetails, TransactionStatus};
use crate::infrastructure::errors::{Error, Result};

#[derive(Clone)]
pub struct TransactionManager {
    provider: Arc<Provider<Http>>,
    network: NetworkConfig,
    v3_provider: UniswapV3ProtocolProvider,
    submission_times: Arc<tokio::sync::RwLock<HashMap<H256, DateTime<Utc>>>>,
}

impl TransactionManager {
    pub fn new(
        provider: Arc<Provider<Http>>,
        network: NetworkConfig,
        v3_provider: UniswapV3ProtocolProvider,
    ) -> Self {
        Self {
            provider,
            network,
            v3_provider,
            submission_times: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Record transaction submission time for tracking
    pub async fn record_submission_time(&self, tx_hash: H256) {
        let mut times = self.submission_times.write().await;
        times.insert(tx_hash, Utc::now());
    }

    /// Get tracked submission time for a transaction
    async fn get_submission_time(&self, tx_hash: H256) -> DateTime<Utc> {
        let times = self.submission_times.read().await;
        times.get(&tx_hash).copied().unwrap_or_else(|| {
            // Fallback: estimate from transaction data if available
            Utc::now() - Duration::minutes(5) // Conservative estimate
        })
    }

    /// Get transaction status by transaction ID
    /// Returns (status, optional complete transaction details)
    /// Details are available for Success/Failed states when receipt is available
    pub async fn get_transaction_status(
        &self,
        tx_id_str: &str,
    ) -> Result<(TransactionStatus, Option<TransactionDetails>)> {
        let tx_hash = H256::from_str(tx_id_str)
            .map_err(|e| Error::Conversion(format!("Invalid transaction hash: {}", e)))?;

        // First, try to get the transaction
        let tx_option = self
            .provider
            .get_transaction(tx_hash)
            .await
            .map_err(|e| Error::Network(format!("Failed to get transaction: {}", e)))?;

        match tx_option {
            Some(tx) => {
                // Transaction exists, now check if it's mined
                let receipt_option = self
                    .provider
                    .get_transaction_receipt(tx_hash)
                    .await
                    .map_err(|e| {
                        Error::Network(format!("Failed to get transaction receipt: {}", e))
                    })?;

                match receipt_option {
                    Some(receipt) => {
                        // Transaction is mined
                        if receipt.status == Some(1.into()) {
                            // Transaction succeeded
                            let details = self
                                .create_transaction_details_from_receipt(&receipt, &tx)
                                .await?;

                            // Get current block number to calculate confirmations
                            let current_block =
                                self.provider.get_block_number().await.map_err(|e| {
                                    Error::Network(format!("Failed to get current block: {}", e))
                                })?;

                            let confirmations = if let Some(tx_block) = receipt.block_number {
                                current_block.saturating_sub(tx_block).as_u64()
                            } else {
                                0
                            };

                            if confirmations >= REQUIRED_BLOCK_CONFIRMATIONS {
                                // Calculate gas efficiency as amount_out per gas_used
                                let gas_efficiency = if let Some(gas_used) = details.gas_used {
                                    if gas_used > 0 && details.amount_out > 0.0 {
                                        (details.amount_out * GAS_EFFICIENCY_MULTIPLIER)
                                            / gas_used as f64
                                    // Normalize to tokens per million gas
                                    } else {
                                        0.0
                                    }
                                } else {
                                    0.0
                                };

                                let status = TransactionStatus::Success {
                                    tx_id: details.tx_id.clone(),
                                    details: format!(
                                        "Successful transaction with {} confirmations",
                                        confirmations
                                    ),
                                    gas_efficiency,
                                };

                                info!(
                                    "Transaction {} succeeded: block {}, gas {}, fees {:.6} ETH (efficiency: {:.2})",
                                    details.tx_id,
                                    details.block_number.map(|b| b.to_string()).unwrap_or_else(|| "unknown".to_string()),
                                    details.gas_used.map(|g| g.to_string()).unwrap_or_else(|| "unknown".to_string()),
                                    details.fees_paid,
                                    gas_efficiency
                                );

                                Ok((status, Some(details)))
                            } else {
                                let status = TransactionStatus::Confirmed {
                                    tx_id: details.tx_id.clone(),
                                    details: format!(
                                        "Confirmed transaction with {} confirmations",
                                        confirmations
                                    ),
                                    confirmations,
                                    required_confirmations: REQUIRED_BLOCK_CONFIRMATIONS,
                                    finality_probability: (confirmations as f64
                                        / REQUIRED_BLOCK_CONFIRMATIONS as f64)
                                        .min(1.0),
                                };

                                info!(
                                    "Transaction {} confirmed with {} confirmations",
                                    details.tx_id, confirmations
                                );

                                Ok((status, Some(details)))
                            }
                        } else {
                            // Transaction failed - also return complete details
                            let details = self
                                .create_transaction_details_from_receipt(&receipt, &tx)
                                .await?;
                            let revert_reason = self.get_revert_reason(&receipt).await;
                            let gas_used_val = receipt.gas_used.map(|g| g.as_u64());

                            let status = TransactionStatus::Failed {
                                tx_id: tx_id_str.to_string(),
                                reason: "Transaction reverted".to_string(),
                                error_code: receipt.status.map(|s| format!("{}", s)),
                                gas_used: gas_used_val,
                                revert_reason: revert_reason.clone(),
                                recovery_suggestion: Some("Check transaction parameters and try again with higher gas limit".to_string()),
                            };

                            error!(
                                "Transaction {} failed: Transaction reverted (gas used: {:?}{})",
                                tx_id_str,
                                gas_used_val,
                                revert_reason
                                    .as_ref()
                                    .map(|r| format!(", reason: {}", r))
                                    .unwrap_or_default()
                            );

                            Ok((status, Some(details)))
                        }
                    }
                    None => {
                        // Transaction exists but not mined yet - no details available
                        let submission_time = self.get_submission_time(tx_hash).await;
                        let status = TransactionStatus::Pending {
                            tx_id: tx_id_str.to_string(),
                            submission_time,
                            last_checked: Utc::now(),
                            block_height: None,
                            retry_count: 0,
                        };

                        debug!("Transaction {} is pending", tx_id_str);

                        Ok((status, None))
                    }
                }
            }
            None => {
                // Transaction doesn't exist - no details available
                let status = TransactionStatus::Failed {
                    tx_id: tx_id_str.to_string(),
                    reason: "Transaction not found".to_string(),
                    error_code: None,
                    gas_used: None,
                    revert_reason: None,
                    recovery_suggestion: Some("Transaction was not found on the network. It might not have been broadcast, was dropped, or the tx_id is incorrect. Check your connection and the transaction ID.".to_string()),
                };

                warn!("Transaction {} not found on network", tx_id_str);

                Ok((status, None))
            }
        }
    }

    /// Get network fee information
    pub async fn get_network_fees(&self) -> Result<NetworkFeeInfo> {
        let gas_price_gwei = self.v3_provider.get_gas_price_gwei().await?;

        let native_price_usd = self.v3_provider.get_native_price_usd().await?;
        let gas_cost_eth = (gas_price_gwei / 1_000_000_000.0) * ETHEREUM_STANDARD_GAS_LIMIT as f64;
        let gas_cost_decimal = Decimal::from_f64(gas_cost_eth)
            .ok_or_else(|| Error::Conversion("Invalid gas cost".to_string()))?;
        let estimated_fee_usd_decimal = gas_cost_decimal * native_price_usd;
        let estimated_fee_usd = estimated_fee_usd_decimal
            .to_f64()
            .ok_or_else(|| Error::Conversion("Fee too large".to_string()))?;

        debug!("🔧 [NETWORK] Current network fees:");
        debug!(
            "🔧 [NETWORK]   Gas Price: {:.2} Gwei ({} Wei)",
            gas_price_gwei,
            (gas_price_gwei * 1_000_000_000.0) as u64
        );
        debug!(
            "🔧 [NETWORK]   Native Token Price: ${:.2}",
            native_price_usd
        );
        debug!(
            "🔧 [NETWORK]   Estimated Fee (21k gas): ${:.6}",
            estimated_fee_usd
        );

        Ok(NetworkFeeInfo {
            gas_limit: ETHEREUM_STANDARD_GAS_LIMIT,
            gas_price_gwei: Some(gas_price_gwei),
            estimated_fee_eth: gas_cost_eth,
            estimated_fee_usd,
            priority: crate::infrastructure::dex::TransactionPriority::Standard,
            fee_currency_symbol: self.network.native_currency_symbol.clone(),
        })
    }

    /// Create transaction details from receipt and transaction
    async fn create_transaction_details_from_receipt(
        &self,
        receipt: &ethers::types::TransactionReceipt,
        tx: &ethers::types::Transaction,
    ) -> Result<TransactionDetails> {
        // Parse swap events from logs to get actual amounts
        let (amount_out, token_out_address) = self
            .parse_swap_amounts_from_logs(&receipt.logs)
            .await
            .unwrap_or((0.0, format!("{:?}", ethers::types::Address::zero())));

        let amount_in = format_units(tx.value, 18)
            .unwrap_or_default()
            .parse()
            .unwrap_or(0.0);

        let actual_price = if amount_in > 0.0 && amount_out > 0.0 {
            amount_out / amount_in
        } else {
            0.0
        };

        let fees_paid = format_units(
            receipt.gas_used.unwrap_or_default() * receipt.effective_gas_price.unwrap_or_default(),
            18,
        )
        .unwrap_or_default()
        .parse()
        .unwrap_or(0.0);

        // Debug: Detailed transaction analysis
        debug!(
            "🔧 [TX_ANALYSIS] Transaction {:?} details:",
            receipt.transaction_hash
        );
        if let Some(gas_used) = receipt.gas_used {
            debug!("🔧 [TX_ANALYSIS]   Gas Used: {} units", gas_used);
            // Note: Gas limit comparison would require accessing tx.gas_limit which may not be available
        }

        if let Some(effective_gas_price) = receipt.effective_gas_price {
            let gas_price_gwei = format_units(effective_gas_price, 9)
                .unwrap_or_default()
                .parse::<f64>()
                .unwrap_or(0.0);
            debug!(
                "🔧 [TX_ANALYSIS]   Effective Gas Price: {:.2} Gwei",
                gas_price_gwei
            );
        }

        debug!("💰 [TX_ANALYSIS]   Network Fees: {:.6} ETH", fees_paid);
        debug!("💰 [TX_ANALYSIS]   Amount In: {:.6}", amount_in);
        debug!("💰 [TX_ANALYSIS]   Amount Out: {:.6}", amount_out);
        debug!("💰 [TX_ANALYSIS]   Effective Price: {:.6}", actual_price);

        if let Some(block_number) = receipt.block_number {
            debug!("⚡ [TX_ANALYSIS]   Block Number: {}", block_number);
        }

        Ok(TransactionDetails {
            transaction_hash: format!("{:?}", receipt.transaction_hash),
            tx_id: format!("{:?}", receipt.transaction_hash),
            block_number: receipt.block_number.map(|b| b.as_u64()),
            status: TransactionStatus::Unknown, // Will be updated by caller
            timestamp: chrono::Utc::now(),
            confirmation_time: None,
            network_fee_eth: Some(fees_paid),
            network_fee_usd: None, // Could be calculated if we have ETH/USD price
            amount_in,
            amount_out,
            token_in_address: format!("{:?}", tx.to.unwrap_or_default()),
            token_out_address,
            actual_price,
            fees_paid,
            fee_currency: self.network.native_currency_symbol.clone(),
            gas_used: receipt.gas_used.map(|g| g.as_u64()),
            gas_price: receipt
                .effective_gas_price
                .map(|gp| format_units(gp, 9).unwrap_or_default()),
        })
    }

    /// Parse swap amounts from transaction logs
    async fn parse_swap_amounts_from_logs(&self, logs: &[Log]) -> Result<(f64, String)> {
        for log in logs {
            if let Ok((_, _, _, amount_out)) = self.parse_swap_event(log).await {
                let amount_out_f64 = format_units(amount_out, 18)
                    .unwrap_or_default()
                    .parse()
                    .unwrap_or(0.0);
                return Ok((amount_out_f64, format!("{:?}", log.address)));
            }
        }
        Ok((0.0, format!("{:?}", ethers::types::Address::zero())))
    }

    /// Helper function to parse swap events - V2 support removed
    async fn parse_swap_event(&self, _log: &Log) -> Result<(Address, Address, U256, U256)> {
        // V2 swap event parsing has been removed since we now use Alchemy pools (V3 only)
        Err(Error::Contract(
            "V2 swap event parsing has been removed. Use V3 events via Alchemy pool data."
                .to_string(),
        ))
    }

    /// Get revert reason from transaction receipt
    async fn get_revert_reason(
        &self,
        receipt: &ethers::types::TransactionReceipt,
    ) -> Option<String> {
        // This is a simplified implementation
        // In practice, you'd need to decode the revert reason from the transaction data
        if receipt.status == Some(0.into()) {
            Some("Transaction reverted".to_string())
        } else {
            None
        }
    }

    /// Helper function to decode Uniswap V2 router function input
    pub async fn decode_router_input(&self, input: &[u8]) -> Result<(Address, Address, U256)> {
        if input.len() < 4 {
            return Err(Error::Contract("Invalid input data length".to_string()));
        }

        // Get function selector (first 4 bytes)
        let selector = &input[..4];

        // Try to decode based on common swap function selectors
        match selector {
            // swapExactTokensForTokens
            [0x38, 0xed, 0x17, 0x39] => {
                let amount_in = U256::from_big_endian(&input[4..36]);
                let path_offset = U256::from_big_endian(&input[100..132]);
                let path_len = U256::from_big_endian(
                    &input[path_offset.as_usize()..path_offset.as_usize() + 32],
                );

                if path_len.as_usize() < 2 {
                    return Err(Error::Contract("Invalid path length".to_string()));
                }

                let token_in = Address::from_slice(
                    &input[path_offset.as_usize() + 32..path_offset.as_usize() + 52],
                );
                let token_out = Address::from_slice(
                    &input[path_offset.as_usize() + 52..path_offset.as_usize() + 72],
                );

                Ok((token_in, token_out, amount_in))
            }
            // swapExactETHForTokens
            [0x7f, 0xf3, 0x6a, 0xb5] => {
                let path_offset = U256::from_big_endian(&input[36..68]);
                let path_len = U256::from_big_endian(
                    &input[path_offset.as_usize()..path_offset.as_usize() + 32],
                );

                if path_len.as_usize() < 2 {
                    return Err(Error::Contract("Invalid path length".to_string()));
                }

                let amount_in = self
                    .provider
                    .get_transaction(H256::from_slice(input))
                    .await?
                    .map(|tx| tx.value)
                    .unwrap_or_default();

                Ok((
                    self.network.weth_address,
                    Address::from_slice(
                        &input[path_offset.as_usize() + 52..path_offset.as_usize() + 72],
                    ),
                    amount_in,
                ))
            }
            // swapExactTokensForETH
            [0x18, 0xcb, 0xaf, 0xe5] => {
                let amount_in = U256::from_big_endian(&input[4..36]);
                let path_offset = U256::from_big_endian(&input[100..132]);
                let path_len = U256::from_big_endian(
                    &input[path_offset.as_usize()..path_offset.as_usize() + 32],
                );

                if path_len.as_usize() < 2 {
                    return Err(Error::Contract("Invalid path length".to_string()));
                }

                let token_in = Address::from_slice(
                    &input[path_offset.as_usize() + 32..path_offset.as_usize() + 52],
                );

                Ok((token_in, self.network.weth_address, amount_in))
            }
            _ => Err(Error::Contract("Unsupported swap function".to_string())),
        }
    }
}
