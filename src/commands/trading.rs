use clap::{Subcommand, ArgAction};
use colored::*;
use crate::error::Error;
use log::{info, error, debug};
use crate::db::Database;
use crate::trading::bot::TradingBotSystem;
use crate::config;
use crate::config::Config;
use dirs;
use std::io::Write;
use std::sync::Arc;
use crate::actors::MessageBus;
use crate::trading::strategy::{Strategy, Position, TradingStrategy, MomentumStrategy};
use serde_json::json;
use chrono::Utc;

// Constants
const APP_NAME: &str = "honeybadger";

// Define a struct for checkpoint data near the top of the file
#[derive(Debug, Clone)]
struct CheckpointData {
    status: serde_json::Value,
    metrics: serde_json::Value,
    timestamp: String,
}

#[derive(Subcommand)]
pub enum TradingCommands {
    /// Start automated trading with a chosen strategy
    Start {
        /// Trading strategy to use (e.g., momentum)
        #[arg(short, long, default_value = "momentum")]
        strategy: String,
        /// Maximum position size in USD
        #[arg(short, long, default_value_t = 100.0)]
        max_position: f64,
        /// Maximum total exposure in USD
        #[arg(short = 'e', long, default_value_t = 500.0)]
        max_exposure: f64,
        /// Price change threshold for signals (%)
        #[arg(long, default_value_t = 5.0)]
        threshold: f64,
        /// Minimum 24h volume in USD
        #[arg(long, default_value_t = 100000.0)]
        min_volume: f64,
        /// Maximum loss per position (%)
        #[arg(long, default_value_t = 5.0)]
        stop_loss: f64,
        /// Run in paper trading mode (no real trades)
        #[arg(long, action = ArgAction::SetTrue)]
        paper: bool,
        /// Run in dry-run mode (synonym for paper trading)
        #[arg(long, action = ArgAction::SetTrue)]
        dry_run: bool,
        /// Market scan interval in seconds
        #[arg(short, long, default_value_t = 60)]
        interval: u64,
        /// Run in the background (daemon mode)
        #[arg(short, long, action = ArgAction::SetTrue)]
        background: bool,
        /// Minimum data points required for analysis
        #[arg(short = 'p', long, default_value_t = 7)]
        min_data_points: u32,
        /// Risk tolerance level (0-5): 0=Conservative, 1=Conservative-Moderate, 2=Moderate, 3=Moderate-Aggressive, 4=Aggressive, 5=Very Aggressive
        #[arg(short = 'r', long, default_value_t = 0)]
        risk_tolerance: u8,
    },
    /// Show current bot status and positions
    Status,
    /// Stop the trading bot
    Stop,
    /// View trading history and performance
    History {
        /// Number of trades to display
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
        /// Show only paper trading history
        #[arg(long, action = ArgAction::SetTrue)]
        paper: bool,
        /// Show only live trading history
        #[arg(long, action = ArgAction::SetTrue)]
        live: bool,
    },
    /// View current open positions
    Positions {
        /// Show only paper trading positions
        #[arg(long, action = ArgAction::SetTrue)]
        paper: bool,
        /// Show only live trading positions
        #[arg(long, action = ArgAction::SetTrue)]
        live: bool,
    },
    /// Analyze latest market data for trading signals
    Analyze {
        /// Trading strategy to use (e.g., momentum)
        #[arg(short, long, default_value = "momentum")]
        strategy: String,
        /// Price change threshold for signals (%)
        #[arg(long, default_value_t = 5.0)]
        threshold: f64,
        /// Minimum 24h volume in USD
        #[arg(long, default_value_t = 100000.0)]
        min_volume: f64,
        /// Maximum loss per position (%)
        #[arg(long, default_value_t = 5.0)]
        stop_loss: f64,
        /// Show verbose debugging information
        #[arg(long, action = ArgAction::SetTrue)]
        debug: bool,
        /// Risk tolerance level (0-5): 0=Conservative, 1=Conservative-Moderate, 2=Moderate, 3=Moderate-Aggressive, 4=Aggressive, 5=Very Aggressive
        #[arg(short = 'r', long, default_value_t = 0)]
        risk_tolerance: u8,
    },
}

