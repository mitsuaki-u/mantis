use super::ethereum::config::addresses::get_common_tokens;
use super::ethereum::EthereumDexClient;
use super::ethereum::{TransactionDetails, TransactionPriority, TransactionStatus};
use crate::application::constants::DEFAULT_SIMULATED_ETH_BALANCE;
use crate::config::Config;
use crate::infrastructure::errors::Error;
use crate::EventRouter;
use log::{error, info, warn};
use std::str::FromStr;
use std::sync::Arc;

/// Main DEX client enum supporting both live and paper trading
#[derive(Clone)]
pub enum DexClient {
    Paper {
        ethereum_client: Box<EthereumDexClient>,
        simulated_eth_balance: f64,
        simulated_weth_balance: f64,
    },
    Live {
        ethereum_client: Box<EthereumDexClient>,
    },
}

impl DexClient {
    /// Get a reference to the underlying ethereum client
    fn ethereum_client(&self) -> &EthereumDexClient {
        match self {
            DexClient::Live { ethereum_client }
            | DexClient::Paper {
                ethereum_client, ..
            } => ethereum_client,
        }
    }

    // Simple delegation methods - the complex logic stays in EthereumDexClient
    pub async fn connect_wallet(
        &mut self,
        private_key_hex: &str,
    ) -> crate::infrastructure::errors::Result<()> {
        match self {
            DexClient::Live { ethereum_client }
            | DexClient::Paper {
                ethereum_client, ..
            } => ethereum_client.connect_wallet(private_key_hex).await,
        }
    }

    pub async fn get_native_balance(&self) -> crate::infrastructure::errors::Result<f64> {
        match self {
            DexClient::Paper {
                simulated_eth_balance,
                ..
            } => Ok(*simulated_eth_balance),
            DexClient::Live { ethereum_client } => ethereum_client.get_native_balance().await,
        }
    }

    /// Update the pool cache with discovered pools from market data events
    pub async fn update_pool_cache(
        &self,
        pools: Vec<crate::application::events::PoolDiscoveryData>,
        source: &str,
    ) {
        self.ethereum_client()
            .update_pool_cache(pools, source)
            .await;
    }

    pub async fn get_wallet_token_balance(
        &self,
        token_address: &str,
    ) -> crate::infrastructure::errors::Result<f64> {
        match self {
            DexClient::Paper {
                simulated_weth_balance,
                ethereum_client,
                ..
            } => {
                // Paper trading: only return balance for WETH (starting capital)
                // Actual token positions are tracked in the position manager
                let weth_address = ethereum_client.resolve_token_address("WETH").await?;

                if token_address.to_lowercase() == weth_address.to_lowercase() {
                    Ok(*simulated_weth_balance)
                } else {
                    // Return 0 for non-WETH tokens - positions are tracked separately
                    Ok(0.0)
                }
            }
            DexClient::Live { ethereum_client } => {
                ethereum_client.get_token_balance(token_address).await
            }
        }
    }

    pub async fn get_token_price_usd(
        &self,
        token_address: &str,
    ) -> crate::infrastructure::errors::Result<f64> {
        use rust_decimal::prelude::ToPrimitive;
        let decimal_price = self
            .ethereum_client()
            .get_token_price_usd(token_address)
            .await?;
        decimal_price.to_f64().ok_or_else(|| {
            crate::infrastructure::errors::Error::Parse(
                "Failed to convert price to f64".to_string(),
            )
        })
    }

    /// Ensure sufficient WETH balance (auto-wrap ETH if needed) for live trading
    /// Validates wrap gas costs against trading_config limits via GasEstimator
    /// For paper trading, this is a no-op
    pub async fn ensure_weth_balance(
        &self,
        required_weth: f64,
        trade_size_usd: f64,
        priority: TransactionPriority,
    ) -> crate::infrastructure::errors::Result<()> {
        match self {
            DexClient::Live { ethereum_client } => {
                ethereum_client
                    .ensure_weth_balance(required_weth, trade_size_usd, priority)
                    .await
            }
            DexClient::Paper { .. } => {
                // Paper trading doesn't need wrapping
                Ok(())
            }
        }
    }

