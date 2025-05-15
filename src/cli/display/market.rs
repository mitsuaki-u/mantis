use colored::*;
use tabled::{Tabled, settings::Style};
use crate::core::models::market::{TokenMetrics, TrendingToken};
use unicode_truncate::UnicodeTruncateStr;
use tabled::builder::Builder;

// Update constants to be the content width without symbols
const NAME_WIDTH: usize = 30;     // Max width for name+symbol

// Let's create a simpler struct without attributes first
#[derive(Tabled)]
struct TokenRow {
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Rank")]
    rank: String,
    #[tabled(rename = "Price")]
    price: String,
    #[tabled(rename = "24h %")]
    change: String,
    #[tabled(rename = "Volume")]
    volume: String,
}

fn get_rank_color(rank: usize) -> ColoredString {
    let rank_str = format!("#{}", rank);
    match rank {
        1..=10 => rank_str.bright_yellow(),
        11..=50 => rank_str.bright_green(),
        51..=100 => rank_str.bright_blue(),
        101..=500 => rank_str.bright_magenta(),
        _ => rank_str.dimmed(),
    }
}

fn format_token_name(name: &str, symbol: &str) -> String {
    let full_name = format!("{} ({})", name, symbol);
    let max_width = NAME_WIDTH;
    
    if full_name.chars().count() > max_width {
        // Only if the name is actually longer than max_width, truncate and add "..."
        let (truncated, _) = full_name.unicode_truncate(max_width - 3);
        format!("{}...", truncated)
    } else {
        // If not truncated, just pad with spaces to fill the width
        format!("{:<width$}", full_name, width = max_width)
    }
}

pub fn display_token_metrics(tokens: &[TokenMetrics]) {
    if tokens.is_empty() {
        println!("No tokens found");
        return;
    }

    // First, create a builder with explicit headers
    let mut builder = Builder::default();
    builder.push_record(["Name", "Rank", "Price", "24h %", "Volume"]);
    
    // Add data rows without ANSI color codes
    for token in tokens {
        let name = format_token_name(&token.name, &token.symbol);
        let rank = token.market_cap_rank
            .map(|r| format!("#{}", r))
            .unwrap_or_else(|| "-".to_string());
        let price = format!("${:.2}", token.price_usd);
        let change = format!("{:+.2}%", token.price_change_24h);
        let volume = format!("${:.1}M", token.volume_24h / 1_000_000.0);
        
        builder.push_record([name, rank, price, change, volume]);
    }
    
    let mut table = builder.build();
    
    // Apply styling
    table.with(Style::modern());
    
    println!("{}", table);
}

pub fn display_trending_tokens(tokens: &[TrendingToken]) {
    if tokens.is_empty() {
        println!("No trending tokens found");
        return;
    }

    // First, create a builder with explicit headers
    let mut builder = Builder::default();
    builder.push_record(["Name", "Rank", "Price", "24h %", "Volume"]);
    
    // Add data rows without ANSI color codes
    for token in tokens {
        let name = format_token_name(&token.name, &token.symbol);
        let rank = format!("#{}", token.market_cap_rank);
        let price = format!("${:.2}", token.price_usd);
        let change = format!("{:+.2}%", token.price_change_24h);
        let volume = token.volume_24h
            .map(|v| format!("${:.1}M", v / 1_000_000.0))
            .unwrap_or_else(|| "-".to_string());
        
        builder.push_record([name, rank, price, change, volume]);
    }
    
    let mut table = builder.build();
    
    // Apply styling
    table.with(Style::modern());
    
    println!("{}", table);
} 