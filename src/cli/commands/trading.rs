use crate::core::config::{self, Config};
use crate::core::error::Error;
use crate::core::models::market::TokenMetrics;
use crate::core::models::token::TokenData;
use crate::domain::trading::execution::bot::TradingBotSystem;
use crate::domain::trading::indicators::IndicatorWeights;
use crate::domain::trading::strategy::MomentumStrategy;
use crate::infra::actors::MessageBus;
use crate::infra::db::repositories::PositionRepository;
use crate::infra::db::repositories::TokenRepository;
use crate::infra::db::repositories::TradeRepository;
use crate::infra::db::Database;
use chrono::{self, Utc};
use clap::{ArgAction, Subcommand};
use colored::*;
use directories::ProjectDirs;
use dirs;
use env_logger;
use log::{debug, error, info, warn, LevelFilter};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use tabled::{settings::Style, Table};

// Constants
const APP_NAME: &str = "honeybadger";

// Define a struct for checkpoint data near the top of the file
#[derive(Debug, Clone)]
struct CheckpointData {
    status: serde_json::Value,
    metrics: serde_json::Value,
    timestamp: String,
}

#[derive(Subcommand, Debug)]
pub enum TradingArgs {
    /// Start the trading bot
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
        /// Confidence threshold for strategy signals (0.0-1.0)
        #[arg(long, default_value_t = 5.0)]
        confidence_threshold: f64,
        /// Minimum 24h volume in USD
        #[arg(long, default_value_t = 100000.0)]
        min_volume: f64,
        /// Maximum loss per position (%)
        #[arg(long, default_value_t = 5.0)]
        stop_loss: f64,
        /// Run in paper trading mode (no real trades)
        #[arg(long, action = ArgAction::SetTrue)]
        paper: bool,
        /// Run in testnet mode (real trades but on a test network)
        #[arg(long, action = ArgAction::SetTrue)]
        testnet: bool,
        /// Market scan interval in seconds
        #[arg(short, long, default_value_t = 60)]
        interval: u64,
        /// Run in the background (daemon mode)
        #[arg(short, long, action = ArgAction::SetTrue)]
        background: bool,
        /// Enable wide scan mode to process all available tokens
        #[arg(long, action = ArgAction::SetTrue)]
        wide_scan: bool,
        /// Minimum data points required for analysis
        #[arg(short = 'p', long, default_value_t = 7)]
        min_data_points: u32,
        /// Risk tolerance level (0-5): 0=Conservative, 1=Conservative-Moderate, 2=Moderate, 3=Moderate-Aggressive, 4=Aggressive, 5=Very Aggressive
        #[arg(short = 'r', long, default_value_t = 0)]
        risk_tolerance: u8,
        /// RSI indicator weight (0-1)
        #[arg(long, default_value_t = 0.3)]
        rsi_weight: f64,
        /// MACD indicator weight (0-1)
        #[arg(long, default_value_t = 0.3)]
        macd_weight: f64,
        /// Bollinger Bands indicator weight (0-1)
        #[arg(long, default_value_t = 0.2)]
        bollinger_weight: f64,
        /// Volume trend indicator weight (0-1)
        #[arg(long, default_value_t = 0.2)]
        volume_weight: f64,
        /// Testing mode for faster signal generation (production, fast, ultra, mock)
        #[arg(long, default_value = "production")]
        testing_mode: String,
    },
    /// Show current bot status and positions
    Status,
    /// Get health report from the supervisor
    Health,
    /// Restart a specific actor
    Restart {
        /// Actor ID to restart (market, strategy, risk, execution, database)
        actor_id: String,
    },
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
    /// Manually close an open position
    Close {
        /// Token ID of the position to close
        #[arg(short, long)]
        token: String,
        /// Exit price for the position
        #[arg(short, long)]
        price: f64,
        /// Close a paper trading position (default) or live position
        #[arg(long, action = ArgAction::SetTrue)]
        paper: bool,
        /// Close a live trading position
        #[arg(long, action = ArgAction::SetTrue)]
        live: bool,
    },
    /// Analyze latest market data for trading signals
    Analyze {
        /// Trading strategy to use (e.g., momentum)
        #[arg(short, long, default_value = "momentum")]
        strategy: String,
        /// Confidence threshold for strategy signals (0.0-1.0)
        #[arg(long, default_value_t = 5.0)]
        confidence_threshold: f64,
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
        /// Testing mode for faster signal generation (production, fast, ultra, mock)
        #[arg(long, default_value = "production")]
        testing_mode: String,
    },
}

