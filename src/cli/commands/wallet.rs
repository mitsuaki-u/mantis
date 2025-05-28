use crate::cli::display::wallet::display_wallet_info;
use crate::core::config::Config;
use crate::core::error::Error;
use crate::domain::dex::DexClient;
use crate::infra::actors::MessageBus;
use crate::infra::api::wallet::get_wallet_info;
use clap::Subcommand;
use log::{error, info};
use std::sync::Arc;

#[derive(Subcommand)]
pub enum WalletCommands {
    /// Get wallet information
    Info {
        /// Wallet address
        address: String,
        /// Chain (e.g., ethereum, solana)
        #[arg(short, long, default_value = "ethereum")]
        chain: String,
    },
}

pub async fn handle_wallet_command(
    command: WalletCommands,
    config: Config,
    message_bus: Arc<MessageBus>,
) -> Result<(), Error> {
    match command {
        WalletCommands::Info { address, chain } => {
            let dex_client_result: Result<DexClient, Error> = if config.dex.is_testnet() {
                info!(
                    "Creating Ethereum DEX client for wallet info (network: {:?})",
                    config.dex.network
                );
                let mut client = DexClient::new_ethereum(&config, message_bus)?;
                if let Some(wallet_config) = &config.dex.wallet {
                    let private_key = if let Some(env_var) = &wallet_config.private_key_env {
                        std::env::var(env_var).ok()
                    } else if let Some(file_path) = &wallet_config.private_key_file {
                        std::fs::read_to_string(file_path)
                            .ok()
                            .map(|s| s.trim().to_string())
                    } else {
                        None
                    };
                    if let Some(pk) = private_key {
                        if let Err(e) = client.connect_wallet(&pk).await {
                            error!("Wallet configured but failed to connect for network: {}. Proceeding without wallet.", e);
                        }
                    } else {
                        info!("No private key for wallet, proceeding without wallet connection for info.");
                    }
                }
                Ok(client)
            } else {
                info!("Creating paper trading DEX client for wallet info");
                DexClient::new_paper_trading(&config)
            };

            match dex_client_result {
                Ok(dex_client) => match get_wallet_info(&address, &chain, &dex_client).await {
                    Ok(info) => display_wallet_info(&info, &address),
                    Err(e) => error!("Failed to fetch wallet info: {}", e),
                },
                Err(e) => {
                    error!("Failed to create DexClient for wallet info: {}", e);
                }
            }
        }
    }
    Ok(())
}
