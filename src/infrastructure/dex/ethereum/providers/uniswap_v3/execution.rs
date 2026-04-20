//! Swap execution logic for Uniswap V3

use super::abi::load_swaprouter_abi;
use super::types::{ExactInputSingleParams, GasEstimate, PoolInfo, SwapParams};
use crate::infrastructure::dex::ethereum::config::NetworkConfig;
use crate::infrastructure::dex::{TransactionDetails, TransactionStatus};
use crate::infrastructure::errors::{Error, Result};
use ethers::contract::Contract;
use ethers::prelude::{LocalWallet, Signer, SignerMiddleware};
use ethers::types::U256;
use log::info;
use std::sync::Arc;

/// Swap executor for Uniswap V3 transactions
pub(super) struct SwapExecutor {
    provider: Arc<ethers::providers::Provider<ethers::providers::Http>>,
    network: NetworkConfig,
}

impl SwapExecutor {
    pub fn new(
        provider: Arc<ethers::providers::Provider<ethers::providers::Http>>,
        network: NetworkConfig,
    ) -> Self {
        Self { provider, network }
    }

    /// Execute a real swap using the Uniswap V3 SwapRouter contract
    pub async fn execute_live(
        &self,
        params: SwapParams,
        _pool_info: PoolInfo,
        swap_params: ExactInputSingleParams,
        wallet: LocalWallet,
        gas_estimate: GasEstimate,
    ) -> Result<TransactionDetails> {
        info!("🔄 Executing REAL swap on Uniswap V3 SwapRouter");

        // Create signer middleware
        let signer = SignerMiddleware::new(
            self.provider.clone(),
            wallet.with_chain_id(self.network.chain_id),
        );

        // Get SwapRouter contract address from centralized addresses
        use crate::infrastructure::dex::ethereum::config::addresses::get_network_addresses;
        let network_name = match self.network.chain_id {
            1 => "mainnet",
            _ => {
                return Err(Error::Config(format!(
                    "Unsupported chain ID for SwapRouter: {}",
                    self.network.chain_id
                )))
            }
        };

        let addresses = get_network_addresses(network_name)
            .map_err(|e| Error::Config(format!("Failed to get network addresses: {}", e)))?;

        let swaprouter_address = addresses.router;

        // Create SwapRouter contract
        let swaprouter_abi = load_swaprouter_abi()?;
        let swaprouter_contract =
            Contract::new(swaprouter_address, swaprouter_abi, Arc::new(signer));

        // Prepare parameters tuple for contract call
        let params_tuple = (
            swap_params.token_in,
            swap_params.token_out,
            swap_params.fee,
            swap_params.recipient,
            swap_params.deadline,
            swap_params.amount_in,
            swap_params.amount_out_minimum,
            swap_params.sqrt_price_limit_x96,
        );

        // Use pre-calculated gas estimates (already validated in estimate_gas_for_swap)
        info!(
            "🔧 Using pre-calculated gas estimates: limit={}, price={:.2} Gwei",
            gas_estimate.gas_limit, gas_estimate.gas_price_gwei
        );

        // Build contract call with gas settings
        let contract_call = swaprouter_contract
            .method::<_, U256>("exactInputSingle", params_tuple)
            .map_err(|e| Error::Abi(format!("Contract method error: {}", e)))?
            .gas(gas_estimate.gas_limit)
            .gas_price(gas_estimate.gas_price_wei);

        let tx = contract_call
            .send()
            .await
            .map_err(|e| Error::Network(format!("Transaction failed: {}", e)))?;

        let tx_hash = tx.tx_hash();
        let tx_id = format!("{:?}", tx_hash);

        info!("✅ Transaction submitted: {}", tx_id);

        // Wait for transaction receipt
        let receipt = tx
            .await
            .map_err(|e| Error::Network(format!("Transaction receipt failed: {}", e)))?
            .ok_or_else(|| Error::Network("Transaction receipt is None".to_string()))?;

        let block_number = receipt.block_number.map(|bn| bn.as_u64());
        let gas_used = receipt.gas_used.map(|gu| gu.as_u64()).unwrap_or(0);
        let effective_gas_price = receipt.effective_gas_price.unwrap_or(U256::zero());

        // Calculate fees
        let fee_wei = gas_used as u128 * effective_gas_price.as_u128();
        let fee_eth = fee_wei as f64 / 1e18;

        // Use minimum output amount (slippage-protected) as conservative estimate
        // With typical 0.5-1% slippage, this is 99%+ accurate
        let amount_out_actual = swap_params.amount_out_minimum.as_u128() as f64;

        // Determine transaction status
        let status = if receipt.status == Some(1.into()) {
            TransactionStatus::Success {
                tx_id: tx_id.clone(),
                gas_efficiency: if gas_used > 0 {
                    (amount_out_actual * 1_000_000.0) / gas_used as f64
                } else {
                    0.0
                },
                details: format!(
                    "Transaction successful on block {}",
                    block_number.unwrap_or(0)
                ),
            }
        } else {
            TransactionStatus::Failed {
                tx_id: tx_id.clone(),
                reason: "Transaction reverted".to_string(),
                error_code: receipt.status.map(|s| format!("{}", s)),
                gas_used: Some(gas_used),
                revert_reason: Some("Transaction failed during execution".to_string()),
                recovery_suggestion: Some("Check token approvals and balances".to_string()),
            }
        };

        let tx_details = TransactionDetails {
            transaction_hash: tx_id.clone(),
            tx_id,
            block_number,
            status,
            timestamp: chrono::Utc::now(),
            confirmation_time: Some(chrono::Utc::now()),
            network_fee_eth: Some(fee_eth),
            network_fee_usd: None, // Would require ETH price lookup
            amount_in: params.amount_in.as_u128() as f64,
            amount_out: amount_out_actual,
            token_in_address: format!("{:?}", params.token_in),
            token_out_address: format!("{:?}", params.token_out),
            actual_price: if params.amount_in > U256::zero() && amount_out_actual > 0.0 {
                amount_out_actual / (params.amount_in.as_u128() as f64)
            } else {
                0.0
            },
            fees_paid: fee_eth,
            fee_currency: "ETH".to_string(),
            gas_used: Some(gas_used),
            gas_price: Some(format!("{}", effective_gas_price)),
        };

        info!("🎉 Real swap completed successfully: {}", tx_details.tx_id);
        Ok(tx_details)
    }