/// Handle trading commands - this version is only called for real-time commands
pub async fn handle_trading_command(
    cmd: TradingArgs,
    config: Config,
    db: Database,
    message_bus: Arc<MessageBus>,
) -> Result<(), Error> {
    match cmd {
        TradingArgs::Start { .. } => {
            // Call the start_trading function with the command, config, db, and message_bus
            start_trading(cmd, config, db, message_bus).await
        }
        TradingArgs::Status => get_trading_status(message_bus, db).await,
        TradingArgs::Health => get_health_report(message_bus, db).await,
        TradingArgs::Restart { actor_id } => restart_actor(message_bus, &actor_id, db).await,
        TradingArgs::Stop => stop_trading(message_bus, db).await,
        TradingArgs::Analyze {
            strategy,
            confidence_threshold,
            min_volume,
            stop_loss,
            debug,
            risk_tolerance,
            testing_mode,
        } => {
            // Create a proper call to the analyze function here
            println!("Analyzing tokens with strategy: {}", strategy);

            if debug {
                setup_logging(LevelFilter::Debug)?;
            } else {
                setup_logging(LevelFilter::Info)?;
            }

            // Create strategy
            let strategy_obj = create_strategy(
                &strategy,
                confidence_threshold,
                min_volume,
                stop_loss,
                Some(7), // min_data_points
                Some(risk_tolerance),
                None, // indicator_weights
                Some(&testing_mode),
            );

            println!("Created strategy: {}", strategy_obj);
            println!("This command is meant to be run directly, not via the message bus.");
            Err(Error::InvalidInput(
                "This command should be handled directly, not via message bus".to_string(),
            ))
        }
        TradingArgs::History { .. } | TradingArgs::Positions { .. } | TradingArgs::Close { .. } => {
            Err(Error::InvalidInput(
                "This command should be handled directly without MessageBus".to_string(),
            ))
        }
    }
}

/// Display trading history - direct database access, no MessageBus required
pub async fn display_trading_history(
    db: &Database,
    is_paper: bool,
    limit: usize,
) -> Result<(), Error> {
    let mode = if is_paper {
        "Paper Trading".bright_yellow()
    } else {
        "Live Trading".bright_red()
    };
    println!("\n📊 {} Raw Trade History:", mode);

    let trade_repo = TradeRepository::new(db.clone(), is_paper);

    // Get recent raw trades
    let history = trade_repo.get_trading_history(limit).await?;

    if history.is_empty() {
        println!("\nNo trading history found.");
        return Ok(());
    }

    // Removed performance stats display

    println!(
        "\n{:<10} {:<15} {:<6} {:<15} {:<15} {:<25} {:<8}",
        "Trade ID", "Token", "Side", "Price", "Size", "Timestamp", "Pos ID"
    );
    println!("{}", "-".repeat(100)); // Adjusted width

    for trade in &history {
        let side_str = if trade.is_buy {
            "BUY".bright_green()
        } else {
            "SELL".bright_red()
        };
        let pos_id_str = trade
            .position_id
            .map_or_else(|| "N/A".dimmed().to_string(), |id| id.to_string());

        println!(
            "{:<10} {:<15} {:<18} ${:<14.4} {:<15.4} {:<25} {:<8}", // Adjusted widths and side color escape codes
            trade.id,
            trade.token_id,
            side_str,
            trade.price,
            trade.size,
            trade.timestamp.format("%Y-%m-%d %H:%M:%S"),
            pos_id_str
        );
    }
    println!("\nDisplaying last {} trades.", history.len());

    Ok(())
}

/// Helper to format P/L with color
fn format_pnl(pnl: f64) -> String {
    // This function is no longer used by display_trading_history
    let pnl_str = format!("${:.2}", pnl.abs());
    if pnl > 0.0 {
        pnl_str.green().to_string()
    } else if pnl < 0.0 {
        format!("-{}", pnl_str).red().to_string()
    } else {
        "$0.00".dimmed().to_string()
    }
}

