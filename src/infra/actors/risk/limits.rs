use super::RiskManagerActor;
use crate::core::error::Result;
use crate::infra::actors::{Event, RiskEvent};
use chrono::Utc;
use log::{info, warn};

/// Check risk limits for a specific token/symbol
pub async fn check_risk_limits(actor: &mut RiskManagerActor, symbol: &str) -> Result<()> {
    info!("Checking risk limits for symbol: {}", symbol);

    // Check token-specific risk score
    if let Some(risk_score) = actor.risk_scores.get(symbol) {
        if *risk_score > 0.8 {
            warn!(
                "High risk detected for {}: risk_score = {:.3}",
                symbol, risk_score
            );

            let risk_event = Event::Risk(RiskEvent::RiskLimitExceeded {
                limit_type: format!("token_risk_{}", symbol),
                current: *risk_score,
                max: 0.8,
                timestamp: Utc::now(),
            });

            actor.message_bus.publish(risk_event).await?;

            // Consider halting trading for this token
            actor.halted_tokens.insert(symbol.to_string());
            warn!("Trading halted for token: {}", symbol);
        }
    }

    // Check token-specific volatility risk
    if let Some(token_risk) = actor.token_risks.get(symbol) {
        if *token_risk > 0.9 {
            warn!(
                "Extreme volatility risk for {}: token_risk = {:.3}",
                symbol, token_risk
            );

            let risk_event = Event::Risk(RiskEvent::RiskLimitExceeded {
                limit_type: format!("volatility_risk_{}", symbol),
                current: *token_risk,
                max: 0.9,
                timestamp: Utc::now(),
            });

            actor.message_bus.publish(risk_event).await?;
        }
    }

    Ok(())
}

/// Check overall portfolio risk limits
pub async fn check_overall_risk_limits(actor: &mut RiskManagerActor) -> Result<()> {
    info!("Checking overall risk limits");

    // Check overall daily loss (actor-tracked)
    if actor.current_daily_loss >= actor.max_daily_loss_limit {
        warn!(
            "RiskManager: Overall daily loss limit exceeded: current_daily_loss (${:.2}) >= max_daily_loss_limit (${:.2})",
            actor.current_daily_loss, actor.max_daily_loss_limit
        );

        let risk_event = Event::Risk(RiskEvent::RiskLimitExceeded {
            limit_type: "overall_daily_loss".to_string(),
            current: actor.current_daily_loss,
            max: actor.max_daily_loss_limit,
            timestamp: Utc::now(),
        });

        actor.message_bus.publish(risk_event).await?;

        // Potentially halt all trading
        warn!("Consider halting all trading due to daily loss limit");
    }

    // Check overall drawdown (actor-tracked)
    if actor.current_drawdown >= actor.max_drawdown_limit {
        warn!(
            "RiskManager: Overall drawdown limit exceeded: current_drawdown (${:.2}) >= max_drawdown_limit (${:.2})",
            actor.current_drawdown, actor.max_drawdown_limit
        );

        let risk_event = Event::Risk(RiskEvent::RiskLimitExceeded {
            limit_type: "overall_drawdown".to_string(),
            current: actor.current_drawdown,
            max: actor.max_drawdown_limit,
            timestamp: Utc::now(),
        });

        actor.message_bus.publish(risk_event).await?;

        // Potentially halt all trading
        warn!("Consider halting all trading due to drawdown limit");
    }

    Ok(())
}

/// Check if trading is allowed for a specific token
pub fn is_trading_allowed(actor: &RiskManagerActor, token_id: &str) -> bool {
    // Check if token is halted
    if actor.halted_tokens.contains(token_id) {
        return false;
    }

    // Check overall limits
    if actor.current_daily_loss >= actor.max_daily_loss_limit {
        return false;
    }

    if actor.current_drawdown >= actor.max_drawdown_limit {
        return false;
    }

    // Check token-specific risk
    if let Some(risk_score) = actor.risk_scores.get(token_id) {
        if *risk_score > 0.8 {
            return false;
        }
    }

    true
}

/// Resume trading for a halted token (manual override)
pub fn resume_token_trading(actor: &mut RiskManagerActor, token_id: &str) {
    if actor.halted_tokens.remove(token_id) {
        info!("Resumed trading for token: {}", token_id);
    }
}

/// Halt trading for a specific token
pub fn halt_token_trading(actor: &mut RiskManagerActor, token_id: &str, reason: &str) {
    actor.halted_tokens.insert(token_id.to_string());
    warn!("Halted trading for token {}: {}", token_id, reason);
}

/// Get list of currently halted tokens
pub fn get_halted_tokens(actor: &RiskManagerActor) -> Vec<String> {
    actor.halted_tokens.iter().cloned().collect()
}
