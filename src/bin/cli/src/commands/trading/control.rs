use crate::application::app::TradingBotSystem;
use crate::config::Config;
use crate::error::Error;
use crate::infrastructure::database::Database;
use crate::EventRouter;
use log::{debug, info, warn};
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use tokio::fs;

use super::args::TradingArgs;

/// Start the trading bot with the given configuration
pub async fn start_trading(
    cmd: TradingArgs,
    config: Config,
    db: Database,
    event_router: Arc<EventRouter>,
) -> Result<(), Error> {
    if let TradingArgs::Start(start_args) = cmd {
        let crate::cli::commands::trading::args::StartArgs {
            strategy,
            max_position,
            max_exposure,
            confidence_threshold,
            momentum_threshold,
            min_volume,
            min_liquidity,
            min_pool_transaction_count,
            stop_loss,
            live,
            network,
            interval,
            background,
            max_tokens_to_scan,
            indicator_profile,
            rsi_weight,
            macd_weight,
            bollinger_weight,
            volume_weight,
            market_data_provider,
        } = *start_args;
        // Override configuration with command line parameters (CLI overrides config)
        let mut config = config;

        // Override network configuration only if explicitly provided
        if let Some(network) = network {
            config.dex.network = Some(network.clone());
        } else {
            // Use network from config file
            let _network_name = config.dex.network.as_deref().ok_or_else(|| {
                Error::Config("No network specified in configuration. Please set dex.network in config or use --network parameter.".to_string())
            })?;
        }

        // Use CLI values if provided, otherwise fall back to config values
        let final_max_position = max_position.unwrap_or(config.trading.max_position_size);
        let final_max_exposure = max_exposure.unwrap_or(config.trading.max_total_exposure);
        let final_interval = interval.unwrap_or(config.data_collection.scan_interval_secs);
        let final_max_tokens_to_scan =
            max_tokens_to_scan.unwrap_or(config.trading.max_tokens_to_scan);

        // Clone values we'll need after moving config
        // Note: These values may have already been overridden by global CLI args in apply_cli_config()
        let signal_confidence_threshold = config.trading.signal_confidence_threshold;
        let trading_min_volume = config.trading.min_volume;
        let trading_min_liquidity = config.trading.min_liquidity;
        let trading_min_pool_transaction_count = config.trading.min_pool_transaction_count;
        let trading_stop_loss = config.trading.stop_loss;

        // DEBUG: Log the config values being read
        debug!(
            "🔍 Config values read: min_volume={}, threshold={}, stop_loss={}",
            trading_min_volume, signal_confidence_threshold, trading_stop_loss
        );

        // Clone config values before moving config
        let max_positions = config.trading.max_positions;
        let is_paper_trading = !config.trading.live_trading;
        let network_name = config.dex.network.as_deref().unwrap_or("ethereum");
        let protocol_name = config.dex.protocol.clone();
        let max_volatility_config = config.trading.max_volatility_24h;

        // Create the trading bot system
        let mut bot_system = TradingBotSystem::new(db, config.clone(), event_router);

        // Resolve strategy-specific thresholds
        let strategy_threshold = match strategy.as_str() {
            "momentum" => momentum_threshold.unwrap_or(signal_confidence_threshold),
            _ => signal_confidence_threshold,
        };

        // For StrategyActor confidence threshold, use CLI value or reasonable default
        let actor_confidence_threshold = confidence_threshold.unwrap_or(0.5);

        // Use CLI values if provided, otherwise fall back to config values
        let final_min_volume = min_volume.unwrap_or(trading_min_volume);
        let final_min_liquidity = min_liquidity.unwrap_or(trading_min_liquidity);
        let final_min_pool_transaction_count =
            min_pool_transaction_count.unwrap_or(trading_min_pool_transaction_count);
        let final_stop_loss = stop_loss.unwrap_or(trading_stop_loss);

        // Display comprehensive configuration summary
        // Box inner width = 60 chars (between the │ chars)
        const W: usize = 60;
        let row = |label: &str, value: &str| {
            println!("│ {:<W$} │", format!("{:<19}{}", label, value), W = W - 1);
        };

        let title = format!("MANTIS TRADING BOT  v{}", env!("CARGO_PKG_VERSION"));
        println!("\n┌{}┐", "─".repeat(W + 2));
        println!("│ {:^W$} │", title, W = W);
        println!("├{}┤", "─".repeat(W + 2));

        // Trading Mode
        println!("│ {:<W$} │", "── TRADING MODE", W = W);
        if is_paper_trading {
            row("Mode:", "📝 Paper Trading (simulation)");
            row("Balance:", "10 WETH (simulated)");
            row("Risk:", "No real funds at risk");
        } else {
            row("Mode:", "💰 LIVE TRADING (real money)");
            row("⚠  WARNING:", "Real funds will be used!");
            row("Risk:", "Capital is at risk of loss");
        }
        row("Network:", &network_name);
        row("Protocol:", &protocol_name);

        println!("├{}┤", "─".repeat(W + 2));

        // Strategy
        println!("│ {:<W$} │", "── STRATEGY", W = W);
        row("Strategy:", &strategy);
        row("Entry Threshold:", &format!("{:.2}", strategy_threshold));
        row("Confidence:", &format!("{:.2}", actor_confidence_threshold));
        if let Some(ref profile) = indicator_profile {
            row("Indicator Profile:", profile);
        }

        println!("├{}┤", "─".repeat(W + 2));

        // Risk Management
        println!("│ {:<W$} │", "── RISK MANAGEMENT", W = W);
        row("Max Positions:", &max_positions.to_string());
        row(
            "Position Size:",
            &format!("${:.0} per trade", final_max_position),
        );
        row(
            "Max Exposure:",
            &format!("${:.0} total", final_max_exposure),
        );
        row("Stop Loss:", &format!("{:.1}%", final_stop_loss));
        row(
            "Take Profit:",
            &format!("{:.1}%", config.trading.take_profit),
        );
        row("Max Volatility:", &format!("{:.1}%", max_volatility_config));

        println!("├{}┤", "─".repeat(W + 2));

        // Market Filters
        println!("│ {:<W$} │", "── MARKET FILTERS", W = W);
        row(
            "Min Volume:",
            &format!("${:.0}k (24h)", final_min_volume / 1000.0),
        );
        row(
            "Min Liquidity:",
            &format!("${:.0}k", final_min_liquidity / 1000.0),
        );
        row(
            "Min Pool Txns:",
            &final_min_pool_transaction_count.to_string(),
        );
        row(
            "Max Tokens:",
            &if final_max_tokens_to_scan == 0 {
                "Unlimited".to_string()
            } else {
                final_max_tokens_to_scan.to_string()
            },
        );
        row("Scan Interval:", &format!("{}s", final_interval));

        println!("└{}┘\n", "─".repeat(W + 2));

        // Confirmation prompt
        print!("Start trading with these settings? [y/N]: ");
        use std::io::{self, Write};
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        let input = input.trim().to_lowercase();

        if input != "y" && input != "yes" {
            println!("\n❌ Trading cancelled by user.");
            return Ok(());
        }

        println!("\n✅ Starting trading bot...\n");

        let mut strategy_params = json!({
            // Momentum strategy parameters
            "momentum_entry_threshold": strategy_threshold,
            // Common parameters for all strategies
            "min_volume": final_min_volume,
            "stop_loss_pct": final_stop_loss,
            "max_volatility_24h": max_volatility_config,
            // Additional bot parameters (not part of strategy configs)
            "min_liquidity": final_min_liquidity,
            "min_pool_transaction_count": final_min_pool_transaction_count,
            "confidence_threshold": actor_confidence_threshold, // For StrategyActor
            // CLI-overridden config values (will be validated in bot.rs)
            "max_position_size": final_max_position,
            "max_total_exposure": final_max_exposure,
            "data_collection_interval": final_interval,
            "paper_trading": !live,  // Invert: --live flag means paper_trading=false
        });

        // Add optional CLI overrides if provided
        if let Some(max_tokens) = max_tokens_to_scan {
            strategy_params["max_tokens_to_scan"] = json!(max_tokens);
        }

        if let Some(market_data_provider) = market_data_provider {
            strategy_params["market_data_provider"] = json!(market_data_provider);
        }

        // Add indicator configuration for momentum strategy
        if strategy == "momentum" {
            // Add indicator profile if specified
            if let Some(ref profile) = indicator_profile {
                strategy_params["indicator_profile"] = json!(profile);
            }

            // Add individual weight fields to match MomentumConfig struct
            if let Some(weight) = rsi_weight {
                strategy_params["rsi_weight"] = json!(weight);
            }
            if let Some(weight) = macd_weight {
                strategy_params["macd_weight"] = json!(weight);
            }
            if let Some(weight) = bollinger_weight {
                strategy_params["bollinger_weight"] = json!(weight);
            }
            if let Some(weight) = volume_weight {
                strategy_params["volume_weight"] = json!(weight);
            }
        }

        // Start the bot with the specified strategy
        bot_system.start(&strategy, &strategy_params).await?;

        // Create state file to indicate bot is running
        let state_file_path = get_state_file_path()?;
        create_state_file(&state_file_path, &strategy).await?;

        // Note: Background mode is not implemented - daemon process management
        // should be handled by the OS (systemd, launchd, etc.)
        if background {
            warn!(
                "Background mode flag ignored - use OS process management (systemd, screen, etc.)"
            );
        }

        bot_system.run_foreground(&state_file_path).await?;

        Ok(())
    } else {
        Err(Error::InvalidInput("Invalid start command".to_string()))
    }
}