/// Handle trading commands
pub async fn handle_trading_command(cmd: TradingCommands, config: Config, message_bus: Arc<MessageBus>) -> Result<(), Error> {
    match cmd {
        TradingCommands::Start { .. } => {
            // Call the start_trading function with the command, config, and message_bus
            start_trading(cmd, config, message_bus).await
        }
        TradingCommands::Stop => {
            stop_trading(message_bus).await
        }
        TradingCommands::Status => {
            get_trading_status(message_bus).await
        }
        TradingCommands::History { limit, paper, live } => {
            // Handle history command with existing implementation
            let db = Database::new()?;
            display_trading_history(&db, paper, limit)?;
            Ok(())
        }
        TradingCommands::Positions { paper, live } => {
            // Handle positions command with existing implementation
            let db = Database::new()?;
            display_open_positions(&db, paper)?;
            Ok(())
        }
        TradingCommands::Analyze { 
            strategy, 
            threshold, 
            min_volume, 
            stop_loss,
            debug,
            risk_tolerance
        } => {
            // Handle analyze command with existing implementation
            // This can be updated later to use the actor-based system
            info!("Analyzing market data with {} strategy", strategy);
            // Implementation here...
            Ok(())
        }
    }
}

fn display_trading_history(db: &Database, is_paper: bool, limit: usize) -> Result<(), Error> {
    let mode = if is_paper { "Paper Trading".bright_yellow() } else { "Live Trading".bright_red() };
    println!("\n{} History:", mode);
    
    // Get trading history
    println!("Querying database for {} trading history, limit: {}", if is_paper { "paper" } else { "live" }, limit);
    let trades = db.get_trading_history(is_paper, limit)?;
    println!("Found {} trade records", trades.len());
    
    if trades.is_empty() {
        println!("No trading history found.");
        return Ok(());
    }
    
    // Display trades
    println!("{:<5} {:<8} {:<10} {:<10} {:<10} {:<12} {:<20}", 
        "#", "Symbol", "Entry", "Exit", "Size", "P&L", "Date");
    println!("{}", "─".repeat(80));
    
    for (i, trade) in trades.iter().enumerate() {
        let pnl_color = if trade.pnl >= 0.0 { trade.pnl.to_string().green() } else { trade.pnl.to_string().red() };
        let date = trade.exit_time.format("%Y-%m-%d %H:%M:%S").to_string();
        
        println!("{:<5} {:<8} ${:<9.4} ${:<9.4} ${:<9.2} ${:<11} {:<20}", 
            i+1, 
            trade.token_id.bright_cyan(),
            trade.entry_price,
            trade.exit_price,
            trade.size,
            pnl_color,
            date);
    }
    
    // Get and display performance stats
    let stats = db.get_performance_stats(is_paper)?;
    
    println!("\nPerformance Summary:");
    println!("Total Trades: {}", stats.total_trades);
    println!("Win Rate: {:.2}%", stats.win_rate);
    println!("Total P&L: ${:.2}", stats.total_pnl);
    println!("Average P&L per Trade: ${:.2}", stats.avg_pnl);
    println!("Largest Win: ${:.2}", stats.max_profit);
    println!("Largest Loss: ${:.2}", stats.max_loss);
    
    Ok(())
}

fn display_open_positions(db: &Database, is_paper: bool) -> Result<(), Error> {
    let mode = if is_paper { "Paper Trading".bright_yellow() } else { "Live Trading".bright_red() };
    println!("\n{} Open Positions:", mode);
    
    println!("Querying database for {} open positions", if is_paper { "paper" } else { "live" });
    let positions = db.get_open_positions(is_paper)?;
    println!("Found {} position records", positions.len());
    
    if positions.is_empty() {
        println!("No open positions.");
        return Ok(());
    }
    
    println!("{:<5} {:<8} {:<10} {:<10} {:<12} {:<12} {:<20}", 
        "#", "Symbol", "Entry", "Current", "Size", "Unreal P&L", "Entry Time");
    println!("{}", "─".repeat(80));
    
    for (i, pos) in positions.iter().enumerate() {
        let pnl_color = if pos.unrealized_pnl >= 0.0 { 
            pos.unrealized_pnl.to_string().green() 
        } else { 
            pos.unrealized_pnl.to_string().red() 
        };
        
        let date = pos.entry_time.format("%Y-%m-%d %H:%M:%S").to_string();
        
        println!("{:<5} {:<8} ${:<9.4} ${:<9.4} ${:<11.2} ${:<11} {:<20}", 
            i+1,
            pos.token_id.bright_cyan(),
            pos.entry_price,
            pos.current_price,
            pos.size,
            pnl_color,
            date);
    }
    
    Ok(())
}

