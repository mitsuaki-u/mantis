use std::sync::Arc;

use ethers::signers::{LocalWallet, Signer};
use ethers::types::{Address, U256};
use ethers::utils::parse_units;
use log::{debug, info, trace, warn};
use rust_decimal::prelude::*;
use rust_decimal::Decimal;

use crate::config::Config;
use crate::core::constants::DEFAULT_V3_SWAP_GAS_LIMIT;
use crate::events::{DexTransactionEvent, Event, SubmittedTransactionInfo};
use crate::infrastructure::constants::{
    DEFAULT_SWAP_DEADLINE_SECS, ETH_TOKEN_SWAP_GAS_ESTIMATE, TOKEN_TOKEN_SWAP_GAS_ESTIMATE,
    WEI_PER_ETH, WEI_PER_GWEI,
};
use crate::infrastructure::dex::{NetworkFeeInfo, TransactionDetails, TransactionStatus};
use crate::infrastructure::errors::{Error, Result};
use crate::EventRouter;

use super::config::NetworkConfig;
use super::tokens::{TokenRegistry, TokenRegistryService};
use super::transactions::TransactionManager;
use crate::infrastructure::dex::ethereum::providers::uniswap_v3::{
    SwapParams, UniswapV3ProtocolProvider,
};

pub struct EthereumDexClient {
    network: NetworkConfig,
    wallet: Option<LocalWallet>,
    transaction_manager: TransactionManager,
    uniswap_v3_provider: UniswapV3ProtocolProvider,
    event_router: Arc<EventRouter>,
    token_registry: Arc<TokenRegistry>,
    provider: Arc<ethers::providers::Provider<ethers::providers::Http>>,
    config: Arc<Config>,
}

impl EthereumDexClient {
    /// Create a new Ethereum DEX client with configuration
    pub async fn new(config: &Config, event_router: Arc<EventRouter>) -> Result<Self> {
        let network = NetworkConfig::from_config(config)?;

        let wallet = if let Some(ref wallet_config) = config.dex.wallet {
            Some(wallet_config.load_wallet()?)
        } else {
            None
        };

        let provider = network.create_provider()?;

        info!("🦄 Creating Uniswap V3 protocol provider");
        let rpc_provider = network.create_configured_provider(config)?;
        let uniswap_v3_provider =
            UniswapV3ProtocolProvider::new(rpc_provider, network.clone(), config.trading.clone());

        let transaction_manager = TransactionManager::new(
            provider.clone(),
            network.clone(),
            uniswap_v3_provider.clone(),
        );
        info!(
            "✅ V3 Provider created successfully: {}",
            uniswap_v3_provider.get_protocol_name()
        );
        info!(
            "🔧 V3 Router: {:?}",
            uniswap_v3_provider.get_router_address()
        );
        info!(
            "🔧 V3 Factory: {:?}",
            uniswap_v3_provider.get_factory_address()
        );

        let token_registry = TokenRegistryService::get();
        token_registry.set_provider(provider.clone()).await;

        info!(
            "✅ Ethereum DEX client initialized for network: {}",
            network.name
        );

        Ok(Self {
            network,
            wallet,
            transaction_manager,
            uniswap_v3_provider,
            event_router,
            token_registry,
            provider,
            config: Arc::new(config.clone()),
        })
    }

    pub async fn connect_wallet(&mut self, private_key_hex: &str) -> Result<()> {
        let private_key_clean = private_key_hex
            .strip_prefix("0x")
            .unwrap_or(private_key_hex);

        let wallet = private_key_clean
            .parse::<LocalWallet>()
            .map_err(|e| Error::Wallet(format!("Invalid private key: {}", e)))?
            .with_chain_id(self.network.chain_id);

        info!("Wallet connected: {:?}", wallet.address());
        self.wallet = Some(wallet.clone());

        let rpc_provider = self.network.create_configured_provider(&self.config)?;
        self.uniswap_v3_provider = UniswapV3ProtocolProvider::new(
            rpc_provider,
            self.network.clone(),
            self.config.trading.clone(),
        );

        Ok(())
    }

    pub async fn get_native_balance(&self) -> Result<f64> {
        if let Some(wallet) = &self.wallet {
            self.uniswap_v3_provider
                .get_native_balance(wallet.address())
                .await
        } else {
            Err(Error::Wallet("No wallet connected".to_string()))
        }
    }

