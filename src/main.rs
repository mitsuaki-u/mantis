use clap::{Parser, ArgAction};
use honeybadger::commands::{Commands, handle_market_command, handle_dex_command, handle_wallet_command, handle_config_command, handle_trading_command};
use log::{info, warn, debug};
use env_logger;
use honeybadger::data::collector::DataCollector;
use honeybadger::error::Error;
use honeybadger::config::Config;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use honeybadger::actors::MessageBus;
use honeybadger::repositories::RepositoryFactory;
use std::io::Write;
use std::fs::File;
use chrono;

#[derive(Parser)]
#[clap(name = "honeybadger")]
#[clap(about = "Command-line interface for trading", version, author)]
#[clap(propagate_version = true)]
struct Cli {
    /// Enable paper trading (simulate trades without real money)
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    paper: bool,

    /// Market scan interval in seconds
    #[arg(long, global = true)]
    scan_interval: Option<u64>,

    /// Maximum position size in USD
    #[arg(long, global = true)]
    max_position: Option<f64>,

    /// Maximum total exposure in USD
    #[arg(long, global = true)]
    max_exposure: Option<f64>,

    /// Strategy type (momentum, rsi, macd, etc.)
    #[arg(long, global = true)]
    strategy: Option<String>,

    /// Strategy signal threshold
    #[arg(long, global = true)]
    threshold: Option<f64>,

    /// Minimum volume required for trading
    #[arg(long, global = true)]
    min_volume: Option<f64>,

    /// Stop loss percentage
    #[arg(long, global = true)]
    stop_loss: Option<f64>,

    /// Take profit percentage
    #[arg(long, global = true)]
    take_profit: Option<f64>,

    /// Maximum number of positions
    #[arg(long, global = true)]
    max_positions: Option<usize>,

    /// Risk tolerance level (0-5): 0=Conservative, 1=Conservative-Moderate, 2=Moderate, 3=Moderate-Aggressive, 4=Aggressive, 5=Very Aggressive
    #[arg(long, global = true)]
    risk_tolerance: Option<u8>,

    /// CoinGecko API key
    #[arg(long, global = true)]
    coingecko_key: Option<String>,

    /// Enable debug logging
    #[arg(short, long, global = true, action = ArgAction::SetTrue)]
    debug: bool,

    /// Write logs to a file
    #[arg(long, global = true)]
    log_file: Option<String>,

    /// Set log level (error, warn, info, debug, trace)
    #[arg(long, global = true, value_parser = ["error", "warn", "info", "debug", "trace"])]
    log_level: Option<String>,

    /// Filter logs by module (comma-separated, e.g., "honeybadger::trading,honeybadger::api")
    #[arg(long, global = true)]
    log_modules: Option<String>,

    #[clap(subcommand)]
    command: Commands,
}


