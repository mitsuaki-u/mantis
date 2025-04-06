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
        // Simple implementation - use max position size
        // Could be enhanced with position sizing based on volatility
        if let Some(vol) = volatility {
            // Reduce position size for higher volatility
            let adjustment = 1.0 - (vol.min(50.0) / 100.0);
            return self.max_position_size * adjustment;
        }
        
        self.max_position_size
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