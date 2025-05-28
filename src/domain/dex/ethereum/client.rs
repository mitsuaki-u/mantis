use std::str::FromStr;
use std::sync::Arc;

use ethers::signers::{LocalWallet, Signer};
use ethers::types::{Address, U256};
use ethers::utils::parse_units;
use log::info;

use crate::config::Config;
use crate::core::error::{Error, Result};
use crate::domain::dex::{
    NetworkFeeInfo, TransactionDetails, TransactionPriority, TransactionStatus,
};
use crate::infra::actors::MessageBus;

use super::network::NetworkConfig;
use super::price::PriceOracle;
use super::protocols::{DexProtocol, ProtocolUtils, SwapParams, UniswapV2Protocol};
use super::transaction::TransactionManager;

pub struct EthereumDexClient {
    network: NetworkConfig,
    wallet: Option<LocalWallet>,
    price_oracle: PriceOracle,
    transaction_manager: TransactionManager,
    protocol: Box<dyn DexProtocol>,
    message_bus: Arc<MessageBus>,
}

impl EthereumDexClient {
    pub fn new(config: &Config, message_bus: Arc<MessageBus>) -> Result<Self> {
        let network = NetworkConfig::from_config(config)?;
        let provider = network.create_provider()?;

        let price_oracle = PriceOracle::new(provider.clone(), network.clone());
        let transaction_manager = TransactionManager::new(provider.clone(), network.clone());

        // Initialize with Uniswap V2 protocol by default
        let protocol: Box<dyn DexProtocol> = Box::new(UniswapV2Protocol::new(
            provider,
            None, // Wallet will be set later
            network.clone(),
            message_bus.clone(),
        ));

        Ok(Self {
            network,
            wallet: None,
            price_oracle,
            transaction_manager,
            protocol,
            message_bus,
        })
    }

    pub async fn connect_wallet(&mut self, private_key_hex: &str) -> Result<()> {
        let private_key_clean = if private_key_hex.starts_with("0x") {
            &private_key_hex[2..]
        } else {
            private_key_hex
        };

        let wallet = private_key_clean
            .parse::<LocalWallet>()
            .map_err(|e| Error::Wallet(format!("Invalid private key: {}", e)))?
            .with_chain_id(self.network.chain_id);

        info!("Wallet connected: {:?}", wallet.address());
        self.wallet = Some(wallet.clone());

        // Update the protocol with the new wallet
        let provider = self.network.create_provider()?;
        self.protocol = Box::new(UniswapV2Protocol::new(
            provider,
            Some(wallet),
            self.network.clone(),
            self.message_bus.clone(),
        ));

        Ok(())
    }

    pub async fn get_native_balance(&self) -> Result<f64> {
        if let Some(wallet) = &self.wallet {
            self.price_oracle.get_native_balance(wallet.address()).await
        } else {
            Err(Error::Wallet("No wallet connected".to_string()))
        }
    }

    pub async fn get_token_balance(&self, token_address_str: &str) -> Result<f64> {
        if let Some(wallet) = &self.wallet {
            self.price_oracle
                .get_token_balance(token_address_str, wallet.address())
                .await
        } else {
            Err(Error::Wallet("No wallet connected".to_string()))
        }
    }

    pub async fn get_token_price_usd(&self, token_address: &str) -> Result<f64> {
        self.price_oracle.get_token_price_usd(token_address).await
    }

    pub async fn execute_swap(
        &self,
        from_token_address_str: &str,
        to_token_address_str: &str,
        amount_in_f64: f64,
        slippage_tolerance: f64,
        price_limit: Option<f64>,
        priority: TransactionPriority,
    ) -> Result<TransactionDetails> {
        if self.wallet.is_none() {
            return Err(Error::Wallet("No wallet connected".to_string()));
        }

        let wallet = self.wallet.as_ref().unwrap();

        // Parse token addresses
        let from_token = Address::from_str(from_token_address_str)
            .map_err(|e| Error::Conversion(format!("Invalid from token address: {}", e)))?;
        let to_token = Address::from_str(to_token_address_str)
            .map_err(|e| Error::Conversion(format!("Invalid to token address: {}", e)))?;

        // Convert amount to U256
        let amount_in = float_to_u256(amount_in_f64, 18)?;

        // Get quote for the swap
        let amount_out = self
            .protocol
            .get_quote(from_token, to_token, amount_in)
            .await?;

        // Check price limit if provided
        if let Some(limit) = price_limit {
            let current_price = amount_out.as_u128() as f64 / amount_in.as_u128() as f64;
            if current_price < limit {
                return Err(Error::Validation(format!(
                    "Current price {} is below limit {}",
                    current_price, limit
                )));
            }
        }

        // Calculate minimum amount out with slippage
        let amount_out_min =
            ProtocolUtils::calculate_amount_out_min(amount_out, slippage_tolerance);

        // Create swap parameters
        let swap_params = SwapParams {
            token_in: from_token,
            token_out: to_token,
            amount_in,
            amount_out_min,
            to: wallet.address(),
            deadline: ProtocolUtils::calculate_deadline(),
            slippage_tolerance,
            priority,
        };

        // Execute the swap
        self.protocol.execute_swap(swap_params).await
    }

    pub async fn get_transaction_status(&self, tx_id_str: &str) -> Result<TransactionStatus> {
        self.transaction_manager
            .get_transaction_status(tx_id_str)
            .await
    }

    pub async fn get_network_fees(&self) -> Result<NetworkFeeInfo> {
        self.transaction_manager.get_network_fees().await
    }

    // Utility methods for protocol information
    pub fn get_protocol_name(&self) -> &'static str {
        self.protocol.get_protocol_name()
    }

    pub fn get_router_address(&self) -> Address {
        self.protocol.get_router_address()
    }

    pub fn get_factory_address(&self) -> Address {
        self.protocol.get_factory_address()
    }

    pub fn get_network_info(&self) -> &NetworkConfig {
        &self.network
    }

    // Method to switch protocols (for future extensibility)
    pub fn switch_protocol(&mut self, protocol_name: &str) -> Result<()> {
        let provider = self.network.create_provider()?;

        match protocol_name.to_lowercase().as_str() {
            "uniswap_v2" | "uniswapv2" => {
                self.protocol = Box::new(UniswapV2Protocol::new(
                    provider,
                    self.wallet.clone(),
                    self.network.clone(),
                    self.message_bus.clone(),
                ));
                Ok(())
            }
            // Future protocols can be added here
            // "sushiswap" => { ... }
            // "uniswap_v3" => { ... }
            _ => Err(Error::Config(format!(
                "Unsupported protocol: {}",
                protocol_name
            ))),
        }
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
        let provider = self
            .network
            .create_provider()
            .expect("Failed to create provider");

        // Create a new protocol instance since Box<dyn DexProtocol> can't be cloned
        let protocol: Box<dyn DexProtocol> = Box::new(UniswapV2Protocol::new(
            provider,
            self.wallet.clone(),
            self.network.clone(),
            self.message_bus.clone(),
        ));

        Self {
            network: self.network.clone(),
            wallet: self.wallet.clone(),
            price_oracle: self.price_oracle.clone(),
            transaction_manager: self.transaction_manager.clone(),
            protocol,
            message_bus: self.message_bus.clone(),
        }
    }
}
