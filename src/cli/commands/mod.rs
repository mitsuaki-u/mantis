use clap::Subcommand;

pub mod market;
pub mod dex;
pub mod wallet;
pub mod config;
pub mod trading;

pub use market::MarketCommands;
pub use dex::DexCommands;
pub use wallet::WalletCommands;
pub use config::ConfigCommands;
pub use trading::TradingArgs;

pub use market::handle_market_command;
pub use dex::handle_dex_command;
pub use wallet::handle_wallet_command;
pub use config::handle_config_command;
pub use trading::handle_trading_command;

#[derive(Subcommand)]
pub enum Commands {
    /// Market commands
    Market {
        #[command(subcommand)]
        command: MarketCommands,
    },
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
