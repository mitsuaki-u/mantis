//! Configuration display commands

use super::formatting::{format_api_key, mask_api_key};
use crate::config::Config;
use crate::error::{Error, Result};
use colored::*;

/// Handle the Show command to display current configuration
pub async fn handle_show(show_secrets: bool, json: bool) -> Result<()> {
    let config = Config::load()?;

    if json {
        // Mask sensitive values unless show_secrets is true
        let mut config_display = config.clone();
        if !show_secrets {
            config_display.api_keys.infura =
                config_display.api_keys.infura.map(|k| mask_api_key(&k));
            config_display.api_keys.alchemy =
                config_display.api_keys.alchemy.map(|k| mask_api_key(&k));
            config_display.database.password =
                config_display.database.password.map(|k| mask_api_key(&k));
            // DEX wallet keys are not stored in config, they're in environment variables
        }

        let json_output = serde_json::to_string_pretty(&config_display)
            .map_err(|e| Error::Config(format!("Failed to serialize config to JSON: {}", e)))?;
        println!("{}", json_output);
        return Ok(());
    }

    // Standard text output
    println!("{}", "Honeybadger Configuration".bold().underline());
    println!();

    // API Keys
    println!("{}", "API Keys:".yellow().bold());
    println!(
        "  Infura: {}",
        format_api_key(config.api_keys.infura.as_deref(), show_secrets).cyan()
    );
    println!(
        "  Alchemy: {}",
        format_api_key(config.api_keys.alchemy.as_deref(), show_secrets).cyan()
    );
    println!();

    // Trading Configuration
    println!("{}", "Trading Configuration:".yellow().bold());
    println!(
        "  Trading Mode: {}",
        if config.trading.live_trading {
            "Live Trading (REAL MONEY)".red().bold()
        } else {
            "Paper Trading (Simulated)".green()
        }
    );
    println!(
        "  Scan Interval: {}{}",
        config.data_collection.scan_interval_secs.to_string().cyan(),
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
    println!(
        "  Min Volume: {}{}",
        format!("${:.2}", config.trading.min_volume).cyan(),
        " USD".dimmed()
    );
    println!(
        "  Min Liquidity: {}{}",
        format!("${:.2}", config.trading.min_liquidity).cyan(),
        " USD".dimmed()
    );
    println!(
        "  Min Pool Transaction Count: {}",
        config.trading.min_pool_transaction_count.to_string().cyan()
    );
    println!("  DEX Protocol: {}", "uniswap_v3".cyan());
    println!(
        "  Max Volatility (24h): {}%",
        config.trading.max_volatility_24h.to_string().cyan()
    );
    println!(
        "  Max Daily Loss: {}{}",
        format!("${:.2}", config.trading.max_daily_loss).cyan(),
        " USD".dimmed()
    );
    println!(
        "  Max Drawdown: {}%",
        config.trading.max_drawdown.to_string().cyan()
    );
    println!(
        "  Max Single Trade Risk: {}%",
        config.trading.max_trade_risk_pct.to_string().cyan()
    );
    println!(
        "  Min Required ETH Balance: {}{}",
        format!("${:.2}", config.trading.min_eth_balance).cyan(),
        " USD".dimmed()
    );
    println!(
        "  Market Data Provider: {}",
        config.trading.market_data_provider.cyan()
    );
    println!();

    // Strategy Configuration
    println!("{}", "Strategy Configuration:".yellow().bold());
    println!("  Strategy Type: {}", config.trading.strategy.cyan());
    println!(
        "  Signal Threshold: {}",
        config
            .trading
            .signal_confidence_threshold
            .to_string()
            .cyan()
    );
    println!();

    // Indicator Configuration (for momentum strategy)
    if config.trading.strategy == "momentum" {
        println!("{}", "Indicator Configuration:".yellow().bold());
        println!(
            "  Indicator Profile: {}",
            config.trading.indicator_profile.cyan()
        );

        // Show profile info
        let profile_info = match config.trading.indicator_profile.as_str() {
            "scalping" => "Ultra-fast (5min scan interval, 40min warmup)",
            "day_trading" => "Balanced (1min scan interval, 50min warmup) [RECOMMENDED]",
            "swing_trading" => "Conservative (1-5min scan intervals, 57min warmup)",
            "standard" => "Traditional (5-15min scan intervals, 61min warmup)",
            _ => "Custom profile",
        };
        println!("    └─ {}", profile_info.dimmed());

        println!("  Indicator Weights:");
        println!(
            "    RSI Weight: {}",
            config.trading.rsi_weight.to_string().cyan()
        );
        println!(
            "    MACD Weight: {}",
            config.trading.macd_weight.to_string().cyan()
        );
        println!(
            "    Bollinger Bands Weight: {}",
            config.trading.bollinger_weight.to_string().cyan()
        );
        println!(
            "    Volume Weight: {}",
            config.trading.volume_weight.to_string().cyan()
        );

        let total_weight = config.trading.rsi_weight
            + config.trading.macd_weight
            + config.trading.bollinger_weight
            + config.trading.volume_weight;
        if (total_weight - 1.0).abs() > 0.01 {
            println!(
                "    {} Weights sum to {:.2} (should sum to 1.0)",
                "⚠".yellow(),
                total_weight
            );
        }
        println!();
    }

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

    // Database Configuration (PostgreSQL)
    println!("{}", "Database Configuration:".yellow().bold());
    println!("  Host: {}", config.database.host.cyan());
    println!("  Port: {}", config.database.port.to_string().cyan());
    println!("  User: {}", config.database.user.cyan());
    println!(
        "  Password: {}",
        format_api_key(config.database.password.as_deref(), show_secrets).cyan()
    );
    println!("  Database: {}", config.database.dbname.cyan());
    println!(
        "  Pool Max Size: {}",
        config.database.pool_max_size.to_string().cyan()
    );
    println!();

    // Data Collection
    println!("{}", "Data Collection:".yellow().bold());
    println!(
        "  Collection Interval: {}{}",
        config.data_collection.scan_interval_secs.to_string().cyan(),
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
    println!("  Protocol: {}", config.dex.protocol.cyan());
    println!(
        "  Network: {}",
        config.dex.network.as_deref().unwrap_or("Not set").cyan()
    );
    println!();

    // RPC Configuration
    println!("{}", "RPC Configuration:".yellow().bold());
    println!(
        "  Primary RPC Provider: {}",
        config.rpc.primary_provider.cyan()
    );
    println!();

    // Tokens to Track
    println!("{}", "Tokens to Track:".yellow().bold());
    if config.trading.tokens_to_track.is_empty() {
        println!("  No specific tokens configured (using scan mode)");
    } else {
        println!("  Configured tokens:");
        for token in &config.trading.tokens_to_track {
            println!("    • {}", token.cyan());
        }
    }
    println!();

    // Logs Configuration
    println!("{}", "Logs Configuration:".yellow().bold());
    println!("  Logs Directory: {}", config.logs.directory.cyan());
    println!();

    // Show current token scanning configuration
    println!("{}", "Token Scanning Configuration:".yellow().bold());
    println!(
        "  Max tokens to scan: {}",
        config.trading.max_tokens_to_scan.to_string().cyan()
    );
    if config.trading.max_tokens_to_scan == 0 {
        println!("  ✓ Unlimited - processing ALL available tokens (may hit rate limits)");
    } else {
        println!(
            "  ✓ Processing up to {} tokens (recommended: 100-200)",
            config.trading.max_tokens_to_scan
        );
    }

    if !config.trading.tokens_to_track.is_empty() {
        println!(
            "  Specific tokens being tracked: {:?}",
            config.trading.tokens_to_track
        );
    }

    Ok(())
}
