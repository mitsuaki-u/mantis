//! Main Uniswap V3 Protocol Provider implementation

use super::execution::SwapExecutor;
use super::gas::GasEstimator;
use super::pool_cache::PoolCache;
use super::pricing::PriceFetcher;
use super::quoter::Quoter;
use super::types::{format_address, ExactInputSingleParams, PoolInfo, SwapParams, V3FeeTier};
use crate::core::constants::DEFAULT_MIN_TRADE_SIZE_FOR_GAS;
use crate::infrastructure::constants::WEI_PER_GWEI;
use crate::infrastructure::dex::ethereum::config::NetworkConfig;
use crate::infrastructure::dex::TransactionDetails;
use crate::infrastructure::errors::{Error, Result};
use ethers::contract::Contract;
use ethers::prelude::{LocalWallet, Middleware, Signer};
use ethers::types::{Address, U256};
use ethers::utils::format_units;
use log::info;
use std::str::FromStr;
use std::sync::Arc;

/// Uniswap V3 protocol provider - handles V3-specific trading operations
/// Uses concentrated liquidity pools with multiple fee tiers (0.01%, 0.05%, 0.3%, 1%)
pub struct UniswapV3ProtocolProvider {
    rpc_provider: Arc<ethers::providers::Provider<ethers::providers::Http>>,
    network: NetworkConfig,
    factory_address: Address,
    trading_config: crate::config::TradingConfig,

    // Specialized modules
    pool_cache: PoolCache,
    quoter: Quoter,
    gas_estimator: GasEstimator,
    price_fetcher: PriceFetcher,
    swap_executor: SwapExecutor,
}

impl UniswapV3ProtocolProvider {
    pub fn new(
        rpc_provider: Arc<ethers::providers::Provider<ethers::providers::Http>>,
        network: NetworkConfig,
        trading_config: crate::config::TradingConfig,
    ) -> Self {
        let factory_address = network.factory_address;

        // Initialize specialized modules
        let pool_cache = PoolCache::new();
        let quoter = Quoter::new(rpc_provider.clone(), network.quoter_address);
        let gas_estimator = GasEstimator::new(
            rpc_provider.clone(),
            network.clone(),
            trading_config.clone(),
        );
        let price_fetcher = PriceFetcher::new(network.clone(), rpc_provider.clone());
        let swap_executor = SwapExecutor::new(rpc_provider.clone(), network.clone());

        Self {
            rpc_provider,
            network,
            factory_address,
            trading_config,
            pool_cache,
            quoter,
            gas_estimator,
            price_fetcher,
            swap_executor,
        }
    }