    pub async fn execute_swap(
        &self,
        params: crate::infrastructure::dex::SwapParams<'_>,
    ) -> crate::infrastructure::errors::Result<TransactionDetails> {
        self.ethereum_client().execute_swap(params).await
    }

    pub async fn get_transaction_status(
        &self,
        tx_hash: &str,
    ) -> crate::infrastructure::errors::Result<(TransactionStatus, Option<TransactionDetails>)>
    {
        self.ethereum_client().get_transaction_status(tx_hash).await
    }

    pub async fn resolve_token_address(
        &self,
        token_id: &str,
    ) -> crate::infrastructure::errors::Result<String> {
        self.ethereum_client().resolve_token_address(token_id).await
    }

    // Constructor methods (moved from factory.rs)
    pub fn new_paper_trading(config: &Config) -> crate::infrastructure::errors::Result<Self> {
        let network = config
            .dex
            .network
            .as_ref()
            .unwrap_or(&"mainnet".to_string())
            .clone();

        info!("Creating paper trading DEX client for network: {}", network);

        let dex_client = DexClient::Paper {
            ethereum_client: Box::new(EthereumDexClient::new_paper_trading(config)?),
            simulated_eth_balance: DEFAULT_SIMULATED_ETH_BALANCE,
            simulated_weth_balance: config.dex.paper_simulated_weth_balance,
        };

        // Log paper trading information
        dex_client.log_paper_trading_info();

        Ok(dex_client)
    }

    pub async fn new_live(
        config: &Config,
        event_router: Arc<EventRouter>,
    ) -> crate::infrastructure::errors::Result<Self> {
        let client = EthereumDexClient::new(config, event_router).await?;
        let mut dex_client = DexClient::Live {
            ethereum_client: Box::new(client),
        };

        // Connect wallet if configured
        if let Some(wallet_config) = &config.dex.wallet {
            let private_key = Self::load_private_key(wallet_config).await?;
            dex_client.connect_wallet(&private_key).await?;
            info!("🔑 Successfully connected wallet for live Ethereum trading");
        } else {
            warn!("⚠️ No wallet configuration found - Live Ethereum trading will not work without a wallet");
        }

        // Log wallet information
        dex_client.log_wallet_info(config).await;

        Ok(dex_client)
    }

    /// Estimate gas fee for a swap in ETH
    pub async fn estimate_swap_gas_fee(
        &self,
        from_token: &str,
        to_token: &str,
        amount: f64,
    ) -> crate::infrastructure::errors::Result<f64> {
        match self {
            DexClient::Live { ethereum_client } => {
                // Resolve token addresses
                let from_addr = ethereum_client.resolve_token_address(from_token).await?;
                let to_addr = ethereum_client.resolve_token_address(to_token).await?;

                // Parse addresses
                let from_address = ethers::types::Address::from_str(&from_addr).map_err(|e| {
                    crate::infrastructure::errors::Error::Conversion(format!(
                        "Invalid from address: {}",
                        e
                    ))
                })?;
                let to_address = ethers::types::Address::from_str(&to_addr).map_err(|e| {
                    crate::infrastructure::errors::Error::Conversion(format!(
                        "Invalid to address: {}",
                        e
                    ))
                })?;

                ethereum_client
                    .estimate_swap_gas_fee(from_address, to_address, amount)
                    .await
            }
            DexClient::Paper {
                ethereum_client, ..
            } => {
                // Paper trading uses same gas estimates as live
                let from_addr = ethereum_client.resolve_token_address(from_token).await?;
                let to_addr = ethereum_client.resolve_token_address(to_token).await?;

                let from_address = ethers::types::Address::from_str(&from_addr).map_err(|e| {
                    crate::infrastructure::errors::Error::Conversion(format!(
                        "Invalid from address: {}",
                        e
                    ))
                })?;
                let to_address = ethers::types::Address::from_str(&to_addr).map_err(|e| {
                    crate::infrastructure::errors::Error::Conversion(format!(
                        "Invalid to address: {}",
                        e
                    ))
                })?;

                ethereum_client
                    .estimate_swap_gas_fee(from_address, to_address, amount)
                    .await
            }
        }
    }