/// Display open positions - direct database access, no MessageBus required
pub async fn display_open_positions(db: &Database, is_paper: bool) -> Result<(), Error> {
    let mode = if is_paper {
        "Paper Trading".bright_yellow()
    } else {
        "Live Trading".bright_red()
    };
    println!("\n{} Positions:", mode);

    // Create position repository
    let position_repo = crate::infra::db::repositories::PositionRepository::new(
        db.clone(), // Use the cloned Database which holds Arc<Pool>
        is_paper,
    );

    // Use await for async repository method
    let positions = position_repo.get_open_positions().await?;

    if positions.is_empty() {
        println!("\nNo open positions found.");
        return Ok(());
    }

    // Print table headers
    println!(
        "{:<20} {:<15} {:<15} {:<15} {:<20} {:<20}",
        "Symbol", "Entry Price", "Current Price", "Size", "Unrealized P/L", "Entry Time"
    );

    // Print each position
    for position in &positions {
        let unrealized_pnl_str = if position.unrealized_pnl.abs() < 0.000001 {
            String::from("-")
        } else {
            format!("{:.4}", position.unrealized_pnl)
        };

        println!(
            "{:<20} ${:<14.4} ${:<14.4} {:<15.4} {:<20} {}",
            position.token_id,
            position.entry_price,
            position.current_price,
            position.size,
            unrealized_pnl_str,
            position.entry_time.to_rfc3339(),
        );
    }

    println!("\nTotal: {} positions", positions.len());

    Ok(())
}

