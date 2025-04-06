use crate::error::Error;
use crate::types::news::NewsItem;
use log::{debug, error};
use reqwest::Client;
use serde_json::Value;
use chrono::DateTime;

const BASE_URL: &str = "https://min-api.cryptocompare.com/data/v2";

pub async fn get_token_news(token_id: &str, symbol: &str) -> Result<Option<NewsItem>, Error> {
    debug!("Fetching news for {} ({})", symbol, token_id);
    
    let config = crate::config::Config::load()?;
    let api_key = config.api_keys.cryptocompare
        .as_ref()
        .ok_or_else(|| Error::Config("CryptoCompare API key not set".to_string()))?;

    let url = format!("{}/news/", BASE_URL);
    debug!("CryptoCompare URL: {}", url);
    
    let client = Client::new();
    let response = client.get(&url)
        .header("authorization", format!("Apikey {}", api_key))
        .query(&[
            ("lang", "EN"),
            ("excludeCategories", "Sponsored"),
            ("categories", &format!("{}|Market", symbol.to_uppercase())),
        ])
        .send()
        .await?;

    match response.status() {
        reqwest::StatusCode::OK => {
            let data: Value = response.json().await?;
            parse_news_response(data)
        }
        status => {
            error!("CryptoCompare API error: {}", status);
            Err(Error::Api(format!("CryptoCompare API error: {}", status)))
        }
    }
}

fn parse_news_response(data: Value) -> Result<Option<NewsItem>, Error> {
    if let Some(news) = data["Data"].as_array() {
        if let Some(latest) = news.first() {
            return Ok(Some(NewsItem {
                title: latest["title"].as_str().unwrap_or_default().to_string(),
                url: latest["url"].as_str().unwrap_or_default().to_string(),
                source: latest["source"].as_str().unwrap_or_default().to_string(),
                published_at: DateTime::from_timestamp(
                    latest["published_on"].as_i64().unwrap_or_default(),
                    0,
                ).unwrap_or_default(),
                categories: latest["categories"]
                    .as_str()
                    .unwrap_or_default()
                    .split('|')
                    .map(|s| s.to_string())
                    .collect(),
            }));
        }
    }

    Ok(None)
} 