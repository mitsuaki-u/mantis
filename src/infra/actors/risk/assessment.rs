use super::RiskManagerActor;
use crate::core::error::Result;
use crate::core::models::market::TokenMetrics;
use log::{debug, info};

/// Assess initial token risk when a new token is encountered
pub async fn assess_token_risk(actor: &mut RiskManagerActor, token_id: &str) -> Result<()> {
    info!("Assessing initial risk for token: {}", token_id);

    // Get token data from repository
    match actor.token_repo.get_token_price_stats(token_id).await {
        Ok(token_stats) => {
            // Calculate risk based on various factors
            let mut risk_score: f64 = 0.5; // Start with medium risk

            // Factor 1: Market cap (lower market cap = higher risk)
            if let Some(market_cap) = token_stats.market_cap {
                if market_cap < 1_000_000.0 {
                    risk_score += 0.3; // Very high risk for micro caps
                } else if market_cap < 10_000_000.0 {
                    risk_score += 0.2; // High risk for small caps
                } else if market_cap < 100_000_000.0 {
                    risk_score += 0.1; // Medium-high risk for mid caps
                }
            } else {
                // No market cap data = higher risk
                risk_score += 0.2;
            }

            // Factor 2: Volume (lower volume = higher risk)
            if token_stats.volume_24h < 100_000.0 {
                risk_score += 0.2; // High risk for low volume
            } else if token_stats.volume_24h < 1_000_000.0 {
                risk_score += 0.1; // Medium risk for medium volume
            }

            // Factor 3: Price volatility (higher volatility = higher risk)
            let price_change_24h = token_stats.price_change_24h.abs();
            if price_change_24h > 20.0 {
                risk_score += 0.2; // High volatility
            } else if price_change_24h > 10.0 {
                risk_score += 0.1; // Medium volatility
            }

            // Clamp risk score to valid range
            risk_score = risk_score.clamp(0.0, 1.0);

            // Store the calculated risk
            actor.risk_scores.insert(token_id.to_string(), risk_score);
            actor.token_risks.insert(token_id.to_string(), risk_score);

            info!(
                "Risk assessment for {}: score = {:.3} (market_cap: ${:.0}, volume: ${:.0}, volatility: {:.1}%)",
                token_id, 
                risk_score, 
                token_stats.market_cap.unwrap_or(0.0), 
                token_stats.volume_24h, 
                price_change_24h
            );
        }
        Err(e) => {
            // If we can't get token data, assign high risk
            let high_risk = 0.8;
            actor.risk_scores.insert(token_id.to_string(), high_risk);
            actor.token_risks.insert(token_id.to_string(), high_risk);

            debug!(
                "Could not assess risk for {} (error: {}), assigning high risk: {:.3}",
                token_id, e, high_risk
            );
        }
    }

    Ok(())
}

/// Calculate appropriate position size based on risk assessment
pub fn calculate_position_size(
    actor: &RiskManagerActor,
    token: &TokenMetrics,
    confidence: f64,
) -> f64 {
    let base_position_size = actor.config.trading.max_position_size;

    // Get risk score for this token (default to medium risk if unknown)
    let risk_score = actor.risk_scores.get(&token.symbol).unwrap_or(&0.5);

    // Calculate risk-adjusted position size
    let risk_factor = 1.0 - risk_score; // Lower risk = larger position
    let confidence_factor = confidence.clamp(0.0, 1.0);

    // Apply portfolio-wide risk considerations
    let portfolio_risk = calculate_portfolio_risk_factor(actor);

    let adjusted_size = base_position_size * risk_factor * confidence_factor * portfolio_risk;

    // Ensure minimum and maximum bounds
    let min_size = base_position_size * 0.1; // At least 10% of max
    let max_size = base_position_size * 0.8; // At most 80% of max for single position

    let final_size = adjusted_size.clamp(min_size, max_size);

    debug!(
        "Position size calculation for {}: base={:.2}, risk_factor={:.3}, confidence={:.3}, portfolio_risk={:.3}, final={:.2}",
        token.symbol, base_position_size, risk_factor, confidence_factor, portfolio_risk, final_size
    );

    final_size
}

/// Calculate portfolio-wide risk factor that affects all position sizes
fn calculate_portfolio_risk_factor(actor: &RiskManagerActor) -> f64 {
    let mut risk_factor = 1.0;

    // Reduce position sizes if we're approaching daily loss limit
    if actor.max_daily_loss_limit > 0.0 {
        let loss_ratio = actor.current_daily_loss / actor.max_daily_loss_limit;
        if loss_ratio > 0.5 {
            risk_factor *= 1.0 - (loss_ratio - 0.5); // Reduce by up to 50%
        }
    }

    // Reduce position sizes if we're approaching drawdown limit
    if actor.max_drawdown_limit > 0.0 {
        let drawdown_ratio = actor.current_drawdown / actor.max_drawdown_limit;
        if drawdown_ratio > 0.5 {
            risk_factor *= 1.0 - (drawdown_ratio - 0.5); // Reduce by up to 50%
        }
    }

    // Ensure we don't go below minimum risk factor
    risk_factor.clamp(0.1, 1.0)
}

/// Assess risk for a trading signal
pub async fn assess_signal_risk(
    actor: &mut RiskManagerActor,
    token_id: &str,
    signal_confidence: f64,
) -> Result<f64> {
    // Ensure we have risk assessment for this token
    if !actor.risk_scores.contains_key(token_id) {
        assess_token_risk(actor, token_id).await?;
    }

    let token_risk = actor.risk_scores.get(token_id).unwrap_or(&0.5);
    let portfolio_risk_factor = calculate_portfolio_risk_factor(actor);

    // Calculate overall signal risk
    // Higher token risk or lower portfolio health = higher signal risk
    let signal_risk = token_risk + (1.0 - portfolio_risk_factor) * 0.3;
    let adjusted_confidence = signal_confidence * (1.0 - signal_risk.clamp(0.0, 0.8));

    debug!(
        "Signal risk assessment for {}: token_risk={:.3}, portfolio_factor={:.3}, original_confidence={:.3}, adjusted_confidence={:.3}",
        token_id, token_risk, portfolio_risk_factor, signal_confidence, adjusted_confidence
    );

    Ok(adjusted_confidence.clamp(0.0, 1.0))
}