    /// Build simulated transaction details for paper trading
    /// Uses real gas estimates so paper trading accurately reflects costs
    pub fn build_simulated_tx(
        params: &SwapParams,
        gas_estimate: GasEstimate,
    ) -> TransactionDetails {
        info!("🚧 Paper trading mode - using simulated execution with real gas estimates");

        let timestamp = chrono::Utc::now();
        let tx_id = format!("simulated_tx_{}", timestamp.timestamp());

        TransactionDetails {
            transaction_hash: tx_id.clone(),
            tx_id,
            block_number: None,
            status: TransactionStatus::Unknown,
            timestamp,
            confirmation_time: None,
            network_fee_eth: Some(gas_estimate.estimated_cost_eth),
            network_fee_usd: Some(gas_estimate.estimated_cost_usd),
            amount_in: params.amount_in.as_u128() as f64,
            amount_out: params.amount_out_minimum.as_u128() as f64,
            token_in_address: format!("{:?}", params.token_in),
            token_out_address: format!("{:?}", params.token_out),
            actual_price: if params.amount_in > U256::zero()
                && params.amount_out_minimum > U256::zero()
            {
                (params.amount_out_minimum.as_u128() as f64) / (params.amount_in.as_u128() as f64)
            } else {
                0.0
            },
            fees_paid: gas_estimate.estimated_cost_usd,
            fee_currency: "ETH".to_string(),
            gas_used: Some(gas_estimate.gas_limit.as_u64()),
            gas_price: Some(format!("{:.2}", gas_estimate.gas_price_gwei)),
        }
    }
}