/// Analyze market data - direct database access, no MessageBus required
pub async fn analyze_market_data(
    db: &Database,
    strategy: String,
    confidence_threshold: f64,
    min_volume: f64,
    stop_loss: f64,
    debug: bool,
    risk_tolerance: u8,
) -> Result<(), Error> {
    // Initialize the token repository
    let token_repo = TokenRepository::new(db.clone(), true);

    // Fetch latest market data for all tokens
    info!("Analyzing market data with {} strategy", strategy);
    let market_data = token_repo.get_latest_market_data().await?;

    if market_data.is_empty() {
        println!("No market data found in database. Please run data collection first.");
        return Ok(());
    }

    // Create a strategy instance with default weights for analysis
    let indicator_weights = IndicatorWeights::default();
    let strategy_instance = create_strategy(
        &strategy,
        confidence_threshold,
        min_volume,
        stop_loss,
        None,
        Some(risk_tolerance),
        Some(indicator_weights),
        None, // No testing_mode parameter for backward compatibility
    );

    // Process each token and identify signals
    let mut buy_signals = Vec::new();
    let mut processed = 0;
    let mut skipped = 0;

    println!("\n📊 Analyzing {} tokens with {} strategy (confidence threshold: {:.1}%, min volume: ${:.2}M)", 
             market_data.len(), strategy, confidence_threshold * 100.0, min_volume / 1_000_000.0);

    // Add header for verbose output
    if debug {
        println!(
            "\n{:<10} {:<8} {:<12} {:<12} {:<10} {:<10}",
            "SYMBOL", "PRICE", "VOLUME (24h)", "CHANGE (24h)", "SIGNAL", "DETAILS"
        );
        println!("{}", "─".repeat(80));
    }

    // Process each token
    for token in market_data.iter() {
        // Convert TokenData to TokenMetrics for analysis
        let metrics = TokenMetrics {
            id: token.id.clone(),
            symbol: token.symbol.clone(),
            name: token.name.clone(),
            price_usd: token.price_usd,
            price_change_24h: token.price_change_24h,
            volume_24h: token.volume_24h,
            market_cap: token.market_cap.unwrap_or(0.0),
            market_cap_rank: token.market_cap_rank.map(|r| r as usize),
            latest_news: token
                .latest_news
                .as_ref()
                .map(|title| crate::types::news::NewsItem {
                    title: title.clone(),
                    url: String::new(),
                    source: "Unknown".to_string(),
                    published_at: Utc::now(),
                    categories: Vec::new(),
                }),
            chain: Some(token.chain.clone()),
            last_updated: token.last_updated.unwrap_or_else(Utc::now),
        };

        // Skip tokens that don't meet minimum volume requirement
        if metrics.volume_24h < min_volume {
            skipped += 1;
            if debug {
                println!(
                    "{:<10} ${:<7.4} ${:<11.2}M {:<12.2}% {:<10} Volume too low",
                    metrics.symbol,
                    metrics.price_usd,
                    metrics.volume_24h / 1_000_000.0,
                    metrics.price_change_24h,
                    "SKIPPED"
                );
            }
            continue;
        }

        // Get historical price data for advanced analysis
        let history_result = token_repo.get_token_history(&token.id, 30).await;
        let history = match history_result {
            Ok(Some((_, history))) => history,
            _ => {
                // If no history, use debug mode to indicate why skipped
                if debug {
                    println!(
                        "{:<10} ${:<7.4} ${:<11.2}M {:<12.2}% {:<10} No price history",
                        metrics.symbol,
                        metrics.price_usd,
                        metrics.volume_24h / 1_000_000.0,
                        metrics.price_change_24h,
                        "SKIPPED"
                    );
                }
                skipped += 1;
                continue;
            }
        };

        // Update strategy with historical data
        let mut strategy_copy = strategy_instance.clone();
        for (price, volume, timestamp) in &history {
            let historical_metrics = crate::types::market::TokenMetrics {
                id: token.id.clone(),
                symbol: token.symbol.clone(),
                name: token.name.clone(),
                price_usd: *price,
                price_change_24h: 0.0, // Not relevant for historical update
                volume_24h: *volume,
                market_cap: token.market_cap.unwrap_or(0.0),
                market_cap_rank: token.market_cap_rank.map(|r| r as usize),
                latest_news: None,
                chain: Some(token.chain.clone()),
                last_updated: *timestamp,
            };
            strategy_copy.update_market_data(&historical_metrics);
        }

        // Analyze the token
        let signal = strategy_copy.analyze(&metrics);
        processed += 1;

        // Display results
        if debug || signal != crate::domain::trading::strategy::Signal::Hold {
            let signal_str = match signal {
                crate::domain::trading::strategy::Signal::Buy => "BUY".green(),
                crate::domain::trading::strategy::Signal::Sell => "SELL".red(),
                crate::domain::trading::strategy::Signal::StrongBuy => "STRONG BUY".bright_green(),
                crate::domain::trading::strategy::Signal::StrongSell => "STRONG SELL".bright_red(),
                crate::domain::trading::strategy::Signal::Hold => "HOLD".yellow(),
            };

            println!(
                "{:<10} ${:<7.4} ${:<11.2}M {:<12.2}% {:<10}",
                metrics.symbol,
                metrics.price_usd,
                metrics.volume_24h / 1_000_000.0,
                metrics.price_change_24h,
                signal_str
            );

            // Store buy signals for summary
            if signal == crate::domain::trading::strategy::Signal::Buy {
                buy_signals.push(format!("{} (${:.4})", metrics.symbol, metrics.price_usd));
            }
        }
    }

    // Display summary
    println!("\n✅ Analysis complete: Processed {} tokens ({} skipped due to low volume or insufficient data)", 
           processed, skipped);

    if buy_signals.is_empty() {
        println!("No BUY signals detected with current parameters.");
    } else {
        println!(
            "\n🔔 BUY signals detected for {} tokens:",
            buy_signals.len()
        );
        for signal in buy_signals {
            println!("  • {}", signal);
        }
        println!("\nRun trading bot with same parameters to automatically trade these signals.");
    }

    Ok(())
}

/// Helper function to set up logging with the given level
fn setup_logging(level: LevelFilter) -> Result<(), Error> {
    // Create a new logger builder
    let mut builder = env_logger::Builder::new();

    // Set the log level
    builder.filter_level(level);

    // Initialize the logger
    builder.init();

    Ok(())
}

/// Get the application data directory
fn get_data_dir() -> Result<PathBuf, Error> {
    let project_dirs = ProjectDirs::from("com", "honeybadger", "honeybadger").ok_or_else(|| {
        Error::Config("Could not determine application data directory".to_string())
    })?;

    let data_dir = project_dirs.data_dir().to_path_buf();

    // Create the directory if it doesn't exist
    if !data_dir.exists() {
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| Error::Io(format!("Failed to create data directory: {}", e)))?;
    }

    Ok(data_dir)
}

