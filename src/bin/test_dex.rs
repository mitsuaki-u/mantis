use honeybadger::core::config::Config;
use honeybadger::domain::dex::DexClient;
use honeybadger::utils::logging;
use log::info;
use std::env;

#[tokio::main]
async fn main() {
    // Set up structured logging for the test DEX binary
    if let Err(e) = logging::init_logger(
        Some("debug"),  // Set debug level by default
        true,           // Debug mode on
        None,           // No log file
        "logs",         // Default logs directory
        "test_dex",     // Command name
        Some("honeybadger::dex=trace"),  // Show trace logs for dex module
    ) {
        eprintln!("Warning: Failed to initialize logger: {}", e);
    }
    
    // Create a custom config with explicit goerli network
    let mut config = Config::load().expect("Failed to load config");
    config.dex.testnet = true;
    config.dex.network = Some("goerli".to_string());
    
    info!("Creating DexClient with config: {:?}", config.dex);
    
    // Try to create a DexClient
    match DexClient::new_testnet(&config) {
        Ok(_client) => {
            info!("Successfully created DexClient");
        },
        Err(e) => {
            log::error!("Failed to create DexClient: {:?}", e);
        }
    }
} 