/// Start the trading bot with the given strategy and parameters
pub async fn start_trading(cmd: TradingCommands, config: Config, message_bus: Arc<MessageBus>) -> Result<(), Error> {
    info!("Starting trading bot with actor-based architecture");
    
    // Extract parameters based on command variant
    let (strategy, threshold, min_volume, max_position, paper, dry_run, interval, stop_loss, background, min_data_points, risk_tolerance) = match cmd {
        TradingCommands::Start { 
            strategy, 
            threshold, 
            min_volume, 
            max_position,
            paper,
            dry_run, 
            interval, 
            stop_loss,
            background,
            min_data_points,
            risk_tolerance,
            .. 
        } => (
            strategy, 
            threshold, 
            min_volume, 
            max_position,
            paper,
            dry_run, 
            interval, 
            stop_loss,
            background,
            min_data_points,
            risk_tolerance
        ),
        _ => {
            return Err(Error::InvalidInput("Invalid command for starting trading bot".to_string()));
        }
    };
    
    // Get the strategy name - it's already a String, no need to unwrap
    let strategy = strategy;
    
    // Update config with command line parameters
    let mut updated_config = config.clone();
    
    // Direct assignment since these are not Options
    updated_config.trading.strategy.threshold = threshold;
    updated_config.trading.strategy.min_volume = min_volume;
    updated_config.trading.max_position_size = max_position;
    updated_config.trading.scan_interval = interval;
    updated_config.trading.risk.stop_loss_pct = stop_loss;
    
    // Use paper trading if either paper_trading or dry_run is specified
    if paper || dry_run {
        updated_config.trading.paper_trading = true;
    }
    
    // Set up database path
    let db_path = config::Config::db_path(&updated_config)?;
    let db_path_str = db_path.to_string_lossy().to_string();
    
    // Create parameters for the strategy
    let params = serde_json::json!({
        "threshold": threshold,
        "min_volume": min_volume,
        "stop_loss": stop_loss,
        "strategy": strategy,
        "dry_run": dry_run,
        "running": true,
        "min_data_points": min_data_points,
        "risk_tolerance": risk_tolerance
    });
    
    // Save the trading state to a file
    let config_dir = dirs::config_dir()
        .ok_or_else(|| Error::Config("Could not determine configuration directory".to_string()))?
        .join(APP_NAME);
    
    // Create directory if it doesn't exist
    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir)
            .map_err(|e| Error::Config(format!("Failed to create config directory: {}", e)))?;
    }
    
    let state_file = config_dir.join("trading_state.json");
    std::fs::write(&state_file, params.to_string())
        .map_err(|e| Error::Io(format!("Failed to write trading state file: {}", e)))?;
    
    // Create and start the actor-based trading system
    let mut bot_system = TradingBotSystem::with_message_bus(updated_config, db_path_str, message_bus);
    
    // Start the bot system with the strategy and risk tolerance parameter
    match bot_system.start(&strategy, &params).await {
        Ok(_) => {
            println!("🚀 Trading bot started successfully with {} strategy", strategy.bright_green());
            println!("Using min_data_points: {}", min_data_points);
            
            let risk_level_desc = match risk_tolerance {
                0 => "Conservative (standard analysis)".bright_blue(),
                1 => "Conservative-Moderate (some flexibility)".bright_green(),
                2 => "Moderate (more signals)".bright_yellow(),
                3 => "Moderate-Aggressive (more signals)".bright_yellow(),
                4 => "Aggressive (maximum signals)".bright_red(),
                5 => "Very Aggressive (maximum signals)".bright_red(),
                _ => "Unknown".bright_white(),
            };
            
            println!("Risk tolerance level: {} - {}", risk_tolerance, risk_level_desc);
            
            println!("Trading bot is now running in the {}. Press Ctrl+C to stop.", 
                if background {
                    "background".bright_blue()
                } else {
                    "foreground".bright_green()
                });
            
            if background {
                println!("Bot is running in {} mode", "background".bright_blue());
                println!("Monitor with: honeybadger trading status");
                println!("Stop with: honeybadger trading stop");
                // Don't wait - return immediately so the command completes
                Ok(())
            } else {
                println!("Bot is running in {} mode (press Ctrl+C to stop)", "foreground".bright_blue());
                println!("In another terminal, you can monitor with: honeybadger trading status");
                println!("Or stop with: honeybadger trading stop");
                
                // Keep the bot running in the foreground until stopped
                bot_system.run_foreground(&state_file).await
            }
        },
        Err(e) => {
            // Clean up state file if startup fails
            let _ = std::fs::remove_file(&state_file);
            error!("Failed to start trading bot: {}", e);
            Err(e)
        }
    }
}