#[tokio::main]
async fn main() -> Result<(), Error> {
    // Parse command-line arguments
    let cli = Cli::parse();
    
    // Set up logging level based on args
    let log_level = if let Some(level) = &cli.log_level {
        level.as_str()
    } else if cli.debug {
        "debug"
    } else {
        "info"
    };
    
    // Initialize logger with appropriate level and module filters
    let mut builder = env_logger::Builder::new();
    
    // Apply module filters if specified
    if let Some(modules_str) = &cli.log_modules {
        // Start with a base filter that sets everything to warn (or a lower level)
        let base_level = log::LevelFilter::Warn;
        let mut filter_string = base_level.to_string().to_lowercase();
        
        // For each specified module, add it to the filter string
        for module in modules_str.split(',') {
            let module = module.trim();
            if !module.is_empty() {
                // Format: "module_name=level"
                let filter_spec = format!(",{}={}", module, log_level);
                filter_string.push_str(&filter_spec);
                println!("Adding log filter: {}={}", module, log_level);
            }
        }
        
        // Parse the combined filter string
        builder.parse_filters(&filter_string);
    } else {
        // No modules specified, use the global log level
        builder.filter_level(log_level.parse().unwrap_or(log::LevelFilter::Info));
    }
    
    builder.format_timestamp(Some(env_logger::fmt::TimestampPrecision::Seconds));
    
    // Define a custom writer to write to both stderr and a file
    struct DualWriter {
        file: File,
    }
    
    impl Write for DualWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            // Write to stderr
            std::io::stderr().write_all(buf)?;
            
            // Write to file
            self.file.write_all(buf)?;
            
            Ok(buf.len())
        }
        
        fn flush(&mut self) -> std::io::Result<()> {
            std::io::stderr().flush()?;
            self.file.flush()
        }
    }
    
    // Load configuration with all sources
    let mut config = Config::load()?;
    
    // Apply command-line argument overrides to config
    apply_cli_config(&mut config, &cli);
    
    // Apply CLI overrides (using the apply_command_line method)
    config.apply_command_line(
        cli.paper,
        cli.scan_interval,
        cli.max_position,
        cli.max_exposure,
        cli.strategy.clone(),
        cli.threshold,
        cli.min_volume,
        cli.stop_loss,
        cli.take_profit,
        cli.max_positions,
        cli.risk_tolerance
    );
    
    // If we have a log file option, update it to use the default logs directory for relative paths
    if let Some(log_file) = &cli.log_file {
        // Determine if the path is absolute or relative
        let log_path = if std::path::Path::new(log_file).is_absolute() {
            log_file.clone()
        } else {
            // For relative paths, use the configured logs directory
            let logs_dir = config.logs.directory.clone();
            let path = std::path::Path::new(&logs_dir).join(log_file);
            path.to_string_lossy().to_string()
        };
        
        // Re-create the log file in the proper location
        let file = match File::create(&log_path) {
            Ok(file) => {
                println!("Writing logs to file: {}", log_path);
                file
            },
            Err(e) => {
                eprintln!("Error creating log file '{}': {}", log_path, e);
                std::process::exit(1);
            }
        };
        
        // Update the writer with the new file
        let dual_writer = DualWriter { file };
        builder.target(env_logger::Target::Pipe(Box::new(dual_writer)));
    } else if cli.debug || cli.log_level.is_some() {
        // Automatically create a log file if debug mode or log level is specified but no log file is provided
        
        // Get the command name for the log file name
        let command_name = match &cli.command {
            Commands::Trading { .. } => "trading",
            Commands::Market { .. } => "market",
            Commands::Dex { .. } => "dex",
            Commands::Wallet { .. } => "wallet",
            Commands::Config { .. } => "config",
        };
        
        // Create a default log file name based on command and current timestamp
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let default_log_name = format!("{}_{}.log", command_name, timestamp);
        
        // Use the configured logs directory
        let logs_dir = config.logs.directory.clone();
        let log_path = std::path::Path::new(&logs_dir).join(default_log_name);
        let log_path_str = log_path.to_string_lossy().to_string();
        
        // Create the logs directory if it doesn't exist
        if let Some(parent) = log_path.parent() {
            if !parent.exists() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    eprintln!("Error creating logs directory '{}': {}", parent.display(), e);
                    // Continue without log file if directory creation fails
                } else {
                    // Try to create the log file
                    match File::create(&log_path) {
                        Ok(file) => {
                            println!("Automatically writing logs to file: {}", log_path_str);
                            let dual_writer = DualWriter { file };
                            builder.target(env_logger::Target::Pipe(Box::new(dual_writer)));
                        },
                        Err(e) => {
                            eprintln!("Error creating default log file '{}': {}", log_path_str, e);
                            // Continue without log file if creation fails
                        }
                    }
                }
            } else {
                // Directory exists, try to create the log file
                match File::create(&log_path) {
                    Ok(file) => {
                        println!("Automatically writing logs to file: {}", log_path_str);
                        let dual_writer = DualWriter { file };
                        builder.target(env_logger::Target::Pipe(Box::new(dual_writer)));
                    },
                    Err(e) => {
                        eprintln!("Error creating default log file '{}': {}", log_path_str, e);
                        // Continue without log file if creation fails
                    }
                }
            }
        }
    }
    
    // Initialize the logger
    builder.init();
    
    info!("Starting HoneyBadger v{}", env!("CARGO_PKG_VERSION"));
    
    // Set up a shutdown signal
    let shutdown_signal = Arc::new(AtomicBool::new(false));
    let shutdown_signal_clone = shutdown_signal.clone();
    
    // Listen for Ctrl+C to handle graceful shutdown
    tokio::spawn(async move {
        if let Ok(()) = tokio::signal::ctrl_c().await {
            info!("Received shutdown signal, preparing to exit...");
            shutdown_signal_clone.store(true, Ordering::SeqCst);
        }
    });
    
    // Initialize shared message bus with proper logging
    let message_bus = MessageBus::instance();
    let bus_id = format!("{:p}", Arc::as_ptr(&message_bus));
    info!("🌐 Using global MessageBus instance [id: {}] for event routing", bus_id);
    
    // Initial subscriber count check (for debugging)
    let market_subs = message_bus.debug_subscriber_count("market").await;
    let strategy_subs = message_bus.debug_subscriber_count("strategy").await;
    let risk_subs = message_bus.debug_subscriber_count("risk").await;
    let execution_subs = message_bus.debug_subscriber_count("execution").await;
    let database_subs = message_bus.debug_subscriber_count("database").await;
    
    debug!("Initial subscriber counts - market: {}, strategy: {}, risk: {}, execution: {}, database: {}", 
          market_subs, strategy_subs, risk_subs, execution_subs, database_subs);
    
    // Start the data collector with configured interval if auto-start is enabled
    let collector_result: Result<Option<DataCollector>, Error> = if config.data_collection.auto_start {
        info!("Auto-starting data collector with interval {} seconds", 
            config.data_collection.interval);
        
        // Create repositories and message bus for collector
        let repo_factory = RepositoryFactory::new(true)?; // Use paper trading for data collection
        let price_repo = Arc::new(repo_factory.price_repository());
        let token_repo = Arc::new(repo_factory.token_repository());
        let message_bus_arc = MessageBus::instance(); // This is already an Arc<MessageBus>
        let config_arc = Arc::new(config.clone());
        
        // Create and start the collector
        let mut collector = DataCollector::new(
            price_repo,
            token_repo,
            config.data_collection.interval,
            config_arc,
            Some(message_bus_arc)
        );
        
        collector.start().await?;
        
        Ok(Some(collector))
    } else {
        info!("Data collector auto-start disabled");
        Ok(None)
    };

    // Handle the collector result
    let mut collector = match collector_result {
        Ok(Some(c)) => Some(c),
        Ok(None) => None,
        Err(e) => {
            warn!("Failed to start data collector: {}. Continuing without data collection.", e);
            None
        }
    };
    
    info!("Core services initialized, processing command...");

    // Process command
    let result = match cli.command {
        Commands::Config { command } => handle_config_command(command).await,
        Commands::Trading { command } => handle_trading_command(command, config, message_bus).await,
        Commands::Market { command } => handle_market_command(command).await,
        Commands::Dex { command } => handle_dex_command(command).await,
        Commands::Wallet { command } => handle_wallet_command(command).await,
    };
    
    // Check if we should apply a graceful shutdown
    let clean_shutdown = !shutdown_signal.load(Ordering::SeqCst);
    
    info!("Command completed, performing {} shutdown...", 
        if clean_shutdown { "normal" } else { "forced" });
    
    // Gracefully shutdown the data collector if it was started
    if let Some(c) = collector.as_mut() {
        match c.stop().await {
            Ok(_) => info!("Data collector shutdown successfully"),
            Err(e) => warn!("Error during data collector shutdown: {}", e),
        }
    }
    
    info!("Shutdown complete");
    result
}