    /// Update the pool cache with discovered pools from market data events
    pub async fn update_pool_cache(
        &self,
        pools: Vec<crate::application::events::PoolDiscoveryData>,
        source: &str,
    ) {
        self.uniswap_v3_provider
            .update_pool_cache(pools, source)
            .await;
    }

    pub async fn get_token_balance(&self, token_identifier: &str) -> Result<f64> {
        let token_address = self
            .token_registry
            .resolve_token(token_identifier, &self.network.name)
            .await?;

        let wallet_address = self
            .wallet
            .as_ref()
            .ok_or_else(|| Error::Config("No wallet configured".to_string()))?
            .address();

        self.uniswap_v3_provider
            .get_wallet_token_balance(&format!("{:?}", token_address), wallet_address)
            .await
    }

    pub async fn get_token_price_usd(&self, token_identifier: &str) -> Result<Decimal> {
        let token_address = self
            .token_registry
            .resolve_token(token_identifier, &self.network.name)
            .await?;

        self.uniswap_v3_provider
            .get_token_price_usd(&format!("{:?}", token_address))
            .await
    }

    /// Wrap ETH to WETH by depositing into the WETH contract
    /// Applies transaction priority settings
    pub async fn wrap_eth_to_weth(
        &self,
        amount_eth: f64,
        priority: crate::infrastructure::dex::TransactionPriority,
    ) -> Result<()> {
        use ethers::prelude::*;

        let wallet = self
            .wallet
            .as_ref()
            .ok_or_else(|| Error::Wallet("No wallet connected for wrapping".to_string()))?;

        info!("💫 Wrapping {:.4} ETH to WETH", amount_eth);

        let weth_address = self
            .token_registry
            .resolve_token("WETH", &self.network.name)
            .await?;

        let weth_abi =
            crate::infrastructure::dex::ethereum::providers::uniswap_v3::load_weth_abi()?;

        let signer = SignerMiddleware::new(
            self.uniswap_v3_provider.get_rpc_provider(),
            wallet.clone().with_chain_id(self.network.chain_id),
        );

        let weth_contract = Contract::new(weth_address, weth_abi, std::sync::Arc::new(signer));

        let amount_wei = U256::from((amount_eth * 1e18) as u64);

        let base_gas_price = self
            .uniswap_v3_provider
            .get_rpc_provider()
            .get_gas_price()
            .await
            .map_err(|e| Error::Network(format!("Failed to get gas price: {}", e)))?;

        let priority_multiplier =
            crate::infrastructure::dex::ethereum::providers::uniswap_v3::get_priority_multiplier(
                priority,
            );

        let adjusted_gas_price =
            U256::from((base_gas_price.as_u128() as f64 * priority_multiplier) as u128);

        let mut call = weth_contract
            .method::<_, ()>("deposit", ())
            .map_err(|e| Error::Abi(format!("Failed to create deposit method call: {}", e)))?;

        call = call.value(amount_wei).gas_price(adjusted_gas_price);

        let tx = call
            .send()
            .await
            .map_err(|e| Error::Network(format!("Failed to send wrap transaction: {}", e)))?;

        info!("📤 Wrap transaction sent: {:?}", tx.tx_hash());

        let receipt = tx
            .await
            .map_err(|e| Error::Network(format!("Wrap transaction failed: {}", e)))?
            .ok_or_else(|| Error::Network("Wrap transaction receipt is None".to_string()))?;

        if receipt.status != Some(1.into()) {
            return Err(Error::Network("Wrap transaction reverted".to_string()));
        }

        info!("✅ Successfully wrapped {:.4} ETH to WETH", amount_eth);
        Ok(())
    }

