//! Configuration setter commands

use crate::config::{Config, WalletConfig};
use crate::error::{Error, Result};
use colored::*;
use serde_json::Value;

/// Handle the Set command to update configuration values
pub async fn handle_set(key: String, value_str: String) -> Result<()> {
    let mut config = Config::load()?;

    // Split the key into parts
    let parts: Vec<&str> = key.split('.').collect();

    if parts.is_empty() || parts.len() > 3 {
        return Err(Error::Config(format!("Invalid configuration key: {}", key)));
    }

    match parts[0] {
        "api_keys" => {
            if parts.len() < 2 {
                return Err(Error::Config(
                    "Please specify which API key (e.g., api_keys.infura)".to_string(),
                ));
            }

            match parts[1] {
                "infura" => config.api_keys.infura = Some(value_str.clone()),
                "alchemy" => config.api_keys.alchemy = Some(value_str.clone()),
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

            // Create JSON value for validation
            let _json_value = if let Ok(int_val) = value_str.parse::<u64>() {
                Value::Number(serde_json::Number::from(int_val))
            } else if let Ok(num) = value_str.parse::<f64>() {
                Value::Number(
                    serde_json::Number::from_f64(num)
                        .unwrap_or_else(|| serde_json::Number::from(0)),
                )
            } else if let Ok(bool_val) = value_str.parse::<bool>() {
                Value::Bool(bool_val)
            } else {
                Value::String(value_str.clone())
            };

            match parts[1] {
                "live_trading" => {
                    config.trading.live_trading = value_str.parse().map_err(|_| {
                        Error::Config(format!("Invalid boolean value: {}", value_str))
                    })?;
                }
                "scan_interval" => {
                    config.data_collection.scan_interval_secs =
                        value_str.parse().map_err(|_| {
                            Error::Config(format!("Invalid integer value: {}", value_str))
                        })?;
                }
                "max_position_size" => {
                    config.trading.max_position_size = value_str.parse().map_err(|_| {
                        Error::Config(format!("Invalid float value: {}", value_str))
                    })?;
                }
                "max_total_exposure" => {
                    config.trading.max_total_exposure = value_str.parse().map_err(|_| {
                        Error::Config(format!("Invalid float value: {}", value_str))
                    })?;
                }
                "strategy" => {
                    config.trading.strategy = value_str.clone();
                }
                "threshold" => {
                    config.trading.signal_confidence_threshold =
                        value_str.parse().map_err(|_| {
                            Error::Config(format!("Invalid float value: {}", value_str))
                        })?;
                }
                "min_volume" => {
                    config.trading.min_volume = value_str.parse().map_err(|_| {
                        Error::Config(format!("Invalid float value: {}", value_str))
                    })?;
                }
                "min_liquidity" => {
                    config.trading.min_liquidity = value_str.parse().map_err(|_| {
                        Error::Config(format!("Invalid float value: {}", value_str))
                    })?;
                }
                "min_pool_transaction_count" => {
                    config.trading.min_pool_transaction_count =
                        value_str.parse().map_err(|_| {
                            Error::Config(format!("Invalid integer value: {}", value_str))
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
                "max_volatility_24h" => {
                    config.trading.max_volatility_24h = value_str.parse().map_err(|_| {
                        Error::Config(format!("Invalid float value: {}", value_str))
                    })?;
                }
                "max_daily_loss" => {
                    config.trading.max_daily_loss = value_str
                        .parse()
                        .map_err(|e| Error::Config(format!("Invalid max daily loss: {}", e)))?;
                }
                "max_drawdown" => {
                    config.trading.max_drawdown = value_str
                        .parse()
                        .map_err(|e| Error::Config(format!("Invalid max drawdown: {}", e)))?;
                }
                "max_single_trade_risk_percentage_of_wallet" => {
                    config.trading.max_trade_risk_pct = value_str.parse().map_err(|e| {
                        Error::Config(format!(
                            "Invalid max single trade risk percentage of wallet: {}",
                            e
                        ))
                    })?;
                }
                "min_required_eth_balance_for_trading" => {
                    config.trading.min_native_balance = value_str.parse().map_err(|e| {
                        Error::Config(format!(
                            "Invalid min required ETH balance for trading: {}",
                            e
                        ))
                    })?;
                }
                "tokens_to_track" => {
                    // Parse JSON array
                    let tokens: Vec<String> = serde_json::from_str(&value_str)
                        .map_err(|e| Error::Config(format!("Invalid JSON array for tokens_to_track: {}. Expected format: '[\"token1\", \"token2\"]'", e)))?;
                    config.trading.tokens_to_track = tokens;
                }
                "max_tokens_to_scan" => {
                    config.trading.max_tokens_to_scan =
                        value_str.parse::<usize>().map_err(|_| {
                            Error::Config(format!(
                                "Invalid max_tokens_to_scan: {}. Must be a non-negative integer",
                                value_str
                            ))
                        })?;
                }
                "market_data_provider" => {
                    // Validate market data provider
                    let valid_providers = ["dexscreener_solana"];
                    if !valid_providers.contains(&value_str.as_str()) {
                        return Err(Error::Config(format!(
                            "Invalid market data provider: {}. Valid values: {}",
                            value_str,
                            valid_providers.join(", ")
                        )));
                    }
                    config.trading.market_data_provider = value_str.clone();
                }
                "min_position_size" => {
                    config.trading.min_position_size = value_str.parse().map_err(|_| {
                        Error::Config(format!("Invalid float value: {}", value_str))
                    })?;
                }
                "rsi_weight" => {
                    config.trading.rsi_weight = value_str.parse().map_err(|_| {
                        Error::Config(format!("Invalid float value: {}", value_str))
                    })?;
                }
                "macd_weight" => {
                    config.trading.macd_weight = value_str.parse().map_err(|_| {
                        Error::Config(format!("Invalid float value: {}", value_str))
                    })?;
                }
                "bollinger_weight" => {
                    config.trading.bollinger_weight = value_str.parse().map_err(|_| {
                        Error::Config(format!("Invalid float value: {}", value_str))
                    })?;
                }
                "volume_weight" => {
                    config.trading.volume_weight = value_str.parse().map_err(|_| {
                        Error::Config(format!("Invalid float value: {}", value_str))
                    })?;
                }
                "indicator_profile" => {
                    // Validate indicator profile
                    let valid_profiles = ["scalping", "day_trading", "swing_trading", "standard"];
                    if !valid_profiles.contains(&value_str.as_str()) {
                        return Err(Error::Config(format!(
                            "Invalid indicator profile: {}. Valid values: {} (use 'day_trading' for 60s scan intervals)",
                            value_str,
                            valid_profiles.join(", ")
                        )));
                    }
                    config.trading.indicator_profile = value_str.clone();
                }
                "max_gas_cost_usd" => {
                    config.trading.max_gas_cost_usd = value_str.parse().map_err(|_| {
                        Error::Config(format!("Invalid float value: {}", value_str))
                    })?;
                }
                "max_gas_cost_percentage" => {
                    config.trading.max_gas_cost_percentage = value_str.parse().map_err(|_| {
                        Error::Config(format!("Invalid float value: {}", value_str))
                    })?;
                }
                "transaction_priority" => {
                    config.trading.transaction_priority = match value_str.to_lowercase().as_str() {
                        "low" => crate::infrastructure::dex::TransactionPriority::Low,
                        "medium" => crate::infrastructure::dex::TransactionPriority::Medium,
                        "standard" => crate::infrastructure::dex::TransactionPriority::Standard,
                        "high" => crate::infrastructure::dex::TransactionPriority::High,
                        "urgent" => crate::infrastructure::dex::TransactionPriority::Urgent,
                        _ => return Err(Error::Config(format!(
                            "Invalid transaction_priority: {}. Must be one of: Low, Medium, Standard, High, Urgent",
                            value_str
                        ))),
                    };
                }
                "max_execution_price_deviation" => {
                    config.trading.max_execution_price_deviation =
                        value_str.parse().map_err(|_| {
                            Error::Config(format!("Invalid float value: {}", value_str))
                        })?;
                }
                "min_portfolio_risk_factor" => {
                    config.trading.min_portfolio_risk_factor = value_str.parse().map_err(|_| {
                        Error::Config(format!("Invalid float value: {}", value_str))
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
        "data_collection" => {
            if parts.len() < 2 {
                return Err(Error::Config("Please specify which data collection parameter (e.g., data_collection.scan_interval_secs)".to_string()));
            }

            match parts[1] {
                "interval" | "scan_interval_secs" => {
                    config.data_collection.scan_interval_secs =
                        value_str.parse().map_err(|_| {
                            Error::Config(format!("Invalid integer value: {}", value_str))
                        })?;
                }
                "history_days" => {
                    config.data_collection.history_days = value_str.parse().map_err(|_| {
                        Error::Config(format!("Invalid integer value: {}", value_str))
                    })?;
                }
                "auto_start" => {
                    config.data_collection.auto_start = value_str.parse().map_err(|_| {
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
                    "Please specify which DEX parameter (e.g., dex.network)".to_string(),
                ));
            }

            match parts[1] {
                "network" => config.dex.network = Some(value_str.clone()),
                "protocol" => {
                    // Validate protocol value
                    let valid_protocols = ["uniswap_v3"];
                    if !valid_protocols.contains(&value_str.as_str()) {
                        return Err(Error::Config(format!(
                            "Invalid protocol: {}. Supported protocols: {}", 
                            value_str,
                            valid_protocols.join(", ")
                        )));
                    }
                    config.dex.protocol = value_str.clone();
                },
                "custom_rpc_url" => config.dex.custom_rpc_url = Some(value_str.clone()),
                "router_address" => config.dex.router_address = Some(value_str.clone()),
                "weth_address" => config.dex.weth_address = Some(value_str.clone()),
                "stablecoin_address" => config.dex.stablecoin_address = Some(value_str.clone()),

                "wallet" => {
                    // Handle nested wallet parameters like dex.wallet.private_key_env
                    if parts.len() < 3 {
                        return Err(Error::Config(
                            "Please specify wallet parameter (e.g., dex.wallet.private_key_env)".to_string(),
                        ));
                    }

                    match parts[2] {
                        "private_key_env" => {
                            // Initialize wallet config if it doesn't exist
                            if config.dex.wallet.is_none() {
                                config.dex.wallet = Some(WalletConfig {
                                    private_key_env: None,
                                    private_key_file: None,
                                });
                            }
                            // Safe: we just ensured wallet is Some above
                            if let Some(wallet) = config.dex.wallet.as_mut() {
                                wallet.private_key_env = Some(value_str.clone());
                            }
                        },
                        "private_key_file" => {
                            // Initialize wallet config if it doesn't exist
                            if config.dex.wallet.is_none() {
                                config.dex.wallet = Some(WalletConfig {
                                    private_key_env: None,
                                    private_key_file: None,
                                });
                            }
                            // Safe: we just ensured wallet is Some above
                            if let Some(wallet) = config.dex.wallet.as_mut() {
                                wallet.private_key_file = Some(value_str.clone());
                            }
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
                "primary_provider" => {
                    // Validate primary provider value
                    let valid_providers = ["infura", "alchemy", "auto"];
                    if !valid_providers.contains(&value_str.as_str()) {
                        return Err(Error::Config(format!(
                            "Invalid primary provider: {}. Supported providers: {}",
                            value_str,
                            valid_providers.join(", ")
                        )));
                    }
                    config.rpc.primary_provider = value_str.clone();
                }
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
    }

    // Save the configuration
    config.save()?;

    println!(
        "{} Configuration value {} updated to: {}",
        "✓".green(),
        key.cyan(),
        value_str
    );

    Ok(())
}
