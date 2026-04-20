//! Gas estimation and validation for Uniswap V3 swaps

use super::abi::load_swaprouter_abi;
use super::types::{ExactInputSingleParams, GasEstimate, SwapParams};
use crate::infrastructure::constants::WEI_PER_GWEI;
use crate::infrastructure::dex::ethereum::config::NetworkConfig;
use crate::infrastructure::dex::TransactionPriority;
use crate::infrastructure::errors::{Error, Result};
use ethers::contract::Contract;
use ethers::prelude::{LocalWallet, Middleware, Signer, SignerMiddleware};
use ethers::types::U256;
use log::info;
use std::sync::Arc;

/// Get the gas price multiplier for a given transaction priority
///
/// Priority multipliers control transaction inclusion speed vs cost tradeoff:
/// - Low (0.9x): Cheaper but slower, may fail during high volatility
/// - Medium/Standard (1.0x): Standard network rate
/// - High (1.2x): Faster inclusion, 20% premium
/// - Urgent (1.5x): Fastest inclusion, 50% premium
pub fn get_priority_multiplier(priority: TransactionPriority) -> f64 {
    match priority {
        TransactionPriority::Low => 0.9,
        TransactionPriority::Medium => 1.0,
        TransactionPriority::Standard => 1.0,
        TransactionPriority::High => 1.2,
        TransactionPriority::Urgent => 1.5,
    }
}

/// Gas estimator for Uniswap V3 swap transactions
pub(super) struct GasEstimator {
    provider: Arc<ethers::providers::Provider<ethers::providers::Http>>,
    network: NetworkConfig,
    trading_config: crate::config::TradingConfig,
}

impl GasEstimator {
    pub fn new(
        provider: Arc<ethers::providers::Provider<ethers::providers::Http>>,
        network: NetworkConfig,
        trading_config: crate::config::TradingConfig,
    ) -> Self {
        Self {
            provider,
            network,
            trading_config,
        }
    }

