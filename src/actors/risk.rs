use super::{Actor, Message, Event, StrategyEvent, RiskEvent, Command, Query, QueryResult};
use crate::error::Error;
use crate::trading::strategy::Signal;
use crate::types::market::TokenMetrics;
use crate::repositories::TokenRepository;
use std::sync::Arc;
use chrono::Utc;
use log::{info, error};

#[derive(Clone)]
pub struct RiskManagerActor {
    token_repo: TokenRepository,
    message_bus: Arc<super::MessageBus>,
    running: bool,
    max_position_size: f64,
    max_daily_loss: f64,
    max_drawdown: f64,
    current_daily_loss: f64,
    current_drawdown: f64,
}

impl RiskManagerActor {
    pub fn new(
        token_repo: TokenRepository,
        message_bus: Arc<super::MessageBus>,
        max_position_size: f64,
        max_daily_loss: f64,
        max_drawdown: f64,
    ) -> Self {
        Self {
            token_repo,
            message_bus,
            running: false,
            max_position_size,
            max_daily_loss,
            max_drawdown,
            current_daily_loss: 0.0,
            current_drawdown: 0.0,
        }
    }

    async fn handle_strategy_signal(&mut self, token_id: String, signal: Signal, confidence: f64) -> Result<(), Error> {
        if !self.running {
            return Ok(());
        }

        // Get token data and convert to TokenMetrics
        let token_data = self.token_repo.get_token_price_stats(&token_id)?;
        let token_metrics = crate::types::market::TokenMetrics::from(&token_data);

        // Check risk limits
        if self.current_daily_loss >= self.max_daily_loss {
            let event = Event::Risk(RiskEvent::RiskLimitExceeded {
                limit_type: "daily_loss".to_string(),
                current: self.current_daily_loss,
                max: self.max_daily_loss,
                timestamp: Utc::now(),
            });
            self.message_bus.publish(event).await?;
            return Ok(());
        }

        if self.current_drawdown >= self.max_drawdown {
            let event = Event::Risk(RiskEvent::RiskLimitExceeded {
                limit_type: "drawdown".to_string(),
                current: self.current_drawdown,
                max: self.max_drawdown,
                timestamp: Utc::now(),
            });
            self.message_bus.publish(event).await?;
            return Ok(());
        }

        // Calculate position size based on risk parameters
        let position_size = self.calculate_position_size(&token_metrics, confidence);

        // Publish risk assessment event
        let event = Event::Risk(RiskEvent::RiskAssessment {
            token_id: token_data.id.clone(),
            signal,
            confidence,
            position_size,
            timestamp: Utc::now(),
        });

        self.message_bus.publish(event).await?;
        Ok(())
    }

    fn calculate_position_size(&self, token: &TokenMetrics, confidence: f64) -> f64 {
        // Base position size on max position size and confidence
        let base_size = self.max_position_size * confidence;

        // Adjust for volatility
        let volatility_factor = 1.0 - (token.price_change_24h.abs() / 100.0);
        
        // Adjust for volume
        let volume_factor = (token.volume_24h / 1_000_000.0).min(1.0);

        base_size * volatility_factor * volume_factor
    }

    async fn update_risk_metrics(&mut self, pnl: f64) -> Result<(), Error> {
        // Update daily loss
        if pnl < 0.0 {
            self.current_daily_loss += pnl.abs();
        }

        // Update drawdown
        if pnl < 0.0 && pnl.abs() > self.current_drawdown {
            self.current_drawdown = pnl.abs();
        }

        // Publish risk metrics update
        let event = Event::Risk(RiskEvent::RiskMetricsUpdate {
            daily_loss: self.current_daily_loss,
            drawdown: self.current_drawdown,
            timestamp: Utc::now(),
        });

        self.message_bus.publish(event).await?;
        Ok(())
    }
}

impl Actor for RiskManagerActor {
    fn start(&mut self) -> Result<(), Error> {
        self.running = true;
        info!("Starting RiskManagerActor");
        Ok(())
    }

    fn stop(&mut self) -> Result<(), Error> {
        self.running = false;
        info!("Stopping RiskManagerActor");
        Ok(())
    }

    fn handle_message(&mut self, msg: Message) -> Result<(), Error> {
        match msg {
            Message::Event(event) => match event {
                Event::Strategy(StrategyEvent::Signal { token_id, signal, confidence, .. }) => {
                    if self.running {
                        let mut this = self.clone();
                        tokio::spawn(async move {
                            if let Err(e) = this.handle_strategy_signal(token_id, signal, confidence).await {
                                error!("Error handling strategy signal: {:?}", e);
                            }
                        });
                    }
                    Ok(())
                },
                Event::Risk(RiskEvent::PositionClosed { pnl, .. }) => {
                    if self.running {
                        let mut this = self.clone();
                        tokio::spawn(async move {
                            if let Err(e) = this.update_risk_metrics(pnl).await {
                                error!("Error updating risk metrics: {:?}", e);
                            }
                        });
                    }
                    Ok(())
                },
                _ => Ok(()),
            },
            Message::Command(cmd) => match cmd {
                Command::Start => {
                    self.running = true;
                    info!("RiskManagerActor received start command");
                    Ok(())
                },
                Command::Stop => {
                    self.running = false;
                    info!("RiskManagerActor received stop command");
                    Ok(())
                },
                Command::UpdateConfig(config) => {
                    // Update risk parameters from config
                    if let Some(size) = config.get("max_position_size").and_then(|v| v.as_f64()) {
                        self.max_position_size = size;
                        info!("Updated max position size to {}", size);
                    }
                    if let Some(loss) = config.get("max_daily_loss").and_then(|v| v.as_f64()) {
                        self.max_daily_loss = loss;
                        info!("Updated max daily loss to {}", loss);
                    }
                    if let Some(drawdown) = config.get("max_drawdown").and_then(|v| v.as_f64()) {
                        self.max_drawdown = drawdown;
                        info!("Updated max drawdown to {}", drawdown);
                    }
                    Ok(())
                },
            },
            Message::Query(query, responder) => match query {
                Query::GetStatus => {
                    let status = format!(
                        "RiskManagerActor running: {}, Daily Loss: {:.2}, Drawdown: {:.2}",
                        self.running, self.current_daily_loss, self.current_drawdown
                    );
                    responder.send(Ok(QueryResult::Status(status)))
                        .map_err(|e| Error::Task(format!("Failed to send status response: {:?}", e)))
                },
                Query::GetMetrics => {
                    let metrics = serde_json::json!({
                        "running": self.running,
                        "max_position_size": self.max_position_size,
                        "max_daily_loss": self.max_daily_loss,
                        "max_drawdown": self.max_drawdown,
                        "current_daily_loss": self.current_daily_loss,
                        "current_drawdown": self.current_drawdown,
                    });
                    responder.send(Ok(QueryResult::Metrics(metrics)))
                        .map_err(|e| Error::Task(format!("Failed to send metrics response: {:?}", e)))
                },
                _ => {
                    responder.send(Err(Error::Task("Unsupported query type".to_string())))
                        .map_err(|e| Error::Task(format!("Failed to send error response: {:?}", e)))
                },
            },
        }
    }
} 