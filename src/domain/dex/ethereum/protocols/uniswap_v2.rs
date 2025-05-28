use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use ethers::contract::Contract;
use ethers::prelude::Middleware;
use ethers::providers::{Http, Provider};
use ethers::signers::{LocalWallet, Signer};
use ethers::types::{Address, H256, U256};
use log::info;

use crate::core::error::{Error, Result};
use crate::domain::dex::{TransactionDetails, TransactionPriority};
use crate::infra::actors::{
    DexTransactionEvent, Event as ActorEvent, MessageBus, SubmittedTransactionInfo,
};

use super::{DexProtocol, ProtocolUtils, SwapParams};
use crate::domain::dex::ethereum::abi::{
    get_erc20_abi, get_uniswap_v2_factory_abi, get_uniswap_v2_pair_abi, get_uniswap_v2_router_abi,
};
use crate::domain::dex::ethereum::network::NetworkConfig;

pub struct UniswapV2Protocol {
    provider: Arc<Provider<Http>>,
    wallet: Option<LocalWallet>,
    network: NetworkConfig,
    message_bus: Arc<MessageBus>,
}

impl UniswapV2Protocol {
    pub fn new(
        provider: Arc<Provider<Http>>,
        wallet: Option<LocalWallet>,
        network: NetworkConfig,
        message_bus: Arc<MessageBus>,
    ) -> Self {
        Self {
            provider,
            wallet,
            network,
            message_bus,
        }
    }

    /// Get the factory contract instance
    fn get_factory_contract(&self) -> Result<Contract<Arc<Provider<Http>>>> {
        let abi = get_uniswap_v2_factory_abi()?;
        Ok(Contract::new(
            self.network.factory_address,
            abi,
            self.provider.clone().into(),
        ))
    }

    /// Get the router contract instance
    fn get_router_contract(&self) -> Result<Contract<Arc<Provider<Http>>>> {
        let abi = get_uniswap_v2_router_abi()?;
        Ok(Contract::new(
            self.network.router_address,
            abi,
            self.provider.clone().into(),
        ))
    }

    /// Get a pair contract instance
    fn get_pair_contract(&self, pair_address: Address) -> Result<Contract<Arc<Provider<Http>>>> {
        let abi = get_uniswap_v2_pair_abi()?;
        Ok(Contract::new(
            pair_address,
            abi,
            self.provider.clone().into(),
        ))
    }

    /// Get an ERC20 contract instance
    fn get_erc20_contract(&self, token_address: Address) -> Result<Contract<Arc<Provider<Http>>>> {
        let abi = get_erc20_abi()?;
        Ok(Contract::new(
            token_address,
            abi,
            self.provider.clone().into(),
        ))
    }

    /// Calculate gas price based on priority
    async fn calculate_gas_price(&self, priority: TransactionPriority) -> Result<U256> {
        let base_gas_price = self
            .provider
            .get_gas_price()
            .await
            .map_err(|e| Error::Network(format!("Failed to get gas price: {}", e)))?;

        let multiplier = match priority {
            TransactionPriority::Low => 0.8,
            TransactionPriority::Standard => 1.0,
            TransactionPriority::High => 1.2,
            TransactionPriority::Urgent => 1.5,
        };

        let adjusted_price = (base_gas_price.as_u128() as f64 * multiplier) as u128;
        Ok(U256::from(adjusted_price))
    }