/// Stop the trading bot
pub async fn stop_trading(_event_router: Arc<EventRouter>, _db: Database) -> Result<(), Error> {
    info!("Stopping trading bot...");

    // Send stop signal to all actors via message bus
    // Note: We need to create proper Event types for system control
    // For now, we'll just remove the state file and let the bot stop naturally

    // Remove state file
    let state_file_path = get_state_file_path()?;
    if state_file_path.exists() {
        fs::remove_file(&state_file_path)
            .await
            .map_err(|e| Error::Io(format!("Failed to remove state file: {}", e)))?;
        info!("State file removed: {:?}", state_file_path);
    }

    info!("Trading bot stopped successfully");
    Ok(())
}

/// Restart a specific actor
pub async fn restart_actor(
    _event_router: Arc<EventRouter>,
    actor_id: &str,
    _db: Database,
) -> Result<(), Error> {
    info!("🔄 Restarting actor: {}", actor_id);

    // Send restart signal to the specific actor
    // Note: We need to create proper Event types for actor control
    // For now, we'll just log the restart request
    warn!(
        "Actor restart functionality not yet implemented for: {}",
        actor_id
    );

    info!("✅ Restart signal sent to actor: {}", actor_id);
    Ok(())
}

/// Get the path to the state file
fn get_state_file_path() -> Result<std::path::PathBuf, Error> {
    let mut path = std::env::current_dir()
        .map_err(|e| Error::Io(format!("Failed to get current directory: {}", e)))?;
    path.push(".mantis_state");
    Ok(path)
}

/// Create a state file indicating the bot is running
async fn create_state_file(path: &Path, strategy: &str) -> Result<(), Error> {
    let state_data = json!({
        "status": "running",
        "strategy": strategy,
        "started_at": chrono::Utc::now().to_rfc3339(),
        "pid": std::process::id(),
    });

    fs::write(path, state_data.to_string())
        .await
        .map_err(|e| Error::Io(format!("Failed to create state file: {}", e)))?;

    Ok(())
}