    /// Estimate gas for a swap transaction
    ///
    /// ## Purpose:
    /// Calculates the expected gas cost for executing a Uniswap V3 swap transaction.
    /// Runs for BOTH paper and live trading to validate that gas costs are acceptable
    /// before attempting to execute trades.
    ///
    /// ## How Gas Estimation Works:
    /// 1. Creates a temporary wallet signer (any address works for estimation)
    /// 2. Calls SwapRouter.exactInputSingle() with .estimate_gas() (simulates without executing)
    /// 3. Adds 20% safety buffer to gas estimate (accounts for price volatility between estimate and execution)
    /// 4. Fetches current network gas price from provider
    /// 5. Applies transaction priority multiplier (Low=0.9x, Standard=1.0x, High=1.2x, Urgent=1.5x)
    /// 6. Calculates total cost in ETH and USD
    /// 7. Validates against configured gas protection limits
    ///
    /// ## Why Gas Estimation Is Important:
    /// - Prevents executing trades where gas costs eat all profits
    /// - Protects against network congestion (high gas periods)
    /// - Ensures trades are economically viable before execution
    /// - Critical for small position sizes where gas can be >5% of trade value
    ///
    /// ## Parameters:
    /// - `params`: Swap parameters including trade size and priority
    /// - `swap_params`: Detailed Uniswap V3 swap parameters (tokens, amounts, deadline, etc.)
    /// - `eth_price_usd`: Current ETH/USD price for cost conversion
    ///
    /// ## Returns:
    /// - `Ok(GasEstimate)`: Gas cost passed validation
    /// - `Err`: Gas cost exceeds limits (absolute USD or % of trade size)
    pub async fn estimate(
        &self,
        params: &SwapParams,
        swap_params: &ExactInputSingleParams,
        eth_price_usd: f64,
    ) -> Result<GasEstimate> {
        info!("⛽ Estimating gas for swap transaction");

        // Create a temporary signer for gas estimation
        // Note: This doesn't need to be the real wallet - gas estimation is read-only
        // and works with any valid signer address. The estimation simulates the transaction
        // without broadcasting it to the network.
        let temp_wallet = LocalWallet::new(&mut rand::thread_rng());
        let signer = SignerMiddleware::new(
            self.provider.clone(),
            temp_wallet.with_chain_id(self.network.chain_id),
        );

        // Get SwapRouter contract address
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

        // Prepare parameters tuple for gas estimation
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

        // Estimate gas for the transaction
        let gas_estimate = swaprouter_contract
            .method::<_, U256>("exactInputSingle", params_tuple)
            .map_err(|e| Error::Abi(format!("Contract method error: {}", e)))?
            .estimate_gas()
            .await
            .map_err(|e| Error::Network(format!("Gas estimation failed: {}", e)))?;

        info!("📊 Gas estimate: {}", gas_estimate);

        // Calculate gas limit with 20% safety buffer
        // Why: Gas estimates are not 100% accurate - actual gas usage can vary due to:
        // - Changes in contract state between estimate and execution
        // - Price movements affecting swap path calculations
        // - Network congestion affecting execution costs
        // The 20% buffer prevents "out of gas" transaction failures
        let gas_limit = if let Some(configured_gas_limit) = params.gas_limit {
            U256::from(configured_gas_limit)
        } else {
            // Add 20% buffer to gas estimate using safe arithmetic
            let buffer_pct = U256::from(crate::core::constants::GAS_ESTIMATE_BUFFER_PCT);
            let buffer_amount = gas_estimate.saturating_mul(buffer_pct) / U256::from(100);
            gas_estimate.saturating_add(buffer_amount)
        };

        // Fetch current network gas price from Ethereum node
        // This is the base fee per gas unit (in Wei) that miners currently accept
        let base_gas_price_wei = self
            .provider
            .get_gas_price()
            .await
            .map_err(|e| Error::Network(format!("Failed to get current gas price: {}", e)))?;
        let base_gas_price_gwei = base_gas_price_wei.as_u128() as f64 / WEI_PER_GWEI;

        // Apply transaction priority multiplier to increase execution speed
        // Higher priority = higher gas price = faster inclusion in blocks
        // Trade-off: Speed vs. Cost
        let priority_multiplier = get_priority_multiplier(params.priority);

        let adjusted_gas_price_gwei = base_gas_price_gwei * priority_multiplier;
        let gas_price_wei = U256::from((adjusted_gas_price_gwei * WEI_PER_GWEI) as u64);

        info!(
            "⛽ Gas price: {:.2} Gwei (base: {:.2} Gwei, priority: {:?}, multiplier: {:.1}x)",
            adjusted_gas_price_gwei, base_gas_price_gwei, params.priority, priority_multiplier
        );

        // Calculate estimated gas cost
        let estimated_gas_cost_wei = gas_limit * gas_price_wei;
        let estimated_cost_eth = estimated_gas_cost_wei.as_u128() as f64 / 1e18;

        info!(
            "💰 Estimated gas cost: {:.6} ETH ({} Wei)",
            estimated_cost_eth, estimated_gas_cost_wei
        );

        let estimated_cost_usd = estimated_cost_eth * eth_price_usd;

        info!(
            "💵 Estimated gas cost: ${:.2} USD (ETH @ ${:.2})",
            estimated_cost_usd, eth_price_usd
        );

        // Gas cost protection checks
        self.validate_cost(estimated_cost_usd, params.trade_size_usd)?;

        Ok(GasEstimate {
            gas_limit,
            gas_price_wei,
            gas_price_gwei: adjusted_gas_price_gwei,
            estimated_cost_eth,
            estimated_cost_usd,
        })
    }

