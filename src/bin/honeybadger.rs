use clap::{ArgAction, Parser, Subcommand};
use honeybadger::cli::commands::trading::{
    close_position, display_open_positions, display_trading_history, open_position,
};
use honeybadger::cli::commands::{
    handle_config_command, handle_dex_command, handle_trading_command, handle_wallet_command,
    ConfigCommands, DexCommands, TradingArgs, WalletCommands,
};
use honeybadger::core::config::Config;
use honeybadger::core::error::Error;
use honeybadger::domain::trading::execution::bot::is_forced_shutdown;
use honeybadger::infra::actors::MessageBus;
use honeybadger::infra::cache::Cache;
use honeybadger::infra::db::repositories::TokenRepository;
use honeybadger::infra::db::Database;
use honeybadger::utils::logging;
use log::{debug, error, info, warn};
use std::io::{self, Write};
use std::sync::Arc;
use std::time::Duration;

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

    /// Confidence threshold for strategy signals (0.0-1.0)
    #[arg(long, global = true)]
    confidence_threshold: Option<f64>,

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

    /// Enable Redis cache
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    enable_cache: bool,

    /// Redis URL
    #[arg(long, global = true)]
    redis_url: Option<String>,

    /// Cache flush interval in seconds
    #[arg(long, global = true)]
    cache_flush_interval: Option<u64>,

    /// Enable debug logging
    #[arg(short, long, global = true, action = ArgAction::SetTrue)]
    debug: bool,

    /// Write logs to a file
    #[arg(long, global = true)]
    log_file: Option<String>,

    /// Set log level (error, warn, info, debug, trace, trade)
    #[arg(long, global = true, value_parser = ["error", "warn", "info", "debug", "trace", "trade"])]
    log_level: Option<String>,

    /// Filter logs by module (comma-separated, e.g., "honeybadger::trading,honeybadger::api")
    #[arg(long, global = true)]
    log_modules: Option<String>,

    #[clap(subcommand)]
    command: MainCommands,
}

#[derive(Subcommand)]
enum MainCommands {
    /// Trading commands
    #[clap(subcommand)]
    Trading(TradingArgs),
    /// DEX (Decentralized Exchange) commands
    #[clap(subcommand)]
    Dex(DexCommands),
    /// Wallet commands
    #[clap(subcommand)]
    Wallet(WalletCommands),
    /// Configuration commands
    #[clap(subcommand)]
    Config(ConfigCommands),
    /// Database management commands
    #[clap(subcommand)]
    Db(DbCommands),
}

#[derive(Parser)]
enum DbCommands {
    /// Reset the database - deletes the database file and recreates it with the latest schema
    Reset,

