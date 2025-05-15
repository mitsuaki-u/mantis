use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct RiskManager {
    pub max_position_size: f64,
    pub max_total_exposure: f64,
    current_exposure: f64,
    positions: HashMap<String, f64>,
}

impl RiskManager {
    pub fn new(max_position_size: f64, max_total_exposure: f64) -> Self {
        Self {
            max_position_size,
            max_total_exposure,
            current_exposure: 0.0,
            positions: HashMap::new(),
        }
    }

    pub fn calculate_position_size(&self, token_price: f64, volatility: Option<f64>) -> f64 {
        // Base position size calculation
        let mut base_size = self.max_position_size;
        
        // Adjust position size based on token price
        // For expensive tokens, ensure we don't go over max size
        if token_price > 1000.0 {
            // For tokens over $1000, reduce the position size to ensure reasonable quantities
            let units = base_size / token_price;
            if units < 0.1 {
                // Ensure we buy at least 0.1 units of the token
                base_size = 0.1 * token_price;
            }
        } else if token_price < 0.1 {
            // For very cheap tokens, ensure position size is meaningful
            let units = base_size / token_price;
            if units > 10000.0 {
                // Cap at 10000 units for cheap tokens to limit exposure
                base_size = 10000.0 * token_price;
            }
        }
        
        // Further adjust for volatility if provided
        if let Some(vol) = volatility {
            // Reduce position size for higher volatility
            let adjustment = 1.0 - (vol.min(50.0) / 100.0);
            return base_size * adjustment;
        }
        
        base_size
    }

    pub fn can_open_position(&self, current_exposure: f64, position_size: f64) -> bool {
        current_exposure + position_size <= self.max_total_exposure
    }

    pub fn add_position(&mut self, token_id: &str, size: f64) {
        self.positions.insert(token_id.to_string(), size);
        self.current_exposure += size;
    }

    pub fn remove_position(&mut self, token_id: &str) {
        if let Some(size) = self.positions.remove(token_id) {
            self.current_exposure -= size;
        }
    }
} 