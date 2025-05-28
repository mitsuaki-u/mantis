use std::collections::HashMap;

/// Risk state tracking for the risk manager
#[derive(Debug, Clone)]
pub struct RiskState {
    pub current_daily_loss: f64,
    pub current_drawdown: f64,
    pub max_daily_loss_limit: f64,
    pub max_drawdown_limit: f64,
    pub token_risks: HashMap<String, f64>,
    pub risk_scores: HashMap<String, f64>,
}

impl RiskState {
    pub fn new(max_daily_loss_limit: f64, max_drawdown_limit: f64) -> Self {
        Self {
            current_daily_loss: 0.0,
            current_drawdown: 0.0,
            max_daily_loss_limit,
            max_drawdown_limit,
            token_risks: HashMap::new(),
            risk_scores: HashMap::new(),
        }
    }

    pub fn update_daily_loss(&mut self, loss: f64) {
        self.current_daily_loss += loss;
    }

    pub fn update_drawdown(&mut self, drawdown: f64) {
        self.current_drawdown = drawdown;
    }

    pub fn is_daily_loss_exceeded(&self) -> bool {
        self.current_daily_loss >= self.max_daily_loss_limit
    }

    pub fn is_drawdown_exceeded(&self) -> bool {
        self.current_drawdown >= self.max_drawdown_limit
    }
}

/// Token-specific risk information
#[derive(Debug, Clone)]
pub struct TokenRisk {
    pub token_id: String,
    pub risk_score: f64,
    pub volatility: f64,
    pub liquidity_score: f64,
    pub market_cap_score: f64,
    pub volume_score: f64,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

impl TokenRisk {
    pub fn new(token_id: String) -> Self {
        Self {
            token_id,
            risk_score: 0.5, // Default medium risk
            volatility: 0.0,
            liquidity_score: 0.0,
            market_cap_score: 0.0,
            volume_score: 0.0,
            last_updated: chrono::Utc::now(),
        }
    }

    pub fn calculate_overall_risk(&self) -> f64 {
        // Weighted average of different risk factors
        let weights = [0.3, 0.25, 0.25, 0.2]; // volatility, liquidity, market_cap, volume
        let scores = [
            self.volatility,
            1.0 - self.liquidity_score,  // Lower liquidity = higher risk
            1.0 - self.market_cap_score, // Lower market cap = higher risk
            1.0 - self.volume_score,     // Lower volume = higher risk
        ];

        weights
            .iter()
            .zip(scores.iter())
            .map(|(w, s)| w * s)
            .sum::<f64>()
            .clamp(0.0, 1.0)
    }

    pub fn update_risk_factors(
        &mut self,
        volatility: Option<f64>,
        liquidity_score: Option<f64>,
        market_cap_score: Option<f64>,
        volume_score: Option<f64>,
    ) {
        if let Some(v) = volatility {
            self.volatility = v.clamp(0.0, 1.0);
        }
        if let Some(l) = liquidity_score {
            self.liquidity_score = l.clamp(0.0, 1.0);
        }
        if let Some(m) = market_cap_score {
            self.market_cap_score = m.clamp(0.0, 1.0);
        }
        if let Some(v) = volume_score {
            self.volume_score = v.clamp(0.0, 1.0);
        }

        self.risk_score = self.calculate_overall_risk();
        self.last_updated = chrono::Utc::now();
    }
}