    /// Check WETH balance and automatically wrap ETH if needed
    /// Validates wrap gas cost against trading_config limits via GasEstimator
    pub async fn ensure_weth_balance(
        &self,
        required_weth: f64,
        trade_size_usd: f64,
        priority: crate::infrastructure::dex::TransactionPriority,
    ) -> Result<()> {
        let weth_balance = self.get_token_balance("WETH").await?;

        if weth_balance >= required_weth {
            debug!(
                "✅ Sufficient WETH balance: {:.4} (need: {:.4})",
                weth_balance, required_weth
            );
            return Ok(());
        }

        let needed = required_weth - weth_balance;
        info!(
            "⚠️ Insufficient WETH: have {:.4}, need {:.4}, must wrap {:.4} ETH",
            weth_balance, required_weth, needed
        );

        let eth_price_usd = self.get_eth_price_usd().await?;

        let wrap_gas_estimate = self
            .uniswap_v3_provider
            .estimate_wrap_gas(needed, priority, eth_price_usd, Some(trade_size_usd))
            .await?;

        let wrap_gas_eth = wrap_gas_estimate.estimated_cost_eth;
        info!(
            "⛽ Estimated wrap gas: {:.6} ETH (${:.2})",
            wrap_gas_eth, wrap_gas_estimate.estimated_cost_usd
        );

        let eth_balance = self.get_native_balance().await?;

        let gas_reserve = wrap_gas_eth * 5.0; // Conservative: wrap + swap + buffer
        let total_needed = needed + gas_reserve;

        if eth_balance < total_needed {
            return Err(Error::Trading(format!(
                "Insufficient ETH: need {:.4} ETH total ({:.4} for WETH + {:.4} gas reserve), have {:.4} ETH",
                total_needed, needed, gas_reserve, eth_balance
            )));
        }

        info!(
            "✅ Gas validation passed. Proceeding with wrap (reserved {:.4} ETH for future gas)",
            gas_reserve
        );

        self.wrap_eth_to_weth(needed, priority).await?;

        info!("🎉 Auto-wrap successful! New WETH balance ready for trade");
        Ok(())
    }

