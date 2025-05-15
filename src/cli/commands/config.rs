use crate::core::config::Config;
use crate::core::error::{Error, Result};
use clap::{ArgAction, Subcommand};
use colored::*;
use serde_json;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Set an API key
    SetKey {
        /// Service name (cryptocompare, coingecko, etherscan)
        service: String,
        /// API key
        key: String,
    },

    /// Show current configuration
    Show {
        /// Show sensitive values like API keys
        #[arg(short, long)]
        show_secrets: bool,

        /// Output as JSON
        #[arg(short, long)]
        json: bool,
    },

    /// Get current configuration value
    Get {
        /// Configuration key (e.g., trading.paper_trading, api_keys.coingecko)
        key: String,
    },

    /// Set configuration value
    Set {
        /// Configuration key (e.g., trading.paper_trading, api.request_timeout)
        key: String,

        /// Value to set
        value: String,
    },

    /// Set a database parameter
    SetDatabase {
        /// Database parameter to set (path, logging)
        #[arg(value_enum, required = true)]
        parameter: DatabaseParameter,

        /// Value to set
        value: String,
    },

    /// Set trading parameters
    SetTrading {
        /// Enable paper trading (simulate trades)
        #[arg(long, action = ArgAction::SetTrue)]
        paper: bool,

        /// Scan interval in seconds
        #[arg(long)]
        scan_interval: Option<u64>,

        /// Maximum position size in USD
        #[arg(long)]
        max_position: Option<f64>,

        /// Maximum total exposure in USD
        #[arg(long)]
        max_exposure: Option<f64>,
    },

    /// Set strategy parameters
    SetStrategy {
        /// Strategy type (momentum, rsi, macd, etc.)
        #[arg(long)]
        strategy_type: Option<String>,

        /// Signal threshold
        #[arg(long)]
        threshold: Option<f64>,

        /// Minimum volume required for trading
        #[arg(long)]
        min_volume: Option<f64>,
    },

    /// Set risk management parameters
    SetRisk {
        /// Stop loss percentage
        #[arg(long)]
        stop_loss: Option<f64>,

        /// Take profit percentage
        #[arg(long)]
        take_profit: Option<f64>,

        /// Maximum positions
        #[arg(long)]
        max_positions: Option<usize>,
    },

    /// Reset configuration to defaults
    Reset {
        /// Force reset without confirmation
        #[arg(short, long)]
        force: bool,
    },

    /// Show configuration file location
    Path,

    /// Set logs directory
    SetLogs {
        /// Directory to set as logs directory
        directory: String,
    },

    /// Set default DEX (decentralized exchange)
    SetDex {
        /// DEX name (uniswap, sushiswap, etc.)
        name: String,

        /// DEX version (v2, v3)
        #[arg(long)]
        version: Option<String>,

        /// Network (ethereum, polygon, etc.)
        #[arg(long)]
        network: Option<String>,
    },
}

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum DatabaseParameter {
    /// Enable SQL query logging
    Logging,
}

