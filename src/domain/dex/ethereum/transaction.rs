use std::str::FromStr;
use std::sync::Arc;

use chrono::{Duration, Utc};
use ethers::prelude::Middleware;
use ethers::providers::{Http, Provider};
use ethers::types::{Address, Log, H256, U256};
use ethers::utils::format_units;

use crate::core::error::{Error, Result};
use crate::domain::dex::ethereum::abi::get_uniswap_v2_pair_abi;
use crate::domain::dex::ethereum::network::NetworkConfig;
use crate::domain::dex::ethereum::price::PriceOracle;
use crate::domain::dex::{NetworkFeeInfo, TransactionDetails, TransactionStatus};

#[derive(Clone)]
pub struct TransactionManager {
    provider: Arc<Provider<Http>>,
    network: NetworkConfig,
    price_oracle: PriceOracle,
}

impl TransactionManager {
    pub fn new(provider: Arc<Provider<Http>>, network: NetworkConfig) -> Self {
        let price_oracle = PriceOracle::new(provider.clone(), network.clone());
        Self {
            provider,
            network,
            price_oracle,
        }
    }

    /// Get transaction status by transaction ID
    pub async fn get_transaction_status(&self, tx_id_str: &str) -> Result<TransactionStatus> {
        self.get_transaction_status_internal(tx_id_str).await
    }

    async fn get_transaction_status_internal(&self, tx_id_str: &str) -> Result<TransactionStatus> {
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

                            let required_confirmations = 12; // Standard for Ethereum

                            if confirmations >= required_confirmations {
                                Ok(TransactionStatus::Success {
                                    details,
                                    execution_time: Duration::seconds(30), // Placeholder
                                    gas_efficiency: 1.0,                   // Placeholder
                                })
                            } else {
                                Ok(TransactionStatus::Confirmed {
                                    details,
                                    confirmations,
                                    required_confirmations,
                                    finality_probability: (confirmations as f64
                                        / required_confirmations as f64)
                                        .min(1.0),
                                })
                            }
                        } else {
                            // Transaction failed
                            let revert_reason = self.get_revert_reason(&receipt).await;
                            Ok(TransactionStatus::Failed {
                                tx_id: tx_id_str.to_string(),
                                reason: "Transaction reverted".to_string(),
                                error_code: receipt.status.map(|s| format!("{}", s)),
                                gas_used: receipt.gas_used.map(|g| g.as_u64()),
                                revert_reason,
                                recovery_suggestion: Some("Check transaction parameters and try again with higher gas limit".to_string()),
                            })
                        }
                    }
                    None => {
                        // Transaction exists but not mined yet
                        Ok(TransactionStatus::Pending {
                            tx_id: tx_id_str.to_string(),
                            submission_time: Utc::now(), // Placeholder - should be from actual submission
                            last_checked: Utc::now(),
                            block_height: None,
                            retry_count: 0,
                        })
                    }
                }
            }
            None => {
                // Transaction doesn't exist
                Ok(TransactionStatus::Failed {
                    tx_id: tx_id_str.to_string(),
                    reason: "Transaction not found".to_string(),
                    error_code: None,
                    gas_used: None,
                    revert_reason: None,
                    recovery_suggestion: Some("Transaction was not found on the network. It might not have been broadcast, was dropped, or the tx_id is incorrect. Check your connection and the transaction ID.".to_string()),
                })
            }
        }
    }

    /// Get network fee information
    pub async fn get_network_fees(&self) -> Result<NetworkFeeInfo> {
        self.get_network_fees_internal().await
    }

    async fn get_network_fees_internal(&self) -> Result<NetworkFeeInfo> {
        let gas_price = self
            .provider
            .get_gas_price()
            .await
            .map_err(|e| Error::Network(format!("Failed to get gas price: {}", e)))?;

        let gas_price_gwei = format_units(gas_price, 9)
            .map_err(|e| Error::Conversion(format!("Failed to format gas price: {}", e)))?
            .parse::<f64>()
            .map_err(|e| Error::Conversion(format!("Failed to parse gas price: {}", e)))?;

        let native_price_usd = self.price_oracle.get_native_price_usd().await?;
        let estimated_fee_usd = (gas_price_gwei / 1_000_000_000.0) * 21000.0 * native_price_usd;

        Ok(NetworkFeeInfo {
            gas_price_gwei: Some(gas_price_gwei),
            estimated_fee_usd,
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
            .unwrap_or((
                0.0,
                "0x0000000000000000000000000000000000000000".to_string(),
            ));

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

        Ok(TransactionDetails {
            tx_id: format!("{:?}", receipt.transaction_hash),
            amount_in,
            token_in_address: format!("{:?}", tx.to.unwrap_or_default()),
            amount_out,
            token_out_address,
            actual_price,
            fees_paid,
            fee_currency: self.network.native_currency_symbol.clone(),
            gas_used: receipt.gas_used.map(|g| g.as_u64()),
            gas_price: receipt.effective_gas_price.map(|gp| {
                format_units(gp, 9)
                    .unwrap_or_default()
                    .parse()
                    .unwrap_or(0.0)
            }),
            block_number: receipt.block_number.map(|bn| bn.as_u64()),
            confirmation_time: Some(Utc::now()),
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
        Ok((
            0.0,
            "0x0000000000000000000000000000000000000000".to_string(),
        ))
    }

    /// Helper function to parse Uniswap V2 swap event
    async fn parse_swap_event(&self, log: &Log) -> Result<(Address, Address, U256, U256)> {
        // Swap event has 3 topics:
        // topic[0] = event signature
        // topic[1] = sender address (indexed)
        // topic[2] = to address (indexed)
        // data contains amount0In, amount1In, amount0Out, amount1Out

        if log.topics.len() != 3 {
            return Err(Error::Contract("Invalid swap event topics".to_string()));
        }

        // Get the pair contract to determine token addresses
        let pair_contract = ethers::contract::Contract::new(
            log.address,
            get_uniswap_v2_pair_abi()?,
            self.provider.clone(),
        );

        let token0: Address = pair_contract
            .method::<_, Address>("token0", ())
            .map_err(|e| Error::Contract(format!("Failed to get token0 method: {}", e)))?
            .call()
            .await
            .map_err(|e| Error::Contract(format!("Failed to get token0: {}", e)))?;

        let token1: Address = pair_contract
            .method::<_, Address>("token1", ())
            .map_err(|e| Error::Contract(format!("Failed to get token1 method: {}", e)))?
            .call()
            .await
            .map_err(|e| Error::Contract(format!("Failed to get token1: {}", e)))?;

        // Parse amounts from event data
        let amount0_in = U256::from_big_endian(&log.data[..32]);
        let amount1_in = U256::from_big_endian(&log.data[32..64]);
        let amount0_out = U256::from_big_endian(&log.data[64..96]);
        let amount1_out = U256::from_big_endian(&log.data[96..128]);

        // Determine which token is input and which is output
        let (token_in, token_out, amount_in, amount_out) = if amount0_in > U256::zero() {
            (token0, token1, amount0_in, amount1_out)
        } else {
            (token1, token0, amount1_in, amount0_out)
        };

        Ok((token_in, token_out, amount_in, amount_out))
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
                    .get_transaction(H256::from_slice(&input))
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