    pub async fn execute_swap(
        &self,
        swap: crate::infrastructure::dex::SwapParams<'_>,
    ) -> Result<TransactionDetails> {
        let from_token_identifier = swap.token_in;
        let to_token_identifier = swap.token_out;
        let amount_in_usd = swap.amount_in;
        let slippage_tolerance = swap.slippage_tolerance;
        let price_limit = swap.price_limit;
        let priority = swap.priority;
        let direction = swap.direction;
        info!(
            "🔄 Executing {} swap: {} -> {} (${:.2}) on {}",
            match direction {
                crate::infrastructure::dex::SwapDirection::Buy => "BUY",
                crate::infrastructure::dex::SwapDirection::Sell => "SELL",
            },
            from_token_identifier,
            to_token_identifier,
            amount_in_usd,
            self.network.name
        );

        let from_token = self
            .token_registry
            .resolve_token(from_token_identifier, &self.network.name)
            .await
            .map_err(|e| {
                Error::Config(format!(
                    "Failed to resolve 'from' token '{}': {}",
                    from_token_identifier, e
                ))
            })?;

        let to_token = self
            .token_registry
            .resolve_token(to_token_identifier, &self.network.name)
            .await
            .map_err(|e| {
                Error::Config(format!(
                    "Failed to resolve 'to' token '{}': {}",
                    to_token_identifier, e
                ))
            })?;

        debug!(
            "✅ Resolved tokens for {} operation: {} -> {}",
            match direction {
                crate::infrastructure::dex::SwapDirection::Buy => "BUY",
                crate::infrastructure::dex::SwapDirection::Sell => "SELL",
            },
            from_token,
            to_token
        );

        let from_token_price = self
            .uniswap_v3_provider
            .get_token_price_usd(&format!("{:?}", from_token))
            .await?;

        let to_token_price = self
            .uniswap_v3_provider
            .get_token_price_usd(&format!("{:?}", to_token))
            .await?;

        let amount_in_usd_decimal = Decimal::from_f64(amount_in_usd)
            .ok_or_else(|| Error::Conversion("Invalid amount_in_usd".to_string()))?;
        let amount_in_tokens = amount_in_usd_decimal / from_token_price;

        let from_token_decimals = self.token_registry.get_token_decimals(from_token).await?;
        let decimals_factor = Decimal::new(10_i64.pow(from_token_decimals as u32), 0);
        let amount_in_units = amount_in_tokens * decimals_factor;
        let amount_in = U256::from(
            amount_in_units
                .to_u64()
                .ok_or_else(|| Error::Conversion("Amount too large for U256".to_string()))?,
        );

        info!("📊 Selecting best pool for swap using V3 Quoter contract");
        let (amount_out_expected, _fee_tier) = self
            .uniswap_v3_provider
            .select_best_pool_for_swap(from_token, to_token, amount_in)
            .await
            .map_err(|e| {
                Error::Trading(format!(
                    "Failed to select pool for {} -> {}: {}",
                    from_token_identifier, to_token_identifier, e
                ))
            })?;

        if amount_out_expected.is_zero() {
            return Err(Error::Trading(format!(
                "Quote returned zero output amount for {} -> {}. This usually indicates insufficient liquidity or invalid token pair.",
                from_token_identifier, to_token_identifier
            )));
        }

        info!(
            "✅ Quote received: {} tokens expected out for {} tokens in",
            amount_out_expected, amount_in
        );

        let slippage_factor = 1.0 - slippage_tolerance;
        let amount_out_min =
            U256::from((amount_out_expected.as_u128() as f64 * slippage_factor) as u128);

        if amount_out_min.is_zero() {
            return Err(Error::Trading(format!(
                "Calculated minimum output amount is zero. This may indicate excessive slippage tolerance ({:.2}%) or very small trade amount.",
                slippage_tolerance * 100.0
            )));
        }

        info!(
            "🛡️ Slippage protection: expecting {} tokens, minimum {} tokens ({}% slippage)",
            amount_out_expected,
            amount_out_min,
            slippage_tolerance * 100.0
        );

        if let Some(limit) = price_limit {
            let limit_decimal = Decimal::from_f64(limit)
                .ok_or_else(|| Error::Conversion("Invalid price limit".to_string()))?;
            if to_token_price > limit_decimal {
                return Err(Error::InvalidInput(format!(
                    "Token price ${:.4} exceeds limit ${:.4}",
                    to_token_price, limit
                )));
            }
        }

        let wallet_address = self
            .wallet
            .as_ref()
            .ok_or_else(|| Error::Config("No wallet configured for trading".to_string()))?
            .address();

        let deadline = U256::from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| Error::Internal(format!("System time error: {}", e)))?
                .as_secs()
                + DEFAULT_SWAP_DEADLINE_SECS,
        );

        let gas_limit = Some(DEFAULT_V3_SWAP_GAS_LIMIT);

        let swap_params = SwapParams {
            token_in: from_token,
            token_out: to_token,
            amount_in,
            amount_out_minimum: amount_out_min,
            to: wallet_address,
            deadline,
            priority,
            gas_limit,
            trade_size_usd: Some(amount_in_usd), // Pass the USD trade size for gas protection
        };

        if self.wallet.is_some() {
            info!("🚀 Executing REAL swap with configured wallet");
        } else {
            warn!("⚠️ No wallet configured - executing simulated swap");
        }
        let tx_details = self
            .uniswap_v3_provider
            .execute_swap(swap_params, self.wallet.clone())
            .await?;

        let submitted_event = DexTransactionEvent::Submitted {
            tx_id: tx_details.tx_id.clone(),
            submitted_details: Some(SubmittedTransactionInfo {
                from_token_address: tx_details.token_in_address.clone(),
                to_token_address: tx_details.token_out_address.clone(),
                amount_in_f64: tx_details.amount_in,
                price_limit,
                dex_name: "Uniswap V3".to_string(),
            }),
            submission_time: tx_details.timestamp,
            priority,
        };

        let execution_event = Event::DexTransaction(Box::new(submitted_event));

        if let Err(e) = self.event_router.publish(execution_event).await {
            warn!(
                "Failed to publish DexTransactionEvent::Submitted for {}: {}",
                tx_details.tx_id, e
            );
            // Don't fail the swap - event publishing is not critical for the transaction itself
        } else {
            debug!(
                "Published DexTransactionEvent::Submitted for transaction: {}",
                tx_details.tx_id
            );
        }

        Ok(tx_details)
    }

    pub async fn get_transaction_status(
        &self,
        tx_id_str: &str,
    ) -> Result<(TransactionStatus, Option<TransactionDetails>)> {
        self.transaction_manager
            .get_transaction_status(tx_id_str)
            .await
    }

    pub async fn get_network_fees(&self) -> Result<NetworkFeeInfo> {
        self.transaction_manager.get_network_fees().await
    }

    pub fn get_protocol_name(&self) -> &'static str {
        self.uniswap_v3_provider.get_protocol_name()
    }

    pub fn get_router_address(&self) -> Address {
        self.uniswap_v3_provider.get_router_address()
    }

    pub fn get_factory_address(&self) -> Address {
        self.uniswap_v3_provider.get_factory_address()
    }

    pub fn get_network_info(&self) -> &NetworkConfig {
        &self.network
    }

    /// Get the connected wallet address, if available
    pub fn get_wallet_address(&self) -> Option<ethers::types::Address> {
        self.wallet.as_ref().map(|w| w.address())
    }

    /// Get the chain ID for the connected wallet, if available
    pub fn get_chain_id(&self) -> Option<u64> {
        Some(self.network.chain_id)
    }

    /// Check if a wallet is connected
    pub fn is_wallet_connected(&self) -> bool {
        self.wallet.is_some()
    }

    /// Resolve a token symbol or ID to a contract address on the current network
    /// Simple wrapper around TokenRegistry for convenience
    pub async fn resolve_token_address(&self, token_identifier: &str) -> Result<String> {
        match self
            .token_registry
            .resolve_token(token_identifier, &self.network.name)
            .await
        {
            Ok(address) => {
                trace!("Resolved {} to {}", token_identifier, address);
                Ok(format!("{:?}", address))
            }
            Err(e) => {
                trace!("Failed to resolve {}: {}", token_identifier, e);
                Err(Error::Config(format!(
                    "Failed to resolve 'to' token '{}': {}",
                    token_identifier, e
                )))
            }
        }
    }

    /// Resolve a token to both its address and authoritative trading symbol
    /// Simple wrapper around TokenRegistry for convenience
    pub async fn resolve_token_with_symbol(
        &self,
        token_identifier: &str,
    ) -> Result<(String, String)> {
        match self
            .token_registry
            .resolve_token(token_identifier, &self.network.name)
            .await
        {
            Ok(address) => match self.token_registry.get_authoritative_symbol(address).await {
                Ok(auth_symbol) => {
                    trace!(
                        "Resolved {} to address {} with symbol {}",
                        token_identifier,
                        address,
                        auth_symbol
                    );
                    Ok((format!("{:?}", address), auth_symbol))
                }
                Err(e) => {
                    warn!("Could not get authoritative symbol for {}: {}. Using input identifier as symbol", token_identifier, e);
                    Ok((format!("{:?}", address), token_identifier.to_string()))
                }
            },
            Err(e) => {
                trace!("Failed to resolve {}: {}", token_identifier, e);
                Err(Error::Config(format!(
                    "Failed to resolve token '{}': {}",
                    token_identifier, e
                )))
            }
        }
    }

    /// Get access to the token registry
    pub fn get_token_registry(&self) -> Arc<TokenRegistry> {
        self.token_registry.clone()
    }

    /// Get the network name
    pub fn get_network_name(&self) -> String {
        self.network.name.clone()
    }

    /// Get access to the V3 provider for advanced operations (replaces price oracle)
    pub fn get_v3_provider(&self) -> &UniswapV3ProtocolProvider {
        &self.uniswap_v3_provider
    }

    /// Get current network gas price in Gwei using multi-provider fallback
    pub async fn get_gas_price_gwei(&self) -> Result<f64> {
        let gas_price_gwei = self.uniswap_v3_provider.get_gas_price_gwei().await?;
        info!("Current network gas price: {:.2} Gwei", gas_price_gwei);
        Ok(gas_price_gwei)
    }

    /// Estimate gas fee for a transaction (swap or wrap) in ETH
    /// For wraps, pass same address for both token_a and token_b (WETH address)
    pub async fn estimate_gas_fee(
        &self,
        token_a: Address,
        token_b: Address,
        _amount: f64,
    ) -> Result<f64> {
        use crate::infrastructure::constants::WETH_WRAP_GAS_ESTIMATE;

        let gas_price_gwei = self.uniswap_v3_provider.get_gas_price_gwei().await?;
        let gas_price_wei = ethers::types::U256::from((gas_price_gwei * WEI_PER_GWEI) as u64);

        let (estimated_gas_limit, operation) =
            if token_a == token_b && token_a == self.network.weth_address {
                (WETH_WRAP_GAS_ESTIMATE, "WETH wrap")
            } else if token_a == self.network.weth_address || token_b == self.network.weth_address {
                (ETH_TOKEN_SWAP_GAS_ESTIMATE, "ETH<->Token swap")
            } else {
                (TOKEN_TOKEN_SWAP_GAS_ESTIMATE, "Token<->Token swap")
            };

        let total_gas_fee_wei = gas_price_wei * ethers::types::U256::from(estimated_gas_limit);
        let gas_fee_eth = total_gas_fee_wei.as_u128() as f64 / WEI_PER_ETH;

        debug!(
            "Estimated gas fee for {}: {:.6} ETH ({} gas @ {:.2} Gwei)",
            operation, gas_fee_eth, estimated_gas_limit, gas_price_gwei
        );

        Ok(gas_fee_eth)
    }

    /// Estimate gas fee for a swap transaction in ETH (legacy wrapper)
    pub async fn estimate_swap_gas_fee(
        &self,
        token_a: Address,
        token_b: Address,
        amount: f64,
    ) -> Result<f64> {
        self.estimate_gas_fee(token_a, token_b, amount).await
    }

    /// Get real-time ETH price in USD using on-chain price oracle (WETH)
    pub async fn get_eth_price_usd(&self) -> Result<f64> {
        let weth_price = self
            .get_token_price_usd(&format!("{:?}", self.network.weth_address))
            .await?;

        let eth_price = weth_price
            .to_f64()
            .ok_or_else(|| Error::Conversion("Failed to convert ETH price to f64".to_string()))?;

        debug!("Current ETH price from DEX: ${:.2}", eth_price);
        Ok(eth_price)
    }

    /// Estimate swap output and fee for a given input amount
    /// Uses the same pool selection logic as live trading
    /// Returns (expected_output_amount, fee_decimal_multiplier)
    /// Example: For 0.3% fee, returns 0.003 (not 0.3)
    pub async fn estimate_swap_output(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
    ) -> Result<(U256, f64)> {
        match self
            .uniswap_v3_provider
            .select_best_pool_for_swap(token_in, token_out, amount_in)
            .await
        {
            Ok((amount_out, fee_tier)) => {
                let fee_decimal =
                    (fee_tier as u32 as f64) / crate::core::constants::V3_FEE_TIER_DIVISOR;
                debug!(
                    "Selected pool for {} -> {}: {:.2}% fee tier (decimal: {:.5}), expected output: {}",
                    &format!("{:?}", token_in)[..8],
                    &format!("{:?}", token_out)[..8],
                    fee_decimal * 100.0,
                    fee_decimal,
                    amount_out
                );
                Ok((amount_out, fee_decimal))
            }
            Err(e) => {
                warn!("Could not select pool for swap: {}. Using fallback 0.3%", e);
                Ok((U256::zero(), crate::core::constants::V3_STANDARD_FEE_PCT))
            }
        }
    }

    /// Create a new Ethereum DEX client for paper trading
    /// This provides real blockchain queries but no wallet connection for trading simulation
    pub fn new_paper_trading(config: &Config) -> Result<Self> {
        let network_config = NetworkConfig::from_config(config)?;
        let provider = network_config.create_provider()?;

        let event_router = Arc::new(crate::EventRouter::with_default_routing());
        let rpc_provider = network_config.create_configured_provider(config)?;
        let uniswap_v3_provider = UniswapV3ProtocolProvider::new(
            rpc_provider,
            network_config.clone(),
            config.trading.clone(),
        );

        let transaction_manager = TransactionManager::new(
            provider.clone(),
            network_config.clone(),
            uniswap_v3_provider.clone(),
        );

        let token_registry = TokenRegistryService::get();
        let registry_clone = token_registry.clone();
        let provider_clone = provider.clone();
        tokio::spawn(async move {
            registry_clone.set_provider(provider_clone).await;
            debug!("Paper trading: Set up provider for TokenRegistry on-chain queries");
        });

        Ok(Self {
            network: network_config,
            wallet: None, // No wallet for paper trading
            transaction_manager,
            uniswap_v3_provider,
            event_router,
            token_registry,
            provider,
            config: Arc::new(config.clone()),
        })
    }
}

/// Helper function to convert f64 to U256 with specified decimals
pub fn float_to_u256(amount: f64, decimals: u8) -> Result<U256> {
    if amount < 0.0 {
        return Err(Error::Conversion("Amount cannot be negative".to_string()));
    }

    let amount_str = format!("{:.18}", amount); // Use 18 decimal places for precision
    parse_units(amount_str, decimals as usize)
        .map_err(|e| Error::Conversion(format!("Failed to convert amount: {}", e)))
        .map(|parse_units| parse_units.into())
}

impl Clone for EthereumDexClient {
    fn clone(&self) -> Self {
        let rpc_provider = self
            .network
            .create_configured_provider(&self.config)
            .expect("Failed to create RPC provider");
        let uniswap_v3_provider = UniswapV3ProtocolProvider::new(
            rpc_provider,
            self.network.clone(),
            self.config.trading.clone(),
        );

        Self {
            network: self.network.clone(),
            wallet: self.wallet.clone(),
            transaction_manager: self.transaction_manager.clone(),
            uniswap_v3_provider,
            event_router: self.event_router.clone(),
            token_registry: self.token_registry.clone(),
            provider: self.provider.clone(),
            config: self.config.clone(),
        }
    }
}