    /// Check and approve token spending if necessary
    async fn ensure_token_approval(&self, token_address: Address, amount: U256) -> Result<()> {
        if let Some(wallet) = &self.wallet {
            let token_contract = self.get_erc20_contract(token_address)?;
            let owner_address = wallet.address();

            // Check current allowance
            let allowance: U256 = token_contract
                .method::<_, U256>("allowance", (owner_address, self.network.router_address))
                .map_err(|e| Error::Contract(format!("Failed to get allowance method: {}", e)))?
                .call()
                .await
                .map_err(|e| Error::Contract(format!("Failed to check allowance: {}", e)))?;

            // If allowance is insufficient, approve the router
            if allowance < amount {
                info!("Approving token spending for router");
                let approve_tx = token_contract
                    .method::<_, H256>("approve", (self.network.router_address, U256::MAX))
                    .map_err(|e| Error::Contract(format!("Failed to get approve method: {}", e)))?;

                let pending_tx = approve_tx.send().await.map_err(|e| {
                    Error::Transaction(format!("Failed to send approval transaction: {}", e))
                })?;

                let _receipt = pending_tx.await.map_err(|e| {
                    Error::Transaction(format!("Approval transaction failed: {}", e))
                })?;

                info!("Token approval successful");
            }
        } else {
            return Err(Error::Wallet("No wallet connected".to_string()));
        }

        Ok(())
    }
}

#[async_trait]
impl DexProtocol for UniswapV2Protocol {
    async fn get_quote(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
    ) -> Result<U256> {
        let router_contract = self.get_router_contract()?;
        let path =
            ProtocolUtils::create_path_through_weth(token_in, token_out, self.network.weth_address);

        let amounts: Vec<U256> = router_contract
            .method::<_, Vec<U256>>("getAmountsOut", (amount_in, path))
            .map_err(|e| Error::Contract(format!("Failed to get getAmountsOut method: {}", e)))?
            .call()
            .await
            .map_err(|e| Error::Contract(format!("Failed to get quote: {}", e)))?;

        amounts
            .last()
            .copied()
            .ok_or_else(|| Error::Contract("Empty amounts array from getAmountsOut".to_string()))
    }