    /// Display the current database schema
    Schema {
        /// Write schema to a file instead of displaying it
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Run database maintenance tasks (VACUUM, ANALYZE, integrity check)
    /// Best run during quiet periods to avoid locking issues
    Maintenance,

    /// Start periodic database maintenance scheduler
    /// This will automatically run maintenance during quiet periods
    PeriodStart,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("DEBUG: Starting honeybadger main function");

    // Parse command line arguments first to get log settings
    let args = Cli::parse();
    println!("DEBUG: Args parsed");

    // Load configuration
    let mut config = Config::load()?;
    println!("DEBUG: Config loaded");

    // Apply command-line overrides to configuration
    apply_cli_config(&mut config, &args);

    // Initialize logging
    logging::init_logger(
        args.log_level.as_deref(),
        args.debug,
        args.log_file.as_deref(),
        &config.logs.directory,
        "honeybadger", // Use app name as command name for now
        args.log_modules.as_deref(),
    )
    .map_err(|e| Error::Config(format!("Logger initialization failed: {}", e)))?;
    println!("DEBUG: Logging initialized");

    // Initialize database
    println!("DEBUG: Initializing database...");
    let db = init_database(&config).await?;
    println!("DEBUG: Database initialized");

    // Create token repository for cache
    let token_repo_for_cache = Arc::new(TokenRepository::new(
        db.clone(),
        config.trading.paper_trading,
    ));

    // Initialize cache
    println!("DEBUG: Initializing cache...");
    let cache = init_cache(&config, token_repo_for_cache).await;
    println!("DEBUG: Cache initialized");

    // Start cache flush task if enabled
    if cache.is_some() {
        println!("DEBUG: Cache is available");
    }

    info!("Honeybadger CLI started");
    info!("Version: {}", env!("CARGO_PKG_VERSION"));
    debug!("Configuration loaded: {:#?}", config);

    println!("DEBUG: About to initialize MessageBus");
    // --- Asynchronous Initializations ---
    let message_bus = if matches!(
        args.command,
        MainCommands::Trading(TradingArgs::Start { .. })
            | MainCommands::Trading(TradingArgs::Status)
            | MainCommands::Trading(TradingArgs::Health)
            | MainCommands::Trading(TradingArgs::Restart { .. })
            | MainCommands::Trading(TradingArgs::Stop)
            | MainCommands::Wallet(_)
    ) {
        println!("DEBUG: Creating MessageBus instance");
        Some(MessageBus::instance())
    } else {
        println!("DEBUG: Skipping MessageBus creation");
        None
    };
    println!("DEBUG: MessageBus initialization completed");

    println!("DEBUG: About to handle commands");

    // Handle commands
    match args.command {
        MainCommands::Trading(args) => {
            if matches!(
                args,
                TradingArgs::History { .. }
                    | TradingArgs::Positions { .. }
                    | TradingArgs::Close { .. }
                    | TradingArgs::Open { .. }
            ) {
                match args {
                    TradingArgs::History { limit, paper, live } => {
                        let is_paper = if live { false } else { paper };
                        display_trading_history(&db, is_paper, limit).await?;
                    }
                    TradingArgs::Positions { paper, live } => {
                        let is_paper = if live { false } else { paper };
                        display_open_positions(&db, is_paper).await?;
                    }
                    TradingArgs::Close {
                        token,
                        price,
                        paper,
                        live,
                    } => {
                        let is_paper = if live { false } else { paper };
                        close_position(&db, &token, Some(price), is_paper).await?;
                    }
                    TradingArgs::Open {
                        token,
                        amount,
                        price,
                        paper,
                        live,
                    } => {
                        let is_paper = if live { false } else { paper };
                        open_position(&db, &token, price, amount, is_paper).await?;
                    }
                    _ => unreachable!(),
                }
            } else if let Some(bus) = message_bus {
                handle_trading_command(args, config.clone(), db.clone(), bus.clone()).await?;
            } else {
                return Err(Error::Config(
                    "MessageBus not initialized for this trading command. This typically means the command is run standalone without full actor system.".to_string(),
                ).into());
            }
        }
        MainCommands::Dex(command) => {
            handle_dex_command(command).await?;
        }
        MainCommands::Wallet(command) => {
            if let Some(bus) = message_bus {
                handle_wallet_command(command, config.clone(), bus).await?;
            } else {
                return Err(Error::Config(
                    "MessageBus not initialized for wallet command.".to_string(),
                )
                .into());
            }
        }
        MainCommands::Config(command) => handle_config_command(command).await?,
        MainCommands::Db(command) => handle_db_command(command, config.clone(), db.clone()).await?,
    }

    Ok(())
}

// Apply CLI configuration overrides
fn apply_cli_config(config: &mut Config, cli: &Cli) {
    if cli.paper {
        config.trading.paper_trading = true;
    }
    if let Some(interval) = cli.scan_interval {
        config.data_collection.interval = interval;
    }
    if let Some(max_pos) = cli.max_position {
        config.trading.max_position_size = max_pos;
    }
    if let Some(max_exp) = cli.max_exposure {
        config.trading.max_total_exposure = max_exp;
    }
    if let Some(ref strategy) = cli.strategy {
        config.trading.strategy = strategy.clone();
    }
    if let Some(threshold) = cli.confidence_threshold {
        config.trading.threshold = threshold;
    }
    if let Some(min_vol) = cli.min_volume {
        config.trading.min_volume = min_vol;
    }
    if let Some(stop_loss) = cli.stop_loss {
        config.trading.stop_loss = stop_loss;
    }
    if let Some(take_profit) = cli.take_profit {
        config.trading.take_profit = take_profit;
    }
    if let Some(max_positions) = cli.max_positions {
        config.trading.max_positions = max_positions;
    }
    if let Some(risk_tolerance) = cli.risk_tolerance {
        config.trading.risk_tolerance = risk_tolerance;
    }
    if let Some(ref key) = cli.coingecko_key {
        config.api_keys.coingecko = Some(key.clone());
    }

    // Cache configuration
    if cli.enable_cache {
        config.cache.enabled = true;
    }
    if let Some(ref url) = cli.redis_url {
        config.cache.redis_url = Some(url.clone());
    }
    if let Some(interval) = cli.cache_flush_interval {
        config.cache.flush_interval = interval;
    }

    // Note: Logging config is handled separately during initialization
}

/// Initialize the database connection pool (async)
async fn init_database(config: &Config) -> Result<Database, Error> {
    info!("Initializing database...");
    // Use the async new method
    Database::new(config).await.map_err(|e| {
        error!("Fatal: Failed to initialize database: {}", e);
        // Provide more context for common connection errors
        if e.to_string().contains("connection refused") {
            eprintln!("Error: Could not connect to the PostgreSQL database.");
            eprintln!("Please ensure PostgreSQL is running and accessible at {}:{}.", config.database.host, config.database.port);
            eprintln!("Check database credentials in config.json or environment variables.");
        } else if e.to_string().contains("password authentication failed") {
            eprintln!("Error: PostgreSQL password authentication failed for user '{}'.", config.database.user);
             eprintln!("Check database credentials in config.json or environment variables (HONEYBADGER_DB_PASSWORD).");
        } else if e.to_string().contains("database") && e.to_string().contains("does not exist") {
             eprintln!("Error: PostgreSQL database '{}' does not exist.", config.database.dbname);
             eprintln!("Please create the database or check the dbname setting in config.json.");
    }
        e
    })
}

/// Initialize the Redis cache connection pool (async)
async fn init_cache(config: &Config, token_repo: Arc<TokenRepository>) -> Option<Cache> {
    if !config.cache.enabled {
        info!("Redis cache is disabled in configuration.");
        return None;
    }

    let redis_url_str = match &config.cache.redis_url {
        Some(url) => {
            info!("Initializing Redis cache at {}...", url);
            url.as_str()
        }
        None => {
            warn!(
                "Redis cache is enabled but URL is not configured. Cache will not be initialized."
            );
            return None;
        }
    };

    let mut cache = Cache::new(redis_url_str, config.cache.flush_interval).await;

    if !cache.is_enabled() {
        warn!("Cache initialization failed or cache is disabled. Cache features inactive.");
        return None;
    }

    cache = cache.with_token_repository(token_repo.clone());

    info!("✅ Redis cache initialized successfully.");
    let cache_clone = Arc::new(cache.clone());
    let flush_interval_secs = config.cache.flush_interval;

    tokio::spawn(async move {
        info!(
            "Starting background cache flush task (interval: {}s)",
            flush_interval_secs
        );
        let mut interval = tokio::time::interval(Duration::from_secs(flush_interval_secs));
        loop {
            // Check global shutdown flag first
            if is_forced_shutdown() {
                info!("Cache flush task: Global shutdown detected, exiting");
                break;
            }

            interval.tick().await;

            // Check again after tick in case shutdown happened during wait
            if is_forced_shutdown() {
                info!("Cache flush task: Global shutdown detected after tick, exiting");
                break;
            }

            debug!("Running periodic cache flush...");
            if let Err(e) = cache_clone.manual_flush().await {
                error!("Error during background cache flush: {}", e);
            }
        }
        info!("Cache flush task terminated gracefully");
    });
    Some(cache)
}

/// Handle database management commands (async)
async fn handle_db_command(command: DbCommands, config: Config, db: Database) -> Result<(), Error> {
    match command {
        DbCommands::Reset => {
            println!(
                "WARNING: This will delete all data in the database '{}'.",
                config.database.dbname
            );
            print!("Are you sure you want to proceed? (yes/no): ");
            io::stdout().flush()?; // Make sure the prompt is shown before reading

            let mut confirmation = String::new();
            io::stdin().read_line(&mut confirmation)?;

            if confirmation.trim().eq_ignore_ascii_case("yes") {
                info!("Starting database reset...");
                Database::reset_database(&config).await?; // Use async reset
                info!("✅ Database reset complete.");
            } else {
                info!("Database reset cancelled.");
            }
        }
        DbCommands::Schema { output } => {
            warn!("Database schema display needs reimplementation for PostgreSQL.");
            // TODO: Implement schema fetching from PostgreSQL information_schema
            println!("Schema display not yet implemented for PostgreSQL.");
            if let Some(_path) = output {
                println!("Cannot write schema to file: Feature not implemented.");
            }
        }
        DbCommands::Maintenance => {
            info!("Running database maintenance...");
            db.perform_maintenance().await?; // Use async maintenance
            info!("✅ Database maintenance complete.");
        }
        DbCommands::PeriodStart => {
            warn!("Periodic database maintenance scheduler not yet implemented for PostgreSQL.");
            println!("Periodic maintenance scheduler not available.");
        }
    }
    Ok(())
}
