use clap::Subcommand;

pub mod config;
pub mod dex;
pub mod trading;
pub mod wallet;

pub use config::ConfigCommands;
pub use dex::DexCommands;
pub use trading::TradingArgs;
pub use wallet::WalletCommands;

pub use config::handle_config_command;
pub use dex::handle_dex_command;
pub use trading::handle_trading_command;
pub use wallet::handle_wallet_command;

#[derive(Subcommand)]
pub enum Commands {
    /// DEX commands
    Dex {
        #[command(subcommand)]
        command: DexCommands,
    },
    /// Wallet commands
    Wallet {
        #[command(subcommand)]
        command: WalletCommands,
    },
    /// Configuration commands
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// Trading bot commands
    Trading {
        #[command(subcommand)]
        command: TradingArgs,
    },
}
