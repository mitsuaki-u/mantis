//! Configuration getter commands

use super::formatting::format_api_key;
use crate::config::Config;
use crate::error::{Error, Result};
use colored::*;

/// Handle the Get command to retrieve configuration values
pub async fn handle_get(key: String) -> Result<()> {
    let config = Config::load()?;

    // Split the key into parts
    let parts: Vec<&str> = key.split('.').collect();

    if parts.is_empty() || parts.len() > 3 {
        return Err(Error::Config(format!("Invalid configuration key: {}", key)));
    }

    let value = match parts[0] {
        "api_keys" => {
            if parts.len() < 2 {
                return Err(Error::Config(
                    "Please specify which API key (e.g., api_keys.infura)".to_string(),
                ));
            }

            match parts[1] {
                "infura" => format_api_key(config.api_keys.infura.as_deref(), false),
                "alchemy" => format_api_key(config.api_keys.alchemy.as_deref(), false),
                _ => return Err(Error::Config(format!("Unknown API key: {}", parts[1]))),
            }
        }
        "trading" => {
            if parts.len() < 2 {
                return Err(Error::Config(
                    "Please specify which trading parameter (e.g., trading.live_trading)"
                        .to_string(),
                ));
            }

            match parts[1] {
                "live_trading" => config.trading.live_trading.to_string(),
                "scan_interval" => config.data_collection.scan_interval_secs.to_string(),
                "max_position_size" => config.trading.max_position_size.to_string(),
                "max_total_exposure" => config.trading.max_total_exposure.to_string(),
                "strategy" => config.trading.strategy.clone(),
                "threshold" => config.trading.signal_confidence_threshold.to_string(),
                "min_volume" => config.trading.min_volume.to_string(),
                "min_liquidity" => config.trading.min_liquidity.to_string(),
                "min_pool_transaction_count" => {
                    config.trading.min_pool_transaction_count.to_string()
                }
                "stop_loss" => config.trading.stop_loss.to_string(),
                "take_profit" => config.trading.take_profit.to_string(),
                "max_positions" => config.trading.max_positions.to_string(),
                "max_volatility_24h" => config.trading.max_volatility_24h.to_string(),
                "max_daily_loss" => config.trading.max_daily_loss.to_string(),
                "max_drawdown" => config.trading.max_drawdown.to_string(),
                "max_single_trade_risk_percentage_of_wallet" => {
                    config.trading.max_trade_risk_pct.to_string()
                }
                "min_required_eth_balance_for_trading" => {
                    config.trading.min_native_balance.to_string()
                }
                "tokens_to_track" => serde_json::to_string(&config.trading.tokens_to_track)
                    .unwrap_or_else(|_| "[]".to_string()),
                "max_tokens_to_scan" => config.trading.max_tokens_to_scan.to_string(),
                "market_data_provider" => config.trading.market_data_provider.clone(),
                "min_position_size" => config.trading.min_position_size.to_string(),
                "rsi_weight" => config.trading.rsi_weight.to_string(),
                "macd_weight" => config.trading.macd_weight.to_string(),
                "bollinger_weight" => config.trading.bollinger_weight.to_string(),
                "volume_weight" => config.trading.volume_weight.to_string(),
                "indicator_profile" => config.trading.indicator_profile.clone(),
                "max_gas_cost_usd" => config.trading.max_gas_cost_usd.to_string(),
                "max_gas_cost_percentage" => config.trading.max_gas_cost_percentage.to_string(),
                "transaction_priority" => format!("{:?}", config.trading.transaction_priority),
                "max_execution_price_deviation" => {
                    config.trading.max_execution_price_deviation.to_string()
                }
                "min_portfolio_risk_factor" => config.trading.min_portfolio_risk_factor.to_string(),
                _ => {
                    return Err(Error::Config(format!(
                        "Unknown trading parameter: {}",
                        parts[1]
                    )))
                }
            }
        }
        "data_collection" => {
            if parts.len() < 2 {
                return Err(Error::Config("Please specify which data collection parameter (e.g., data_collection.scan_interval_secs)".to_string()));
            }

            match parts[1] {
                "interval" | "scan_interval_secs" => {
                    config.data_collection.scan_interval_secs.to_string()
                }
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
                    "Please specify which DEX parameter (e.g., dex.network)".to_string(),
                ));
            }

            match parts[1] {
                "network" => config
                    .dex
                    .network
                    .as_deref()
                    .unwrap_or("Not set")
                    .to_string(),
                "protocol" => config.dex.protocol.clone(),
                "custom_rpc_url" => config
                    .dex
                    .custom_rpc_url
                    .as_deref()
                    .unwrap_or("Not set")
                    .to_string(),
                "router_address" => config
                    .dex
                    .router_address
                    .as_deref()
                    .unwrap_or("Not set")
                    .to_string(),
                "weth_address" => config.dex.weth_address.as_deref().unwrap_or("Not set").to_string(),
                "stablecoin_address" => config.dex.stablecoin_address.as_deref().unwrap_or("Not set").to_string(),

                "wallet" => {
                    // Handle nested wallet parameters like dex.wallet.private_key_env
                    if parts.len() < 3 {
                        return Err(Error::Config(
                            "Please specify wallet parameter (e.g., dex.wallet.private_key_env)".to_string(),
                        ));
                    }

                    match parts[2] {
                        "private_key_env" => {
                            config.dex.wallet
                                .as_ref()
                                .and_then(|w| w.private_key_env.as_ref())
                                .unwrap_or(&"Not set".to_string())
                                .clone()
                        },
                        "private_key_file" => {
                            config.dex.wallet
                                .as_ref()
                                .and_then(|w| w.private_key_file.as_ref())
                                .unwrap_or(&"Not set".to_string())
                                .clone()
                        },
                        _ => {
                            return Err(Error::Config(format!(
                                "Unknown wallet parameter: {}. Supported: private_key_env, private_key_file",
                                parts[2]
                            )))
                        }
                    }
                }
                _ => {
                    return Err(Error::Config(format!(
                    "Unknown DEX parameter: {}. Supported: network, protocol, custom_rpc_url, router_address, weth_address, stablecoin_address, wallet",
                    parts[1]
                )))
                }
            }
        }
        "rpc" => {
            if parts.len() < 2 {
                return Err(Error::Config(
                    "Please specify which RPC parameter (e.g., rpc.primary_provider)".to_string(),
                ));
            }

            match parts[1] {
                "primary_provider" => config.rpc.primary_provider.clone(),
                _ => {
                    return Err(Error::Config(format!(
                        "Unknown RPC parameter: {}. Supported: primary_provider",
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
    Ok(())
}