// Add this function to clean up the trading state file
fn cleanup_trading_state() -> Result<(), Error> {
    // Check common locations where the state file might be
    let possible_dirs = vec![
        dirs::config_dir().map(|d| d.join(APP_NAME)),
        dirs::data_dir().map(|d| d.join(APP_NAME)),
        dirs::home_dir().map(|d| d.join(".config").join(APP_NAME)),
        Some(std::path::PathBuf::from("./"))
    ];
    
    for maybe_dir in possible_dirs {
        if let Some(dir) = maybe_dir {
            let state_file = dir.join("trading_state.json");
            if state_file.exists() {
                info!("Found trading state file at: {:?}", state_file);
                if let Err(e) = std::fs::remove_file(&state_file) {
                    error!("Error removing trading state file: {}", e);
                } else {
                    info!("Successfully removed trading state file");
                }
            }
        }
    }
    
    Ok(())
}

/// Stop the trading bot
pub async fn stop_trading(message_bus: Arc<MessageBus>) -> Result<(), Error> {
    info!("Stopping trading bot");
    
    // Log the message bus ID for debugging
    let bus_id = format!("{:p}", Arc::as_ptr(&message_bus));
    debug!("stop_trading using MessageBus [id: {}]", bus_id);
    
    // Attempt to clean up any state files regardless of where they might be
    cleanup_trading_state()?;
    
    // Check if the trading bot is running by checking for the state file
    let config_dir = dirs::config_dir()
        .ok_or_else(|| Error::Config("Could not determine configuration directory".to_string()))?
        .join(APP_NAME);
    
    let state_file = config_dir.join("trading_state.json");
    
    if !state_file.exists() {
        println!("Trading bot is not running or has already been stopped");
        return Ok(());
    }
    
    // Try to load the existing configuration
    let config = match config::Config::load() {
        Ok(config) => config,
        Err(e) => {
            error!("Error loading config: {}", e);
            config::Config::default()
        }
    };
    
    // Try to read the state file
    let state_str = match std::fs::read_to_string(&state_file) {
        Ok(s) => s,
        Err(e) => {
            error!("Error reading trading state file: {}", e);
            // Delete the state file and return
            let _ = std::fs::remove_file(&state_file);
            println!("Trading bot state file was corrupted and has been removed");
            return Ok(());
        }
    };
    
    // Parse the state
    let state: serde_json::Value = match serde_json::from_str(&state_str) {
        Ok(s) => s,
        Err(e) => {
            error!("Error parsing trading state file: {}", e);
            // Delete the state file and return
            let _ = std::fs::remove_file(&state_file);
            println!("Trading bot state file was corrupted and has been removed");
            return Ok(());
        }
    };
    
    // Set up database path
    let db_path = config::Config::db_path(&config)?;
    let db_path_str = db_path.to_string_lossy().to_string();
    
    // Create the actor-based trading system with the shared message bus
    let mut bot_system = TradingBotSystem::with_message_bus(config, db_path_str, message_bus);
    
    // Stop the trading bot
    match bot_system.stop().await {
        Ok(_) => {
            println!("Trading bot stopped successfully");
            // Delete the state file
            if let Err(e) = std::fs::remove_file(&state_file) {
                error!("Error removing trading state file: {}", e);
            }
            Ok(())
        },
        Err(e) => {
            error!("Failed to stop trading bot: {}", e);
            // Delete the state file anyway, as it's likely in a bad state
            let _ = std::fs::remove_file(&state_file);
            Err(e)
        }
    }
}

