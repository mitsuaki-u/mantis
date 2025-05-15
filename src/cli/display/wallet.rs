use colored::*;
use crate::core::models::wallet::WalletInfo;
use chrono::{DateTime, Utc};

// Display constants for table formatting
const ADDRESS_WIDTH: usize = 42;    // Standard ETH address length
const HASH_WIDTH: usize = 66;       // Standard tx hash length
const TOKEN_NAME_WIDTH: usize = 20; // Max width for token name
const VALUE_WIDTH: usize = 15;      // Width for value columns

pub fn display_wallet_info(info: &WalletInfo, address: &str) {
    println!("\n👛 {} {}", "Wallet Information".bright_yellow(), "────────────────────────");
    println!("Address: {}", address);
    println!("Balance: {} ETH", info.balance);

    if !info.tokens.is_empty() {
        println!("\n🪙 Token Holdings");
        println!("────────────────────────────────────");
        for token in &info.tokens {
            println!("Token: {} ({})", 
                token.name.bright_white(), 
                token.symbol.bright_yellow()
            );
            println!("Balance: {}", token.balance);
            if let Some(price) = token.price_usd {
                println!("Price: ${:.4}", price);
            }
            if let Some(value) = token.value_usd {
                println!("Value: ${:.2}", value);
            }
            println!("────────────────────────────────────");
        }
    }

    if !info.transactions.is_empty() {
        display_transactions(info);
    }
}

fn display_transactions(info: &WalletInfo) {
    println!("\n📝 Recent Transactions");
    println!("────────────────────────────────────");
    for tx in &info.transactions {
        let dt = DateTime::<Utc>::from_timestamp(tx.timestamp, 0)
            .unwrap()
            .format("%Y-%m-%d %H:%M:%S");
            
        println!("Hash: {:.width$}", tx.hash, width = HASH_WIDTH);
        println!("Time: {}", dt);
        println!("Type: {}", tx.tx_type);
        println!("From: {:.width$}", tx.from, width = ADDRESS_WIDTH);
        println!("To: {:.width$}", tx.to, width = ADDRESS_WIDTH);
        println!("Value: {:.value$}", tx.value, value = VALUE_WIDTH);
        
        if let Some(transfers) = &tx.token_transfers {
            println!("Token Transfers:");
            for transfer in transfers {
                println!("  {:.value$} {:<name_width$} -> {}", 
                    transfer.value,
                    transfer.symbol,
                    if transfer.to == info.address { 
                        "IN".bright_green() 
                    } else { 
                        "OUT".bright_red() 
                    },
                    value = VALUE_WIDTH,
                    name_width = TOKEN_NAME_WIDTH
                );
            }
        }
        println!("────────────────────────────────────");
    }
} 