/// Start the trading bot with the given strategy and parameters
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
        testnet,
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
        info!("Starting trading bot with {} strategy", strategy);

        // Create parameters object
        let params = serde_json::json!({
            "threshold": confidence_threshold,
            "min_volume": min_volume,
            "stop_loss": stop_loss,
            "min_data_points": min_data_points,
            "risk_tolerance": risk_tolerance,
            "testing_mode": testing_mode,
            "indicator_weights": {
                "rsi": rsi_weight,
                "macd": macd_weight,
                "bollinger_bands": bollinger_weight,
                "volume": volume_weight,
            }
        });

        // Save the trading state to a file
        let config_dir = dirs::config_dir()
            .ok_or_else(|| {
                Error::Config("Could not determine configuration directory".to_string())
            })?
            .join(APP_NAME);

        // Create directory if it doesn't exist
        if !config_dir.exists() {
            std::fs::create_dir_all(&config_dir)
                .map_err(|e| Error::Config(format!("Failed to create config directory: {}", e)))?;
        }

        let state_file = config_dir.join("trading_state.json");
        std::fs::write(&state_file, params.to_string())
            .map_err(|e| Error::Io(format!("Failed to write trading state file: {}", e)))?;

        // Update config with command line parameters
        let mut updated_config = config.clone();

        // Direct assignment since these are not Options
        updated_config.trading.threshold = confidence_threshold;
        updated_config.trading.min_volume = min_volume;
        updated_config.trading.max_position_size = max_position;
        updated_config.data_collection.interval = interval;
        updated_config.trading.stop_loss = stop_loss;
        updated_config.trading.wide_scan_mode = wide_scan;

        // Use paper trading if paper is specified
        if testnet {
            updated_config.dex.testnet = true;
            info!("Running in TESTNET mode - real transactions on test network");
        } else if paper {
            updated_config.trading.paper_trading = true;
            info!("📝 Running in paper trading mode - no real transactions");
        } else {
            info!("🔴 Running in LIVE trading mode - REAL trades will be executed!");
        }

        // Log wide scan mode
        if wide_scan {
            info!("🔍 Wide scan mode ENABLED - will process all available tokens");
        } else {
            info!("🔍 Wide scan mode DISABLED - will only process tracked tokens");
        }

        // Create and start the actor-based trading system, passing db
        let mut bot_system = TradingBotSystem::new(db, updated_config);
        bot_system = bot_system.with_message_bus(message_bus);

        // Create the strategy object
        let strategy = create_strategy(
            &strategy,
            confidence_threshold,
            min_volume,
            stop_loss,
            Some(min_data_points as usize),
            Some(risk_tolerance),
            Some(IndicatorWeights::default()),
            Some(&testing_mode),
        );

        match bot_system.start(strategy.name(), &params).await {
            Ok(_) => {
                println!(
                    "🚀 Trading bot started successfully with {} strategy",
                    strategy.name().bright_green()
                );
                println!("Using min_data_points: {}", min_data_points);
                println!("Indicator Weights:");
                println!("  RSI: {:.1}%", rsi_weight * 100.0);
                println!("  MACD: {:.1}%", macd_weight * 100.0);
                println!("  Bollinger Bands: {:.1}%", bollinger_weight * 100.0);
                println!("  Volume: {:.1}%", volume_weight * 100.0);

                println!(
                    "Wide scan mode: {}",
                    if wide_scan {
                        "ENABLED (all available tokens)".bright_green()
                    } else {
                        "DISABLED (tracked tokens only)".bright_yellow()
                    }
                );

                let risk_level_desc = match risk_tolerance {
                    0 => "Conservative (standard analysis)".bright_blue(),
                    1 => "Conservative-Moderate (some flexibility)".bright_green(),
                    2 => "Moderate (more signals)".bright_yellow(),
                    3 => "Moderate-Aggressive (more signals)".bright_yellow(),
                    4 => "Aggressive (maximum signals)".bright_red(),
                    5 => "Very Aggressive (maximum signals)".bright_red(),
                    _ => "Unknown".bright_white(),
                };

                println!(
                    "Risk tolerance level: {} - {}",
                    risk_tolerance, risk_level_desc
                );

                println!(
                    "Trading bot is now running in the {}. Press Ctrl+C to stop.",
                    if background {
                        "background".bright_blue()
                    } else {
                        "foreground".bright_green()
                    }
                );

                if background {
                    println!("Bot is running in {} mode", "background".bright_blue());
                    println!("Monitor with: honeybadger trading status");
                    println!("Stop with: honeybadger trading stop");
                    // Don't wait - return immediately so the command completes
                    Ok(())
                } else {
                    println!(
                        "Bot is running in {} mode (press Ctrl+C to stop)",
                        "foreground".bright_blue()
                    );
                    println!(
                        "In another terminal, you can monitor with: honeybadger trading status"
                    );
                    println!("Or stop with: honeybadger trading stop");

                    // Keep the bot running in the foreground until stopped
                    bot_system.run_foreground(&state_file).await
                }
            }
            Err(e) => {
                // Clean up state file if startup fails
                let _ = std::fs::remove_file(&state_file);
                error!("Failed to start trading bot: {}", e);
                Err(e)
            }
        }
    } else {
        Err(Error::InvalidInput(
            "Invalid command for starting trading bot".to_string(),
        ))
    }
}