    async fn execute_swap(&self, params: SwapParams) -> Result<TransactionDetails> {
        if self.wallet.is_none() {
            return Err(Error::Wallet("No wallet connected".to_string()));
        }

        // Ensure token approval if not swapping from ETH
        if params.token_in != self.network.weth_address {
            self.ensure_token_approval(params.token_in, params.amount_in)
                .await?;
        }

        let router_contract = self.get_router_contract()?;
        let path = ProtocolUtils::create_path_through_weth(
            params.token_in,
            params.token_out,
            self.network.weth_address,
        );

        let gas_price = self.calculate_gas_price(params.priority.clone()).await?;

        // Execute the appropriate swap function based on token types
        let pending_tx = if params.token_in == self.network.weth_address {
            // Swapping ETH for tokens
            router_contract
                .method::<_, H256>(
                    "swapExactETHForTokens",
                    (params.amount_out_min, path, params.to, params.deadline),
                )
                .map_err(|e| {
                    Error::Contract(format!("Failed to get swapExactETHForTokens method: {}", e))
                })?
                .value(params.amount_in)
                .gas_price(gas_price)
        } else if params.token_out == self.network.weth_address {
            // Swapping tokens for ETH
            router_contract
                .method::<_, H256>(
                    "swapExactTokensForETH",
                    (
                        params.amount_in,
                        params.amount_out_min,
                        path,
                        params.to,
                        params.deadline,
                    ),
                )
                .map_err(|e| {
                    Error::Contract(format!("Failed to get swapExactTokensForETH method: {}", e))
                })?
                .gas_price(gas_price)
        } else {
            // Swapping tokens for tokens
            router_contract
                .method::<_, H256>(
                    "swapExactTokensForTokens",
                    (
                        params.amount_in,
                        params.amount_out_min,
                        path,
                        params.to,
                        params.deadline,
                    ),
                )
                .map_err(|e| {
                    Error::Contract(format!(
                        "Failed to get swapExactTokensForTokens method: {}",
                        e
                    ))
                })?
                .gas_price(gas_price)
        };

        let pending_tx = pending_tx
            .send()
            .await
            .map_err(|e| Error::Transaction(format!("Failed to send swap transaction: {}", e)))?;

        let tx_hash = pending_tx.tx_hash();
        info!("Swap transaction submitted: {:?}", tx_hash);

        // Emit transaction event
        let event = ActorEvent::DexTransaction(DexTransactionEvent::Submitted {
            tx_id: format!("{:?}", tx_hash),
            submitted_details: Some(SubmittedTransactionInfo {
                from_token_address: format!("{:?}", params.token_in),
                to_token_address: format!("{:?}", params.token_out),
                amount_in_f64: ethers::utils::format_units(params.amount_in, 18)
                    .unwrap_or_default()
                    .parse()
                    .unwrap_or(0.0),
                slippage_tolerance: None, // Could be calculated from params if needed
                price_limit: None,
                dex_name: "Uniswap V2".to_string(),
            }),
            submission_time: Utc::now(),
            priority: params.priority.clone(),
        });

        if let Err(e) = self.message_bus.publish(event).await {
            log::warn!("Failed to publish transaction event: {}", e);
        }

        // Wait for transaction confirmation
        let receipt = pending_tx
            .await
            .map_err(|e| Error::Transaction(format!("Transaction failed: {}", e)))?
            .ok_or_else(|| Error::Transaction("Transaction receipt not found".to_string()))?;

        // Create transaction details
        let tx_details = TransactionDetails {
            tx_id: format!("{:?}", tx_hash),
            amount_in: ethers::utils::format_units(params.amount_in, 18)
                .unwrap_or_default()
                .parse()
                .unwrap_or(0.0),
            token_in_address: format!("{:?}", params.token_in),
            amount_out: 0.0, // Will be calculated from logs
            token_out_address: format!("{:?}", params.token_out),
            actual_price: 0.0, // Will be calculated
            fees_paid: ethers::utils::format_units(
                receipt.gas_used.unwrap_or_default()
                    * receipt.effective_gas_price.unwrap_or_default(),
                18,
            )
            .unwrap_or_default()
            .parse()
            .unwrap_or(0.0),
            fee_currency: self.network.native_currency_symbol.clone(),
            gas_used: receipt.gas_used.map(|g| g.as_u64()),
            gas_price: receipt.effective_gas_price.map(|gp| {
                ethers::utils::format_units(gp, 9)
                    .unwrap_or_default()
                    .parse()
                    .unwrap_or(0.0)
            }),
            block_number: receipt.block_number.map(|bn| bn.as_u64()),
            confirmation_time: Some(Utc::now()),
        };

        Ok(tx_details)
    }

    async fn get_pair_address(&self, token_a: Address, token_b: Address) -> Result<Address> {
        let factory_contract = self.get_factory_contract()?;

        let pair_address: Address = factory_contract
            .method::<_, Address>("getPair", (token_a, token_b))
            .map_err(|e| Error::Contract(format!("Failed to get getPair method: {}", e)))?
            .call()
            .await
            .map_err(|e| Error::Contract(format!("Failed to get pair address: {}", e)))?;

        if pair_address == Address::zero() {
            return Err(Error::Contract("Pair does not exist".to_string()));
        }

        Ok(pair_address)
    }

    async fn get_reserves(&self, pair_address: Address) -> Result<(U256, U256)> {
        let pair_contract = self.get_pair_contract(pair_address)?;

        let (reserve0, reserve1, _): (u128, u128, u32) = pair_contract
            .method::<_, (u128, u128, u32)>("getReserves", ())
            .map_err(|e| Error::Contract(format!("Failed to get getReserves method: {}", e)))?
            .call()
            .await
            .map_err(|e| Error::Contract(format!("Failed to get reserves: {}", e)))?;

        Ok((U256::from(reserve0), U256::from(reserve1)))
    }

    fn get_router_address(&self) -> Address {
        self.network.router_address
    }

    fn get_factory_address(&self) -> Address {
        self.network.factory_address
    }

    fn get_protocol_name(&self) -> &'static str {
        "Uniswap V2"
    }
}
