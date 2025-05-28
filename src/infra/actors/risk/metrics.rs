use super::RiskManagerActor;
use crate::core::error::Result;
use log::{debug, info};

/// Update risk metrics based on PnL
pub async fn update_risk_metrics(actor: &mut RiskManagerActor, pnl: f64) -> Result<()> {
    debug!("Updating risk metrics with PnL: {:.2}", pnl);

    // Update daily loss if PnL is negative
    if pnl < 0.0 {
        actor.current_daily_loss += pnl.abs();
        info!("Updated daily loss: {:.2}", actor.current_daily_loss);
    }

    // Update drawdown calculation
    // This is a simplified version - in practice, you'd track high water mark
    if pnl < 0.0 {
        actor.current_drawdown += pnl.abs();
        info!("Updated drawdown: {:.2}", actor.current_drawdown);
    }

    Ok(())
}

/// Update token-specific risk score
pub fn update_token_risk(actor: &mut RiskManagerActor, token_id: &str, risk_score: f64) {
    let clamped_score = risk_score.clamp(0.0, 1.0);
    actor
        .token_risks
        .insert(token_id.to_string(), clamped_score);
    debug!("Updated token risk for {}: {:.3}", token_id, clamped_score);
}

/// Get token risk score
pub fn get_token_risk(actor: &RiskManagerActor, token_id: &str) -> Option<f64> {
    actor.token_risks.get(token_id).copied()
}

/// Update general risk score for a token
pub fn update_risk_score(actor: &mut RiskManagerActor, token_id: &str, score: f64) {
    let clamped_score = score.clamp(0.0, 1.0);
    actor
        .risk_scores
        .insert(token_id.to_string(), clamped_score);
    debug!("Updated risk score for {}: {:.3}", token_id, clamped_score);
}

/// Get general risk score for a token
pub fn get_risk_score(actor: &RiskManagerActor, token_id: &str) -> Option<f64> {
    actor.risk_scores.get(token_id).copied()
}

/// Reset daily metrics (typically called at start of new trading day)
pub fn reset_daily_metrics(actor: &mut RiskManagerActor) {
    actor.current_daily_loss = 0.0;
    info!("Reset daily loss metrics");
}

/// Calculate portfolio-wide risk metrics
pub fn calculate_portfolio_risk(actor: &RiskManagerActor) -> f64 {
    if actor.risk_scores.is_empty() {
        return 0.5; // Default medium risk
    }

    let total_risk: f64 = actor.risk_scores.values().sum();
    let average_risk = total_risk / actor.risk_scores.len() as f64;

    // Factor in current losses
    let loss_factor = if actor.max_daily_loss_limit > 0.0 {
        actor.current_daily_loss / actor.max_daily_loss_limit
    } else {
        0.0
    };

    let drawdown_factor = if actor.max_drawdown_limit > 0.0 {
        actor.current_drawdown / actor.max_drawdown_limit
    } else {
        0.0
    };

    // Increase risk score based on current losses
    let adjusted_risk = average_risk + (loss_factor * 0.3) + (drawdown_factor * 0.2);
    adjusted_risk.clamp(0.0, 1.0)
}
