use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PositionDetails {
    // pub token_id: String, // Not strictly needed if key in HashMap is token_id
    pub entry_price: f64,
    pub size: f64, // quantity
    pub current_price: f64,
    pub unrealized_pnl: f64,
    // pub last_update_timestamp: Option<chrono::DateTime<chrono::Utc>>, // Consider adding later
}

#[derive(Debug, Clone)]
pub struct RiskManager {
    pub max_position_size: f64, // Max value (USD) of a single position at entry
    pub max_total_exposure: f64, // Max total value (USD) of all concurrent positions at entry
    current_total_value: f64,   // Current total market value of all positions
    initial_total_exposure: f64, // Sum of initial_value_at_entry for all open positions
    positions: HashMap<String, PositionDetails>, // Keyed by normalized token_id
}

impl RiskManager {
    pub fn new(max_position_size: f64, max_total_exposure: f64) -> Self {
        Self {
            max_position_size,
            max_total_exposure,
            current_total_value: 0.0,
            initial_total_exposure: 0.0,
            positions: HashMap::new(),
        }
    }

    pub fn calculate_position_size(&self, _token_price: f64, volatility: Option<f64>) -> f64 {
        // Base position size calculation (in USD)
        let mut base_size_usd = self.max_position_size;

        // This logic seems to be about adjusting the *USD amount* to invest,
        // not deriving units from a fixed USD amount based on token price brackets.
        // The original logic might need re-evaluation if max_position_size is a USD cap.
        // For now, assuming max_position_size is a USD cap and this function returns a target USD investment.

        // Example: if token_price is very high, one might still want to cap the USD invested.
        // if token_price > 1000.0 {
        //     // e.g. base_size_usd = base_size_usd.min(some_other_cap_for_expensive_tokens)
        // }

        // Further adjust for volatility if provided
        if let Some(vol) = volatility {
            // Reduce position size for higher volatility
            // Ensure vol is treated as a percentage, e.g., 0.0 to 100.0
            let vol_abs = vol.abs();
            let adjustment = 1.0 - (vol_abs.min(50.0) / 100.0); // Example: Max 50% reduction for vol
            base_size_usd *= adjustment;
        }

        base_size_usd.max(0.0) // Ensure it's not negative
    }

    // Checks if opening a new position of proposed_initial_value_usd would exceed max_total_exposure
    pub fn can_open_position(&self, proposed_initial_value_usd: f64) -> bool {
        (self.initial_total_exposure + proposed_initial_value_usd) <= self.max_total_exposure
    }

    pub fn add_position(&mut self, token_id: &str, size_quantity: f64, entry_price_usd: f64) {
        let initial_value = size_quantity * entry_price_usd;
        let details = PositionDetails {
            entry_price: entry_price_usd,
            size: size_quantity,
            current_price: entry_price_usd, // Initially, current_price is entry_price
            unrealized_pnl: 0.0,
        };
        self.positions.insert(token_id.to_string(), details);
        self.initial_total_exposure += initial_value;
        self.current_total_value += initial_value; // Current value also starts at initial value
    }

    pub fn remove_position(&mut self, token_id: &str) -> Option<PositionDetails> {
        if let Some(removed_position) = self.positions.remove(token_id) {
            let initial_value_removed = removed_position.size * removed_position.entry_price;
            self.initial_total_exposure -= initial_value_removed;
            self.current_total_value -= removed_position.size * removed_position.current_price; // Decrease by its last known current value
            Some(removed_position)
        } else {
            None
        }
    }

    pub fn update_market_data(&mut self, token_id: &str, new_price: f64) -> Option<f64> {
        let mut pnl_change_for_this_update: Option<f64> = None;
        if let Some(position) = self.positions.get_mut(token_id) {
            let old_position_value = position.size * position.current_price;

            position.current_price = new_price;
            position.unrealized_pnl =
                (position.current_price - position.entry_price) * position.size;

            let new_position_value = position.size * position.current_price;
            let value_change = new_position_value - old_position_value;
            self.current_total_value += value_change;
            pnl_change_for_this_update = Some(value_change); // PNL change from this specific price update

            log::trace!(
                "RiskManager (Domain): Updated token: {}, Old Price: Not Stored Directly, New Price: {:.4}, Size: {}, Entry: {:.4}, PnL: {:.2}, New Value: {:.2}",
                token_id, new_price, position.size, position.entry_price, position.unrealized_pnl, new_position_value
            );
        } else {
            log::trace!(
                "RiskManager (Domain): update_market_data called for token_id: {} (no position found), price: {}",
                token_id,
                new_price
            );
        }
        pnl_change_for_this_update
    }

    // Getter for current total value of all positions
    pub fn get_current_total_value(&self) -> f64 {
        self.current_total_value
    }

    // Getter for initial total exposure
    pub fn get_initial_total_exposure(&self) -> f64 {
        self.initial_total_exposure
    }

    // Getter for a specific position's details
    pub fn get_position_details(&self, token_id: &str) -> Option<&PositionDetails> {
        self.positions.get(token_id)
    }

    // Getter for all position details
    pub fn get_all_positions(&self) -> &HashMap<String, PositionDetails> {
        &self.positions
    }
}
