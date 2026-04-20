//! Token quality filtering to avoid scam/suspicious tokens

use crate::core::constants::{MAX_TOKEN_SYMBOL_LENGTH, MIN_TOKEN_SYMBOL_LENGTH};
use log::warn;

/// Check if a token passes quality filters to avoid scam/suspicious tokens
pub(super) fn is_token_quality_acceptable(symbol: &str, _name: &str, address: &str) -> bool {
    if is_suspicious_address(address) {
        warn!("🚫 Rejecting token {} - suspicious address pattern", symbol);
        return false;
    }

    if symbol.len() < MIN_TOKEN_SYMBOL_LENGTH || symbol.len() > MAX_TOKEN_SYMBOL_LENGTH {
        warn!(
            "🚫 Rejecting token {} - invalid symbol length ({})",
            symbol,
            symbol.len()
        );
        return false;
    }

    true
}

/// Check for suspicious address patterns
fn is_suspicious_address(address: &str) -> bool {
    let addr_lower = address.to_lowercase();

    use crate::infrastructure::dex::ethereum::config::addresses::NetworkAddresses;
    let mainnet_addresses = NetworkAddresses::mainnet();
    let usdt_addr = format!("{:?}", mainnet_addresses.usdt).to_lowercase();
    let usdc_addr = format!("{:?}", mainnet_addresses.usdc).to_lowercase();
    let weth_addr = format!("{:?}", mainnet_addresses.weth).to_lowercase();
    let wbtc_addr = format!("{:?}", mainnet_addresses.wbtc).to_lowercase();

    let known_tokens = [
        (usdt_addr.as_str(), "USDT"), // Real USDT
        (usdc_addr.as_str(), "USDC"), // Real USDC
        (weth_addr.as_str(), "WETH"), // Real WETH
        (wbtc_addr.as_str(), "WBTC"), // Real WBTC
    ];

    for (known_addr, known_symbol) in &known_tokens {
        if addr_lower.starts_with(&known_addr[..10]) && addr_lower != *known_addr {
            warn!(
                "🚫 Suspicious address {} - similar to {} but not exact match",
                address, known_symbol
            );
            return true;
        }
    }

    false
}
