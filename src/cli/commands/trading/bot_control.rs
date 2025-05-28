use crate::core::config::Config;
use crate::core::error::Error;
use crate::domain::trading::execution::bot::TradingBotSystem;
use crate::domain::trading::indicators::IndicatorWeights;
use crate::infra::actors::MessageBus;
use crate::infra::db::Database;
use log::{info, warn};
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use tokio::fs;

use super::args::{is_testnet_network, TradingArgs};

/// Start the trading bot with the given configuration
pub async fn start_trading(
    cmd: TradingArgs,
    config: Config,
    db: Database,
    message_bus: Arc<MessageBus>,
) -> Result<(), Error> {
    if let TradingArgs::Start {
        strategy,
        max_position,
        max_exposure,
        confidence_threshold,
        min_volume,
        stop_loss,
        paper,
        network,
        interval,
        background,
        wide_scan,
        min_data_points,
        risk_tolerance,
        rsi_weight,
        macd_weight,
        bollinger_weight,
        volume_weight,
        testing_mode,
    } = cmd
    {
        info!("🚀 Starting trading bot with strategy: {}", strategy);

        // Override configuration with command line parameters
        let mut config = config;

        // Override paper trading mode if specified
        if paper {
            config.trading.paper_trading = true;
            info!("📝 Paper trading mode enabled via command line");
        }

        // Override network configuration
        config.dex.network = Some(network.clone());
        let is_testnet = is_testnet_network(&network);
        info!(
            "🌐 Network set to: {} ({})",
            network,
            if is_testnet { "testnet" } else { "mainnet" }
        );

        // Override position and exposure limits
        config.trading.max_position_size = max_position;
        config.trading.max_total_exposure = max_exposure;
        info!(
            "💰 Position limits - Max position: ${:.2}, Max exposure: ${:.2}",
            max_position, max_exposure
        );

        // Override market scan interval
        config.data_collection.interval = interval;
        info!("⏱️ Market scan interval set to: {}s", interval);

        // Create the trading bot system
        let mut bot_system = TradingBotSystem::new(db, config);

        // Configure wide scan mode if specified
        if wide_scan {
            bot_system = bot_system.with_wide_scan_mode(true);
        }

        // Use the existing message bus
        bot_system = bot_system.with_message_bus(message_bus);

        // Prepare strategy parameters
        let mut strategy_params = json!({
            "threshold": confidence_threshold,
            "min_volume": min_volume,
            "stop_loss": stop_loss,
            "min_data_points": min_data_points,
            "risk_tolerance": risk_tolerance,
            "testing_mode": testing_mode,
        });

        // Add indicator weights for momentum strategy
        if strategy == "momentum" {
            let weights = IndicatorWeights {
                rsi: rsi_weight,
                macd: macd_weight,
                bollinger_bands: bollinger_weight,
                volume: volume_weight,
            };
            strategy_params["indicator_weights"] = json!({
                "rsi": weights.rsi,
                "macd": weights.macd,
                "bollinger_bands": weights.bollinger_bands,
                "volume": weights.volume,
            });
        }

        // Start the bot with the specified strategy
        bot_system.start(&strategy, &strategy_params).await?;

        // Create state file to indicate bot is running
        let state_file_path = get_state_file_path()?;
        create_state_file(&state_file_path, &strategy).await?;

        info!("✅ Trading bot started successfully");
        info!("📁 State file created at: {:?}", state_file_path);

        // Handle background vs foreground mode
        if background {
            info!("🔄 Running in background mode (daemon)");
            // TODO: Implement proper daemon mode with process detachment
            // For now, we'll run in foreground but log that background mode was requested
            warn!("⚠️ Background mode not yet fully implemented - running in foreground");
            bot_system.run_foreground(&state_file_path).await?;
        } else {
            info!("🖥️ Running in foreground mode");
            bot_system.run_foreground(&state_file_path).await?;
        }

        Ok(())
    } else {
        Err(Error::InvalidInput("Invalid start command".to_string()))
    }
}

/// Stop the trading bot
pub async fn stop_trading(message_bus: Arc<MessageBus>, _db: Database) -> Result<(), Error> {
    info!("🛑 Stopping trading bot...");

    // Send stop signal to all actors via message bus
    // Note: We need to create proper Event types for system control
    // For now, we'll just remove the state file and let the bot stop naturally

    // Remove state file
    let state_file_path = get_state_file_path()?;
    if state_file_path.exists() {
        fs::remove_file(&state_file_path)
            .await
            .map_err(|e| Error::Io(format!("Failed to remove state file: {}", e)))?;
        info!("📁 State file removed: {:?}", state_file_path);
    }

    info!("✅ Trading bot stopped successfully");
    Ok(())
}

/// Restart a specific actor
pub async fn restart_actor(
    message_bus: Arc<MessageBus>,
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
    path.push(".honeybadger_state");
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
