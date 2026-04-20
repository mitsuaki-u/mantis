//! Mantis Trading Bot CLI
//!
//! A modular command-line interface for the Mantis trading bot.
//! This binary serves as the main entry point and orchestrates the various
//! subsystems using the existing modularized CLI, config, and infrastructure.

use clap::Parser;
use log::{debug, info};
use mantis::bootstrap::{init_cache, init_database};
use mantis::cli::{
    apply_cli_config, handle_config_command, handle_database_command, handle_trading_command,
    Commands, GlobalArgs,
};
use mantis::config::Config;
use mantis::error::Error;
use mantis::infrastructure::database::repositories::TokenRepository;
use mantis::infrastructure::dex::DexClient;
use mantis::infrastructure::logging;
use mantis::EventRouter;
use std::sync::Arc;
use validator::Validate;

#[derive(Parser)]
#[clap(name = "mantis")]
#[clap(
    about = "Mantis - Automated cryptocurrency trading bot",
    long_about = "Mantis - Automated cryptocurrency trading bot\n\nA modular trading system with configurable strategies, risk management,\nand support for multiple DEX protocols. Run in paper trading mode to test\nstrategies without risking real funds.\n\nUse 'mantis <command> --help' for detailed information on each command.",
    version,
    author
)]
#[clap(propagate_version = true)]
struct Cli {
    #[command(flatten)]
    global: GlobalArgs,

    #[command(subcommand)]
    command: Commands,
}

/// Helper function to determine if we need the trading system components
fn should_start_trading_system(command: &Commands) -> bool {
    use mantis::cli::commands::TradingArgs;
    if let Commands::Trading(trading_args) = command {
        matches!(
            trading_args.as_ref(),
            TradingArgs::Start(_)
                | TradingArgs::Status
                | TradingArgs::Health
                | TradingArgs::Restart { .. }
                | TradingArgs::Stop
        )
    } else {
        false
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let args = Cli::parse();

    // Load configuration
    let mut config = Config::load()?;

    // Apply command-line overrides to configuration
    apply_cli_config(&mut config, &args.global);

    // Validate config after CLI overrides - ensure all values are valid
    config.trading.validate().map_err(|e| {
        Error::Config(format!(
            "Trading configuration validation failed after CLI overrides: {}",
            e
        ))
    })?;
    config.data_collection.validate().map_err(|e| {
        Error::Config(format!(
            "Data collection configuration validation failed after CLI overrides: {}",
            e
        ))
    })?;

    // Initialize logging
    logging::init_logger(
        args.global.log_level.as_deref(),
        args.global.debug,
        args.global.log_file.as_deref(),
        &config.logs.directory,
        "mantis",
        args.global.log_modules.as_deref(),
    )
    .map_err(|e| Error::Config(format!("Logger initialization failed: {}", e)))?;

    info!("Mantis CLI started");
    info!("Version: {}", env!("CARGO_PKG_VERSION"));
    debug!("Configuration loaded successfully");

    // Initialize database
    debug!("Initializing database...");
    let db = init_database(&config).await?;

    // Create token repository for cache
    let token_repo_for_cache = Arc::new(TokenRepository::new(
        db.clone(),
        !config.trading.live_trading,
    ));

    // Initialize cache
    debug!("Initializing cache...");
    let cache = init_cache(&config, token_repo_for_cache).await;
    if cache.is_some() {
        debug!("Cache is available");
    }

    // Initialize EventRouter only for trading system commands
    let event_router = if should_start_trading_system(&args.command) {
        debug!("Initializing EventRouter for trading system...");
        Some(Arc::new(EventRouter::with_default_routing()))
    } else {
        None
    };

    // Route to appropriate command handler
    match args.command {
        Commands::Trading(trading_args) => {
            handle_trading_command_with_routing(*trading_args, config, db, event_router).await?
        }
        Commands::Config(config_cmd) => handle_config_command(config_cmd).await?,
        Commands::Database(db_cmd) => handle_database_command(db_cmd, config, db).await?,
    }

    Ok(())
}

/// Handle trading commands with proper routing between standalone and system commands
async fn handle_trading_command_with_routing(
    args: mantis::cli::commands::TradingArgs,
    config: Config,
    db: mantis::infrastructure::database::Database,
    event_router: Option<Arc<EventRouter>>,
) -> Result<(), Error> {
    use mantis::cli::commands::trading::{
        display_trading_history, display_transactions, positions,
    };
    use mantis::cli::commands::TradingArgs;

    // Handle standalone commands that don't need the full trading system
    match &args {
        TradingArgs::History {
            limit,
            paper: _,
            live,
        } => {
            let is_paper = !*live; // Default to paper trading unless --live is specified
            display_trading_history(&db, is_paper, *limit).await?;
        }
        TradingArgs::Positions {
            paper: _,
            live,
            closed_limit: _,
        } => {
            let is_paper = !*live; // Default to paper trading unless --live is specified

            // Create DexClient for production-ready fee calculations
            let dex_client = if let Some(bus) = event_router.as_ref() {
                match if is_paper {
                    DexClient::new_paper_trading(&config)
                } else {
                    DexClient::new_live(&config, bus.clone()).await
                } {
                    Ok(client) => {
                        info!("Using production-ready DexClient for accurate P&L calculations");
                        Some(Arc::new(client))
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to create DexClient, using legacy fee calculation: {}",
                            e
                        );
                        None
                    }
                }
            } else {
                log::warn!("EventRouter not initialized for DexClient creation");
                None
            };

            positions(&db, is_paper, &config, dex_client).await?;
        }
        TradingArgs::Transactions {
            limit,
            sync,
            failed,
            pending,
            confirmed,
        } => {
            display_transactions(&config, db, *limit, *sync, *failed, *pending, *confirmed).await?;
        }
        // System commands that need the full trading system
        _ => {
            if let Some(bus) = event_router {
                handle_trading_command(args, config, db, bus).await?;
            } else {
                return Err(Error::Config(
                    "EventRouter not initialized for this trading command".to_string(),
                ));
            }
        }
    }

    Ok(())
}