    /// Get current ETH price in USD (uses real on-chain price for both live and paper trading)
    pub async fn get_eth_price_usd(&self) -> crate::infrastructure::errors::Result<f64> {
        self.ethereum_client().get_eth_price_usd().await
    }

    /// Estimate the output amount and fee for a swap
    /// Uses the same pool selection logic as live trading
    /// Returns (expected_output_amount, fee_percentage)
    pub async fn estimate_swap_output(
        &self,
        from_token: &str,
        to_token: &str,
        amount_in_usd: f64,
    ) -> crate::infrastructure::errors::Result<(f64, f64)> {
        // Resolve token addresses
        let from_addr = self
            .ethereum_client()
            .resolve_token_address(from_token)
            .await?;
        let to_addr = self
            .ethereum_client()
            .resolve_token_address(to_token)
            .await?;

        // Parse to Address types
        let from_address = ethers::types::Address::from_str(&from_addr).map_err(|e| {
            crate::infrastructure::errors::Error::Conversion(format!("Invalid from address: {}", e))
        })?;
        let to_address = ethers::types::Address::from_str(&to_addr).map_err(|e| {
            crate::infrastructure::errors::Error::Conversion(format!("Invalid to address: {}", e))
        })?;

        // CRITICAL FIX: Convert USD amount to WETH amount using current ETH price
        // Get current ETH price in USD
        let eth_price_usd = self.get_eth_price_usd().await?;

        // Convert USD to WETH amount (e.g., $200 / $3000 = 0.0667 WETH)
        let weth_amount = amount_in_usd / eth_price_usd;

        // Get available WETH balance and validate we have enough
        let weth_balance = match self {
            DexClient::Paper {
                simulated_weth_balance,
                ..
            } => *simulated_weth_balance,
            DexClient::Live { .. } => {
                // For live trading, get actual WETH balance
                // Note: This assumes from_token is WETH - should be improved for other base tokens
                let weth_addr = self.ethereum_client().resolve_token_address("WETH").await?;
                self.get_wallet_token_balance(&weth_addr).await?
            }
        };

        // Safety check: Ensure we don't try to trade more than we have
        if weth_amount > weth_balance {
            return Err(crate::infrastructure::errors::Error::InvalidInput(format!(
                "Insufficient balance: Need {:.8} WETH but only have {:.8} WETH (${:.2} USD at ${:.2}/ETH)",
                weth_amount, weth_balance, amount_in_usd, eth_price_usd
            )));
        }

        info!(
            "💰 Converting position size: ${:.2} USD / ${:.2} per ETH = {:.8} WETH (balance: {:.8} WETH)",
            amount_in_usd, eth_price_usd, weth_amount, weth_balance
        );

        // Parse WETH amount to wei units (18 decimals)
        let amount_in = ethers::utils::parse_units(weth_amount.to_string(), "ether")
            .map_err(|e| {
                crate::infrastructure::errors::Error::Parse(format!(
                    "Failed to parse WETH amount: {}",
                    e
                ))
            })?
            .into();

        let (amount_out, fee_pct) = self
            .ethereum_client()
            .estimate_swap_output(from_address, to_address, amount_in)
            .await?;

        // Convert amount_out back to f64
        let amount_out_f64 = ethers::utils::format_units(amount_out, "ether")
            .map_err(|e| {
                crate::infrastructure::errors::Error::Parse(format!(
                    "Failed to format output: {}",
                    e
                ))
            })?
            .parse::<f64>()
            .map_err(|e| {
                crate::infrastructure::errors::Error::Parse(format!(
                    "Failed to parse output: {}",
                    e
                ))
            })?;

        Ok((amount_out_f64, fee_pct))
    }

    // Private helper methods

    /// Load private key from wallet configuration for live trading
    async fn load_private_key(
        wallet_config: &crate::config::WalletConfig,
    ) -> crate::infrastructure::errors::Result<String> {
        if let Some(env_var) = &wallet_config.private_key_env {
            std::env::var(env_var).map_err(|_| {
                Error::Config(format!(
                    "Cannot load private key from environment variable: {}",
                    env_var
                ))
            })
        } else if let Some(file_path) = &wallet_config.private_key_file {
            Ok(std::fs::read_to_string(file_path)
                .map_err(|e| {
                    Error::Config(format!(
                        "Cannot read private key file: {} - {}",
                        file_path, e
                    ))
                })?
                .trim()
                .to_string())
        } else {
            Err(Error::Config(
                "No private key configuration found for live Ethereum trading".to_string(),
            ))
        }
    }

