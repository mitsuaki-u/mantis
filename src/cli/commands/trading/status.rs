use crate::core::config::{self};
use crate::core::error::Error;
use crate::domain::trading::execution::bot::TradingBotSystem;
use crate::infra::actors::MessageBus;
use crate::infra::db::Database;
use colored::*;
use log::{debug, error, info};
use std::sync::Arc;

// Constants
const APP_NAME: &str = "honeybadger";

/// Get the current status of the trading bot
pub async fn get_trading_status(message_bus: Arc<MessageBus>, db: Database) -> Result<(), Error> {
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

    // Create the actor-based trading system with the shared message bus, passing db
    let bot_system = TradingBotSystem::new(db, config);
    let bot_system = bot_system.with_message_bus(message_bus);

    // Print the status
    println!("🤖 Trading Bot Status");
    println!("───────────────────");

    println!(
        "Running: {}",
        if running {
            "Yes".bright_green()
        } else {
            "No".bright_red()
        }
    );

    // If the bot is running, try to get detailed status
    if running {
        // Read the state file to get strategy and other info
        if let Ok(state_str) = std::fs::read_to_string(&state_file) {
            if let Ok(state) = serde_json::from_str::<serde_json::Value>(&state_str) {
                let paper_trading = state["paper_trading"].as_bool().unwrap_or(true);
                let strategy = state["strategy"].as_str().unwrap_or("unknown");

                println!(
                    "Mode: {}",
                    if paper_trading {
                        "Paper Trading".bright_yellow()
                    } else {
                        "LIVE TRADING".bright_red()
                    }
                );

                println!("Strategy: {}", strategy.bright_cyan());

                // First check if we have a checkpoint file for more accurate status
                if checkpoint_file.exists() {
                    if let Ok(checkpoint_str) = std::fs::read_to_string(&checkpoint_file) {
                        if let Ok(checkpoint) =
                            serde_json::from_str::<serde_json::Value>(&checkpoint_str)
                        {
                            println!("\nActor Status:");

                            // Actor status from checkpoint
                            if let Some(status) = checkpoint["status"].as_object() {
                                if let Some(actors) =
                                    status.get("actors").and_then(|a| a.as_object())
                                {
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
                            if let Some(tokens) = checkpoint["status"]["tokens_tracked"].as_array()
                            {
                                let token_list: Vec<String> = tokens
                                    .iter()
                                    .filter_map(|t| t.as_str().map(|s| s.to_string()))
                                    .collect();
                                println!(
                                    "\nTracking {} tokens: {}",
                                    token_list.len(),
                                    token_list.join(", ")
                                );
                            }

                            // We've displayed everything from the checkpoint
                            return Ok(());
                        }
                    }
                }

                // If we get here, we didn't have a valid checkpoint, so use the system query
                println!("\nActor Status:");

                // Try to get status from the bot system (which may or may not work)
                let status = bot_system.get_status().await?;
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
                    let token_list: Vec<String> = tokens
                        .iter()
                        .filter_map(|t| t.as_str().map(|s| s.to_string()))
                        .collect();
                    println!(
                        "\nTracking {} tokens: {}",
                        token_list.len(),
                        token_list.join(", ")
                    );
                }
            }
        }
    } else {
        println!("Bot is not running. Start with: honeybadger trading start");
    }

    Ok(())
}

/// Get health report from the supervisor
pub async fn get_health_report(message_bus: Arc<MessageBus>, db: Database) -> Result<(), Error> {
    info!("Getting supervisor health report");

    // Check if the trading bot is running by checking for the state file
    let config_dir = dirs::config_dir()
        .ok_or_else(|| Error::Config("Could not determine configuration directory".to_string()))?
        .join(APP_NAME);

    let state_file = config_dir.join("trading_state.json");

    if !state_file.exists() {
        return Err(Error::InvalidInput(
            "Trading bot is not running".to_string(),
        ));
    }

    // Try to load the existing configuration
    let config = match config::Config::load() {
        Ok(config) => config,
        Err(e) => {
            error!("Error loading config: {}", e);
            config::Config::default()
        }
    };

    // Create the actor-based trading system with the shared message bus, passing db
    let bot_system = TradingBotSystem::new(db, config);
    let bot_system = bot_system.with_message_bus(message_bus);

    // Get health report from supervisor
    match bot_system.get_health_report().await {
        Ok(report) => {
            println!("📊 Supervisor Health Report");
            println!("──────────────────────────");

            // Display actors health status
            if let Some(actors) = report["actors"].as_object() {
                println!("\nActor Health Status:");

                for (name, status) in actors {
                    let running = status["running"].as_bool().unwrap_or(false);
                    let failures = status["failure_count"].as_u64().unwrap_or(0);
                    let health_status = status["health_status"].as_str().unwrap_or("Unknown");

                    let status_display = if running {
                        "Running".bright_green()
                    } else {
                        "Stopped".bright_red()
                    };

                    let health_display = match health_status {
                        "Good" => health_status.bright_green(),
                        "Degraded" => health_status.yellow(),
                        "Critical" => health_status.bright_red(),
                        _ => health_status.normal(),
                    };

                    println!(
                        "  {}: {} | Health: {} | Failures: {}",
                        name.to_uppercase(),
                        status_display,
                        health_display,
                        if failures > 0 {
                            failures.to_string().red()
                        } else {
                            failures.to_string().green()
                        }
                    );
                }
            }

            // Display system health status
            if let Some(system) = report["system"].as_object() {
                println!("\nSystem Health:");

                if let Some(uptime) = system.get("uptime_seconds").and_then(|u| u.as_u64()) {
                    let hours = uptime / 3600;
                    let minutes = (uptime % 3600) / 60;
                    let seconds = uptime % 60;
                    println!(
                        "  Uptime: {} hours, {} minutes, {} seconds",
                        hours, minutes, seconds
                    );
                }

                if let Some(memory) = system.get("memory_usage_mb").and_then(|m| m.as_f64()) {
                    println!("  Memory Usage: {:.2} MB", memory);
                }

                if let Some(overall) = system.get("overall_health").and_then(|h| h.as_str()) {
                    let health_display = match overall {
                        "Good" => overall.bright_green(),
                        "Degraded" => overall.yellow(),
                        "Critical" => overall.bright_red(),
                        _ => overall.normal(),
                    };
                    println!("  Overall Health: {}", health_display);
                }
            }

            Ok(())
        }
        Err(e) => {
            error!("Failed to get health report: {}", e);
            println!("❌ Failed to get health report: {}", e);
            Err(e)
        }
    }
}