pub async fn handle_config_command(command: ConfigCommands) -> Result<()> {
    match command {
        ConfigCommands::SetKey { service, key } => {
            let mut config = Config::load()?;
            config.set_api_key(&service, key)?;
            println!(
                "{} API key for {} updated successfully",
                "✓".green(),
                service.cyan()
            );
        }

        ConfigCommands::Show { show_secrets, json } => {
            let config = Config::load()?;

            if json {
                // Mask sensitive values unless show_secrets is true
                let mut config_display = config.clone();
                if !show_secrets {
                    config_display.api_keys.coingecko = config_display
                        .api_keys
                        .coingecko
                        .map(|_| "*****".to_string());
                    config_display.api_keys.cryptocompare = config_display
                        .api_keys
                        .cryptocompare
                        .map(|_| "*****".to_string());
                    config_display.api_keys.etherscan = config_display
                        .api_keys
                        .etherscan
                        .map(|_| "*****".to_string());
                    config_display.database.password = config_display
                        .database
                        .password
                        .map(|_| "*****".to_string());
                    // TODO: Mask DEX wallet keys if applicable
                }

                let json_output = serde_json::to_string_pretty(&config_display).map_err(|e| {
                    Error::Config(format!("Failed to serialize config to JSON: {}", e))
                })?;
                println!("{}", json_output);
                return Ok(());
            }

            // Standard text output
            println!("{}", "Honeybadger Configuration".bold().underline());
            println!();

            // API Keys
            println!("{}", "API Keys:".yellow().bold());
            println!(
                "  CoinGecko: {}",
                config
                    .api_keys
                    .coingecko
                    .as_deref()
                    .unwrap_or("Not set")
                    .cyan()
            );
            println!(
                "  CryptoCompare: {}",
                config
                    .api_keys
                    .cryptocompare
                    .as_deref()
                    .unwrap_or("Not set")
                    .cyan()
            );
            println!(
                "  Etherscan: {}",
                config
                    .api_keys
                    .etherscan
                    .as_deref()
                    .unwrap_or("Not set")
                    .cyan()
            );
            println!();

            // Trading Configuration
            println!("{}", "Trading Configuration:".yellow().bold());
            println!(
                "  Paper Trading: {}",
                if config.trading.paper_trading {
                    "Enabled".green()
                } else {
                    "Disabled".red()
                }
            );
            println!(
                "  Scan Interval: {}{}",
                config.data_collection.interval.to_string().cyan(),
                " seconds".dimmed()
            );
            println!(
                "  Max Position Size: {}{}",
                format!("${:.2}", config.trading.max_position_size).cyan(),
                " USD".dimmed()
            );
            println!(
                "  Max Total Exposure: {}{}",
                format!("${:.2}", config.trading.max_total_exposure).cyan(),
                " USD".dimmed()
            );
            println!();

            // Strategy Configuration
            println!("{}", "Strategy Configuration:".yellow().bold());
            println!("  Strategy Type: {}", config.trading.strategy.cyan());
            println!(
                "  Signal Threshold: {}",
                config.trading.threshold.to_string().cyan()
            );
            println!(
                "  Min Volume: {}{}",
                format!("${:.2}", config.trading.min_volume).cyan(),
                " USD".dimmed()
            );
            println!();

            // Risk Configuration
            println!("{}", "Risk Management:".yellow().bold());
            println!(
                "  Stop Loss: {}%",
                config.trading.stop_loss.to_string().cyan()
            );
            println!(
                "  Take Profit: {}%",
                config.trading.take_profit.to_string().cyan()
            );
            println!(
                "  Max Positions: {}",
                config.trading.max_positions.to_string().cyan()
            );
            println!();

            // API Configuration
            println!("{}", "API Configuration:".yellow().bold());
            println!("  CoinGecko URL: {}", config.api.coingecko_url.cyan());
            println!(
                "  Request Timeout: {}{}",
                config.api.request_timeout.to_string().cyan(),
                " seconds".dimmed()
            );
            println!(
                "  Max Retries: {}",
                config.api.max_retries.to_string().cyan()
            );
            println!();

            // Database Configuration (PostgreSQL)
            println!("{}", "Database Configuration:".yellow().bold());
            println!("  Host: {}", config.database.host.cyan());
            println!("  Port: {}", config.database.port.to_string().cyan());
            println!("  User: {}", config.database.user.cyan());
            println!("  Database: {}", config.database.dbname.cyan());
            println!(
                "  Pool Max Size: {}",
                config.database.pool_max_size.to_string().cyan()
            );
            println!(
                "  Query Logging: {}",
                if config.database.query_logging {
                    "Enabled".green()
                } else {
                    "Disabled".red()
                }
            );
            println!();

            // Data Collection
            println!("{}", "Data Collection:".yellow().bold());
            println!(
                "  Collection Interval: {}{}",
                config.data_collection.interval.to_string().cyan(),
                " seconds".dimmed()
            );
            println!(
                "  History Days: {}{}",
                config.data_collection.history_days.to_string().cyan(),
                " days".dimmed()
            );
            println!(
                "  Auto-start Collection: {}",
                if config.data_collection.auto_start {
                    "Enabled".green()
                } else {
                    "Disabled".red()
                }
            );
            println!();

            // DEX Configuration
            println!("{}", "DEX Configuration:".yellow().bold());
            println!("  Default DEX: {}", config.dex.name.cyan());
            println!(
                "  Version: {}",
                config.dex.version.as_deref().unwrap_or("Not set").cyan()
            );
            println!(
                "  Network: {}",
                config.dex.network.as_deref().unwrap_or("Not set").cyan()
            );
            println!();

            // Logs Configuration
            println!("{}", "Logs Configuration:".yellow().bold());
            println!("  Logs Directory: {}", config.logs.directory.cyan());
            println!();
        }

        ConfigCommands::Get { key } => {
            let config = Config::load()?;

            // Split the key into parts
            let parts: Vec<&str> = key.split('.').collect();

            if parts.len() < 1 || parts.len() > 3 {
                return Err(Error::Config(format!("Invalid configuration key: {}", key)));
            }

            let value = match parts[0] {
                "api_keys" => {
                    if parts.len() < 2 {
                        return Err(Error::Config(
                            "Please specify which API key (e.g., api_keys.coingecko)".to_string(),
                        ));
                    }

                    match parts[1] {
                        "coingecko" => config
                            .api_keys
                            .coingecko
                            .as_deref()
                            .unwrap_or("Not set")
                            .to_string(),
                        "cryptocompare" => config
                            .api_keys
                            .cryptocompare
                            .as_deref()
                            .unwrap_or("Not set")
                            .to_string(),
                        "etherscan" => config
                            .api_keys
                            .etherscan
                            .as_deref()
                            .unwrap_or("Not set")
                            .to_string(),
                        _ => return Err(Error::Config(format!("Unknown API key: {}", parts[1]))),
                    }
                }
                "trading" => {
                    if parts.len() < 2 {
                        return Err(Error::Config(
                            "Please specify which trading parameter (e.g., trading.paper_trading)"
                                .to_string(),
                        ));
                    }

                    match parts[1] {
                        "paper_trading" => config.trading.paper_trading.to_string(),
                        "scan_interval" => config.data_collection.interval.to_string(),
                        "max_position_size" => config.trading.max_position_size.to_string(),
                        "max_total_exposure" => config.trading.max_total_exposure.to_string(),
                        "strategy" => config.trading.strategy.clone(),
                        "threshold" => config.trading.threshold.to_string(),
                        "min_volume" => config.trading.min_volume.to_string(),
                        "stop_loss" => config.trading.stop_loss.to_string(),
                        "take_profit" => config.trading.take_profit.to_string(),
                        "max_positions" => config.trading.max_positions.to_string(),
                        _ => {
                            return Err(Error::Config(format!(
                                "Unknown trading parameter: {}",
                                parts[1]
                            )))
                        }
                    }
                }
                "api" => {
                    if parts.len() < 2 {
                        return Err(Error::Config(
                            "Please specify which API parameter (e.g., api.coingecko_url)"
                                .to_string(),
                        ));
                    }

                    match parts[1] {
                        "coingecko_url" => config.api.coingecko_url.clone(),
                        "request_timeout" => config.api.request_timeout.to_string(),
                        "max_retries" => config.api.max_retries.to_string(),
                        _ => {
                            return Err(Error::Config(format!(
                                "Unknown API parameter: {}",
                                parts[1]
                            )))
                        }
                    }
                }
                "data_collection" => {
                    if parts.len() < 2 {
                        return Err(Error::Config("Please specify which data collection parameter (e.g., data_collection.interval)".to_string()));
                    }

                    match parts[1] {
                        "interval" => config.data_collection.interval.to_string(),
                        "history_days" => config.data_collection.history_days.to_string(),
                        "auto_start" => config.data_collection.auto_start.to_string(),
                        _ => {
                            return Err(Error::Config(format!(
                                "Unknown data collection parameter: {}",
                                parts[1]
                            )))
                        }
                    }
                }
                "dex" => {
                    if parts.len() < 2 {
                        return Err(Error::Config(
                            "Please specify which DEX parameter (e.g., dex.name)".to_string(),
                        ));
                    }

                    match parts[1] {
                        "name" => config.dex.name.clone(),
                        "version" => config
                            .dex
                            .version
                            .as_deref()
                            .unwrap_or("Not set")
                            .to_string(),
                        "network" => config
                            .dex
                            .network
                            .as_deref()
                            .unwrap_or("Not set")
                            .to_string(),
                        _ => {
                            return Err(Error::Config(format!(
                                "Unknown DEX parameter: {}",
                                parts[1]
                            )))
                        }
                    }
                }
                _ => {
                    return Err(Error::Config(format!(
                        "Unknown configuration section: {}",
                        parts[0]
                    )))
                }
            };

            println!("{}: {}", key.cyan(), value);
        }

        ConfigCommands::Set {
            key,
            value: value_str,
        } => {
            let mut config = Config::load()?;

            // Split the key into parts
            let parts: Vec<&str> = key.split('.').collect();

            if parts.len() < 1 || parts.len() > 3 {
                return Err(Error::Config(format!("Invalid configuration key: {}", key)));
            }

            match parts[0] {
                "api_keys" => {
                    if parts.len() < 2 {
                        return Err(Error::Config(
                            "Please specify which API key (e.g., api_keys.coingecko)".to_string(),
                        ));
                    }

                    match parts[1] {
                        "coingecko" => config.api_keys.coingecko = Some(value_str.clone()),
                        "cryptocompare" => config.api_keys.cryptocompare = Some(value_str.clone()),
                        "etherscan" => config.api_keys.etherscan = Some(value_str.clone()),
                        _ => return Err(Error::Config(format!("Unknown API key: {}", parts[1]))),
                    }
                }
                "trading" => {
                    if parts.len() < 2 {
                        return Err(Error::Config(
                            "Please specify which trading parameter (e.g., trading.paper_trading)"
                                .to_string(),
                        ));
                    }

                    match parts[1] {
                        "paper_trading" => {
                            config.trading.paper_trading = value_str.parse().map_err(|_| {
                                Error::Config(format!("Invalid boolean value: {}", value_str))
                            })?;
                        }
                        "scan_interval" => {
                            config.data_collection.interval = value_str.parse().map_err(|_| {
                                Error::Config(format!("Invalid integer value: {}", value_str))
                            })?;
                        }
                        "max_position_size" => {
                            config.trading.max_position_size = value_str.parse().map_err(|_| {
                                Error::Config(format!("Invalid float value: {}", value_str))
                            })?;
                        }
                        "max_total_exposure" => {
                            config.trading.max_total_exposure =
                                value_str.parse().map_err(|_| {
                                    Error::Config(format!("Invalid float value: {}", value_str))
                                })?;
                        }
                        "strategy" => {
                            config.trading.strategy = value_str.clone();
                        }
                        "threshold" => {
                            config.trading.threshold = value_str.parse().map_err(|_| {
                                Error::Config(format!("Invalid float value: {}", value_str))
                            })?;
                        }
                        "min_volume" => {
                            config.trading.min_volume = value_str.parse().map_err(|_| {
                                Error::Config(format!("Invalid float value: {}", value_str))
                            })?;
                        }
                        "stop_loss" => {
                            config.trading.stop_loss = value_str.parse().map_err(|_| {
                                Error::Config(format!("Invalid float value: {}", value_str))
                            })?;
                        }
                        "take_profit" => {
                            config.trading.take_profit = value_str.parse().map_err(|_| {
                                Error::Config(format!("Invalid float value: {}", value_str))
                            })?;
                        }
                        "max_positions" => {
                            config.trading.max_positions = value_str.parse().map_err(|_| {
                                Error::Config(format!("Invalid integer value: {}", value_str))
                            })?;
                        }
                        _ => {
                            return Err(Error::Config(format!(
                                "Unknown trading parameter: {}",
                                parts[1]
                            )))
                        }
                    }
                }
                "api" => {
                    if parts.len() < 2 {
                        return Err(Error::Config(
                            "Please specify which API parameter (e.g., api.coingecko_url)"
                                .to_string(),
                        ));
                    }

                    match parts[1] {
                        "coingecko_url" => config.api.coingecko_url = value_str.clone(),
                        "request_timeout" => {
                            config.api.request_timeout = value_str.parse().map_err(|_| {
                                Error::Config(format!("Invalid integer value: {}", value_str))
                            })?;
                        }
                        "max_retries" => {
                            config.api.max_retries = value_str.parse().map_err(|_| {
                                Error::Config(format!("Invalid integer value: {}", value_str))
                            })?;
                        }
                        _ => {
                            return Err(Error::Config(format!(
                                "Unknown API parameter: {}",
                                parts[1]
                            )))
                        }
                    }
                }
                "data_collection" => {
                    if parts.len() < 2 {
                        return Err(Error::Config("Please specify which data collection parameter (e.g., data_collection.interval)".to_string()));
                    }

                    match parts[1] {
                        "interval" => {
                            config.data_collection.interval = value_str.parse().map_err(|_| {
                                Error::Config(format!("Invalid integer value: {}", value_str))
                            })?;
                        }
                        "history_days" => {
                            config.data_collection.history_days =
                                value_str.parse().map_err(|_| {
                                    Error::Config(format!("Invalid integer value: {}", value_str))
                                })?;
                        }
                        "auto_start" => {
                            config.data_collection.auto_start =
                                value_str.parse().map_err(|_| {
                                    Error::Config(format!("Invalid boolean value: {}", value_str))
                                })?;
                        }
                        _ => {
                            return Err(Error::Config(format!(
                                "Unknown data collection parameter: {}",
                                parts[1]
                            )))
                        }
                    }
                }
                "dex" => {
                    if parts.len() < 2 {
                        return Err(Error::Config(
                            "Please specify which DEX parameter (e.g., dex.name)".to_string(),
                        ));
                    }

                    match parts[1] {
                        "name" => config.dex.name = value_str.clone(),
                        "version" => config.dex.version = Some(value_str.clone()),
                        "network" => config.dex.network = Some(value_str.clone()),
                        _ => {
                            return Err(Error::Config(format!(
                                "Unknown DEX parameter: {}",
                                parts[1]
                            )))
                        }
                    }
                }
                _ => {
                    return Err(Error::Config(format!(
                        "Unknown configuration section: {}",
                        parts[0]
                    )))
                }
            }

            // Save the configuration
            config.save()?;

            println!(
                "{} Configuration value {} updated to: {}",
                "✓".green(),
                key.cyan(),
                value_str
            );
        }

        ConfigCommands::SetDatabase { parameter, value } => {
            let mut config = Config::load()?;

            match parameter {
                DatabaseParameter::Logging => {
                    config.database.query_logging = value
                        .parse()
                        .map_err(|_| Error::Config(format!("Invalid boolean value: {}", value)))?;
                }
            }

            config.save()?;
            println!(
                "✅ Database configuration updated: {:?} = {}",
                parameter, value
            );
        }

        ConfigCommands::SetTrading {
            paper,
            scan_interval,
            max_position,
            max_exposure,
        } => {
            let mut config = Config::load()?;
            let mut updated = false;

            if paper {
                config.trading.paper_trading = true;
                println!(
                    "{} Paper trading set to: {}",
                    "✓".green(),
                    "Enabled".green()
                );
                updated = true;
            }

            if let Some(interval) = scan_interval {
                config.data_collection.interval = interval;
                println!(
                    "{} Scan interval set to: {}{}",
                    "✓".green(),
                    interval.to_string().cyan(),
                    " seconds".dimmed()
                );
                updated = true;
            }

            if let Some(max_position) = max_position {
                config.trading.max_position_size = max_position;
                println!(
                    "{} Max position size set to: {}{}",
                    "✓".green(),
                    format!("${:.2}", max_position).cyan(),
                    " USD".dimmed()
                );
                updated = true;
            }

            if let Some(max_exposure) = max_exposure {
                config.trading.max_total_exposure = max_exposure;
                println!(
                    "{} Max total exposure set to: {}{}",
                    "✓".green(),
                    format!("${:.2}", max_exposure).cyan(),
                    " USD".dimmed()
                );
                updated = true;
            }

            if !updated {
                println!("{} No changes specified", "!".yellow());
                return Ok(());
            }

            // Save the configuration
            config.save()?;
        }

        ConfigCommands::SetStrategy {
            strategy_type,
            threshold,
            min_volume,
        } => {
            let mut config = Config::load()?;
            let mut updated = false;

            if let Some(strategy_type) = strategy_type {
                config.trading.strategy = strategy_type.clone();
                println!(
                    "{} Strategy type set to: {}",
                    "✓".green(),
                    strategy_type.cyan()
                );
                updated = true;
            }

            if let Some(threshold) = threshold {
                config.trading.threshold = threshold;
                println!(
                    "{} Signal threshold set to: {}",
                    "✓".green(),
                    threshold.to_string().cyan()
                );
                updated = true;
            }

            if let Some(min_volume) = min_volume {
                config.trading.min_volume = min_volume;
                println!(
                    "{} Minimum volume set to: {}{}",
                    "✓".green(),
                    format!("${:.2}", min_volume).cyan(),
                    " USD".dimmed()
                );
                updated = true;
            }

            if !updated {
                println!("{} No changes specified", "!".yellow());
                return Ok(());
            }

            // Save the configuration
            config.save()?;
        }

        ConfigCommands::SetRisk {
            stop_loss,
            take_profit,
            max_positions,
        } => {
            let mut config = Config::load()?;
            let mut updated = false;

            if let Some(stop_loss) = stop_loss {
                config.trading.stop_loss = stop_loss;
                println!(
                    "{} Stop loss set to: {}%",
                    "✓".green(),
                    stop_loss.to_string().cyan()
                );
                updated = true;
            }

            if let Some(take_profit) = take_profit {
                config.trading.take_profit = take_profit;
                println!(
                    "{} Take profit set to: {}%",
                    "✓".green(),
                    take_profit.to_string().cyan()
                );
                updated = true;
            }

            if let Some(max_positions) = max_positions {
                config.trading.max_positions = max_positions;
                println!(
                    "{} Max positions set to: {}",
                    "✓".green(),
                    max_positions.to_string().cyan()
                );
                updated = true;
            }

            if !updated {
                println!("{} No changes specified", "!".yellow());
                return Ok(());
            }

            // Save the configuration
            config.save()?;
        }

        ConfigCommands::Reset { force } => {
            if !force {
                println!(
                    "{} This will reset all configuration to defaults.",
                    "Warning:".bright_yellow()
                );
                println!("Run with --force to confirm this action.");
                return Ok(());
            }

            // Create a new default configuration
            let config = Config::default();

            // Save it
            config.save()?;

            println!("{} Configuration reset to defaults", "✓".green());
        }

        ConfigCommands::Path => {
            let path = crate::config::get_config_path()?;
            println!(
                "Configuration file location: {}",
                path.to_string_lossy().cyan()
            );
        }

        ConfigCommands::SetLogs { directory } => {
            let mut config = Config::load()?;

            // Ensure the directory exists
            let path = std::path::Path::new(&directory);
            if !path.exists() {
                println!(
                    "{} Directory does not exist, creating: {}",
                    "!".yellow(),
                    directory
                );
                std::fs::create_dir_all(path)
                    .map_err(|e| Error::Config(format!("Failed to create directory: {}", e)))?;
            }

            config.logs.directory = directory.clone();
            config.save()?;

            println!("{} Log directory set to: {}", "✓".green(), directory.cyan());
        }

        ConfigCommands::SetDex {
            name,
            version,
            network,
        } => {
            let mut config = Config::load()?;

            config.dex.name = name.clone();
            config.dex.version = version.clone();
            config.dex.network = network.clone();

            config.save()?;

            println!("{} Default DEX set to: {}", "✓".green(), name.cyan());
            if let Some(version) = version {
                println!("  Version: {}", version.cyan());
            }
            if let Some(network) = network {
                println!("  Network: {}", network.cyan());
            }
        }
    }

    Ok(())
}