    /// Log comprehensive wallet information for live trading
    pub async fn log_wallet_info(&self, config: &Config) {
        info!("=== Wallet Information ===");

        // Network information
        let network_name = config.dex.network.as_deref().unwrap_or("ethereum");
        info!("Network: {}", network_name);

        // Get wallet address if available
        if let DexClient::Live { ethereum_client } = self {
            if let Some(wallet_address) = ethereum_client.get_wallet_address() {
                info!("Wallet Address: {:?}", wallet_address);
            }
            if let Some(chain_id) = ethereum_client.get_chain_id() {
                info!("Chain ID: {}", chain_id);
            }

            // Network configuration details
            let network_info = ethereum_client.get_network_info();
            info!("RPC URL: {}", network_info.rpc_url);
            info!("Router Address: {:?}", ethereum_client.get_router_address());
            info!(
                "Factory Address: {:?}",
                ethereum_client.get_factory_address()
            );
            info!("Protocol: {}", ethereum_client.get_protocol_name());
        }

        // Fetch and log native balance
        match self.get_native_balance().await {
            Ok(balance) => {
                let currency = if network_name.to_lowercase().contains("polygon")
                    || network_name.to_lowercase().contains("mumbai")
                {
                    "MATIC"
                } else {
                    "ETH"
                };
                info!("Native Balance: {:.6} {}", balance, currency);

                // Warn if balance is low
                let min_required = config.trading.min_eth_balance;
                if balance < min_required {
                    warn!("LOW BALANCE WARNING: Current balance ({:.6} {}) is below minimum required ({:.6} {}) for trading!",
                          balance, currency, min_required, currency);
                } else {
                    info!(
                        "Balance sufficient for trading (min required: {:.6} {})",
                        min_required, currency
                    );
                }
            }
            Err(e) => {
                error!("Failed to fetch native balance: {:?}", e);
            }
        }

        // Fetch network fees
        if let DexClient::Live { ethereum_client } = self {
            match ethereum_client.get_network_fees().await {
                Ok(fees) => {
                    if let Some(gas_price) = fees.gas_price_gwei {
                        info!(
                            "Network Fees - Gas Price: {:.2} Gwei, Estimated Cost: ${:.4} {}",
                            gas_price, fees.estimated_fee_usd, fees.fee_currency_symbol
                        );
                    } else {
                        info!(
                            "Network Fees - Estimated Cost: ${:.4} {}",
                            fees.estimated_fee_usd, fees.fee_currency_symbol
                        );
                    }
                }
                Err(e) => {
                    warn!("Could not fetch current network fees: {:?}", e);
                }
            }
        }

        // Check balances for common trading tokens
        self.log_token_balances(network_name).await;

        info!("=== End Wallet Information ===");
    }

    /// Log common token balances
    async fn log_token_balances(&self, network_name: &str) {
        match get_common_tokens(network_name) {
            Ok(tokens) => {
                info!("Checking common token balances...");

                for (symbol, address) in tokens {
                    match self.get_wallet_token_balance(&address).await {
                        Ok(balance) => {
                            if balance > 0.0 {
                                info!("   {}: {:.6}", symbol, balance);
                            } else {
                                info!("   {}: 0", symbol);
                            }
                        }
                        Err(_) => {
                            info!("   {}: Unable to fetch", symbol);
                        }
                    }
                }
            }
            Err(_) => {
                info!(
                    "Token balance checking not supported for network: {}",
                    network_name
                );
            }
        }
    }

    /// Log paper trading information
    pub fn log_paper_trading_info(&self) {
        if let DexClient::Paper {
            simulated_eth_balance,
            simulated_weth_balance,
            ..
        } = self
        {
            info!(
                "Paper trading mode: ETH={:.2}, WETH={:.2} (simulated - no real funds)",
                simulated_eth_balance, simulated_weth_balance
            );
        }
    }
}
