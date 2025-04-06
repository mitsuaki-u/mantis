use clap::Subcommand;
use crate::error::Error;
use crate::api::wallet::get_wallet_info;
use crate::display::wallet::display_wallet_info;
use log::error;

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

pub async fn handle_wallet_command(command: WalletCommands) -> Result<(), Error> {
    match command {
        WalletCommands::Info { address, chain } => {
            match get_wallet_info(&address, &chain).await {
                Ok(info) => display_wallet_info(&info, &address),
                Err(e) => error!("Failed to fetch wallet info: {}", e),
            }
        }
    }
    Ok(())
} 