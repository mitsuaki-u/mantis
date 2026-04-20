//! GraphQL query execution for Uniswap V3 subgraph

use crate::infrastructure::errors::{Error, Result};
use log::{debug, info, warn};
use reqwest::Client;

/// Execute GraphQL query against the configured Uniswap V3 subgraph with retry logic
pub(super) async fn execute_v3_query(
    client: &Client,
    subgraph_url: &str,
    api_key: Option<&str>,
    query: &str,
) -> Result<serde_json::Value> {
    const MAX_RETRIES: u32 = 3;
    const INITIAL_BACKOFF_MS: u64 = 500;

    debug!("Executing V3 subgraph query against {}", subgraph_url);

    let mut last_error = None;

    for attempt in 1..=MAX_RETRIES {
        match execute_v3_query_once(client, &subgraph_url, api_key, query).await {
            Ok(data) => {
                if attempt > 1 {
                    info!(
                        "✅ Query succeeded on retry attempt {}/{}",
                        attempt, MAX_RETRIES
                    );
                }
                return Ok(data);
            }
            Err(e) => {
                last_error = Some(e);

                if attempt < MAX_RETRIES {
                    let backoff_ms = INITIAL_BACKOFF_MS * (2_u64.pow(attempt - 1));
                    warn!(
                        "⚠️ Query failed (attempt {}/{}), retrying in {}ms: {}",
                        attempt,
                        MAX_RETRIES,
                        backoff_ms,
                        last_error.as_ref().unwrap()
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(backoff_ms)).await;
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        Error::Network("Query failed after all retries with unknown error".to_string())
    }))
}

/// Execute a single query attempt without retry
async fn execute_v3_query_once(
    client: &Client,
    url: &str,
    api_key: Option<&str>,
    query: &str,
) -> Result<serde_json::Value> {
    let mut request = client
        .post(url)
        .json(&serde_json::json!({ "query": query }));

    if let Some(key) = api_key {
        request = request.header("Authorization", format!("Bearer {}", key));
    }

    let response = request.send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                Error::Network(format!("Subgraph query timeout: {}", e))
            } else if e.is_connect() {
                Error::Network(format!("Failed to connect to subgraph: {}", e))
            } else {
                Error::Network(format!("Subgraph query failed: {}", e))
            }
        })?;

    let status = response.status();
    if !status.is_success() {
        if status.as_u16() == 429 {
            return Err(Error::RateLimit(
                "Subgraph rate limit exceeded, will retry with backoff".to_string(),
            ));
        }

        if status.is_server_error() {
            return Err(Error::Network(format!(
                "Subgraph server error ({}), may be transient",
                status
            )));
        }

        return Err(Error::Api(format!(
            "Subgraph returned error status: {}",
            status
        )));
    }

    let data: serde_json::Value = response
        .json()
        .await
        .map_err(|e| Error::Parse(format!("Failed to parse subgraph response: {}", e)))?;

    if let Some(errors) = data.get("errors") {
        return Err(Error::Api(format!("Subgraph GraphQL errors: {}", errors)));
    }

    Ok(data)
}