// Add this function to clean up the trading state file
fn cleanup_trading_state() -> Result<(), Error> {
    // Check common locations where the state file might be
    let possible_dirs = vec![
        dirs::config_dir().map(|d| d.join(APP_NAME)),
        dirs::data_dir().map(|d| d.join(APP_NAME)),
        dirs::home_dir().map(|d| d.join(".config").join(APP_NAME)),
        Some(std::path::PathBuf::from("./")),
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
pub async fn stop_trading(message_bus: Arc<MessageBus>, db: Database) -> Result<(), Error> {
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
    let _state: serde_json::Value = match serde_json::from_str(&state_str) {
        Ok(s) => s,
        Err(e) => {
            error!("Error parsing trading state file: {}", e);
            // Delete the state file and return
            let _ = std::fs::remove_file(&state_file);
            println!("Trading bot state file was corrupted and has been removed");
            return Ok(());
        }
    };

    // Create the actor-based trading system with the shared message bus, passing db
    let mut bot_system = TradingBotSystem::new(db, config);
    bot_system = bot_system.with_message_bus(message_bus);

    // Stop the trading bot
    match bot_system.stop().await {
        Ok(_) => {
            println!("Trading bot stopped successfully");
            // Delete the state file
            if let Err(e) = std::fs::remove_file(&state_file) {
                error!("Error removing trading state file: {}", e);
            }
            Ok(())
        }
        Err(e) => {
            error!("Failed to stop trading bot: {}", e);
            // Delete the state file anyway, as it's likely in a bad state
            let _ = std::fs::remove_file(&state_file);
            Err(e)
        }
    }
}

/// Get the status of the trading bot
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

/// Restart a specific actor
pub async fn restart_actor(
    message_bus: Arc<MessageBus>,
    actor_id: &str,
    db: Database,
) -> Result<(), Error> {
    info!("Restarting actor: {}", actor_id);

    // Validate actor ID
    let valid_actors = vec!["market", "strategy", "risk", "execution", "database"];
    if !valid_actors.contains(&actor_id) {
        return Err(Error::InvalidInput(format!(
            "Invalid actor ID: {}. Valid actors are: {}",
            actor_id,
            valid_actors.join(", ")
        )));
    }

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

    // Restart the actor
    match bot_system.restart_actor(actor_id).await {
        Ok(_) => {
            println!(
                "✅ Successfully restarted {} actor",
                actor_id.to_uppercase()
            );
            Ok(())
        }
        Err(e) => {
            error!("Failed to restart actor: {}", e);
            println!(
                "❌ Failed to restart {} actor: {}",
                actor_id.to_uppercase(),
                e
            );
            Err(e)
        }
    }
}

fn create_strategy(
    strategy: &str,
    confidence_threshold: f64,
    min_volume: f64,
    stop_loss: f64,
    min_data_points: Option<usize>,
    risk_tolerance: Option<u8>,
    indicator_weights: Option<IndicatorWeights>,
    testing_mode: Option<&str>,
) -> crate::trading::strategy::Strategy {
    // First check if we're explicitly requesting a mock strategy
    if strategy == "mock" {
        info!("Creating mock strategy for testing");
        let mut mock_strategy = crate::trading::strategy::MockStrategy::new(
            confidence_threshold,
            min_volume,
            stop_loss,
        );

        // Configure mock strategy based on risk tolerance
        if let Some(level) = risk_tolerance {
            let hold_duration = match level {
                0 => 300, // Conservative: 5 minutes
                1 => 240, // Conservative-Moderate: 4 minutes
                2 => 180, // Moderate: 3 minutes
                3 => 120, // Moderate-Aggressive: 2 minutes
                4 => 60,  // Aggressive: 1 minute
                _ => 30,  // Very Aggressive: 30 seconds
            };
            mock_strategy = mock_strategy.with_hold_duration(hold_duration);

            let success_rate = match level {
                0 => 0.7,  // Conservative: 70% success
                1 => 0.65, // Conservative-Moderate: 65% success
                2 => 0.6,  // Moderate: 60% success
                3 => 0.55, // Moderate-Aggressive: 55% success
                4 => 0.5,  // Aggressive: 50% success
                _ => 0.45, // Very Aggressive: 45% success
            };
            mock_strategy = mock_strategy.with_success_rate(success_rate);
        }

        // Use market scan interval to set signal interval if available
        mock_strategy = mock_strategy.with_signal_interval(60); // Default to 60s

        return crate::trading::strategy::Strategy::new(Box::new(mock_strategy));
    }

    // Otherwise, handle regular strategies
    match strategy {
        "momentum" => {
            let mut strategy = MomentumStrategy::new(confidence_threshold, min_volume, stop_loss);

            // Then set weights using the builder pattern
            strategy = strategy.with_indicator_weights(
                indicator_weights.unwrap_or_else(|| IndicatorWeights::default()),
            );

            if let Some(points) = min_data_points {
                strategy = strategy.with_min_data_points(points);
            }

            if let Some(level) = risk_tolerance {
                strategy = strategy.with_risk_tolerance(level as f64);
            }

            // Apply testing mode if provided
            if let Some(mode) = testing_mode {
                let trading_mode = match mode.to_lowercase().as_str() {
                    "fast" => crate::trading::indicators::TradingMode::FastTest,
                    "ultra" => crate::trading::indicators::TradingMode::UltraFast,
                    "mock" => crate::trading::indicators::TradingMode::Mock,
                    _ => crate::trading::indicators::TradingMode::Production,
                };
                strategy = strategy.with_trading_mode(trading_mode);
                info!("🔧 Setting trading mode to: {:?}", trading_mode);
            }

            crate::trading::strategy::Strategy::new(Box::new(strategy))
        }
        _ => {
            println!(
                "Unsupported strategy: {}. Using momentum instead.",
                strategy
            );
            let mut strategy = MomentumStrategy::new(confidence_threshold, min_volume, stop_loss);

            // Then set weights using the builder pattern
            strategy = strategy.with_indicator_weights(
                indicator_weights.unwrap_or_else(|| IndicatorWeights::default()),
            );

            if let Some(points) = min_data_points {
                strategy = strategy.with_min_data_points(points);
            }

            if let Some(level) = risk_tolerance {
                strategy = strategy.with_risk_tolerance(level as f64);
            }

            // Apply testing mode if provided
            if let Some(mode) = testing_mode {
                let trading_mode = match mode.to_lowercase().as_str() {
                    "fast" => crate::trading::indicators::TradingMode::FastTest,
                    "ultra" => crate::trading::indicators::TradingMode::UltraFast,
                    "mock" => crate::trading::indicators::TradingMode::Mock,
                    _ => crate::trading::indicators::TradingMode::Production,
                };
                strategy = strategy.with_trading_mode(trading_mode);
                info!("🔧 Setting trading mode to: {:?}", trading_mode);
            }

            crate::trading::strategy::Strategy::new(Box::new(strategy))
        }
    }
}

/// Handle the analyze command - No DB passed directly
pub async fn handle_analyze(
    db: &Database,
    strategy: &str,
    confidence_threshold: f64,
    min_volume: f64,
    stop_loss: f64,
    debug: bool,
    risk_tolerance: u8,
    testing_mode: Option<&str>,
) -> Result<(), Error> {
    println!("Analyzing tokens with strategy: {}", strategy);

    if debug {
        setup_logging(LevelFilter::Debug)?;
    } else {
        setup_logging(LevelFilter::Info)?;
    }

    // Remove internal DB init
    warn!("handle_analyze needs to initialize DB/Repositories to fetch market data.");

    // Create strategy object (doesn't require DB access)
    let strategy_obj = create_strategy(
        strategy,
        confidence_threshold,
        min_volume,
        stop_loss,
        Some(7), // Default min_data_points
        Some(risk_tolerance),
        None, // Default indicator_weights
        testing_mode,
    );

    println!("Strategy created: {:#?}", strategy_obj);
    println!("Fetching market data using provided DB...");

    // Use the passed db to create repository
    let token_repo = TokenRepository::new(db.clone(), true); // Assume paper mode for analysis
    let market_data_result = token_repo.get_latest_market_data().await;

    let market_data: Vec<TokenData> = match market_data_result {
        Ok(data) => data,
        Err(e) => {
            error!("Failed to fetch market data: {}", e);
            return Err(e);
        }
    };

    if market_data.is_empty() {
        println!("No market data found to analyze.");
        return Ok(());
    }

    println!("Analyzing {} tokens...", market_data.len());

    // Placeholder: Analyze data and display signals
    // let signals = analyze_data(strategy_obj, market_data).await?;
    // display_signals(signals);
    // Adapt the rest of analyze_market_data function here if it was split out
    warn!("Analysis logic needs to be reimplemented here or called from here.");

    Ok(())
}

/// Close an open position
pub async fn close_position(
    db: &Database,
    token_id: &str,
    exit_price: Option<f64>,
    is_paper: bool,
) -> Result<(), Error> {
    let mode = if is_paper { "Paper" } else { "Live" };
    println!(
        "Attempting to close {} position for token: {}",
        mode, token_id
    );

    // Create position repository
    let position_repo = PositionRepository::new(db.clone(), is_paper);

    // Find the open position and its ID
    let position_info = match position_repo.get_position_by_token_id(token_id).await? {
        Some((id, p)) => {
            info!(
                "Found open {} position ID {} for token {}",
                mode, id, token_id
            );
            Some((id, p))
        }
        None => {
            error!("No open {} position found for token {}", mode, token_id);
            return Err(Error::NotFound(format!(
                "Open position not found: {}",
                token_id
            )));
        }
    };

    // Ensure we found the position
    let (position_id, open_position) = match position_info {
        Some(info) => info,
        None => return Ok(()), // Already handled error case above, return Ok here
    };

    let final_exit_price = match exit_price {
        Some(price) => price,
        None => {
            // Fetch latest price if not provided (requires TokenRepository)
            warn!(
                "Exit price not provided for {}, fetching latest price...",
                token_id
            );
            let token_repo = TokenRepository::new(db.clone(), is_paper);
            match token_repo.get_token_price_stats(token_id).await {
                Ok(stats) => stats.price_usd,
                Err(e) => {
                    error!("Failed to fetch latest price for {}: {}. Using position current price as fallback.", token_id, e);
                    open_position.current_price // Use current price as fallback
                }
            }
        }
    };

    // Record the closure using the repository, passing the fetched position_id
    match position_repo
        .record_position_close_with_trade(
            position_id, // Pass the correct position ID
            &open_position.token_id,
            final_exit_price,
            open_position.size,
            open_position.entry_price,
            open_position.entry_time,
            Utc::now(),
        )
        .await
    {
        Ok(completed) => {
            println!("✅ Successfully closed {} position: {:#?}", mode, completed);
            Ok(())
        }
        Err(e) => {
            error!(
                "❌ Failed to close {} position for {}: {}",
                mode, token_id, e
            );
            Err(e)
        }
    }
}

// Removed async main run_trading_commands function

// Make the main function async
pub async fn run_trading_commands(matches: &clap::ArgMatches) -> crate::core::error::Result<()> {
    // Load configuration
    let config = crate::config::Config::load()?;

    Ok(())
}
