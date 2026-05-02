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

/// Main DEX client enum.
///
/// Variants:
/// - `SolanaPaper` — Solana paper trading. No Ethereum deps. Simulates swaps
///   locally with a configurable starting balance.
/// - `Paper` — Ethereum paper trading. Uses `EthereumDexClient` for gas and
///   price estimates but does not execute real transactions.
/// - `Live` — Ethereum live trading. Full on-chain execution via Uniswap V3.
///   Solana live execution (Jupiter swaps) is planned but not yet implemented.
#[derive(Clone)]
pub enum DexClient {
    /// Solana paper trading — no Ethereum client needed
    SolanaPaper {
        simulated_sol_balance: f64,
        simulated_base_token_balance: f64,
    },
    /// Ethereum paper trading
    Paper {
        ethereum_client: Box<EthereumDexClient>,
        simulated_eth_balance: f64,
        simulated_weth_balance: f64,
    },
    /// Ethereum live trading
    Live {
        ethereum_client: Box<EthereumDexClient>,
    },
}

impl DexClient {
    fn ethereum_client(&self) -> Option<&EthereumDexClient> {
        match self {
            DexClient::Live { ethereum_client }
            | DexClient::Paper {
                ethereum_client, ..
            } => Some(ethereum_client),
            DexClient::SolanaPaper { .. } => None,
        }
    }

    pub async fn connect_wallet(
        &mut self,
        private_key_hex: &str,
    ) -> crate::infrastructure::errors::Result<()> {
        match self {
            DexClient::Live { ethereum_client }
            | DexClient::Paper {
                ethereum_client, ..
            } => ethereum_client.connect_wallet(private_key_hex).await,
            DexClient::SolanaPaper { .. } => Ok(()),
        }
    }

    pub async fn get_native_balance(&self) -> crate::infrastructure::errors::Result<f64> {
        match self {
            DexClient::SolanaPaper {
                simulated_sol_balance,
                ..
            } => Ok(*simulated_sol_balance),
            DexClient::Paper {
                simulated_eth_balance,
                ..
            } => Ok(*simulated_eth_balance),
            DexClient::Live { ethereum_client } => ethereum_client.get_native_balance().await,
        }
    }

    pub async fn update_pool_cache(
        &self,
        pools: Vec<crate::application::events::PoolDiscoveryData>,
        source: &str,
    ) {
        if let Some(client) = self.ethereum_client() {
            client.update_pool_cache(pools, source).await;
        }
    }

