use crate::types::market::TokenMetrics;

pub struct TechnicalAnalysis;

impl TechnicalAnalysis {
    pub fn calculate_rsi(prices: &[f64], period: usize) -> Option<f64> {
        if prices.len() < period + 1 {
            return None;
        }
        
        let mut gains = 0.0;
        let mut losses = 0.0;
        
        for i in 1..=period {
            let diff = prices[prices.len() - i] - prices[prices.len() - i - 1];
            if diff >= 0.0 {
                gains += diff;
            } else {
                losses -= diff;
            }
        }
        
        if losses == 0.0 {
            return Some(100.0);
        }
        
        let rs = gains / losses;
        let rsi = 100.0 - (100.0 / (1.0 + rs));
        
        Some(rsi)
    }
    
    pub fn is_overbought(token: &TokenMetrics) -> bool {
        // Simple example: consider a token overbought if it's up more than 20% in 24h
        token.price_change_24h > 20.0
    }
    
    pub fn is_oversold(token: &TokenMetrics) -> bool {
        // Simple example: consider a token oversold if it's down more than 20% in 24h
        token.price_change_24h < -20.0
    }
    
    pub fn calculate_moving_average(prices: &[f64], period: usize) -> Option<f64> {
        if prices.len() < period {
            return None;
        }
        
        let sum: f64 = prices[prices.len() - period..].iter().sum();
        Some(sum / period as f64)
    }
} 