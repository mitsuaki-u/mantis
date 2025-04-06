use crate::error::Error;
use log::{debug, error, warn};
use reqwest::Client;
use serde_json::Value;
use tokio::time::{sleep, Duration};

const BASE_URL: &str = "https://api.coingecko.com/api/v3";
const RETRY_DELAY: u64 = 30; // Delay in seconds before retrying after rate limit

pub async fn get_market_data(limit: usize) -> Result<Value, Error> {
    debug!("Fetching market data for top {} tokens", limit);
    
    let url = format!(
        "{}/coins/markets?vs_currency=usd&order=market_cap_desc&per_page={}&page=1&sparkline=false&price_change_percentage=24h",
        BASE_URL, limit
    );
    debug!("CoinGecko URL: {}", url);
    
    make_request(&url).await
}

pub async fn get_trending() -> Result<Value, Error> {
    debug!("Fetching trending tokens");
    let url = format!("{}/search/trending", BASE_URL);
    debug!("CoinGecko URL: {}", url);
    
    make_request(&url).await
}

async fn make_request(url: &str) -> Result<Value, Error> {
    let client = Client::builder()
        .user_agent("Mozilla/5.0")
        .build()?;
        
    let response = client.get(url)
        .header("accept", "application/json")
        .send()
        .await?;

    match response.status() {
        reqwest::StatusCode::OK => {
            let data = response.json().await?;
            Ok(data)
        }
        reqwest::StatusCode::TOO_MANY_REQUESTS => {
            warn!("Rate limit exceeded, waiting {} seconds before retry...", RETRY_DELAY);
            sleep(Duration::from_secs(RETRY_DELAY)).await;
            Err(Error::RateLimit("CoinGecko rate limit exceeded".to_string()))
        }
        status => {
            error!("CoinGecko API error: {}", status);
            Err(Error::Api(format!("CoinGecko API error: {}", status)))
        }
    }
} 