    pub async fn get_wallet_token_balance(
        &self,
        token_address: &str,
    ) -> crate::infrastructure::errors::Result<f64> {
        match self {
            DexClient::SolanaPaper {
                simulated_base_token_balance,
                ..
            } => Ok(*simulated_base_token_balance),
            DexClient::Paper {
                simulated_weth_balance,
                ethereum_client,
                ..
            } => {
                let weth_address = ethereum_client.resolve_token_address("WETH").await?;
                if token_address.to_lowercase() == weth_address.to_lowercase() {
                    Ok(*simulated_weth_balance)
                } else {
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
        match self {
            DexClient::SolanaPaper { .. } => {
                // Solana paper trading — price validation is skipped at the strategy level
                // for non-0x addresses, so this should not be called. Return 0 gracefully.
                Ok(0.0)
            }
            DexClient::Paper {
                ethereum_client, ..
            }
            | DexClient::Live { ethereum_client } => {
                use rust_decimal::prelude::ToPrimitive;
                let decimal_price = ethereum_client.get_token_price_usd(token_address).await?;
                decimal_price.to_f64().ok_or_else(|| {
                    crate::infrastructure::errors::Error::Parse(
                        "Failed to convert price to f64".to_string(),
                    )
                })
            }
        }
    }

    pub async fn ensure_weth_balance(
        &self,
        required_weth: f64,
        trade_size_usd: f64,
        priority: TransactionPriority,
    ) -> crate::infrastructure::errors::Result<()> {
        match self {
            DexClient::SolanaPaper { .. } | DexClient::Paper { .. } => Ok(()),
            DexClient::Live { ethereum_client } => {
                ethereum_client
                    .ensure_weth_balance(required_weth, trade_size_usd, priority)
                    .await
            }
        }
    }

    pub async fn execute_swap(
        &self,
        params: crate::infrastructure::dex::SwapParams<'_>,
    ) -> crate::infrastructure::errors::Result<TransactionDetails> {
        match self {
            DexClient::SolanaPaper { .. } => {
                // Solana paper trading — execution is simulated by the execution actor.
                // This path should not be reached during paper trading.
                Err(Error::Config(
                    "execute_swap called on SolanaPaper client — use paper simulation path"
                        .to_string(),
                ))
            }
            DexClient::Paper {
                ethereum_client, ..
            }
            | DexClient::Live { ethereum_client } => ethereum_client.execute_swap(params).await,
        }
    }

    pub async fn get_transaction_status(
        &self,
        tx_hash: &str,
    ) -> crate::infrastructure::errors::Result<(TransactionStatus, Option<TransactionDetails>)>
    {
        match self {
            DexClient::SolanaPaper { .. } => Ok((
                TransactionStatus::Confirmed {
                    tx_id: "paper-simulated".to_string(),
                    details: "Solana paper trade — simulated".to_string(),
                    confirmations: 1,
                    required_confirmations: 1,
                    finality_probability: 1.0,
                },
                None,
            )),
            DexClient::Paper {
                ethereum_client, ..
            }
            | DexClient::Live { ethereum_client } => {
                ethereum_client.get_transaction_status(tx_hash).await
            }
        }
    }

    pub async fn resolve_token_address(
        &self,
        token_id: &str,
    ) -> crate::infrastructure::errors::Result<String> {
        match self {
            DexClient::SolanaPaper { .. } => Ok(token_id.to_string()),
            DexClient::Paper {
                ethereum_client, ..
            }
            | DexClient::Live { ethereum_client } => {
                ethereum_client.resolve_token_address(token_id).await
            }
        }
    }

    pub async fn estimate_swap_gas_fee(
        &self,
        from_token: &str,
        to_token: &str,
        amount: f64,
    ) -> crate::infrastructure::errors::Result<f64> {
        match self {
            DexClient::SolanaPaper { .. } => {
                // Solana fees are ~$0.0005 per transaction
                Ok(0.0005)
            }
            DexClient::Paper {
                ethereum_client, ..
            }
            | DexClient::Live { ethereum_client } => {
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

    pub async fn get_eth_price_usd(&self) -> crate::infrastructure::errors::Result<f64> {
        match self {
            DexClient::SolanaPaper { .. } => {
                // Return SOL price approximation — not used for Solana paper trading
                Ok(150.0)
            }
            DexClient::Paper {
                ethereum_client, ..
            }
            | DexClient::Live { ethereum_client } => ethereum_client.get_eth_price_usd().await,
        }
    }

    pub async fn estimate_swap_output(
        &self,
        from_token: &str,
        to_token: &str,
        amount_in_usd: f64,
    ) -> crate::infrastructure::errors::Result<(f64, f64)> {
        match self {
            DexClient::SolanaPaper {
                simulated_base_token_balance,
                ..
            } => {
                // Simulate swap: assume 0.3% DEX fee, output ≈ input (paper trading)
                let fee_pct = 0.003_f64;
                let output = amount_in_usd * (1.0 - fee_pct);
                let _ = simulated_base_token_balance; // balance checked by caller
                Ok((output, fee_pct))
            }
            DexClient::Paper {
                ethereum_client, ..
            }
            | DexClient::Live { ethereum_client } => {
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
                let eth_price_usd = ethereum_client.get_eth_price_usd().await?;
                let weth_amount = amount_in_usd / eth_price_usd;

                let weth_balance = match self {
                    DexClient::Paper {
                        simulated_weth_balance,
                        ..
                    } => *simulated_weth_balance,
                    DexClient::Live { .. } => {
                        let weth_addr = ethereum_client.resolve_token_address("WETH").await?;
                        ethereum_client.get_token_balance(&weth_addr).await?
                    }
                    DexClient::SolanaPaper { .. } => unreachable!(),
                };

                if weth_amount > weth_balance {
                    return Err(crate::infrastructure::errors::Error::InvalidInput(format!(
                        "Insufficient balance: Need {:.8} WETH but only have {:.8} WETH",
                        weth_amount, weth_balance
                    )));
                }

                let amount_in = ethers::utils::parse_units(weth_amount.to_string(), "ether")
                    .map_err(|e| {
                        crate::infrastructure::errors::Error::Parse(format!(
                            "Failed to parse WETH amount: {}",
                            e
                        ))
                    })?
                    .into();

                let (amount_out, fee_pct) = ethereum_client
                    .estimate_swap_output(from_address, to_address, amount_in)
                    .await?;

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
        }
    }

    // ── Constructors ──────────────────────────────────────────────────────────

    pub fn new_paper_trading(config: &Config) -> crate::infrastructure::errors::Result<Self> {
        let network = config
            .dex
            .network
            .as_deref()
            .unwrap_or("mainnet")
            .to_string();
        info!("Creating paper trading DEX client for network: {}", network);

        if network == "solana" {
            let client = DexClient::SolanaPaper {
                simulated_sol_balance: DEFAULT_SIMULATED_ETH_BALANCE,
                simulated_base_token_balance: config.dex.paper_simulated_weth_balance,
            };
            info!(
                "Paper trading mode: SOL={:.2}, base_token={:.2} (simulated - no real funds)",
                DEFAULT_SIMULATED_ETH_BALANCE, config.dex.paper_simulated_weth_balance,
            );
            return Ok(client);
        }

        let dex_client = DexClient::Paper {
            ethereum_client: Box::new(EthereumDexClient::new_paper_trading(config)?),
            simulated_eth_balance: DEFAULT_SIMULATED_ETH_BALANCE,
            simulated_weth_balance: config.dex.paper_simulated_weth_balance,
        };
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

        if let Some(wallet_config) = &config.dex.wallet {
            let private_key = Self::load_private_key(wallet_config).await?;
            dex_client.connect_wallet(&private_key).await?;
            info!("🔑 Successfully connected wallet for live trading");
        } else {
            warn!("⚠️ No wallet configuration found - live trading will not work without a wallet");
        }

        dex_client.log_wallet_info(config).await;
        Ok(dex_client)
    }

    // ── Private helpers ───────────────────────────────────────────────────────

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
                "No private key configuration found".to_string(),
            ))
        }
    }

    pub async fn log_wallet_info(&self, config: &Config) {
        info!("=== Wallet Information ===");
        let network_name = config.dex.network.as_deref().unwrap_or("ethereum");
        info!("Network: {}", network_name);

        if let Some(client) = self.ethereum_client() {
            if let Some(wallet_address) = client.get_wallet_address() {
                info!("Wallet Address: {:?}", wallet_address);
            }
            if let Some(chain_id) = client.get_chain_id() {
                info!("Chain ID: {}", chain_id);
            }
            let network_info = client.get_network_info();
            info!("RPC URL: {}", network_info.rpc_url);
            info!("Router Address: {:?}", client.get_router_address());
            info!("Factory Address: {:?}", client.get_factory_address());
            info!("Protocol: {}", client.get_protocol_name());
        }

        match self.get_native_balance().await {
            Ok(balance) => {
                let currency = config.dex.base_token_symbol();
                info!("Native Balance: {:.6} {}", balance, currency);
                let min_required = config.trading.min_native_balance;
                if balance < min_required {
                    warn!(
                        "LOW BALANCE: {:.6} {} below minimum {:.6}",
                        balance, currency, min_required
                    );
                }
            }
            Err(e) => error!("Failed to fetch native balance: {:?}", e),
        }

        if let Some(client) = self.ethereum_client() {
            match client.get_network_fees().await {
                Ok(fees) => {
                    if let Some(gas_price) = fees.gas_price_gwei {
                        info!(
                            "Network Fees - Gas Price: {:.2} Gwei, Estimated Cost: ${:.4} {}",
                            gas_price, fees.estimated_fee_usd, fees.fee_currency_symbol
                        );
                    }
                }
                Err(e) => warn!("Could not fetch current network fees: {:?}", e),
            }
            self.log_token_balances(network_name).await;
        }

        info!("=== End Wallet Information ===");
    }

    async fn log_token_balances(&self, network_name: &str) {
        match get_common_tokens(network_name) {
            Ok(tokens) => {
                info!("Checking common token balances...");
                for (symbol, address) in tokens {
                    match self.get_wallet_token_balance(&address).await {
                        Ok(balance) => info!("   {}: {:.6}", symbol, balance),
                        Err(_) => info!("   {}: Unable to fetch", symbol),
                    }
                }
            }
            Err(_) => info!(
                "Token balance checking not supported for network: {}",
                network_name
            ),
        }
    }

    pub fn log_paper_trading_info(&self) {
        match self {
            DexClient::SolanaPaper {
                simulated_sol_balance,
                simulated_base_token_balance,
            } => {
                info!(
                    "Paper trading mode: SOL={:.2}, base_token={:.2} (simulated - no real funds)",
                    simulated_sol_balance, simulated_base_token_balance
                );
            }
            DexClient::Paper {
                simulated_eth_balance,
                simulated_weth_balance,
                ..
            } => {
                info!(
                    "Paper trading mode: native={:.2}, base_token={:.2} (simulated - no real funds)",
                    simulated_eth_balance, simulated_weth_balance
                );
            }
            DexClient::Live { .. } => {}
        }
    }
}