/// Get the status of the trading bot
pub async fn get_trading_status(message_bus: Arc<MessageBus>) -> Result<(), Error> {
    info!("Getting trading bot status");
    
    // Log the message bus ID for debugging
    let bus_id = format!("{:p}", Arc::as_ptr(&message_bus));
    debug!("get_trading_status using MessageBus [id: {}]", bus_id);
    
    // Check if the trading bot is running by checking for the state file
    let config_dir = dirs::config_dir()
        .ok_or_else(|| Error::Config("Could not determine configuration directory".to_string()))?
        .join(APP_NAME);
    
    let state_file = config_dir.join("trading_state.json");
    let checkpoint_file = config_dir.join("trading_checkpoint.json");
    
    let running = state_file.exists();
    
    // Try to load the existing configuration
    let config = match config::Config::load() {
        Ok(config) => config,
        Err(e) => {
            error!("Error loading config: {}", e);
            config::Config::default()
        }
    };
    
    // Set up database path
    let db_path = config::Config::db_path(&config)?;
    let db_path_str = db_path.to_string_lossy().to_string();
    
    // Create the actor-based trading system with the shared message bus
    let bot_system = TradingBotSystem::with_message_bus(config, db_path_str, message_bus);
    
    // Print the status
    println!("🤖 Trading Bot Status");
    println!("───────────────────");
    
    println!("Running: {}", 
        if running { "Yes".bright_green() } 
        else { "No".bright_red() }
    );
    
    // If the bot is running, try to get detailed status
    if running {
        // Read the state file to get strategy and other info
        if let Ok(state_str) = std::fs::read_to_string(&state_file) {
            if let Ok(state) = serde_json::from_str::<serde_json::Value>(&state_str) {
                let paper_trading = state["dry_run"].as_bool().unwrap_or(true);
                let strategy = state["strategy"].as_str().unwrap_or("unknown");
                
                println!("Mode: {}", 
                    if paper_trading { "Paper Trading".bright_yellow() } 
                    else { "LIVE TRADING".bright_red() }
                );
                
                println!("Strategy: {}", strategy.bright_cyan());
                
                // First check if we have a checkpoint file for more accurate status
                if checkpoint_file.exists() {
                    if let Ok(checkpoint_str) = std::fs::read_to_string(&checkpoint_file) {
                        if let Ok(checkpoint) = serde_json::from_str::<serde_json::Value>(&checkpoint_str) {
                            println!("\nActor Status:");
                            
                            // Actor status from checkpoint
                            if let Some(status) = checkpoint["status"].as_object() {
                                if let Some(actors) = status.get("actors").and_then(|a| a.as_object()) {
                                    for (name, status) in actors {
                                        let status_str = if status.as_bool().unwrap_or(false) {
                                            "Running".bright_green()
                                        } else {
                                            "Stopped".bright_red()
                                        };
                                        println!("  {} Actor: {}", name.to_uppercase(), status_str);
                                    }
                                }
                            }
                            
                            // Show last checkpoint time
                            if let Some(timestamp) = checkpoint["timestamp"].as_str() {
                                println!("\nLast checkpoint: {}", timestamp);
                            }
                            
                            // Display tracking tokens from checkpoint
                            if let Some(tokens) = checkpoint["status"]["tokens_tracked"].as_array() {
                                let token_list: Vec<String> = tokens.iter()
                                    .filter_map(|t| t.as_str().map(|s| s.to_string()))
                                    .collect();
                                println!("\nTracking {} tokens: {}", token_list.len(), token_list.join(", "));
                            }
                            
                            // We've displayed everything from the checkpoint
                            return Ok(());
                        }
                    }
                }
                
                // If we get here, we didn't have a valid checkpoint, so use the system query
                println!("\nActor Status:");
                
                // Try to get status from the bot system (which may or may not work)
                if let Ok(status) = bot_system.get_status().await {
                    if let Some(actors) = status["actors"].as_object() {
                        for (name, status) in actors {
                            let status_str = if status.as_bool().unwrap_or(false) {
                                "Running".bright_green()
                            } else {
                                "Stopped".bright_red()
                            };
                            println!("  {} Actor: {}", name.to_uppercase(), status_str);
                        }
                    }
                    
                    // Display tracking tokens
                    if let Some(tokens) = status["tokens_tracked"].as_array() {
                        let token_list: Vec<String> = tokens.iter()
                            .filter_map(|t| t.as_str().map(|s| s.to_string()))
                            .collect();
                        println!("\nTracking {} tokens: {}", token_list.len(), token_list.join(", "));
                    }
                }
            }
        }
    } else {
        println!("Bot is not running. Start with: honeybadger trading start");
    }
    
    Ok(())
} 