// Helper function to apply CLI arguments to the config
fn apply_cli_config(config: &mut Config, cli: &Cli) {
    let mut applied = false;

    // API Keys
    if let Some(key) = &cli.coingecko_key {
        config.api_keys.coingecko = Some(key.clone());
        applied = true;
    }

    // Trading configuration
    if cli.paper {
        config.trading.paper_trading = true;
        applied = true;
    }

    if let Some(interval) = cli.scan_interval {
        config.trading.scan_interval = interval;
        applied = true;
    }

    if let Some(size) = cli.max_position {
        config.trading.max_position_size = size;
        applied = true;
    }

    if let Some(exposure) = cli.max_exposure {
        config.trading.max_total_exposure = exposure;
        applied = true;
    }

    // Strategy configuration
    if let Some(strategy) = &cli.strategy {
        config.trading.strategy.strategy_type = strategy.clone();
        applied = true;
    }

    if let Some(threshold) = cli.threshold {
        config.trading.strategy.threshold = threshold;
        applied = true;
    }

    if let Some(volume) = cli.min_volume {
        config.trading.strategy.min_volume = volume;
        applied = true;
    }

    // Risk configuration
    if let Some(stop_loss) = cli.stop_loss {
        config.trading.risk.stop_loss_pct = stop_loss;
        applied = true;
    }

    if let Some(take_profit) = cli.take_profit {
        config.trading.risk.take_profit_pct = take_profit;
        applied = true;
    }

    if let Some(max_positions) = cli.max_positions {
        config.trading.risk.max_positions = max_positions;
        applied = true;
    }

    if let Some(risk_level) = cli.risk_tolerance {
        config.trading.risk_tolerance = risk_level;
        applied = true;
    }

    if applied {
        info!("Applied command-line configuration overrides");
    }
} 