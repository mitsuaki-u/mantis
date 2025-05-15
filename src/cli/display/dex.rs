use colored::*;
use crate::core::models::dex::{DexPair, DexStats};

// Display constants for table formatting
const SYMBOL_WIDTH: usize = 10;     // Width for token symbols
const PRICE_WIDTH: usize = 12;      // Width for price values
const VOLUME_WIDTH: usize = 15;     // Width for volume values
const LIQUIDITY_WIDTH: usize = 15;  // Width for liquidity values

pub fn display_dex_pairs(pairs: &[DexPair]) {
    println!("\n🔄 {} {}", "DEX Pairs".bright_blue(), "────────────────────────");
    for pair in pairs {
        println!("Pair: {:<width$} / {:<width$}", 
            pair.token0.symbol,
            pair.token1.symbol,
            width = SYMBOL_WIDTH
        );
        println!("Price: ${:>width$.4}", pair.price, width = PRICE_WIDTH);
        println!("Volume 24h: ${:>width$.2}", pair.volume_24h, width = VOLUME_WIDTH);
        println!("Liquidity: ${:>width$.2}", pair.liquidity, width = LIQUIDITY_WIDTH);
        println!("────────────────────────────────────");
    }
}

pub fn display_dex_stats(stats: &DexStats, dex: &str) {
    println!("\n📊 {} Stats {}", dex.bright_blue(), "────────────────────────");
    println!("Total Volume 24h: ${:>width$.2}M", 
        stats.volume_24h / 1_000_000.0,
        width = VOLUME_WIDTH
    );
    println!("Total Liquidity: ${:>width$.2}M", 
        stats.total_liquidity / 1_000_000.0,
        width = LIQUIDITY_WIDTH
    );
    println!("Number of Pairs: {}", stats.pair_count);
} 