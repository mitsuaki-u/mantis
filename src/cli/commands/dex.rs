use clap::Subcommand;
use crate::core::error::Error;
use crate::infra::api::{get_dex_pair, get_dex_stats};
use crate::cli::display::dex::{display_dex_pairs, display_dex_stats};
use log::error;

#[derive(Subcommand)]
pub enum DexCommands {
    /// Get DEX pair information
    Pair {
        /// Token address
        address: String,
        /// Chain (e.g., solana, ethereum)
        chain: String,
    },
    /// Get DEX statistics
    Stats {
        /// DEX name (e.g., raydium)
        dex: String,
        /// Chain (e.g., solana, ethereum)
        chain: String,
    },
}

pub async fn handle_dex_command(command: DexCommands) -> Result<(), Error> {
    match command {
        DexCommands::Pair { address, chain } => {
            match get_dex_pair(&address, &chain).await {
                Ok(pairs) => display_dex_pairs(&pairs),
                Err(e) => error!("Failed to fetch DEX pair: {}", e),
            }
        },
        DexCommands::Stats { dex, chain } => {
            match get_dex_stats(&dex, &chain).await {
                Ok(stats) => display_dex_stats(&stats, &dex),
                Err(e) => error!("Failed to fetch DEX stats: {}", e),
            }
        },
    }
    Ok(())
} 