    /// Get the protocol name
    pub fn get_protocol_name(&self) -> &'static str {
        "Uniswap V3"
    }

    /// Get the router contract address
    pub fn get_router_address(&self) -> Address {
        self.network.router_address
    }

    /// Get the factory contract address
    pub fn get_factory_address(&self) -> Address {
        self.factory_address
    }

    /// Get a reference to the RPC provider
    pub fn get_rpc_provider(&self) -> Arc<ethers::providers::Provider<ethers::providers::Http>> {
        self.rpc_provider.clone()
    }

    /// Update cache with discovered pools from market data
    pub async fn update_pool_cache(
        &self,
        pools: Vec<crate::application::events::PoolDiscoveryData>,
        source: &str,
    ) {
        self.pool_cache.update(pools, source).await;
    }

    /// Get all cached pools for a token pair
    async fn get_cached_pools(&self, token_a: Address, token_b: Address) -> Option<Vec<PoolInfo>> {
        self.pool_cache.get_pools(token_a, token_b).await
    }

    /// Select best pool from multiple pools using Uniswap Quoter contract
    /// Gets actual quotes from each pool and picks the one with best output
    async fn select_best_pool_for_trade(
        &self,
        pools: Vec<PoolInfo>,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
    ) -> Result<Option<PoolInfo>> {
        // Create a closure that captures self and delegates to quoter
        let quote_fn =
            |token_in: Address, token_out: Address, amount_in: U256, fee_tier: V3FeeTier| {
                let quoter = &self.quoter;
                async move {
                    quoter
                        .quote_single(token_in, token_out, amount_in, fee_tier)
                        .await
                }
            };

        self.pool_cache
            .select_best(pools, token_in, token_out, amount_in, quote_fn)
            .await
    }

    /// Get a quote for exact input single swap using Uniswap V3 Quoter contract
    pub async fn get_quote_exact_input_single(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        fee_tier: V3FeeTier,
    ) -> Result<U256> {
        self.quoter
            .quote_single(token_in, token_out, amount_in, fee_tier)
            .await
    }

    /// Select the best pool for a swap by querying all available pools
    /// Returns the expected output amount and the fee tier of the selected pool
    pub async fn select_best_pool_for_swap(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
    ) -> Result<(U256, V3FeeTier)> {
        let pools = self.get_cached_pools(token_in, token_out).await;
        self.quoter
            .quote_best(token_in, token_out, amount_in, pools)
            .await
    }

    /// Execute a Uniswap V3 swap with optional wallet
    pub async fn execute_swap(
        &self,
        params: SwapParams,
        wallet: Option<LocalWallet>,
    ) -> Result<TransactionDetails> {
        info!(
            "🔄 Executing Uniswap V3 swap: {} -> {} (amount_in: {}, min_out: {})",
            format_address(params.token_in),
            format_address(params.token_out),
            params.amount_in,
            params.amount_out_minimum
        );

        // Step 1: Get all cached pools for this token pair
        let cached_pools = self
            .get_cached_pools(params.token_in, params.token_out)
            .await;

        let pool_info = match cached_pools {
            Some(pools) if !pools.is_empty() => {
                // Use Quoter contract to get actual quotes and select best pool
                match self
                    .select_best_pool_for_trade(
                        pools,
                        params.token_in,
                        params.token_out,
                        params.amount_in,
                    )
                    .await?
                {
                    Some(pool) => {
                        info!(
                            "✅ Using best pool for swap: {:?} (fee: {:?}, TVL: ${:.2})",
                            pool.pool_address, pool.fee_tier, pool.tvl_usd
                        );
                        pool
                    }
                    None => {
                        return Err(Error::Dex(format!(
                            "No valid pool found for {}/{} after querying all cached pools",
                            format_address(params.token_in),
                            format_address(params.token_out)
                        )));
                    }
                }
            }
            _ => {
                return Err(Error::Dex(format!(
                    "No cached V3 pools found for {}/{}. Ensure MarketDataActor is running and has scanned pools.",
                    format_address(params.token_in),
                    format_address(params.token_out)
                )));
            }
        };

        // Step 2: Critical validations before executing swap

        // 2a. Validate trade_size_usd meets minimum requirements for gas protection
        let trade_size_usd = params.trade_size_usd.ok_or_else(|| {
            Error::Trading(
                "trade_size_usd is required for gas protection and risk management".to_string(),
            )
        })?;

        let min_trade_size = DEFAULT_MIN_TRADE_SIZE_FOR_GAS;
        if trade_size_usd < min_trade_size {
            return Err(Error::Trading(format!(
                "Trade size ${:.2} is below minimum ${:.2} required for gas protection",
                trade_size_usd, min_trade_size
            )));
        }

        info!(
            "💵 Trade size: ${:.2} USD (minimum: ${:.2})",
            trade_size_usd, min_trade_size
        );

        // 2b. Validate minimum output amount
        if params.amount_out_minimum == U256::zero() {
            return Err(Error::Trading(
                "amount_out_minimum cannot be zero - indicates invalid quote".to_string(),
            ));
        }

        // 2c. Check wallet has sufficient ETH balance for gas
        if let Some(ref wallet) = wallet {
            let eth_balance = self
                .get_native_balance(wallet.address())
                .await
                .map_err(|e| Error::Wallet(format!("Failed to get ETH balance: {}", e)))?;

            let min_native_required = self.trading_config.min_native_balance;

            if eth_balance < min_native_required {
                return Err(Error::Trading(format!(
                    "Insufficient ETH balance for gas: {:.6} ETH < {:.6} ETH minimum",
                    eth_balance, min_native_required
                )));
            }

            info!(
                "✅ ETH balance check passed: {:.6} ETH >= {:.6} ETH minimum",
                eth_balance, min_native_required
            );
        }

        // Step 3: Prepare swap parameters for Uniswap V3 SwapRouter
        let deadline = params.deadline;
        let recipient = params.to;
        let amount_out_minimum = params.amount_out_minimum;

        // Step 4: Build the ExactInputSingle parameters struct
        let swap_params = ExactInputSingleParams {
            token_in: params.token_in,
            token_out: params.token_out,
            fee: pool_info.fee_tier as u32,
            recipient,
            deadline,
            amount_in: params.amount_in,
            amount_out_minimum,
            sqrt_price_limit_x96: U256::zero(),
        };

        info!(
            "🎯 Swap details: Pool fee {:.2}%, Deadline: {}, Min out: {}",
            (pool_info.fee_tier as u32 as f64) / crate::core::constants::V3_FEE_TIER_DIVISOR
                * 100.0,
            deadline,
            amount_out_minimum
        );

        // Step 5: Estimate gas and validate costs (for both paper and real trading)
        let eth_price_usd = self.price_fetcher.get_eth_price_f64().await?;
        let gas_estimate = self
            .gas_estimator
            .estimate(&params, &swap_params, eth_price_usd)
            .await?;

        // Step 6: Execute the swap (real or simulated)
        if let Some(wallet) = wallet {
            // Execute real swap with wallet
            self.swap_executor
                .execute_live(params, pool_info, swap_params, wallet, gas_estimate)
                .await
        } else {
            // Return simulated transaction result for testing/paper trading
            let tx_details = SwapExecutor::build_simulated_tx(&params, gas_estimate);
            info!("✅ Swap simulation completed: {}", tx_details.tx_id);
            Ok(tx_details)
        }
    }

    /// Get the USD price of a token
    pub async fn get_token_price_usd(&self, token_address: &str) -> Result<rust_decimal::Decimal> {
        self.price_fetcher.get_token_price_usd(token_address).await
    }

    /// Get the USD price of the native currency (ETH/MATIC)
    pub async fn get_native_price_usd(&self) -> Result<rust_decimal::Decimal> {
        self.price_fetcher.get_native_price_usd().await
    }

    /// Get wallet token balance
    pub async fn get_wallet_token_balance(
        &self,
        token_address: &str,
        wallet_address: Address,
    ) -> Result<f64> {
        let token_addr = Address::from_str(token_address)
            .map_err(|e| Error::Parse(format!("Invalid token address: {}", e)))?;

        let provider = self.rpc_provider.clone();

        let contract = Contract::new(token_addr, super::abi::load_erc20_abi()?, provider);

        let balance: U256 = contract
            .method::<_, U256>("balanceOf", wallet_address)
            .map_err(|e| Error::Contract(format!("Failed to get balanceOf method: {}", e)))?
            .call()
            .await
            .map_err(|e| Error::Contract(format!("Failed to get balance: {}", e)))?;

        // Get token decimals
        let decimals: u8 = contract
            .method::<_, u8>("decimals", ())
            .map_err(|e| Error::Contract(format!("Failed to get decimals method: {}", e)))?
            .call()
            .await
            .map_err(|e| Error::Contract(format!("Failed to get decimals: {}", e)))?;

        let balance_f64 = format_units(balance, decimals as usize)
            .map_err(|e| Error::Parse(format!("Failed to format balance: {}", e)))?
            .parse::<f64>()
            .map_err(|e| Error::Parse(format!("Failed to parse balance: {}", e)))?;

        Ok(balance_f64)
    }

    /// Get native currency balance
    pub async fn get_native_balance(&self, wallet_address: Address) -> Result<f64> {
        let provider = self.rpc_provider.clone();

        let balance = provider
            .get_balance(wallet_address, None)
            .await
            .map_err(|e| Error::Network(format!("Failed to get balance: {}", e)))?;

        let balance_f64 = format_units(balance, self.network.native_currency_decimals as usize)
            .map_err(|e| Error::Parse(format!("Failed to format balance: {}", e)))?
            .parse::<f64>()
            .map_err(|e| Error::Parse(format!("Failed to parse balance: {}", e)))?;

        Ok(balance_f64)
    }

    /// Estimate gas for a WETH wrap operation
    /// Delegates to GasEstimator for accurate contract-based estimation
    pub async fn estimate_wrap_gas(
        &self,
        amount_eth: f64,
        priority: crate::infrastructure::dex::TransactionPriority,
        eth_price_usd: f64,
        trade_size_usd: Option<f64>,
    ) -> Result<super::types::GasEstimate> {
        self.gas_estimator
            .estimate_wrap(amount_eth, priority, eth_price_usd, trade_size_usd)
            .await
    }

    /// Get current gas price in Gwei
    pub async fn get_gas_price_gwei(&self) -> Result<f64> {
        let provider = self.rpc_provider.clone();

        let gas_price_wei = provider
            .get_gas_price()
            .await
            .map_err(|e| Error::Network(format!("Failed to get gas price: {}", e)))?;

        let gas_price_gwei = gas_price_wei.as_u128() as f64 / WEI_PER_GWEI;
        Ok(gas_price_gwei)
    }
}

impl Clone for UniswapV3ProtocolProvider {
    fn clone(&self) -> Self {
        Self {
            rpc_provider: self.rpc_provider.clone(),
            network: self.network.clone(),
            factory_address: self.factory_address,
            trading_config: self.trading_config.clone(),
            pool_cache: PoolCache {
                inner: self.pool_cache.inner.clone(),
            },
            quoter: Quoter::new(self.rpc_provider.clone(), self.network.quoter_address),
            gas_estimator: GasEstimator::new(
                self.rpc_provider.clone(),
                self.network.clone(),
                self.trading_config.clone(),
            ),
            price_fetcher: PriceFetcher::new(self.network.clone(), self.rpc_provider.clone()),
            swap_executor: SwapExecutor::new(self.rpc_provider.clone(), self.network.clone()),
        }
    }
}
