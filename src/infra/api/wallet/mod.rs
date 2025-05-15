use crate::core::error::Error;
use crate::core::models::wallet::{WalletInfo, TokenHolding, Transaction};
use log::{debug, info};
use reqwest::Client;
use serde_json::Value;

const BASE_URL: &str = "https://api.etherscan.io/api";

pub async fn get_wallet_info(address: &str, chain: &str) -> Result<WalletInfo, Error> {
    info!("Fetching wallet info for {} on {}", address, chain);
    
    let (balance, tokens) = get_wallet_holdings(address, chain).await?;
    let transactions = get_wallet_transactions(address, chain).await?;

    Ok(WalletInfo {
        address: address.to_string(),
        balance,
        tokens,
        transactions,
    })
}

async fn get_wallet_holdings(address: &str, chain: &str) -> Result<(f64, Vec<TokenHolding>), Error> {
    let config = crate::config::Config::load()?;
    let api_key = config.api_keys.etherscan
        .as_ref()
        .ok_or_else(|| Error::Config("Etherscan API key not set".to_string()))?;

    let client = Client::new();
    let url = format!("{}/module=account&action=balance&address={}&tag=latest", BASE_URL, address);
    debug!("Fetching wallet balance: {}", url);

    let response = client.get(&url)
        .query(&[("apikey", api_key)])
        .send()
        .await?
        .error_for_status()?;
        
    let data: Value = response.json().await?;
    
    let balance = data["result"]
        .as_str()
        .ok_or_else(|| Error::Parse("Invalid balance response".to_string()))?
        .parse::<f64>()
        .map_err(|_| Error::Parse("Failed to parse balance".to_string()))? / 1e18;

    // Get token holdings
    let tokens_url = format!("{}/module=account&action=tokentx&address={}&sort=desc", BASE_URL, address);
    debug!("Fetching token transactions: {}", tokens_url);

    let response = client.get(&tokens_url)
        .query(&[("apikey", api_key)])
        .send()
        .await?
        .error_for_status()?;
        
    let data: Value = response.json().await?;
    
    let mut tokens = parse_token_holdings(&data)
        .ok_or_else(|| Error::Parse("Failed to parse token holdings".to_string()))?;

    // Fetch current prices for tokens
    enrich_token_prices(&mut tokens).await?;

    Ok((balance, tokens))
}

async fn get_wallet_transactions(address: &str, chain: &str) -> Result<Vec<Transaction>, Error> {
    let config = crate::config::Config::load()?;
    let api_key = config.api_keys.etherscan
        .as_ref()
        .ok_or_else(|| Error::Config("Etherscan API key not set".to_string()))?;

    let client = Client::new();
    let url = format!("{}/module=account&action=txlist&address={}&sort=desc", BASE_URL, address);
    debug!("Fetching transactions: {}", url);

    let response = client.get(&url)
        .query(&[("apikey", api_key)])
        .send()
        .await?
        .error_for_status()?;
        
    let data: Value = response.json().await?;
    
    let transactions = data["result"]
        .as_array()
        .ok_or_else(|| Error::Parse("Invalid transaction response".to_string()))?
        .iter()
        .filter_map(|tx| parse_transaction(tx))
        .collect();

    Ok(transactions)
}

fn parse_transaction(tx: &Value) -> Option<Transaction> {
    Some(Transaction {
        hash: tx["hash"].as_str()?.to_string(),
        timestamp: tx["timeStamp"].as_str()?.parse().ok()?,
        tx_type: if tx["to"].as_str()? == "0x0000000000000000000000000000000000000000" {
            "Contract Creation".to_string()
        } else {
            "Transfer".to_string()
        },
        from: tx["from"].as_str()?.to_string(),
        to: tx["to"].as_str()?.to_string(),
        value: tx["value"].as_str()?.to_string(),
        token_transfers: None,
    })
}

fn parse_token_holdings(data: &Value) -> Option<Vec<TokenHolding>> {
    let mut token_map = std::collections::HashMap::new();
    
    if let Some(txs) = data["result"].as_array() {
        for tx in txs {
            let token_name = tx["tokenName"].as_str()?;
            let token_symbol = tx["tokenSymbol"].as_str()?;
            let balance = tx["value"].as_str()?.parse::<f64>().ok()?;
            
            token_map.entry(token_symbol.to_string())
                .and_modify(|holding: &mut TokenHolding| {
                    holding.balance = format!("{:.4}", balance);
                })
                .or_insert_with(|| TokenHolding {
                    name: token_name.to_string(),
                    symbol: token_symbol.to_string(),
                    balance: format!("{:.4}", balance),
                    price_usd: None,
                    value_usd: None,
                });
        }
    }
    
    Some(token_map.into_values().collect())
}

async fn enrich_token_prices(tokens: &mut Vec<TokenHolding>) -> Result<(), Error> {
    let client = Client::new();
    
    for token in tokens {
        let url = format!(
            "https://api.coingecko.com/api/v3/simple/price?ids={}&vs_currencies=usd",
            token.symbol.to_lowercase()
        );
        
        if let Ok(response) = client.get(&url).send().await {
            if let Ok(data) = response.json::<Value>().await {
                if let Some(price) = data[token.symbol.to_lowercase()]["usd"].as_f64() {
                    token.price_usd = Some(price);
                    if let Ok(balance) = token.balance.parse::<f64>() {
                        token.value_usd = Some(price * balance);
                    }
                }
            }
        }
    }
    
    Ok(())
} 