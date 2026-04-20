use super::RiskManagerActor;
use crate::application::errors::Result;
use crate::events::{Event, RiskEvent};
use chrono::Utc;
use log::warn;

/// Check overall portfolio risk limits using features layer
pub async fn check_overall_risk_limits(actor: &mut RiskManagerActor) -> Result<()> {
    // Check overall risk limits using features layer
    let violations = crate::core::risk::check_overall_risk_limits(
        actor.risk_metrics.current_daily_loss,
        actor.risk_metrics.max_daily_loss_limit,
        actor.risk_metrics.current_drawdown,
        actor.risk_metrics.max_drawdown_limit,
    );

    for violation in violations {
        // Publish infrastructure event
        let risk_event = Event::Risk(RiskEvent::RiskLimitExceeded {
            limit_type: violation.limit_type.clone(),
            current: violation.current_value,
            max: violation.max_value,
            timestamp: Utc::now(),
        });
        actor.event_router.publish(risk_event).await?;

        // Consider halting all trading for serious violations
        if violation.should_halt_trading {
            warn!(
                "Consider halting all trading due to {}",
                violation.limit_type
            );
        }
    }

    Ok(())
}

/// Check if trading is allowed for a specific token using features layer
pub async fn check_trading_allowed(
    actor: &mut RiskManagerActor,
    token_id: &str,
    signal: &crate::core::domain::trading::Signal,
    signal_metadata: &crate::core::domain::trading::SignalMetadata,
) -> Result<bool> {
    let max_allowed_positions = actor.config.trading.max_positions;

    // For BUY signals, atomically reserve a position slot BEFORE any other checks
    // This prevents race conditions where multiple concurrent signals bypass the position limit
    if signal.is_buy() {
        match actor
            .position_repo
            .try_reserve_position_slot(&signal_metadata.correlation_id, max_allowed_positions)
            .await
        {
            Ok(true) => {
                log::info!(
                    "[{}] ✅ Position slot reserved ({}/{})",
                    &signal_metadata.correlation_id[..8],
                    "checking",
                    max_allowed_positions
                );
            }
            Ok(false) => {
                log::warn!(
                    "[{}] ❌ Position limit reached ({}/{}), rejecting BUY signal for {}",
                    &signal_metadata.correlation_id[..8],
                    max_allowed_positions,
                    max_allowed_positions,
                    token_id
                );
                return Ok(false);
            }
            Err(e) => {
                log::error!(
                    "[{}] Failed to reserve position slot: {} - rejecting signal for safety",
                    &signal_metadata.correlation_id[..8],
                    e
                );
                return Ok(false);
            }
        }
    }

    // Use database count for accurate position count (for display purposes)
    // Note: Actual race prevention is handled by atomic reservations above
    let total_positions = match actor.position_repo.get_open_position_count().await {
        Ok(count) => {
            log::debug!("Position count from database: {}", count);
            count
        }
        Err(e) => {
            log::error!(
                "Failed to get position count from database: {}, falling back to in-memory count",
                e
            );
            actor.get_all_positions().len()
        }
    };

    // Use features layer function
    let ctx = crate::core::risk::limits::TradingLimitsContext {
        current_daily_loss: actor.risk_metrics.current_daily_loss,
        max_daily_loss_limit: actor.risk_metrics.max_daily_loss_limit,
        current_drawdown: actor.risk_metrics.current_drawdown,
        max_drawdown_limit: actor.risk_metrics.max_drawdown_limit,
        max_positions: max_allowed_positions,
        current_positions: total_positions,
    };
    let allowed =
        crate::core::risk::check_trading_allowed(token_id, signal, &actor.halted_tokens, &ctx);

    Ok(allowed)
}

/// Resume trading for a halted token (manual override) using features layer
pub fn resume_token_trading(actor: &mut RiskManagerActor, token_id: &str) -> bool {
    crate::core::risk::resume_token_trading(&mut actor.halted_tokens, token_id)
}

/// Halt trading for a specific token using features layer
pub fn halt_token_trading(actor: &mut RiskManagerActor, token_id: &str, reason: &str) {
    crate::core::risk::halt_token_trading(&mut actor.halted_tokens, token_id, reason);
}

/// Get list of currently halted tokens using features layer
pub fn get_halted_tokens(actor: &RiskManagerActor) -> Vec<String> {
    crate::core::risk::get_halted_tokens(&actor.halted_tokens)
}
