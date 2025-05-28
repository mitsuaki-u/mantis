use crate::core::config::Config;
use crate::core::error::Error;
use crate::domain::dex::DexClient;
use log::{error, info, warn};
use std::sync::Arc;

/// Create a DEX client based on configuration
pub async fn create_dex_client(
    config: &Config,
    message_bus: Arc<super::super::MessageBus>,
) -> Result<DexClient, Error> {
    if config.dex.is_testnet() {
        info!(
            "🧪 Creating Ethereum DEX client for network: {:?}",
            config.dex.network
        );

        // Create testnet client
        let mut client = DexClient::new_ethereum(config, message_bus)?;

        // Try to connect wallet if available
        if let Some(wallet_config) = &config.dex.wallet {
            let private_key = if let Some(env_var) = &wallet_config.private_key_env {
                std::env::var(env_var).map_err(|_| {
                    Error::Config(format!(
                        "Cannot load private key from environment variable: {}",
                        env_var
                    ))
                })?
            } else if let Some(file_path) = &wallet_config.private_key_file {
                std::fs::read_to_string(file_path)
                    .map_err(|e| {
                        Error::Config(format!(
                            "Cannot read private key file: {} - {}",
                            file_path, e
                        ))
                    })?
                    .trim()
                    .to_string()
            } else {
                return Err(Error::Config(
                    "No private key configuration found for Ethereum trading".to_string(),
                ));
            };

            // Connect wallet
            client.connect_wallet(&private_key).await?;
            info!("🔑 Successfully connected wallet for Ethereum trading");
        } else {
            warn!("⚠️ No wallet configuration found - Ethereum trading will not work without a wallet");
        }

        Ok(client)
    } else if config.trading.paper_trading {
        info!("📝 Creating paper trading DEX client");
        DexClient::new_paper_trading(config)
    } else {
        info!("🔴 Creating live Ethereum DEX client");
        Ok(DexClient::new_ethereum(config, message_bus)?)
    }
}

/// Fetch and log the initial native wallet balance
pub async fn fetch_and_log_initial_balance(dex_client: &DexClient) {
    info!("Attempting to fetch initial native wallet balance...");
    match dex_client.get_native_balance().await {
        Ok(balance) => {
            info!("💰 Initial native wallet balance: {:.6}", balance);
        }
        Err(e) => {
            error!("Failed to fetch initial native wallet balance: {:?}", e);
        }
    }
}
