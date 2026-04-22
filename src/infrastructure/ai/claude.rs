use crate::infrastructure::errors::{Error, Result};
use log::{debug, warn};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const MODEL: &str = "claude-haiku-4-5-20251001";
const MAX_TOKENS: u32 = 256;

/// Result of Claude's analysis of a trading signal
#[derive(Debug, Clone)]
pub struct AIDecision {
    pub approve: bool,
    pub confidence: u8, // 0-100
    pub reasoning: String,
}

/// Thin client for Anthropic Claude — used by the AI Advisor actor.
///
/// Uses claude-haiku-4-5 (fastest, cheapest) with a cached system prompt.
/// Each signal analysis costs ~$0.001 with prompt caching enabled.
#[derive(Clone)]
pub struct ClaudeAdvisor {
    client: Client,
    api_key: String,
}

impl ClaudeAdvisor {
    pub fn new(api_key: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self { client, api_key }
    }

    /// Analyse a BUY signal and return APPROVE/REJECT with reasoning.
    pub async fn analyse_signal(
        &self,
        symbol: &str,
        mint: &str,
        rsi: f64,
        bollinger_pct: f64,
        volume_24h: f64,
        price_change_24h: f64,
        price_usd: f64,
        momentum_score: f64,
        open_positions: usize,
        max_positions: usize,
        daily_pnl_pct: f64,
    ) -> Result<AIDecision> {
        let mint_short = &mint[..mint.len().min(8)];

        let user_content = format!(
            "TOKEN: {symbol} ({mint_short})\n\
             SIGNAL: BUY\n\
             STRATEGY: Momentum\n\
             \n\
             TECHNICAL INDICATORS:\n\
             - RSI: {rsi:.1} ({rsi_label})\n\
             - Bollinger position: {bollinger_pct:.0}% of band\n\
             - Momentum score: {momentum_score:.2}\n\
             - 24h price change: {price_change_24h:+.1}%\n\
             - Price: ${price_usd:.6}\n\
             - 24h volume: ${volume_24h:.0}\n\
             \n\
             PORTFOLIO STATE:\n\
             - Open positions: {open_positions}/{max_positions}\n\
             - Daily P&L: {daily_pnl_pct:+.1}%",
            rsi_label = if rsi < 30.0 { "oversold" } else if rsi > 70.0 { "overbought" } else { "neutral" },
        );

        let body = AnthropicRequest {
            model: MODEL.to_string(),
            max_tokens: MAX_TOKENS,
            system: vec![SystemContent {
                content_type: "text".to_string(),
                text: SYSTEM_PROMPT.to_string(),
                cache_control: Some(CacheControl { cache_type: "ephemeral".to_string() }),
            }],
            messages: vec![Message {
                role: "user".to_string(),
                content: user_content,
            }],
        };

        let response = self
            .client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "prompt-caching-2024-07-31")
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Network(format!("Claude API request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::Api(format!("Claude API error {}: {}", status, text)));
        }

        let resp: AnthropicResponse = response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("Failed to parse Claude response: {}", e)))?;

        let text = resp
            .content
            .into_iter()
            .find(|c| c.content_type == "text")
            .map(|c| c.text)
            .unwrap_or_default();

        debug!("Claude raw response: {}", text);
        parse_decision(&text)
    }
}

/// Parse Claude's structured response into an AIDecision.
fn parse_decision(text: &str) -> Result<AIDecision> {
    let text = text.trim();
    let upper = text.to_uppercase();

    let approve = if upper.contains("DECISION: APPROVE") {
        true
    } else if upper.contains("DECISION: REJECT") {
        false
    } else {
        // Fallback: approve if APPROVE appears more than REJECT
        upper.matches("APPROVE").count() > upper.matches("REJECT").count()
    };

    let confidence = text
        .lines()
        .find(|l| l.to_uppercase().contains("CONFIDENCE:"))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|s| s.trim().parse::<u8>().ok())
        .unwrap_or(50);

    let reasoning = text
        .lines()
        .find(|l| l.to_uppercase().starts_with("REASONING:"))
        .and_then(|l| l.splitn(2, ':').nth(1))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| {
            // Use full text as fallback if format doesn't match
            warn!("Claude response did not match expected format, using full text");
            text.lines().last().unwrap_or("No reasoning provided").to_string()
        });

    Ok(AIDecision { approve, confidence, reasoning })
}

const SYSTEM_PROMPT: &str = "\
You are Mantis, an AI trading agent on Solana. You analyse momentum trading \
signals generated by technical indicators and decide whether to approve or reject them.\
\n\nBe concise. Consider momentum strength, risk, and portfolio context.\
\n\nAlways respond in exactly this format:\
\nDECISION: APPROVE or REJECT\
\nCONFIDENCE: 0-100\
\nREASONING: One sentence maximum.";

// ── Anthropic API types ───────────────────────────────────────────────────────

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    system: Vec<SystemContent>,
    messages: Vec<Message>,
}

#[derive(Serialize)]
struct SystemContent {
    #[serde(rename = "type")]
    content_type: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl>,
}

#[derive(Serialize)]
struct CacheControl {
    #[serde(rename = "type")]
    cache_type: String,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<ResponseContent>,
}

#[derive(Deserialize)]
struct ResponseContent {
    #[serde(rename = "type")]
    content_type: String,
    text: String,
}
