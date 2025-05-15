use crate::core::error::Error;
use crate::core::models::token::{DataProvider, TokenData};
use serde_json::Value;

/// Convert CoinGecko API response to our canonical TokenData models
pub fn convert_coingecko_to_token_data(coin_data: &Value) -> Result<TokenData, Error> {
    // Extract required fields with proper error handling
    let id = coin_data["id"]
        .as_str()
        .ok_or_else(|| Error::Parse("Missing token id in CoinGecko response".to_string()))?;

    let symbol = coin_data["symbol"]
        .as_str()
        .ok_or_else(|| Error::Parse("Missing token symbol in CoinGecko response".to_string()))?;

    let name = coin_data["name"]
        .as_str()
        .ok_or_else(|| Error::Parse("Missing token name in CoinGecko response".to_string()))?;

    let price = coin_data["current_price"].as_f64().ok_or_else(|| {
        Error::Parse("Missing or invalid price in CoinGecko response".to_string())
    })?;

    // Create the base TokenData with required fields
    let mut token_data = TokenData::new(id, symbol, name, price);

    // Add optional fields when available
    if let Some(price_change) = coin_data["price_change_percentage_24h"].as_f64() {
        token_data.price_change_24h = price_change;
    }

    if let Some(volume) = coin_data["total_volume"].as_f64() {
        token_data.volume_24h = volume;
    }

    if let Some(market_cap) = coin_data["market_cap"].as_f64() {
        token_data.market_cap = Some(market_cap);
    }

    if let Some(market_cap_rank) = coin_data["market_cap_rank"].as_u64() {
        token_data.market_cap_rank = Some(market_cap_rank as u32);
    }

    // Normalize data
    token_data.normalize();

    Ok(token_data)
}

/// Convert DEX API response to our canonical TokenData model
pub fn convert_dex_to_token_data(dex_data: &Value, chain: &str) -> Result<TokenData, Error> {
    // Extract required fields with proper error handling
    let address = dex_data["address"]
        .as_str()
        .ok_or_else(|| Error::Parse("Missing token address in DEX response".to_string()))?;

    let symbol = dex_data["symbol"]
        .as_str()
        .ok_or_else(|| Error::Parse("Missing token symbol in DEX response".to_string()))?;

    let name = dex_data["name"]
        .as_str()
        .ok_or_else(|| Error::Parse("Missing token name in DEX response".to_string()))?;

    let price = dex_data["price_usd"].as_f64().unwrap_or(0.0); // DEX tokens might not have price data

    // Create the base TokenData with required fields
    let mut token_data = TokenData::new(address, symbol, name, price);

    // Add dex-specific fields
    token_data.chain = chain.to_string();
    token_data.address = Some(address.to_string());

    // Add volume if available
    if let Some(volume) = dex_data["volume_24h"].as_f64() {
        token_data.volume_24h = volume;
    }

    // Normalize data
    token_data.normalize();

    Ok(token_data)
}

/// Batch convert multiple API responses to TokenData
pub fn batch_convert_to_token_data(data: &[Value], provider: DataProvider) -> Vec<TokenData> {
    match provider {
        DataProvider::CoinGecko => data
            .iter()
            .filter_map(|coin| convert_coingecko_to_token_data(coin).ok())
            .collect(),
        DataProvider::Database => {
            // Database conversion should use the From trait instead
            Vec::new()
        }
        DataProvider::Custom => {
            // Custom provider logic
            Vec::new()
        }
    }
}