    /// Estimate gas for a WETH wrap transaction
    ///
    /// ## Purpose:
    /// Calculates the expected gas cost for wrapping ETH to WETH (deposit operation).
    /// Used to validate wrap costs before executing auto-wraps for trading.
    ///
    /// ## Parameters:
    /// - `amount_eth`: Amount of ETH to wrap
    /// - `priority`: Transaction priority level
    /// - `eth_price_usd`: Current ETH/USD price for cost conversion
    /// - `trade_size_usd`: Optional trade size for percentage validation
    ///
    /// ## Returns:
    /// - `Ok(GasEstimate)`: Gas cost passed validation
    /// - `Err`: Gas cost exceeds limits
    pub async fn estimate_wrap(
        &self,
        amount_eth: f64,
        priority: TransactionPriority,
        eth_price_usd: f64,
        trade_size_usd: Option<f64>,
    ) -> Result<GasEstimate> {
        use super::abi::load_weth_abi;

        info!("⛽ Estimating gas for WETH wrap ({:.4} ETH)", amount_eth);

        // Create a temporary signer for gas estimation
        let temp_wallet = LocalWallet::new(&mut rand::thread_rng());
        let signer = SignerMiddleware::new(
            self.provider.clone(),
            temp_wallet.with_chain_id(self.network.chain_id),
        );

        // Get WETH contract address
        let weth_address = self.network.weth_address;

        // Load WETH ABI
        let weth_abi = load_weth_abi()?;

        // Create WETH contract instance
        let weth_contract = Contract::new(weth_address, weth_abi, Arc::new(signer));

        // Convert ETH amount to wei
        let amount_wei = U256::from((amount_eth * 1e18) as u64);

        // Estimate gas for deposit() call
        let gas_estimate = weth_contract
            .method::<_, ()>("deposit", ())
            .map_err(|e| Error::Abi(format!("Failed to create deposit method: {}", e)))?
            .value(amount_wei)
            .estimate_gas()
            .await
            .map_err(|e| Error::Network(format!("Gas estimation for wrap failed: {}", e)))?;

        info!("📊 Wrap gas estimate: {}", gas_estimate);

        // Add 20% safety buffer
        let buffer_pct = U256::from(crate::core::constants::GAS_ESTIMATE_BUFFER_PCT);
        let buffer_amount = gas_estimate.saturating_mul(buffer_pct) / U256::from(100);
        let gas_limit = gas_estimate.saturating_add(buffer_amount);

        // Fetch current network gas price
        let base_gas_price_wei = self
            .provider
            .get_gas_price()
            .await
            .map_err(|e| Error::Network(format!("Failed to get current gas price: {}", e)))?;
        let base_gas_price_gwei = base_gas_price_wei.as_u128() as f64 / WEI_PER_GWEI;

        // Apply transaction priority multiplier
        let priority_multiplier = get_priority_multiplier(priority);
        let adjusted_gas_price_gwei = base_gas_price_gwei * priority_multiplier;
        let gas_price_wei = U256::from((adjusted_gas_price_gwei * WEI_PER_GWEI) as u64);

        info!(
            "⛽ Gas price: {:.2} Gwei (base: {:.2} Gwei, priority: {:?}, multiplier: {:.1}x)",
            adjusted_gas_price_gwei, base_gas_price_gwei, priority, priority_multiplier
        );

        // Calculate estimated gas cost
        let estimated_gas_cost_wei = gas_limit * gas_price_wei;
        let estimated_cost_eth = estimated_gas_cost_wei.as_u128() as f64 / 1e18;
        let estimated_cost_usd = estimated_cost_eth * eth_price_usd;

        info!(
            "💰 Estimated wrap gas cost: {:.6} ETH (${:.2} USD)",
            estimated_cost_eth, estimated_cost_usd
        );

        // Validate gas cost
        self.validate_cost(estimated_cost_usd, trade_size_usd)?;

        Ok(GasEstimate {
            gas_limit,
            gas_price_wei,
            gas_price_gwei: adjusted_gas_price_gwei,
            estimated_cost_eth,
            estimated_cost_usd,
        })
    }

    /// Validate gas cost protection limits based on trading configuration
    /// Note: Minimum trade size is already validated in execute_swap()
    pub fn validate_cost(
        &self,
        estimated_gas_cost_usd: f64,
        trade_size_usd: Option<f64>,
    ) -> Result<()> {
        // Use actual configuration values from trading config
        let max_gas_cost_usd = self.trading_config.max_gas_cost_usd;
        let max_gas_cost_percentage = self.trading_config.max_gas_cost_percentage;

        info!(
            "🔍 Gas protection limits: max_cost=${:.2}, max_percentage={:.1}%",
            max_gas_cost_usd, max_gas_cost_percentage
        );

        // Check maximum gas cost in USD
        if estimated_gas_cost_usd > max_gas_cost_usd {
            return Err(Error::Trading(format!(
                "Estimated gas cost ${:.2} exceeds maximum allowed ${:.2}",
                estimated_gas_cost_usd, max_gas_cost_usd
            )));
        }

        // Check gas cost as percentage of trade size (only if trade size is known)
        if let Some(trade_size) = trade_size_usd {
            if trade_size <= 0.0 {
                return Err(Error::Trading(format!(
                    "Invalid trade size: ${:.2}. Trade size must be positive.",
                    trade_size
                )));
            }

            let gas_percentage = (estimated_gas_cost_usd / trade_size) * 100.0;
            if gas_percentage > max_gas_cost_percentage {
                return Err(Error::Trading(format!(
                    "Gas cost {:.1}% of trade size exceeds maximum allowed {:.1}%",
                    gas_percentage, max_gas_cost_percentage
                )));
            }

            info!(
                "✅ Gas cost protection checks passed: ${:.2} ({:.1}% of trade)",
                estimated_gas_cost_usd, gas_percentage
            );
        } else {
            info!(
                "✅ Gas cost protection check passed: ${:.2} (trade size unknown, percentage check skipped)",
                estimated_gas_cost_usd
            );
        }

        Ok(())
    }
}
