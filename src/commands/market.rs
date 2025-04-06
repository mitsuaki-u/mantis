use clap::Subcommand;
use colored::*;
use crate::error::Error;
use crate::api::market::get_market_overview;
use crate::types::market::MarketOptions;
use crate::display::market::{display_token_metrics, display_trending_tokens};
use log::error;

#[derive(Subcommand)]
pub enum MarketCommands {
    /// Get market overview including trending, gainers, and losers
    Overview {
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },
    /// Get trending tokens
    Trending {
        #[arg(short, long, default_value = "50")]
        limit: usize,
    },
    /// Get top gainers
    Gainers {
        #[arg(short, long, default_value = "50")]
        limit: usize,
        #[arg(long)]
        min_cap: Option<f64>,
        #[arg(long)]
        max_cap: Option<f64>,
    },
    /// Get top losers
    Losers {
        #[arg(short, long, default_value = "50")]
        limit: usize,
        #[arg(long)]
        min_cap: Option<f64>,
        #[arg(long)]
        max_cap: Option<f64>,
    },
}

pub async fn handle_market_command(command: MarketCommands) -> Result<(), Error> {
    match command {
        MarketCommands::Overview { limit } => {
            let options = MarketOptions {
                limit,
                min_market_cap: None,
                max_market_cap: None,
            };
            match get_market_overview(options).await {
                Ok(overview) => {
                    println!("\n📈 {} (Top {})", "Trending Tokens".bright_yellow(), limit);
                    display_trending_tokens(&overview.trending);
                    println!("\n🚀 {} (Top {})", "Top Gainers".bright_green(), limit);
                    display_token_metrics(&overview.gainers);
                    println!("\n💥 {} (Top {})", "Top Losers".bright_red(), limit);
                    display_token_metrics(&overview.losers);
                }
                Err(e) => error!("Failed to fetch market overview: {}", e),
            }
        }
        MarketCommands::Trending { limit } => {
            let options = MarketOptions {
                limit,
                min_market_cap: None,
                max_market_cap: None,
            };
            match get_market_overview(options).await {
                Ok(overview) => {
                    println!("\n📈 {} (Top {})", "Trending Tokens".bright_yellow(), limit);
                    display_trending_tokens(&overview.trending);
                }
                Err(e) => error!("Failed to fetch trending tokens: {}", e),
            }
        }
        MarketCommands::Gainers { limit, min_cap, max_cap } => {
            let options = MarketOptions {
                limit,
                min_market_cap: min_cap,
                max_market_cap: max_cap,
            };
            match get_market_overview(options).await {
                Ok(overview) => {
                    println!("\n🚀 {} (Top {})", "Top Gainers".bright_green(), limit);
                    if min_cap.is_some() || max_cap.is_some() {
                        println!("Market Cap Filter: ${:.0}M - ${:.0}M", 
                            min_cap.unwrap_or(0.0),
                            max_cap.unwrap_or(f64::INFINITY)
                        );
                    }
                    display_token_metrics(&overview.gainers);
                }
                Err(e) => error!("Failed to fetch top gainers: {}", e),
            }
        }
        MarketCommands::Losers { limit, min_cap, max_cap } => {
            let options = MarketOptions {
                limit,
                min_market_cap: min_cap,
                max_market_cap: max_cap,
            };
            match get_market_overview(options).await {
                Ok(overview) => {
                    println!("\n💥 {} (Top {})", "Top Losers".bright_red(), limit);
                    if min_cap.is_some() || max_cap.is_some() {
                        println!("Market Cap Filter: ${:.0}M - ${:.0}M", 
                            min_cap.unwrap_or(0.0),
                            max_cap.unwrap_or(f64::INFINITY)
                        );
                    }
                    display_token_metrics(&overview.losers);
                }
                Err(e) => error!("Failed to fetch top losers: {}", e),
            }
        }
    }
    Ok(())
} 